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
    /// Discord Rich Presence settings (`[discord]` in `config.toml`).
    #[serde(default)]
    pub discord: DiscordConfig,
}

/// What the Discord member list shows next to the user's name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum StatusDisplay {
    /// "Playing Bananium" (Discord's default).
    #[default]
    Name,
    /// The activity's first line, e.g. "Playing Minecraft 1.21.1".
    Details,
    /// The activity's second line, e.g. "Fabric · 49 mods".
    State,
}

/// Discord Rich Presence settings. Everything is shown by default; each
/// switch hides one detail. The struct is `serde(default)` so a partial
/// `[discord]` table only overrides what it mentions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default)]
pub struct DiscordConfig {
    /// Master switch: off clears the presence and disconnects from Discord.
    pub enabled: bool,
    /// Show an activity while no game is running (browsing, installing,
    /// idle in the launcher); off means presence only while playing.
    pub show_in_launcher: bool,
    /// Say what's being browsed on Modrinth, down to the project viewed.
    pub show_browsing: bool,
    /// Show installs and downloads, with a progress bar.
    pub show_tasks: bool,
    pub show_version: bool,
    pub show_loader: bool,
    pub show_loader_version: bool,
    pub show_mod_count: bool,
    /// The offline username being played as.
    pub show_username: bool,
    /// The instance's name (in the large image's tooltip).
    pub show_instance_name: bool,
    /// Put the instance name in the status line itself ("Playing My
    /// Survival") instead of "Playing Minecraft 1.21.1".
    pub instance_name_in_status: bool,
    /// A Modrinth modpack's own icon as the large image.
    pub show_modpack_icon: bool,
    /// The "elapsed" timer.
    pub show_elapsed: bool,
    /// "View modpack" / "Get Bananium" link buttons.
    pub show_buttons: bool,
    pub status_display: StatusDisplay,
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            show_in_launcher: true,
            show_browsing: true,
            show_tasks: true,
            show_version: true,
            show_loader: true,
            show_loader_version: true,
            show_mod_count: true,
            show_username: true,
            show_instance_name: true,
            instance_name_in_status: false,
            show_modpack_icon: true,
            show_elapsed: true,
            show_buttons: true,
            status_display: StatusDisplay::Name,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_concurrent_downloads: 8,
            theme: "banana".to_string(),
            java_path: None,
            discord: DiscordConfig::default(),
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
    discord: Option<DiscordConfig>,
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
            if let Some(v) = file.discord {
                cfg.discord = v;
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
        let mut table = read_table(paths)?;
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
        write_table(paths, &table)?;
        Self::load(paths, ConfigOverrides::default())
    }

    /// Replace the `[discord]` table in `config.toml`, keeping every other
    /// key, and return the re-loaded config.
    pub fn update_discord(paths: &Paths, discord: &DiscordConfig) -> Result<Self> {
        let mut table = read_table(paths)?;
        let value = toml::Value::try_from(discord)
            .map_err(|e| Error::Config(format!("failed to encode [discord]: {e}")))?;
        table.insert("discord".into(), value);
        write_table(paths, &table)?;
        Self::load(paths, ConfigOverrides::default())
    }
}

/// `config.toml` as a raw table (empty when the file doesn't exist), so an
/// update can preserve keys this version doesn't know about.
fn read_table(paths: &Paths) -> Result<toml::Table> {
    let path = paths.config_toml();
    if !path.is_file() {
        return Ok(toml::Table::new());
    }
    let text = std::fs::read_to_string(&path)?;
    toml::from_str(&text).map_err(|source| Error::TomlParse {
        path,
        source: Box::new(source),
    })
}

fn write_table(paths: &Paths, table: &toml::Table) -> Result<()> {
    let text = toml::to_string_pretty(table)
        .map_err(|e| Error::Config(format!("failed to write config.toml: {e}")))?;
    std::fs::write(paths.config_toml(), text)?;
    Ok(())
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

    #[test]
    fn discord_table_round_trips_and_partial_tables_use_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        std::fs::write(
            paths.config_toml(),
            "theme = \"midnight\"\n[discord]\nshow_username = false\n",
        )
        .unwrap();
        let cfg = Config::load(&paths, ConfigOverrides::default()).unwrap();
        assert!(!cfg.discord.show_username);
        assert!(cfg.discord.enabled, "unmentioned keys keep their defaults");

        let wanted = DiscordConfig {
            enabled: false,
            status_display: StatusDisplay::Details,
            ..DiscordConfig::default()
        };
        let cfg = Config::update_discord(&paths, &wanted).unwrap();
        assert_eq!(cfg.discord, wanted);
        assert_eq!(cfg.theme, "midnight");

        // Other updates leave [discord] alone.
        let cfg = Config::update_file(&paths, Some(3), None).unwrap();
        assert_eq!(cfg.discord, wanted);
    }
}
