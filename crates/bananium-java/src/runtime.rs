//! Mojang's own Java runtimes, the way the official launcher gets them.
//!
//! Every version profile names a runtime *component* (`javaVersion.component`,
//! e.g. `java-runtime-delta` for Java 21, `jre-legacy` for Java 8). Mojang's
//! runtime index ([`RUNTIME_INDEX_URL`]) maps each platform + component to a
//! per-component manifest, which lists the runtime's whole file tree:
//! directories, files (with SHA-1 and download URL, and an `executable`
//! flag), and symlinks.
//!
//! This module only models that data and lays a downloaded tree out on
//! disk; fetching is the caller's job (`bananium-api` downloads each file
//! into the content-addressed store, then hands [`install_tree`] a way to
//! find those blobs).

use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Mojang's java-runtime index: platform -> component -> releases.
pub const RUNTIME_INDEX_URL: &str =
    "https://piston-meta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

/// The component a profile gets when it doesn't name one. Only very old
/// profiles lack `javaVersion`, and the official launcher runs those on
/// Java 8.
pub const DEFAULT_COMPONENT: &str = "jre-legacy";

/// Written into a component's directory once every file is in place, so a
/// half-finished download is never mistaken for an installed runtime.
const MARKER: &str = ".bananium-runtime.json";

/// `all.json`: platform key -> component name -> available releases
/// (in practice exactly one each).
pub type RuntimeIndex = HashMap<String, HashMap<String, Vec<RuntimeRelease>>>;

/// One runtime release for a platform + component.
#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeRelease {
    pub manifest: RemoteFile,
    pub version: RuntimeVersion,
}

/// A downloadable file as Mojang describes it.
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteFile {
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeVersion {
    /// e.g. `"21.0.7"`.
    pub name: String,
}

/// A component's manifest: its complete file tree, keyed by relative path.
#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeManifest {
    pub files: BTreeMap<String, RuntimeEntry>,
}

/// One entry of a runtime's file tree.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RuntimeEntry {
    Directory,
    File {
        #[serde(default)]
        executable: bool,
        downloads: FileDownloads,
    },
    /// A symlink; `target` is relative to the link's own directory.
    Link {
        target: String,
    },
}

/// Mojang offers each file raw and (usually) LZMA-compressed. Bananium
/// uses the raw bytes, whose SHA-1 the manifest publishes.
#[derive(Debug, Clone, Deserialize)]
pub struct FileDownloads {
    pub raw: RemoteFile,
}

/// Recorded in the component directory (`.bananium-runtime.json`) after a
/// successful install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledRuntime {
    pub component: String,
    /// The runtime's own version, e.g. `"21.0.7"`.
    pub version: String,
    /// SHA-1 of the manifest it was installed from; a newer manifest from
    /// Mojang means the runtime was updated upstream.
    pub manifest_sha1: String,
}

/// Mojang's platform key for this machine, or `None` where Mojang
/// publishes no runtimes (e.g. aarch64 Linux).
pub fn platform_key() -> Option<&'static str> {
    platform_key_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn platform_key_for(os: &str, arch: &str) -> Option<&'static str> {
    Some(match (os, arch) {
        ("windows", "x86_64") => "windows-x64",
        ("windows", "x86") => "windows-x86",
        ("windows", "aarch64") => "windows-arm64",
        ("linux", "x86_64") => "linux",
        ("linux", "x86") => "linux-i386",
        ("macos", "x86_64") => "mac-os",
        ("macos", "aarch64") => "mac-os-arm64",
        _ => return None,
    })
}

/// The runtime release for `component` on this machine, if Mojang has one.
pub fn release_for<'a>(index: &'a RuntimeIndex, component: &str) -> Option<&'a RuntimeRelease> {
    index.get(platform_key()?)?.get(component)?.first()
}

/// The `java` executable inside an installed component directory. macOS
/// runtimes ship as a `jre.bundle`.
pub fn java_executable(component_dir: &Path) -> PathBuf {
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    let bundle = component_dir
        .join("jre.bundle")
        .join("Contents")
        .join("Home")
        .join("bin")
        .join(exe);
    if bundle.is_file() {
        bundle
    } else {
        component_dir.join("bin").join(exe)
    }
}

/// What's installed in `component_dir`, or `None` if it isn't (completely).
pub fn installed(component_dir: &Path) -> Option<InstalledRuntime> {
    let text = std::fs::read_to_string(component_dir.join(MARKER)).ok()?;
    let marker: InstalledRuntime = serde_json::from_str(&text).ok()?;
    java_executable(component_dir).is_file().then_some(marker)
}

/// A manifest path as a safe relative path: `None` if it could escape the
/// component directory.
fn safe_relative(rel: &str) -> Option<PathBuf> {
    let path = Path::new(rel);
    path.components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then(|| path.to_path_buf())
}

/// Lay `manifest`'s tree out in `component_dir` and mark it installed.
///
/// `place(sha1, dest)` puts the already-downloaded, already-verified file
/// with that SHA-1 at `dest` (the caller materializes it from its store).
/// Any previous install is removed first, so files dropped upstream don't
/// linger. Executable files get their `+x` bit and links are recreated as
/// symlinks (Unix only; Windows runtimes carry neither).
pub fn install_tree(
    component_dir: &Path,
    manifest: &RuntimeManifest,
    installed: &InstalledRuntime,
    mut place: impl FnMut(&str, &Path) -> std::io::Result<()>,
) -> Result<()> {
    if component_dir.exists() {
        std::fs::remove_dir_all(component_dir)?;
    }
    std::fs::create_dir_all(component_dir)?;
    // BTreeMap order puts every directory before its contents.
    for (rel, entry) in &manifest.files {
        let rel_path = safe_relative(rel).ok_or_else(|| Error::BadRuntimePath(rel.clone()))?;
        let dest = component_dir.join(rel_path);
        match entry {
            RuntimeEntry::Directory => std::fs::create_dir_all(&dest)?,
            RuntimeEntry::File {
                executable,
                downloads,
            } => {
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                place(&downloads.raw.sha1, &dest)?;
                if *executable {
                    make_executable(&dest)?;
                }
            }
            RuntimeEntry::Link { target } => link(target, &dest)?,
        }
    }
    std::fs::write(
        component_dir.join(MARKER),
        serde_json::to_string_pretty(installed).expect("marker serializes"),
    )?;
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(perms.mode() | 0o755);
    std::fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn link(target: &str, dest: &Path) -> std::io::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::os::unix::fs::symlink(target, dest)
}

#[cfg(not(unix))]
fn link(_target: &str, _dest: &Path) -> std::io::Result<()> {
    // Mojang's Windows runtimes contain no links; nothing to do if one
    // ever appears (the JVM doesn't need them).
    Ok(())
}

/// Every Mojang runtime installed under `java_dir`, by component name.
pub fn list_installed(java_dir: &Path) -> Vec<(PathBuf, InstalledRuntime)> {
    let Ok(entries) = std::fs::read_dir(java_dir) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter_map(|dir| installed(&dir).map(|m| (dir, m)))
        .collect();
    found.sort_by(|a, b| a.1.component.cmp(&b.1.component));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{
        "files": {
            "bin": {"type": "directory"},
            "bin/java": {"type": "file", "executable": true,
                "downloads": {"raw": {"sha1": "aa", "size": 4, "url": "https://x/java"},
                              "lzma": {"sha1": "bb", "size": 2, "url": "https://x/java.lzma"}}},
            "bin/java.exe": {"type": "file", "executable": true,
                "downloads": {"raw": {"sha1": "aa", "size": 4, "url": "https://x/java"}}},
            "lib/modules": {"type": "file",
                "downloads": {"raw": {"sha1": "cc", "size": 7, "url": "https://x/modules"}}},
            "legal/java.base": {"type": "link", "target": "../lib"}
        }
    }"#;

    fn marker() -> InstalledRuntime {
        InstalledRuntime {
            component: "java-runtime-delta".into(),
            version: "21.0.7".into(),
            manifest_sha1: "m1".into(),
        }
    }

    #[test]
    fn platform_keys_match_mojangs_names() {
        assert_eq!(platform_key_for("windows", "x86_64"), Some("windows-x64"));
        assert_eq!(platform_key_for("linux", "x86_64"), Some("linux"));
        assert_eq!(platform_key_for("macos", "aarch64"), Some("mac-os-arm64"));
        assert_eq!(
            platform_key_for("linux", "aarch64"),
            None,
            "Mojang has no linux-arm64 runtime"
        );
    }

    #[test]
    fn index_lookup_uses_this_platform() {
        let key = platform_key().unwrap_or("linux");
        let json = format!(
            r#"{{"{key}": {{"java-runtime-delta": [{{"manifest": {{"sha1": "m1", "size": 1, "url": "https://x/m"}}, "version": {{"name": "21.0.7", "released": "2025"}}}}]}}}}"#
        );
        let index: RuntimeIndex = serde_json::from_str(&json).unwrap();
        if platform_key().is_some() {
            let release = release_for(&index, "java-runtime-delta").unwrap();
            assert_eq!(release.version.name, "21.0.7");
            assert!(release_for(&index, "jre-legacy").is_none());
        }
    }

    #[test]
    fn install_tree_places_files_marks_executables_and_writes_the_marker() {
        let dir = tempfile::tempdir().unwrap();
        let component = dir.path().join("java-runtime-delta");
        let manifest: RuntimeManifest = serde_json::from_str(MANIFEST).unwrap();
        let mut placed = Vec::new();
        install_tree(&component, &manifest, &marker(), |sha1, dest| {
            placed.push(sha1.to_string());
            std::fs::write(dest, sha1)
        })
        .unwrap();

        assert_eq!(placed, ["aa", "aa", "cc"]);
        assert_eq!(
            std::fs::read_to_string(component.join("lib/modules")).unwrap(),
            "cc"
        );
        assert_eq!(installed(&component), Some(marker()));
        assert!(java_executable(&component).is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(component.join("bin/java"))
                .unwrap()
                .permissions()
                .mode();
            assert!(mode & 0o111 != 0, "java must be executable");
            let link = std::fs::read_link(component.join("legal/java.base")).unwrap();
            assert_eq!(link, Path::new("../lib"));
        }
        assert_eq!(list_installed(dir.path()).len(), 1);
    }

    #[test]
    fn an_interrupted_install_is_not_reported_installed() {
        let dir = tempfile::tempdir().unwrap();
        let manifest: RuntimeManifest = serde_json::from_str(MANIFEST).unwrap();
        let result = install_tree(dir.path(), &manifest, &marker(), |sha1, dest| {
            if sha1 == "cc" {
                Err(std::io::Error::other("network dropped"))
            } else {
                std::fs::write(dest, sha1)
            }
        });
        assert!(result.is_err());
        assert_eq!(installed(dir.path()), None);
    }

    #[test]
    fn paths_escaping_the_component_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let manifest: RuntimeManifest =
            serde_json::from_str(r#"{"files": {"../evil": {"type": "directory"}}}"#).unwrap();
        assert!(matches!(
            install_tree(&dir.path().join("c"), &manifest, &marker(), |_, _| Ok(())),
            Err(Error::BadRuntimePath(_))
        ));
    }
}
