use std::path::PathBuf;

use bananium_core::Config;
use serde::Serialize;

/// The subset of `Paths` surfaced to frontends, for `bananium config show`.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedPaths {
    pub home: PathBuf,
    pub config_toml: PathBuf,
    pub store_dir: PathBuf,
    pub instances_dir: PathBuf,
    pub java_dir: PathBuf,
    pub assets_dir: PathBuf,
}

/// One instance's summary, as surfaced by `Command::InstanceList` — enough
/// for a frontend's instance list without a separate lookup per row.
#[derive(Debug, Clone, Serialize)]
pub struct InstanceSummary {
    pub slug: String,
    pub name: String,
    pub mc_version: String,
    pub ram_mb: Option<u32>,
    pub jvm_args: Vec<String>,
    /// Whether `bananium_instance::InstanceStore::is_running` currently
    /// sees a live pid recorded for this instance.
    pub running: bool,
}

/// The result of a successfully dispatched `Command`. One variant per
/// `Command` variant.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum CommandOutput {
    ConfigShown {
        paths: ResolvedPaths,
        config: Config,
    },
    Installed {
        instance: String,
        mc_version: String,
    },
    /// `--dry-run`: nothing was executed, this is just the rendering.
    LaunchPlanned {
        instance: String,
        command_line: String,
    },
    Launched {
        instance: String,
        pid: u32,
        /// Where this run's JVM stdout/stderr were redirected — the process
        /// is spawned detached from the frontend's own stdio (see
        /// `Session::launch`'s doc comment), so this is how a caller finds
        /// the output after the fact.
        log_path: PathBuf,
    },
    InstanceListed {
        instances: Vec<InstanceSummary>,
    },
    InstanceUpdated {
        instance: String,
    },
}
