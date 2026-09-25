//! Screenshots across instances, settings, and Java detection.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use bananium_core::Config;

use super::Session;
use crate::error::{Error, Result};
use crate::output::{CommandOutput, JavaInstall, Screenshot};

impl Session {
    /// The live config (see `Command::ConfigSet`).
    pub(super) fn config(&self) -> Config {
        self.config.read().expect("config lock poisoned").clone()
    }

    /// `Command::ConfigSet`: persist to `config.toml` and apply to this
    /// session immediately.
    pub(super) fn config_set(
        &self,
        max_concurrent_downloads: Option<usize>,
        java_path: Option<PathBuf>,
    ) -> Result<CommandOutput> {
        let java_path = java_path.map(|p| (!p.as_os_str().is_empty()).then_some(p));
        let updated = Config::update_file(&self.paths, max_concurrent_downloads, java_path)?;
        *self.config.write().expect("config lock poisoned") = updated;
        self.config_show()
    }

    /// `Command::JavaList`: every JVM detected on this machine. Runs each
    /// candidate's `java -version`, so it's blocking work kept off the
    /// async runtime's worker threads.
    pub(super) async fn java_list(&self) -> Result<CommandOutput> {
        let found = tokio::task::spawn_blocking(bananium_java::find_all_java)
            .await
            .unwrap_or_default();
        Ok(CommandOutput::JavaListed {
            installs: found
                .into_iter()
                .map(|c| JavaInstall {
                    path: c.path,
                    major_version: c.major_version,
                })
                .collect(),
        })
    }

    /// `Command::ScreenshotList`: PNGs from every instance's (or one
    /// instance's) `screenshots/` folder, newest first.
    pub(super) fn screenshot_list(&self, instance: Option<&str>) -> Result<CommandOutput> {
        let instances = self.instances();
        let configs = match instance {
            Some(slug) => vec![(slug.to_string(), instances.load(slug)?)],
            None => instances.list_configs()?,
        };
        let mut shots = Vec::new();
        for (slug, cfg) in configs {
            let dir = self.paths.instance_minecraft_dir(&slug).join("screenshots");
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("png"))
                {
                    continue;
                }
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                shots.push(Screenshot {
                    instance: slug.clone(),
                    instance_name: cfg.name.clone(),
                    file_name: entry.file_name().to_string_lossy().into_owned(),
                    path,
                    size: meta.len(),
                    taken_unix: meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or_default(),
                });
            }
        }
        shots.sort_by_key(|s| std::cmp::Reverse(s.taken_unix));
        Ok(CommandOutput::ScreenshotListed { screenshots: shots })
    }

    /// `Command::ScreenshotDelete`. The path comes from the webview, so it's
    /// checked to really be a PNG directly inside some instance's
    /// `screenshots/` folder before anything is deleted.
    pub(super) fn screenshot_delete(&self, path: &Path) -> Result<CommandOutput> {
        let canonical = std::fs::canonicalize(path)
            .map_err(|_| Error::NotAScreenshot(path.display().to_string()))?;
        let instances_dir = std::fs::canonicalize(self.paths.instances_dir())?;
        let parent = canonical.parent();
        let is_screenshot = canonical
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("png"))
            && parent.is_some_and(|p| p.file_name().is_some_and(|n| n == "screenshots"))
            && canonical.starts_with(&instances_dir);
        if !is_screenshot {
            return Err(Error::NotAScreenshot(path.display().to_string()));
        }
        std::fs::remove_file(&canonical)?;
        Ok(CommandOutput::ScreenshotDeleted {
            path: path.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bananium_core::Paths;

    #[tokio::test]
    async fn screenshot_delete_refuses_paths_outside_screenshot_folders() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let session = Session::new(paths.clone(), Config::default()).unwrap();
        bananium_instance::InstanceStore::new(paths.clone())
            .create_named("1.21.1", Some("main"))
            .unwrap();

        let shots = paths.instance_minecraft_dir("main").join("screenshots");
        std::fs::create_dir_all(&shots).unwrap();
        let shot = shots.join("2026-01-01.png");
        std::fs::write(&shot, "png").unwrap();
        let options = paths.instance_minecraft_dir("main").join("options.png");
        std::fs::write(&options, "not a screenshot").unwrap();

        assert!(matches!(
            session.screenshot_delete(&options),
            Err(Error::NotAScreenshot(_))
        ));
        assert!(options.is_file());

        match session.screenshot_list(None).unwrap() {
            CommandOutput::ScreenshotListed { screenshots } => {
                assert_eq!(screenshots.len(), 1);
                assert_eq!(screenshots[0].instance, "main");
            }
            other => panic!("unexpected {other:?}"),
        }
        session.screenshot_delete(&shot).unwrap();
        assert!(!shot.exists());
    }
}
