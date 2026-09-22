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
}

pub type Result<T> = std::result::Result<T, Error>;
