//! Instance model, lockfile, mod graph, and import/export formats.
//!
//! M1 only needs a minimal slice of this: a vanilla instance is just a
//! version pin and a game directory. `instance new|clone|rm|...`, groups,
//! and the mod lockfile arrive in M3/M4.

pub mod error;

use std::path::PathBuf;

use bananium_core::Paths;
use serde::{Deserialize, Serialize};

pub use error::{Error, Result};

/// Which mod loader an instance uses. Only `Vanilla` exists until Fabric/Quilt
/// (M5) and Forge/NeoForge (M6) land.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Loader {
    #[default]
    Vanilla,
}

/// The contents of an instance's `instance.toml`: enough to know what
/// version to launch and how. The mod lockfile (`bananium.lock.toml`) is a
/// separate file, owned by M4's mod-management code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceConfig {
    pub name: String,
    pub mc_version: String,
    #[serde(default)]
    pub loader: Loader,
    /// Per-instance Java override; falls back to `Config::java_path`, then
    /// auto-detection, when absent.
    #[serde(default)]
    pub java_path: Option<PathBuf>,
    /// Extra JVM arguments appended after the profile's own (M3: JVM
    /// profiles/RAM auto-sizing; unused so far).
    #[serde(default)]
    pub jvm_args: Vec<String>,
}

/// Reads and writes `instance.toml` files under `Paths::instances_dir()`.
pub struct InstanceStore {
    paths: Paths,
}

impl InstanceStore {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    /// A filesystem-safe slug for a raw Minecraft version id like "1.21.1".
    pub fn slugify(mc_version: &str) -> String {
        mc_version
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    /// Create a minimal vanilla instance for `mc_version` if one doesn't
    /// already exist, returning its slug either way.
    pub fn ensure_vanilla(&self, mc_version: &str) -> Result<String> {
        let slug = Self::slugify(mc_version);
        if !self.paths.instance_toml(&slug).is_file() {
            let cfg = InstanceConfig {
                name: mc_version.to_string(),
                mc_version: mc_version.to_string(),
                loader: Loader::Vanilla,
                java_path: None,
                jvm_args: Vec::new(),
            };
            self.save(&slug, &cfg)?;
        }
        std::fs::create_dir_all(self.paths.instance_minecraft_dir(&slug))?;
        Ok(slug)
    }

    /// Write `cfg` to `<slug>/instance.toml`, creating the instance
    /// directory if it doesn't exist yet.
    pub fn save(&self, slug: &str, cfg: &InstanceConfig) -> Result<()> {
        std::fs::create_dir_all(self.paths.instance_dir(slug))?;
        std::fs::write(self.paths.instance_toml(slug), toml::to_string_pretty(cfg)?)?;
        Ok(())
    }

    /// Read `<slug>/instance.toml`, erroring with [`Error::NotFound`] if it
    /// doesn't exist (rather than a raw io error).
    pub fn load(&self, slug: &str) -> Result<InstanceConfig> {
        let path = self.paths.instance_toml(slug);
        let text = std::fs::read_to_string(&path).map_err(|_| Error::NotFound(slug.to_string()))?;
        Ok(toml::from_str(&text)?)
    }

    /// Every installed instance's slug, sorted.
    pub fn list(&self) -> Result<Vec<String>> {
        let dir = self.paths.instances_dir();
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut slugs = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    slugs.push(name.to_string());
                }
            }
        }
        slugs.sort();
        Ok(slugs)
    }

    /// Resolve an optional CLI-supplied instance name: the name itself if
    /// given, otherwise the sole installed instance, otherwise an error
    /// asking the caller to disambiguate.
    pub fn resolve(&self, requested: Option<&str>) -> Result<String> {
        match requested {
            Some(slug) => {
                if self.paths.instance_toml(slug).is_file() {
                    Ok(slug.to_string())
                } else {
                    Err(Error::NotFound(slug.to_string()))
                }
            }
            None => {
                let mut slugs = self.list()?;
                match slugs.len() {
                    0 => Err(Error::NotFound("<none installed yet>".into())),
                    1 => Ok(slugs.remove(0)),
                    _ => Err(Error::Ambiguous(slugs)),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_vanilla_is_idempotent_and_creates_the_game_dir() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths.clone());

        let slug = store.ensure_vanilla("1.21.1").unwrap();
        assert_eq!(slug, "1.21.1");
        assert!(paths.instance_minecraft_dir(&slug).is_dir());

        let again = store.ensure_vanilla("1.21.1").unwrap();
        assert_eq!(again, slug);
        let cfg = store.load(&slug).unwrap();
        assert_eq!(cfg.mc_version, "1.21.1");
        assert_eq!(cfg.loader, Loader::Vanilla);
    }

    #[test]
    fn resolve_picks_the_only_instance() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths);
        store.ensure_vanilla("1.20.1").unwrap();
        assert_eq!(store.resolve(None).unwrap(), "1.20.1");
    }

    #[test]
    fn resolve_is_ambiguous_with_multiple_instances() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths);
        store.ensure_vanilla("1.20.1").unwrap();
        store.ensure_vanilla("1.21.1").unwrap();
        assert!(matches!(store.resolve(None), Err(Error::Ambiguous(_))));
        assert_eq!(store.resolve(Some("1.20.1")).unwrap(), "1.20.1");
    }
}
