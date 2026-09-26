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

    /// Modrinth didn't answer within the request timeout, on the last
    /// attempt. Split out from [`Error::Http`] so a frontend can say
    /// "Modrinth didn't respond" instead of showing a raw transport error.
    #[error("Modrinth didn't respond in time ({url})")]
    Timeout { url: String },

    /// No connection to Modrinth could be made at all (DNS, refused, no
    /// route) on the last attempt — usually means the machine is offline.
    #[error("couldn't reach Modrinth ({url}): {source}")]
    Unreachable {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    /// Every retry attempt for a request failed with a transport error
    /// (never a clean non-2xx response, which surfaces as [`Error::Status`]
    /// instead).
    #[error("request to {url} failed after all retries")]
    RetriesExhausted { url: String },
}

/// This crate's `Result` alias, per the workspace convention.
pub type Result<T> = std::result::Result<T, Error>;
