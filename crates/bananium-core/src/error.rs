use std::path::PathBuf;

/// The shared error taxonomy. Other crates define their own domain-specific
/// error enums and convert into (or wrap) this one rather than growing it
/// indefinitely.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to parse TOML at {path}: {source}")]
    TomlParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("failed to serialize TOML: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("could not determine the platform home directory")]
    NoHomeDir,

    #[error("not found: {0}")]
    NotFound(String),
}

/// Shorthand for `Result<T, bananium_core::Error>`, used throughout this crate.
pub type Result<T> = std::result::Result<T, Error>;
