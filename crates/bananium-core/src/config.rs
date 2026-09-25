use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::paths::Paths;

/// Resolved configuration: defaults -> `config.toml` -> environment ->
/// caller-supplied overrides (typically CLI flags). Each layer only
/// overwrites the fields it actually sets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Config {
    /// Upper bound on simultaneous downloads (the download engine's
    /// `Semaphore` size); default 8. Peak memory scales with this, not with
    /// queue length.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
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
                source: Box::new(source),
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

    /// Persist settings to `config.toml`, touching only the keys given
    /// (`Some`) and preserving everything else in the file, including keys
    /// this version doesn't know about. `java_path: Some(None)` removes the
    /// key (back to auto-detection). Returns the freshly re-loaded config —
    /// which environment variables can still override, so the caller shows
    /// the value that will actually be used.
    pub fn update_file(
        paths: &Paths,
        max_concurrent_downloads: Option<usize>,
        java_path: Option<Option<PathBuf>>,
    ) -> Result<Self> {
        let path = paths.config_toml();
        let mut table: toml::Table = if path.is_file() {
            let text = std::fs::read_to_string(&path)?;
            toml::from_str(&text).map_err(|source| Error::TomlParse {
                path: path.clone(),
                source: Box::new(source),
            })?
        } else {
            toml::Table::new()
        };
        if let Some(n) = max_concurrent_downloads {
            if n == 0 {
                return Err(Error::Config(
                    "max_concurrent_downloads must be at least 1".into(),
                ));
            }
            table.insert(
                "max_concurrent_downloads".into(),
                toml::Value::Integer(n as i64),
            );
        }
        match java_path {
            Some(Some(p)) => {
                table.insert(
                    "java_path".into(),
                    toml::Value::String(p.to_string_lossy().into_owned()),
                );
            }
            Some(None) => {
                table.remove("java_path");
            }
            None => {}
        }
        let text = toml::to_string_pretty(&table)
            .map_err(|e| Error::Config(format!("failed to write config.toml: {e}")))?;
        std::fs::write(&path, text)?;
        Self::load(paths, ConfigOverrides::default())
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

    #[test]
    fn update_file_changes_only_given_keys() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        std::fs::write(
            paths.config_toml(),
            "theme = \"midnight\"\nfuture_key = 1\n",
        )
        .unwrap();

        let cfg = Config::update_file(&paths, Some(4), Some(Some("/opt/java".into()))).unwrap();
        assert_eq!(cfg.max_concurrent_downloads, 4);
        assert_eq!(
            cfg.java_path.as_deref(),
            Some(std::path::Path::new("/opt/java"))
        );
        assert_eq!(cfg.theme, "midnight");
        let text = std::fs::read_to_string(paths.config_toml()).unwrap();
        assert!(text.contains("future_key"));

        let cfg = Config::update_file(&paths, None, Some(None)).unwrap();
        assert_eq!(cfg.java_path, None);
        assert_eq!(cfg.max_concurrent_downloads, 4);
        assert!(Config::update_file(&paths, Some(0), None).is_err());
    }
}
