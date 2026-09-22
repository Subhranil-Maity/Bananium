use serde::{Deserialize, Serialize};

/// Every action a frontend can ask for. New variants land milestone by
/// milestone; nothing outside `bananium-api` may add capability that isn't
/// expressed here first (see PLAN.md's frontend contract).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    /// Print the resolved config and paths.
    ConfigShow,
    /// Download everything needed to launch `version` offline afterwards,
    /// creating a minimal instance for it if one doesn't exist yet.
    Install { version: String },
    /// Launch an instance. `instance` is optional only when exactly one is
    /// installed. `profile` selects a named local (offline) profile,
    /// defaulting to the first one saved (or a freshly created "Player").
    Launch {
        #[serde(default)]
        instance: Option<String>,
        #[serde(default)]
        profile: Option<String>,
        #[serde(default)]
        dry_run: bool,
    },
}
