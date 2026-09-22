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
pub use output::{CommandOutput, ResolvedPaths};
pub use session::Session;

// Re-exported so frontends can construct a `Session` without depending on
// bananium-core directly (frontend crates depend only on bananium-api).
pub use bananium_core::{Config, ConfigOverrides, Paths};

/// Initialize `tracing` for a binary frontend. See `bananium_core::logging`.
pub fn init_logging() {
    bananium_core::logging::init();
}
