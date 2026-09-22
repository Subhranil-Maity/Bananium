//! Domain types, layered config, paths, and the error taxonomy shared across the workspace.

pub mod config;
pub mod error;
pub mod logging;
pub mod paths;

pub use config::{Config, ConfigOverrides};
pub use error::{Error, Result};
pub use paths::Paths;
