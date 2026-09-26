//! What a frontend is told about long-running tasks (installs, updates,
//! identifying content): their kind, and where each stands in the queue.
//! The queue itself is `session::tasks`.

use serde::{Deserialize, Serialize};

/// What a task does. Part of a task's dedup identity, and lets a frontend
/// find "the install of this project into this instance" among the tasks
/// in flight without guessing from labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, strum::IntoStaticStr)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TaskKind {
    /// `Command::Install`: a Minecraft (+ Fabric) version as an instance.
    Install,
    /// `Command::ModpackInstall`.
    ModpackInstall,
    /// `Command::ContentInstall`.
    ContentInstall,
    /// `Command::ContentUpdate`.
    ContentUpdate,
    /// `Command::PresetApply`.
    PresetApply,
    /// `Command::ContentIdentify`.
    ContentIdentify,
    /// Downloading a Mojang Java runtime a launch needs.
    JavaRuntime,
}

impl TaskKind {
    /// Kinds that create a new instance: a pending one "owns" its name, so
    /// a second creation under that name is refused even if it's a
    /// different kind (a modpack and a plain install both called "COB").
    pub fn creates_instance(self) -> bool {
        matches!(self, TaskKind::Install | TaskKind::ModpackInstall)
    }
}

/// Whether a task is waiting for its turn or doing its work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Queued,
    Running,
}

/// One task in the queue, as `Command::TaskList` reports it — enough for a
/// frontend to rebuild its task tray after a reload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TaskInfo {
    pub task_id: String,
    pub kind: TaskKind,
    /// What the task is, for people: "Installing Sodium into COB".
    pub label: String,
    /// The instance it works on, when known up front.
    pub instance: Option<String>,
    /// The Modrinth project it installs, for content and modpack installs.
    pub project: Option<String>,
    pub state: TaskState,
    /// 1-based place in the waiting line; `None` once running.
    pub position: Option<u32>,
    /// The step a running task is on ("Placing modpack files").
    pub phase: Option<String>,
}
