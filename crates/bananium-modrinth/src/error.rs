//! This crate's error taxonomy, following the workspace convention of one
//! `thiserror` enum and `Result` alias per crate rather than a shared type.

/// Everything that can go wrong talking to the Modrinth API.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A transport-level failure (DNS, TLS, connect, a body that couldn't
    /// be read) — never a non-2xx response, which is [`Error::Status`]
    /// instead, so callers can tell "the network is down" apart from "the
    /// server said no."
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// A response Modrinth returned outside 2xx, either not retryable
    /// (e.g. a 404) or one that stayed unsuccessful after every retry
    /// attempt was exhausted.
    #[error("modrinth returned {status} for {url}")]
    Status { status: u16, url: String },

    /// The response body didn't deserialize as the JSON shape expected for
    /// this endpoint — most likely means Modrinth's API shape has drifted
    /// from what this client models.
    #[error("failed to parse response from {url}: {source}")]
    Json {
        url: String,
        #[source]
        source: serde_json::Error,
    },

    /// Every retry attempt for a request failed with a transport error
    /// (never a clean non-2xx response, which surfaces as [`Error::Status`]
    /// instead).
    #[error("request to {url} failed after all retries")]
    RetriesExhausted { url: String },
}

/// This crate's `Result` alias, per the workspace convention.
pub type Result<T> = std::result::Result<T, Error>;
