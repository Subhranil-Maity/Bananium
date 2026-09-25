#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Store(#[from] bananium_store::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to read native library archive: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("no java runtime is configured or was found on PATH")]
    NoJavaFound,

    #[error("failed to parse profiles.toml: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("failed to write profiles.toml: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("invalid username {0:?}: use 3-16 letters, digits, or '_'")]
    InvalidPlayerName(String),

    #[error("a profile named {0:?} already exists")]
    ProfileExists(String),

    #[error("no profile named {0:?}")]
    ProfileNotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;
