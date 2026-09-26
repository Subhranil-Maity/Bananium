use serde::{Deserialize, Serialize};

use crate::presence::PresenceStatus;
use crate::task::TaskKind;

/// Everything a frontend can be told about while a `Command` runs. Every
/// long-running task reports `Progress` under a stable `task_id`, so every
/// frontend renders progress/cancellation/failure identically without
/// inventing its own scheme.
///
/// Byte counts are `u64` in Rust but typed as `number` in the generated
/// TypeScript: serde_json emits them as plain JSON numbers, not `bigint`,
/// and no real download approaches 2^53 bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
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
    ///
    /// `task_id` is `"<parent task id>/<file key>"`: everything before the
    /// first `/` is the task the file belongs to (the one its
    /// `OverallProgress`/`TaskCompleted` events use).
    Progress {
        task_id: String,
        label: String,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        bytes_done: u64,
        #[cfg_attr(feature = "ts", ts(type = "number | null"))]
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
    ///
    /// Throttled to about ten per second per task, and the last one before
    /// `TaskCompleted` is always exact — see `Session::download_tracked`.
    OverallProgress {
        task_id: String,
        label: String,
        /// What's being fetched right now (e.g. a maven coordinate or asset
        /// path), when downloading.
        current_file: Option<String>,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        bytes_done: u64,
        #[cfg_attr(feature = "ts", ts(type = "number | null"))]
        bytes_total: Option<u64>,
        /// Average bytes/sec since the job started (total bytes done over
        /// total elapsed time, not an instantaneous rate).
        bytes_per_sec: f64,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        files_done: usize,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        files_total: usize,
    },
    /// A long-running task was accepted and is waiting for its turn (see
    /// the task queue's rules in `session::tasks`), sent the moment it's
    /// requested — before any network work. Sent again with a new
    /// `position` whenever the line ahead of it moves.
    TaskQueued {
        task_id: String,
        kind: TaskKind,
        label: String,
        instance: Option<String>,
        project: Option<String>,
        /// 1-based place in the waiting line.
        position: u32,
    },
    /// A queued task got its turn and is now running.
    TaskStarted {
        task_id: String,
    },
    /// A running task's request to a service (Modrinth) failed or was held
    /// back, and is being retried; `attempt` is the attempt about to be
    /// made. Cleared by the task's next progress event.
    TaskRetrying {
        task_id: String,
        attempt: u32,
        max_attempts: u32,
        /// Why, for people: "Modrinth didn't respond".
        reason: String,
    },
    /// Like `TaskRetrying`, for a request that isn't part of a task (a
    /// search, a project page), so a frontend can say "Modrinth is slow to
    /// respond — retrying" instead of showing an endless skeleton.
    ServiceRetrying {
        service: String,
        attempt: u32,
        max_attempts: u32,
        reason: String,
    },
    /// A queued task was cancelled before it started.
    TaskCancelled {
        task_id: String,
    },
    TaskCompleted {
        task_id: String,
    },
    TaskFailed {
        task_id: String,
        error: String,
    },
    /// A game this session launched has exited (on its own or via
    /// `Command::InstanceKill`). `exit_code` is `None` when the process was
    /// killed by a signal.
    InstanceExited {
        instance: String,
        exit_code: Option<i32>,
    },
    /// A game this session launched has started: `started_unix` (seconds)
    /// is when, `player` the offline username it's playing as.
    InstanceLaunched {
        instance: String,
        pid: u32,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        started_unix: u64,
        player: String,
    },
    /// The Discord Rich Presence connection changed state (connecting,
    /// connected, waiting for Discord, turned off).
    PresenceStatusChanged {
        status: PresenceStatus,
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
