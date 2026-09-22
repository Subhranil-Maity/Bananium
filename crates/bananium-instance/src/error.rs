#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to parse instance.toml: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("failed to write instance.toml: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("instance {0:?} not found")]
    NotFound(String),

    #[error("multiple instances installed ({0:?}); specify which one to launch")]
    Ambiguous(Vec<String>),

    #[error(
        "instance name {0:?} must be non-empty and contain only letters, digits, '-', and '_'"
    )]
    InvalidName(String),

    #[error("an instance named {0:?} already exists for a different Minecraft version")]
    NameInUse(String),
}

pub type Result<T> = std::result::Result<T, Error>;
