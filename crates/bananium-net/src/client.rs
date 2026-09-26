use serde::de::DeserializeOwned;

use crate::error::{Error, Result};

/// The longest a response may go without delivering a single byte before
/// it's treated as stalled and fails (then retried by whoever made it).
pub const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Thin wrapper around a single shared `reqwest::Client`. Every crate that
/// needs HTTP shares one of these rather than constructing its own —
/// connection pooling and the concurrency budget both depend on that.
#[derive(Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
}

impl HttpClient {
    /// Build a client tagged with `user_agent`. Deliberately has no blanket
    /// *total* request timeout: this client is shared with the download
    /// engine, where a multi-hundred-MB file can legitimately take longer
    /// than any fixed cap. It does have a connect timeout and a per-read
    /// timeout ([`READ_TIMEOUT`]), which bound a *stalled* connection
    /// without capping a slow-but-moving one. [`HttpClient::get_bytes`]
    /// (small metadata only) applies its own total timeout on top of this.
    pub fn new(user_agent: &str) -> Result<Self> {
        let inner = reqwest::Client::builder()
            .user_agent(user_agent)
            // Fail fast when there's genuinely no network, so the
            // offline-cache fallback in `MetaClient` kicks in quickly
            // instead of after a long OS-level connect stall.
            .connect_timeout(std::time::Duration::from_secs(10))
            // A body stream that stops sending bytes would otherwise wait
            // forever: no error ever surfaces, so the downloader's
            // resume-and-retry never gets a chance to run.
            .read_timeout(READ_TIMEOUT)
            .build()?;
        Ok(Self { inner })
    }

    /// Escape hatch to the underlying `reqwest::Client`, e.g. to hand it to
    /// [`crate::Downloader`] so downloads share the same connection pool.
    pub fn inner(&self) -> &reqwest::Client {
        &self.inner
    }

    /// GET `url` and deserialize the body as JSON.
    pub async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let bytes = self.get_bytes(url).await?;
        serde_json::from_slice(&bytes).map_err(|source| Error::Json {
            url: url.to_string(),
            source,
        })
    }

    /// GET `url` and return the raw response body under a fixed 30s total
    /// timeout. Used directly (rather than via [`HttpClient::get_json`]) by
    /// callers that need the exact bytes they received to cache verbatim,
    /// e.g. `bananium_meta::MetaClient`'s offline mirror.
    pub async fn get_bytes(&self, url: &str) -> Result<bytes::Bytes> {
        let response = self
            .inner
            .get(url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Status {
                status: status.as_u16(),
                url: url.to_string(),
            });
        }
        Ok(response.bytes().await?)
    }
}
