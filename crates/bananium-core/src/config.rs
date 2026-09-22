use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::paths::Paths;

/// Resolved configuration: defaults -> `config.toml` -> environment ->
/// caller-supplied overrides (typically CLI flags). Each layer only
/// overwrites the fields it actually sets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Upper bound on simultaneous downloads (the download engine's
    /// `Semaphore` size); default 8. Peak memory scales with this, not with
    /// queue length.
    pub max_concurrent_downloads: usize,
    /// TUI theme name; themes are TOML (M2+, not yet implemented).
    pub theme: String,
    /// Explicit JVM path, overriding auto-detection in `bananium_java::find_java`.
    pub java_path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_concurrent_downloads: 8,
            theme: "banana".to_string(),
            java_path: None,
        }
    }
}

/// The subset of `Config` that may appear in `config.toml`; every field is
/// optional so a partial file only overrides what it mentions.
#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
    max_concurrent_downloads: Option<usize>,
    theme: Option<String>,
    java_path: Option<PathBuf>,
}

/// The subset of `Config` a frontend may override directly (e.g. via CLI
/// flags), applied after `config.toml` and the environment. `None` means
/// "don't override this field."
#[derive(Debug, Default, Clone)]
pub struct ConfigOverrides {
    pub max_concurrent_downloads: Option<usize>,
    pub theme: Option<String>,
    pub java_path: Option<PathBuf>,
}

impl Config {
    pub fn load(paths: &Paths, overrides: ConfigOverrides) -> Result<Self> {
        let mut cfg = Config::default();

        let toml_path = paths.config_toml();
        if toml_path.is_file() {
            let text = std::fs::read_to_string(&toml_path)?;
            let file: ConfigFile = toml::from_str(&text).map_err(|source| Error::TomlParse {
                path: toml_path.clone(),
                source,
            })?;
            if let Some(v) = file.max_concurrent_downloads {
                cfg.max_concurrent_downloads = v;
            }
            if let Some(v) = file.theme {
                cfg.theme = v;
            }
            if let Some(v) = file.java_path {
                cfg.java_path = Some(v);
            }
        }

        if let Ok(v) = std::env::var("BANANIUM_MAX_CONCURRENT_DOWNLOADS") {
            match v.parse() {
                Ok(n) => cfg.max_concurrent_downloads = n,
                Err(_) => {
                    return Err(Error::Config(format!(
                        "BANANIUM_MAX_CONCURRENT_DOWNLOADS={v:?} is not a valid number"
                    )))
                }
            }
        }
        if let Ok(v) = std::env::var("BANANIUM_THEME") {
            cfg.theme = v;
        }
        if let Ok(v) = std::env::var("BANANIUM_JAVA_PATH") {
            cfg.java_path = Some(PathBuf::from(v));
        }

        if let Some(v) = overrides.max_concurrent_downloads {
            cfg.max_concurrent_downloads = v;
        }
        if let Some(v) = overrides.theme {
            cfg.theme = v;
        }
        if let Some(v) = overrides.java_path {
            cfg.java_path = Some(v);
        }

        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_nothing_present() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let cfg = Config::load(&paths, ConfigOverrides::default()).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn config_toml_overrides_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        std::fs::write(paths.config_toml(), "theme = \"midnight\"\n").unwrap();
        let cfg = Config::load(&paths, ConfigOverrides::default()).unwrap();
        assert_eq!(cfg.theme, "midnight");
        assert_eq!(cfg.max_concurrent_downloads, 8);
    }

    #[test]
    fn overrides_win_over_everything() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        std::fs::write(paths.config_toml(), "theme = \"midnight\"\n").unwrap();
        let overrides = ConfigOverrides {
            theme: Some("banana".into()),
            ..Default::default()
        };
        let cfg = Config::load(&paths, overrides).unwrap();
        assert_eq!(cfg.theme, "banana");
    }
}
