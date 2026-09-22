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
    /// creating an instance for it named `name` (or a fresh random name
    /// when omitted) if one doesn't already exist under that name.
    /// Distinct names let several instances share the same `version`.
    Install {
        version: String,
        #[serde(default)]
        name: Option<String>,
    },
    /// Launch an instance. `instance` is optional only when exactly one is
    /// installed. `profile` selects a named local (offline) profile,
    /// defaulting to the first one saved (or a freshly created "Player").
    /// Refused (outside `dry_run`) when the instance already has a live
    /// pid recorded — only one process per instance at a time, for now.
    Launch {
        #[serde(default)]
        instance: Option<String>,
        #[serde(default)]
        profile: Option<String>,
        #[serde(default)]
        dry_run: bool,
    },
    /// Every installed instance, for a frontend's instance list.
    InstanceList,
    /// Update an instance's RAM cap and/or extra JVM arguments; either
    /// field left as `None` here leaves that setting untouched.
    InstanceSet {
        instance: String,
        /// `Some(0)` clears the cap back to the JVM default (a real 0 MB
        /// cap isn't meaningful, so it doubles as the "unset" sentinel
        /// rather than needing a nested `Option`); `Some(n>0)` sets it;
        /// `None` leaves the current value alone.
        #[serde(default)]
        ram_mb: Option<u32>,
        /// Replaces the stored extra-JVM-args list entirely when given
        /// (an empty `Vec` clears it); `None` leaves the current list
        /// alone.
        #[serde(default)]
        jvm_args: Option<Vec<String>>,
    },
}
