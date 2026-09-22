#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("blob {0} is not present in the store")]
    MissingBlob(String),
}

pub type Result<T> = std::result::Result<T, Error>;
