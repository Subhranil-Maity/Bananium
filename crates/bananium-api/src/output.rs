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
    },
}
