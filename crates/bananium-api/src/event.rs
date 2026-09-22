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
    /// A step forward on a running task, identified by `task_id`. See
    /// `impl From<bananium_net::Progress>` below — this is how the download
    /// engine's progress reaches a frontend without `bananium-net` knowing
    /// anything about `bananium-api`.
    Progress {
        task_id: String,
        label: String,
        bytes_done: u64,
        bytes_total: Option<u64>,
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
        }
    }
}
