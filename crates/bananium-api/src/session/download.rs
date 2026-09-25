//! Running a batch of downloads as one user-visible task.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bananium_net::{DownloadSpec, Downloader};

use super::Session;
use crate::error::{Error, Result};
use crate::event::Event;

/// Process-wide counter behind [`Session::new_task_id`].
static TASK_SEQ: AtomicU64 = AtomicU64::new(1);

impl Session {
    /// A fresh id for one long-running task, unique within this process so
    /// two concurrent installs never share a progress bar.
    pub(super) fn new_task_id(&self, kind: &str) -> String {
        format!("{kind}-{}", TASK_SEQ.fetch_add(1, Ordering::Relaxed))
    }

    /// Run `f` as task `task_id`: a failure is announced as
    /// `Event::TaskFailed` (success is announced by `f` itself, via
    /// [`Session::download_tracked`], because some tasks keep working after
    /// their downloads finish).
    pub(super) async fn tracked<T>(
        &self,
        task_id: &str,
        f: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        let result = f.await;
        match &result {
            Ok(_) => self.emit(Event::TaskCompleted {
                task_id: task_id.to_string(),
            }),
            Err(err) => self.emit(Event::TaskFailed {
                task_id: task_id.to_string(),
                error: err.to_string(),
            }),
        }
        result
    }

    /// Download every spec as one task, reporting a single aggregate
    /// `OverallProgress` under `task_id`.
    ///
    /// Progress is **throttled** to one event per [`PROGRESS_INTERVAL`]:
    /// an install fires thousands of per-file updates a second, and
    /// forwarding each one flooded the desktop webview's IPC (the UI froze
    /// mid-install) and overflowed the event channel, silently dropping
    /// later events — including `TaskCompleted`, which left finished tasks
    /// stuck "running". A final, exact update is always sent at the end.
    ///
    /// The aggregate is computed here because `bananium-net` only ever sees
    /// one file at a time. Specs without an `expected_size` count toward
    /// `files_total` but not `bytes_total`, which is then a lower bound.
    pub(super) async fn download_tracked(
        &self,
        task_id: &str,
        label: &str,
        specs: Vec<DownloadSpec>,
    ) -> Result<()> {
        let total = specs.len();
        let overall_total: u64 = specs.iter().filter_map(|s| s.expected_size).sum();
        let bytes_total = (overall_total > 0).then_some(overall_total);

        let downloader = Downloader::new(
            self.http.inner().clone(),
            self.config().max_concurrent_downloads,
        );
        let started = Instant::now();
        let state = Arc::new(Mutex::new(Aggregate::default()));
        let on_progress: bananium_net::ProgressFn = {
            let state = state.clone();
            let tx = self.events_tx.clone();
            let task_id = task_id.to_string();
            let label = label.to_string();
            Arc::new(move |p: bananium_net::Progress| {
                let mut s = state.lock().expect("progress mutex poisoned");
                let complete = p.bytes_total.is_some_and(|t| p.bytes_done >= t);
                if complete {
                    s.completed.insert(p.task_id.clone());
                }
                s.bytes.insert(p.task_id, p.bytes_done);
                s.current = Some(p.label);
                if s.last_emit.is_some_and(|t| t.elapsed() < PROGRESS_INTERVAL) {
                    return;
                }
                s.last_emit = Some(Instant::now());
                let done: u64 = s.bytes.values().sum();
                let _ = tx.send(Event::OverallProgress {
                    task_id: task_id.clone(),
                    label: label.clone(),
                    current_file: s.current.clone(),
                    bytes_done: done,
                    bytes_total,
                    bytes_per_sec: rate(done, started),
                    files_done: s.completed.len(),
                    files_total: total,
                });
            })
        };

        let results = downloader.download_all(specs, on_progress).await;
        let failures: Vec<String> = results
            .iter()
            .filter_map(|r| r.as_ref().err().map(|e| e.to_string()))
            .collect();

        // The exact final state, never throttled: every successful file is
        // done, whether or not its size was known up front.
        let done: u64 = state
            .lock()
            .expect("progress mutex poisoned")
            .bytes
            .values()
            .sum();
        self.emit(Event::OverallProgress {
            task_id: task_id.to_string(),
            label: label.to_string(),
            current_file: None,
            bytes_done: bytes_total.map_or(done, |t| done.max(t)),
            bytes_total,
            bytes_per_sec: rate(done, started),
            files_done: total - failures.len(),
            files_total: total,
        });
        match failures.first() {
            Some(first) => Err(Error::DownloadsFailed(failures.len(), total, first.clone())),
            None => Ok(()),
        }
    }

    /// Report a non-download phase of `task_id` (e.g. linking assets into
    /// place) as `done` of `total` steps, throttled like downloads. `last`
    /// is the caller's timestamp of its previous emission.
    pub(super) fn phase_progress(
        &self,
        task_id: &str,
        label: &str,
        done: usize,
        total: usize,
        last: &mut Option<Instant>,
    ) {
        if done != total && last.is_some_and(|t| t.elapsed() < PROGRESS_INTERVAL) {
            return;
        }
        *last = Some(Instant::now());
        self.emit(Event::OverallProgress {
            task_id: task_id.to_string(),
            label: label.to_string(),
            current_file: None,
            bytes_done: 0,
            bytes_total: None,
            bytes_per_sec: 0.0,
            files_done: done,
            files_total: total,
        });
    }
}

/// Minimum spacing between progress events for one task.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Running totals behind one [`Session::download_tracked`] call.
#[derive(Default)]
struct Aggregate {
    /// Bytes done per file (keyed by spec task id), so a retried or resumed
    /// file is never double-counted.
    bytes: HashMap<String, u64>,
    completed: HashSet<String>,
    current: Option<String>,
    last_emit: Option<Instant>,
}

fn rate(done: u64, started: Instant) -> f64 {
    let elapsed = started.elapsed().as_secs_f64();
    if elapsed > 0.0 {
        done as f64 / elapsed
    } else {
        0.0
    }
}
