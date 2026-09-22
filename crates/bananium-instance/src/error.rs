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
}

pub type Result<T> = std::result::Result<T, Error>;
