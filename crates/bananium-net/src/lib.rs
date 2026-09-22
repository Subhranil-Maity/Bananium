//! HTTP client, concurrent resumable download engine, checksum verification, rate limiting.

pub mod client;
pub mod download;
pub mod error;

pub use client::HttpClient;
pub use download::{hash_file, no_progress, DownloadSpec, Downloader, Progress, ProgressFn};
pub use error::{Error, Result};
