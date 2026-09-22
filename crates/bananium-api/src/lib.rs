//! The frontend facade: `Session`, `Command`, `Event`, and the task registry.
//! Every frontend depends only on this crate.

pub mod command;
pub mod error;
pub mod event;
pub mod output;
pub mod session;

pub use command::Command;
pub use error::{Error, Result};
pub use event::Event;
pub use output::{CommandOutput, InstanceSummary, ResolvedPaths};
pub use session::Session;

// Re-exported so frontends can construct a `Session` without depending on
// bananium-core directly (frontend crates depend only on bananium-api).
pub use bananium_core::{Config, ConfigOverrides, Paths};

/// Re-exported so a frontend can validate a user-typed instance name (and
/// re-prompt immediately on a bad one) without round-tripping through
/// `Session::dispatch`, while still depending on only `bananium-api` per
/// the frontend contract — `bananium_instance::InstanceStore::create_named`
/// is the actual source of truth this mirrors.
pub use bananium_instance::is_valid_name as is_valid_instance_name;

/// Initialize `tracing` for a binary frontend. See `bananium_core::logging`.
pub fn init_logging() {
    bananium_core::logging::init();
}
