use serde::{Deserialize, Serialize};

/// Everything a frontend can be told about while a `Command` runs. Every
/// long-running task reports `Progress` under a stable `task_id`, so every
/// frontend renders progress/cancellation/failure identically without
/// inventing its own scheme.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Log {
        level: String,
        message: String,
    },
    /// A step forward on one file within a running task, identified by
    /// `task_id`. See `impl From<bananium_net::Progress>` below — this is
    /// how the download engine's progress reaches a frontend without
    /// `bananium-net` knowing anything about `bananium-api`. `label`
    /// identifies exactly what this file is (e.g. a maven coordinate or
    /// asset path), not just its category, so a frontend can show the user
    /// what's actually being fetched right now.
    Progress {
        task_id: String,
        label: String,
        bytes_done: u64,
        bytes_total: Option<u64>,
        bytes_per_sec: f64,
    },
    /// Aggregate progress across every file in a multi-file job (e.g. one
    /// `install`), summed by `Session` from the individual `Progress`
    /// events for that job's files — `bananium-net` only ever sees one file
    /// at a time, so it can't produce this itself. This is what a frontend
    /// should render as "the" progress bar for a command like `install`;
    /// per-file `Progress` events are there for a frontend that also wants
    /// a detailed per-file view.
    OverallProgress {
        task_id: String,
        label: String,
        bytes_done: u64,
        bytes_total: Option<u64>,
        /// Average bytes/sec since the job started (total bytes done over
        /// total elapsed time, not an instantaneous rate).
        bytes_per_sec: f64,
        files_done: usize,
        files_total: usize,
    },
    TaskCompleted {
        task_id: String,
    },
    TaskFailed {
        task_id: String,
        error: String,
    },
}

/// Lets `Session` turn a raw download-engine progress update into a public
/// `Event` with one `.into()`/`Event::from(p)` call, keeping `bananium-net`
/// itself free of any dependency on this crate.
impl From<bananium_net::Progress> for Event {
    fn from(p: bananium_net::Progress) -> Self {
        Event::Progress {
            task_id: p.task_id,
            label: p.label,
            bytes_done: p.bytes_done,
            bytes_total: p.bytes_total,
            bytes_per_sec: p.bytes_per_sec,
        }
    }
}
