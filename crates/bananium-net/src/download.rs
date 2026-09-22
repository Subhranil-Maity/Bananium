use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use sha1::{Digest, Sha1};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Semaphore;

use crate::error::{Error, Result};

/// One file to fetch and verify. `dest` is the *final* path; the engine
/// downloads to a sibling `.part` file first and only renames it into place
/// once the checksum (when given) is confirmed.
#[derive(Debug, Clone)]
pub struct DownloadSpec {
    pub url: String,
    /// Final on-disk location. Must be the real destination, not a temp
    /// path — the engine derives its working `.part` file from this.
    pub dest: PathBuf,
    /// SHA-1 to verify against once the download completes. `None` skips
    /// verification entirely (used only for content that has no published
    /// hash to check against).
    pub expected_sha1: Option<String>,
    /// Expected final size, used both as a fast pre-check against an
    /// already-cached file and as a fallback `Content-Length` for progress
    /// reporting when the server doesn't send one.
    pub expected_size: Option<u64>,
    /// Opaque id surfaced on every [`Progress`] event for this download, so
    /// a frontend can track it across resumes/retries.
    pub task_id: String,
    /// Human-readable label surfaced alongside progress (e.g. a mod jar's
    /// display name).
    pub label: String,
}

/// A single progress update for one in-flight (or just-verified) download.
#[derive(Debug, Clone)]
pub struct Progress {
    pub task_id: String,
    pub label: String,
    /// Bytes written so far, *including* any bytes that were already on
    /// disk from a resumed partial download.
    pub bytes_done: u64,
    /// Total expected size, if known (from the response's `Content-Length`
    /// or the spec's `expected_size`).
    pub bytes_total: Option<u64>,
    /// Bytes/sec measured over *this attempt* (resets on retry, and
    /// excludes any bytes a resume started from), so it reflects the actual
    /// current transfer rate rather than an average skewed by a resumed
    /// offset or an earlier slow/failed attempt. `0.0` for the instant
    /// already-verified fast path, where nothing was actually transferred.
    pub bytes_per_sec: f64,
}

/// Callback invoked with each [`Progress`] update. Boxed and cloneable so
/// the same sink can be shared across every concurrently in-flight download.
pub type ProgressFn = Arc<dyn Fn(Progress) + Send + Sync>;

/// A [`ProgressFn`] that discards every update — handy in tests and
/// anywhere progress reporting genuinely doesn't matter.
pub fn no_progress() -> ProgressFn {
    Arc::new(|_| {})
}

/// Bounded-concurrency, resumable, checksum-verifying download engine.
/// Peak memory scales with `concurrency * chunk size`, not with queue
/// length, because each in-flight download streams straight to disk.
pub struct Downloader {
    client: reqwest::Client,
    semaphore: Arc<Semaphore>,
    /// Total attempts (including the first) before a download gives up.
    max_attempts: u32,
}

impl Downloader {
    /// `concurrency` is clamped to at least 1 and becomes the `Semaphore`
    /// size bounding simultaneous in-flight downloads.
    pub fn new(client: reqwest::Client, concurrency: usize) -> Self {
        Self {
            client,
            semaphore: Arc::new(Semaphore::new(concurrency.max(1))),
            max_attempts: 5,
        }
    }

    /// Download a single spec, honoring this downloader's concurrency limit
    /// and retry policy. A no-op (aside from one progress event) if `dest`
    /// already exists and verifies against `expected_sha1`/`expected_size`.
    pub async fn download(&self, spec: &DownloadSpec, on_progress: ProgressFn) -> Result<()> {
        let _permit = self.semaphore.acquire().await.expect("semaphore closed");
        download_one(&self.client, spec, self.max_attempts, &on_progress).await
    }

    /// Download every spec, bounded by this downloader's concurrency limit.
    /// Returns one `Result` per spec, in the same order they were given, so
    /// a caller can tell exactly which files failed without the others being
    /// aborted.
    pub async fn download_all(
        &self,
        specs: Vec<DownloadSpec>,
        on_progress: ProgressFn,
    ) -> Vec<Result<()>> {
        let mut set = tokio::task::JoinSet::new();
        for (idx, spec) in specs.into_iter().enumerate() {
            let client = self.client.clone();
            let sem = self.semaphore.clone();
            let progress = on_progress.clone();
            let max_attempts = self.max_attempts;
            set.spawn(async move {
                // The permit is acquired *inside* the spawned task, not
                // before spawning it, so every task can be queued up front
                // while only `concurrency` of them actually run at once.
                let _permit = sem.acquire_owned().await.expect("semaphore closed");
                (
                    idx,
                    download_one(&client, &spec, max_attempts, &progress).await,
                )
            });
        }

        let mut collected = Vec::new();
        while let Some(joined) = set.join_next().await {
            collected.push(joined.expect("download task panicked"));
        }
        // `JoinSet` yields tasks in completion order, not spawn order —
        // sort back to spawn order so callers can zip results with specs.
        collected.sort_by_key(|(idx, _)| *idx);
        collected.into_iter().map(|(_, res)| res).collect()
    }
}

/// True if `path` already exists and matches `expected_size` /
/// `expected_sha1` (when given). Used both for `spec.dest` (the fast path
/// that makes repeat `install` runs, and every offline run, avoid the
/// network entirely) and for a `.part` file that might already be a
/// complete, correct download sitting there unrenamed — see
/// [`download_one`].
async fn file_verifies(
    path: &Path,
    expected_size: Option<u64>,
    expected_sha1: Option<&str>,
) -> bool {
    let Ok(meta) = tokio::fs::metadata(path).await else {
        return false;
    };
    if let Some(expected_size) = expected_size {
        if meta.len() != expected_size {
            return false;
        }
    }
    match expected_sha1 {
        Some(expected) => matches!(hash_file(path).await, Ok(actual) if actual == expected),
        None => true,
    }
}

/// The full single-file download lifecycle: skip if already verified,
/// otherwise retry up to `max_attempts` times with exponential backoff,
/// verifying the checksum (if any) before the final atomic rename into
/// place. A checksum mismatch discards the bad data and retries clean
/// rather than resuming from a possibly-corrupt partial file.
async fn download_one(
    client: &reqwest::Client,
    spec: &DownloadSpec,
    max_attempts: u32,
    on_progress: &ProgressFn,
) -> Result<()> {
    if file_verifies(
        &spec.dest,
        spec.expected_size,
        spec.expected_sha1.as_deref(),
    )
    .await
    {
        on_progress(Progress {
            task_id: spec.task_id.clone(),
            label: spec.label.clone(),
            bytes_done: spec.expected_size.unwrap_or(0),
            bytes_total: spec.expected_size,
            bytes_per_sec: 0.0,
        });
        return Ok(());
    }

    if let Some(parent) = spec.dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let tmp_path = tmp_path_for(&spec.dest);

    // A `.part` file can be a complete, byte-correct download that never
    // got renamed — e.g. a previous run was killed between finishing the
    // write and committing it. Check before touching the network at all:
    // if it already verifies, just commit it, for free (no re-hash needed
    // — `file_verifies` already did that).
    if file_verifies(&tmp_path, spec.expected_size, spec.expected_sha1.as_deref()).await {
        tokio::fs::rename(&tmp_path, &spec.dest).await?;
        return Ok(());
    }

    let mut last_err: Option<Error> = None;

    for attempt in 0..max_attempts {
        if attempt > 0 {
            // 200ms, 400ms, 800ms, ... capped at 2^6 multiplier so a flaky
            // connection doesn't turn into a multi-minute stall.
            let backoff = Duration::from_millis(200 * 2u64.pow(attempt.min(6)));
            tracing::warn!(url = %spec.url, attempt, ?backoff, "retrying download");
            tokio::time::sleep(backoff).await;
        }

        match try_download_once(client, spec, &tmp_path, on_progress).await {
            Ok(()) => match verify_and_commit(spec, &tmp_path).await {
                Ok(()) => return Ok(()),
                Err(err) => last_err = Some(err),
            },
            Err(err) => last_err = Some(err),
        }
    }

    Err(last_err.unwrap_or(Error::RetriesExhausted {
        url: spec.url.clone(),
    }))
}

/// Verify the completed `.part` file's checksum (if `expected_sha1` was
/// given) and atomically rename it into its final location. On a mismatch,
/// the `.part` file is deleted so the *next* attempt starts from scratch
/// instead of resuming corrupt bytes.
async fn verify_and_commit(spec: &DownloadSpec, tmp_path: &Path) -> Result<()> {
    if let Some(expected) = &spec.expected_sha1 {
        let actual = hash_file(tmp_path).await?;
        if &actual != expected {
            let _ = tokio::fs::remove_file(tmp_path).await;
            return Err(Error::ChecksumMismatch {
                url: spec.url.clone(),
                expected: expected.clone(),
                actual,
            });
        }
    }
    tokio::fs::rename(tmp_path, &spec.dest).await?;
    Ok(())
}

/// The working path a download streams into before being verified and
/// atomically renamed to `dest`. Named `<dest-filename>.part`, sitting next
/// to `dest` so the final rename is guaranteed to stay on the same
/// filesystem (no cross-device rename failure).
fn tmp_path_for(dest: &Path) -> PathBuf {
    let file_name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    dest.with_file_name(format!("{file_name}.part"))
}

/// One attempt at streaming `spec.url` into `tmp_path`. If a partial
/// `.part` file already exists (from an interrupted previous attempt),
/// sends a `Range: bytes=N-` request to resume it; if the server responds
/// `200 OK` instead of `206 Partial Content` (i.e. it doesn't support or
/// honored the Range request), the attempt restarts from byte 0 rather than
/// silently appending mismatched data. Does not verify the checksum — that
/// happens once in [`verify_and_commit`], after this returns.
async fn try_download_once(
    client: &reqwest::Client,
    spec: &DownloadSpec,
    tmp_path: &Path,
    on_progress: &ProgressFn,
) -> Result<()> {
    let mut existing = tokio::fs::metadata(tmp_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    // A `.part` file at or past the expected size can never be resumed —
    // the server will (rightfully) reject `Range: bytes=<existing>-` with
    // 416, and since nothing about that response changes `existing`, every
    // subsequent attempt would send the exact same invalid request and get
    // stuck failing identically forever. (This is reachable in practice:
    // an interrupted write, a killed process, or anything else that leaves
    // a `.part` file that doesn't correspond to a valid resume point.)
    // `download_one` already checked whether this exact file is a
    // *correct* complete download before calling here, so if we get this
    // far and it's still this size, it's not just complete — it's wrong.
    if existing > 0 && spec.expected_size.is_some_and(|size| existing >= size) {
        tracing::warn!(
            path = %tmp_path.display(), existing, expected = ?spec.expected_size,
            "discarding a .part file that can't be resumed (at or past the expected size but failed checksum)"
        );
        tokio::fs::remove_file(tmp_path).await.ok();
        existing = 0;
    }

    let mut request = client.get(&spec.url);
    let mut bytes_done = 0u64;
    if existing > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={existing}-"));
        bytes_done = existing;
    }

    let response = request.send().await?;
    let status = response.status();

    let mut file = if status == reqwest::StatusCode::PARTIAL_CONTENT {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(tmp_path)
            .await?
    } else if status.is_success() {
        // Either nothing to resume, or the server ignored our Range header —
        // either way, start this attempt from scratch.
        bytes_done = 0;
        tokio::fs::File::create(tmp_path).await?
    } else if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        // Belt-and-suspenders for the case above: the size check didn't
        // catch it (e.g. `expected_size` was `None`, or the server's real
        // content length doesn't match Mojang's metadata for some other
        // reason), but the server rejected the range anyway. Drop the
        // unresumable partial so the *next* attempt starts clean instead
        // of repeating this exact request forever.
        tokio::fs::remove_file(tmp_path).await.ok();
        return Err(Error::Status {
            status: status.as_u16(),
            url: spec.url.clone(),
        });
    } else {
        return Err(Error::Status {
            status: status.as_u16(),
            url: spec.url.clone(),
        });
    };

    let total = response
        .content_length()
        .map(|len| bytes_done + len)
        .or(spec.expected_size);

    // Stream chunk-by-chunk straight to disk — the response body is never
    // buffered whole in memory, which is what keeps a 400 MB modpack file
    // from blowing the RAM budget during download.
    let attempt_started = std::time::Instant::now();
    let mut bytes_this_attempt = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        bytes_done += chunk.len() as u64;
        bytes_this_attempt += chunk.len() as u64;
        let elapsed = attempt_started.elapsed().as_secs_f64();
        let bytes_per_sec = if elapsed > 0.0 {
            bytes_this_attempt as f64 / elapsed
        } else {
            0.0
        };
        on_progress(Progress {
            task_id: spec.task_id.clone(),
            label: spec.label.clone(),
            bytes_done,
            bytes_total: total,
            bytes_per_sec,
        });
    }
    file.flush().await?;
    Ok(())
}

/// Stream a file through a SHA-1 hasher a fixed buffer at a time, so
/// verifying a 400 MB modpack never materializes it in RAM.
pub async fn hash_file(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(to_hex(&hasher.finalize()))
}

/// Lowercase hex encoding, matching the case Mojang publishes SHA-1s in.
fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
