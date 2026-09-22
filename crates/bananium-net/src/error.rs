#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Core(#[from] bananium_core::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("unexpected status {status} for {url}")]
    Status { status: u16, url: String },

    #[error("checksum mismatch for {url}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        url: String,
        expected: String,
        actual: String,
    },

    #[error("json parse error for {url}: {source}")]
    Json {
        url: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("download of {url} failed after all retries")]
    RetriesExhausted { url: String },
}

pub type Result<T> = std::result::Result<T, Error>;
