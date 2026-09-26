//! The frontend facade: `Session`, `Command`, `Event`, and the task registry.
//! Every frontend depends only on this crate.

pub mod command;
pub mod error;
pub mod event;
pub mod output;
pub mod presence;
pub mod session;
pub mod task;

pub use command::{Command, ModpackSource, SearchSort};
pub use error::{Error, Result};
pub use event::Event;
pub use output::{
    CommandOutput, ContentUpdateInfo, FabricLoaderSummary, FileEntry, GalleryItem, InstanceSummary,
    JavaInstall, LauncherLogFile, LogFile, LogStatus, ModpackSummary, ModrinthHit, ModrinthProject,
    ModrinthVersion, ProfileSummary, ResolvedPaths, Screenshot, SkippedEntry, VersionSummary,
};
pub use presence::{
    LauncherView, PresencePreview, PresenceStatus, PreviewButton, PreviewScenario, DISCORD_APP_ID,
    RETRY_INTERVAL_SECS,
};
pub use session::Session;
pub use task::{TaskInfo, TaskKind, TaskState};

// Re-exported so frontends can construct a `Session` without depending on
// bananium-core directly (frontend crates depend only on bananium-api).
pub use bananium_core::{Config, ConfigOverrides, DiscordConfig, Paths, StatusDisplay};

/// Re-exported so a frontend can validate a user-typed instance name (and
/// re-prompt immediately on a bad one) without round-tripping through
/// `Session::dispatch`, while still depending on only `bananium-api` per
/// the frontend contract — `bananium_instance::InstanceStore::create_named`
/// is the actual source of truth this mirrors.
pub use bananium_instance::is_valid_name as is_valid_instance_name;

/// Content types that appear in `Command`/`CommandOutput`, re-exported so
/// frontends can name them while depending on only `bananium-api`.
pub use bananium_instance::{ContentEntry, ContentKind, ModrinthStatus, Preset, PresetEntry};

pub use bananium_core::logging::LogOptions;

/// Initialize `tracing` for a binary frontend: this run's log file under
/// `<home>/logs`, the panic hook, and optionally stderr. Returns the log
/// file's path. See `bananium_core::logging`.
pub fn init_logging(paths: &Paths, opts: LogOptions) -> Option<std::path::PathBuf> {
    bananium_core::logging::init(paths, opts)
}

/// Log the clean-shutdown marker. Frontends call this right before exiting
/// on purpose; a log without it is reported as a crash or unclean exit.
pub fn log_shutdown(reason: &str) {
    bananium_core::logging::shutdown(reason);
}
