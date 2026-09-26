//! Java: Mojang's official runtimes (downloaded into
//! `<BANANIUM_HOME>/java/<component>/`), JVMs detected on the machine, and
//! choosing which one a launch uses.
//!
//! The default is always the runtime Mojang's own profile names for the
//! version (`javaVersion.component`), exactly like the official launcher.
//! A user-chosen executable — per instance, or globally — overrides it.

use std::path::{Path, PathBuf};

use bananium_instance::InstanceConfig;
use bananium_java::runtime::{
    self, RuntimeIndex, RuntimeManifest, DEFAULT_COMPONENT, RUNTIME_INDEX_URL,
};
use bananium_meta::{MetaClient, VersionProfile};
use bananium_net::DownloadSpec;
use bananium_store::BlobStore;

use super::tasks::TaskSpec;
use super::Session;
use crate::error::{Error, Result};
use crate::output::{CommandOutput, JavaInstall};
use crate::task::TaskKind;

/// The runtime component `profile` asks for, and its Java major version.
pub(super) fn required_runtime(profile: &VersionProfile) -> (String, Option<u32>) {
    match &profile.java_version {
        Some(j) => (j.component.clone(), Some(j.major_version)),
        // Only ancient profiles omit it; the official launcher uses Java 8.
        None => (DEFAULT_COMPONENT.to_string(), Some(8)),
    }
}

/// Java's major version from a runtime version name: `"21.0.7"` -> 21,
/// `"1.8.0_51"` -> 8, `"8u51"` -> 8.
fn major_from_version_name(name: &str) -> Option<u32> {
    let mut parts = name
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty());
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

/// A single plain directory name, so a component can't point elsewhere.
fn valid_component(component: &str) -> bool {
    !component.is_empty()
        && component
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && component != "."
        && component != ".."
}

impl Session {
    fn meta_client(&self) -> MetaClient {
        MetaClient::new(self.http.clone(), self.paths.clone())
    }

    /// Mojang's runtime index (cached in `meta/` for offline use).
    async fn runtime_index(&self) -> Result<RuntimeIndex> {
        let cache = self.paths.meta_dir().join("java-runtime").join("all.json");
        Ok(self
            .meta_client()
            .fetch_cached("java runtime index", RUNTIME_INDEX_URL, &cache)
            .await?)
    }

    /// Make sure Mojang's runtime `component` is installed and current,
    /// downloading it as part of task `task_id` if not. Returns its `java`
    /// executable, or `None` when Mojang publishes no such runtime for this
    /// platform (e.g. aarch64 Linux).
    pub(super) async fn ensure_runtime(
        &self,
        task_id: &str,
        component: &str,
    ) -> Result<Option<PathBuf>> {
        if !valid_component(component) {
            return Err(Error::NoSuitableJava {
                component: component.to_string(),
                major: None,
            });
        }
        let dir = self.paths.java_component_dir(component);
        let installed = runtime::installed(&dir);
        let index = match self.runtime_index().await {
            Ok(index) => index,
            // Offline with no cached index: an installed runtime still works.
            Err(err) if installed.is_some() => {
                tracing::warn!(
                    "Java runtime index unavailable ({err}); using the installed {component}"
                );
                return Ok(Some(runtime::java_executable(&dir)));
            }
            Err(err) => return Err(err),
        };
        let Some(release) = runtime::release_for(&index, component) else {
            tracing::info!("Mojang has no {component} runtime for this platform");
            return Ok(installed.map(|_| runtime::java_executable(&dir)));
        };
        if installed
            .as_ref()
            .is_some_and(|m| m.manifest_sha1 == release.manifest.sha1)
        {
            return Ok(Some(runtime::java_executable(&dir)));
        }

        let manifest: RuntimeManifest = self
            .meta_client()
            .fetch_cached(
                "java runtime manifest",
                &release.manifest.url,
                &self
                    .paths
                    .meta_dir()
                    .join("java-runtime")
                    .join(format!("{component}-{}.json", release.manifest.sha1)),
            )
            .await?;
        let specs: Vec<DownloadSpec> = manifest
            .files
            .iter()
            .filter_map(|(path, entry)| match entry {
                runtime::RuntimeEntry::File { downloads, .. } => Some(DownloadSpec {
                    url: downloads.raw.url.clone(),
                    dest: self.paths.store_blob(&downloads.raw.sha1),
                    expected_sha1: Some(downloads.raw.sha1.clone()),
                    expected_size: Some(downloads.raw.size),
                    task_id: format!("{task_id}/java:{}", downloads.raw.sha1),
                    label: path.clone(),
                }),
                _ => None,
            })
            .collect();
        let version = release.version.name.clone();
        tracing::info!(
            "downloading Java {version} ({component}, {} files)",
            specs.len()
        );
        self.download_tracked(
            task_id,
            &format!("Java {version} (Mojang {component})"),
            specs,
        )
        .await?;

        let blobs = BlobStore::new(self.paths.clone());
        let marker = runtime::InstalledRuntime {
            component: component.to_string(),
            version,
            manifest_sha1: release.manifest.sha1.clone(),
        };
        runtime::install_tree(&dir, &manifest, &marker, |sha1, dest| {
            blobs
                .materialize(sha1, dest)
                .map(|_| ())
                .map_err(std::io::Error::other)
        })?;
        tracing::info!("installed Java {} ({component})", marker.version);
        Ok(Some(runtime::java_executable(&dir)))
    }

    /// The JVM a launch of an instance on `profile` uses:
    ///
    /// 1. the instance's own Java setting, then the global one — a chosen
    ///    executable that isn't runnable is an error, never silently
    ///    swapped for another;
    /// 2. otherwise Mojang's runtime for the version, downloaded first if
    ///    it's missing (a dry run only reports where it would be);
    /// 3. only where Mojang has no runtime for this platform, a detected
    ///    JVM of the required major version.
    pub(super) async fn resolve_java(
        &self,
        instance_cfg: &InstanceConfig,
        profile: &VersionProfile,
        dry_run: bool,
    ) -> Result<PathBuf> {
        let chosen = instance_cfg
            .java_path
            .clone()
            .or_else(|| self.config().java_path.clone());
        if let Some(path) = chosen {
            tracing::info!("using the Java chosen in settings: {}", path.display());
            let probe = path.clone();
            let found = tokio::task::spawn_blocking(move || bananium_java::probe_java(&probe))
                .await
                .ok()
                .flatten();
            return found
                .map(|c| c.path)
                .ok_or_else(|| Error::JavaNotRunnable(path.display().to_string()));
        }

        let (component, major) = required_runtime(profile);
        let dir = self.paths.java_component_dir(&component);
        if runtime::installed(&dir).is_some() {
            return Ok(runtime::java_executable(&dir));
        }
        if runtime::platform_key().is_some() {
            if dry_run {
                return Ok(runtime::java_executable(&dir));
            }
            // No instance: several launches may share one runtime, and the
            // download itself is safe to overlap (one writer per file).
            let spec = TaskSpec::new(
                TaskKind::JavaRuntime,
                format!("Downloading Java ({component})"),
            );
            let ticket = self.enqueue_task("java", spec, || Ok(()))?;
            let task_id = ticket.task_id().to_string();
            let provisioned = self
                .tracked(ticket, self.ensure_runtime(&task_id, &component))
                .await?;
            if let Some(path) = provisioned {
                return Ok(path);
            }
        }

        // No Mojang runtime for this platform: fall back to a detected JVM
        // of exactly the major version the game expects.
        let wanted = major;
        tracing::warn!(
            "no Mojang {component} runtime for this platform; looking for a system Java {major:?}"
        );
        let system = tokio::task::spawn_blocking(bananium_java::find_all_java)
            .await
            .unwrap_or_default();
        system
            .into_iter()
            .find(|c| wanted.is_none_or(|m| c.major_version == m))
            .map(|c| c.path)
            .ok_or(Error::NoSuitableJava { component, major })
    }

    /// `Command::JavaList`: installed Mojang runtimes first, then every JVM
    /// detected on the machine.
    pub(super) async fn java_list(&self) -> Result<CommandOutput> {
        let mut installs: Vec<JavaInstall> = runtime::list_installed(&self.paths.java_dir())
            .into_iter()
            .map(|(dir, m)| JavaInstall {
                path: runtime::java_executable(&dir),
                major_version: major_from_version_name(&m.version).unwrap_or(0),
                source: "mojang".to_string(),
                component: Some(m.component),
                version: Some(m.version),
                size_bytes: Some(dir_size(&dir)),
            })
            .collect();
        let found = tokio::task::spawn_blocking(bananium_java::find_all_java)
            .await
            .unwrap_or_default();
        installs.extend(found.into_iter().map(|c| JavaInstall {
            path: c.path,
            major_version: c.major_version,
            source: "system".to_string(),
            component: None,
            version: None,
            size_bytes: None,
        }));
        Ok(CommandOutput::JavaListed { installs })
    }

    /// `Command::JavaRuntimeRemove`: delete one downloaded Mojang runtime.
    /// It's downloaded again the next time something needs it.
    pub(super) fn java_runtime_remove(&self, component: &str) -> Result<CommandOutput> {
        if !valid_component(component) {
            return Err(Error::InvalidPath(component.to_string()));
        }
        let dir = self.paths.java_component_dir(component);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        Ok(CommandOutput::JavaRuntimeRemoved {
            component: component.to_string(),
        })
    }

    /// `Command::InstanceJava`: which Mojang runtime an instance uses by
    /// default, and whether it's downloaded yet.
    pub(super) async fn instance_java(&self, instance: &str) -> Result<CommandOutput> {
        let cfg = self.instances().load(instance)?;
        let (_, profile) = self
            .resolve_profile(
                &self.meta_client(),
                &cfg.mc_version,
                cfg.loader,
                cfg.loader_version.as_deref(),
            )
            .await?;
        let (component, major_version) = required_runtime(&profile);
        let dir = self.paths.java_component_dir(&component);
        let installed = runtime::installed(&dir);
        Ok(CommandOutput::InstanceJavaShown {
            instance: instance.to_string(),
            installed_version: installed.map(|m| m.version),
            path: runtime::java_executable(&dir),
            available: runtime::platform_key().is_some(),
            component,
            major_version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_versions_parse_from_runtime_names() {
        assert_eq!(major_from_version_name("21.0.7"), Some(21));
        assert_eq!(major_from_version_name("1.8.0_51"), Some(8));
        assert_eq!(major_from_version_name("8u51"), Some(8));
        assert_eq!(major_from_version_name("17.0.8+7"), Some(17));
    }

    #[test]
    fn component_names_must_be_plain() {
        assert!(valid_component("java-runtime-delta"));
        assert!(!valid_component(".."));
        assert!(!valid_component("../x"));
        assert!(!valid_component("a/b"));
    }
}
