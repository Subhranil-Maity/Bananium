//! The task queue: every long-running command (install, modpack install,
//! content install/update, preset apply, identify) becomes a task here
//! *before* it does any work, so a frontend can show it the moment it's
//! requested — queued, then running — instead of only once its first
//! download starts.
//!
//! Scheduling rules:
//! - Tasks on the **same instance** run one at a time, in the order they
//!   were requested. A mod installed into a modpack that's still
//!   installing waits for the pack rather than racing it on the lockfile.
//! - Tasks on **different instances** run in parallel, at most
//!   [`MAX_PARALLEL_TASKS`] at once, so three packs don't split the
//!   bandwidth and Modrinth's rate limit three ways and all crawl.
//! - A request that's **identical** to one already queued or running (same
//!   kind, instance and project — or a second new instance under a pending
//!   name) is refused with [`Error::AlreadyQueued`]: that is what a
//!   double-click, or a click on a button that didn't seem to respond,
//!   produces.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::{broadcast, Notify};

use crate::error::{Error, Result};
use crate::event::Event;
use crate::task::{TaskInfo, TaskKind, TaskState};

/// Most tasks that may run at once (on different instances).
pub(super) const MAX_PARALLEL_TASKS: usize = 2;

tokio::task_local! {
    /// The id of the task whose work is running on this Tokio task, so
    /// code deep inside it (the Modrinth client's retry observer) can
    /// report against the right task without threading the id through.
    pub(super) static CURRENT_TASK: String;
}

/// What a task is, before it's queued.
pub(super) struct TaskSpec {
    pub(super) kind: TaskKind,
    pub(super) label: String,
    pub(super) instance: Option<String>,
    pub(super) project: Option<String>,
}

impl TaskSpec {
    pub(super) fn new(kind: TaskKind, label: impl Into<String>) -> Self {
        Self {
            kind,
            label: label.into(),
            instance: None,
            project: None,
        }
    }

    pub(super) fn instance(mut self, instance: impl Into<String>) -> Self {
        self.instance = Some(instance.into());
        self
    }

    pub(super) fn project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Whether `self` asks for the same thing as `other`, so it would be a
    /// duplicate of it.
    fn duplicates(&self, other: &TaskSpec) -> bool {
        if self.instance.is_none() || self.instance != other.instance {
            return false;
        }
        (self.kind.creates_instance() && other.kind.creates_instance())
            || (self.kind == other.kind && self.project == other.project)
    }
}

struct Slot {
    task_id: String,
    spec: TaskSpec,
    phase: Option<String>,
    queued_at: Instant,
}

impl Slot {
    fn info(&self, state: TaskState, position: Option<u32>) -> TaskInfo {
        TaskInfo {
            task_id: self.task_id.clone(),
            kind: self.spec.kind,
            label: self.spec.label.clone(),
            instance: self.spec.instance.clone(),
            project: self.spec.project.clone(),
            state,
            position,
            phase: self.phase.clone(),
        }
    }

    fn queued_event(&self, position: u32) -> Event {
        Event::TaskQueued {
            task_id: self.task_id.clone(),
            kind: self.spec.kind,
            label: self.spec.label.clone(),
            instance: self.spec.instance.clone(),
            project: self.spec.project.clone(),
            position,
        }
    }
}

#[derive(Default)]
struct QueueState {
    running: Vec<Slot>,
    waiting: VecDeque<Slot>,
}

impl QueueState {
    fn all(&self) -> impl Iterator<Item = &Slot> {
        self.running.iter().chain(self.waiting.iter())
    }

    /// The index in `waiting` of the first task allowed to start now: its
    /// instance isn't busy with a running task, no earlier waiting task is
    /// for the same instance (so one instance's tasks keep their order),
    /// and there's a free slot.
    fn next_eligible(&self) -> Option<usize> {
        if self.running.len() >= MAX_PARALLEL_TASKS {
            return None;
        }
        fn busy<'a>(instance: &Option<String>, mut among: impl Iterator<Item = &'a Slot>) -> bool {
            instance.is_some() && among.any(|s| &s.spec.instance == instance)
        }
        (0..self.waiting.len()).find(|&i| {
            let instance = &self.waiting[i].spec.instance;
            !busy(instance, self.running.iter()) && !busy(instance, self.waiting.iter().take(i))
        })
    }
}

/// The queue itself; one per [`super::Session`].
pub(super) struct TaskQueue {
    state: Mutex<QueueState>,
    notify: Notify,
    events: broadcast::Sender<Event>,
}

impl TaskQueue {
    pub(super) fn new(events: broadcast::Sender<Event>) -> Self {
        Self {
            state: Mutex::default(),
            notify: Notify::new(),
            events,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, QueueState> {
        self.state.lock().expect("task queue mutex poisoned")
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    /// Put `spec` in the waiting line as `task_id` and announce it with
    /// `Event::TaskQueued`, or refuse it as a duplicate. `precheck` runs
    /// under the queue lock, so a check like "that instance name is free"
    /// can't race another request making the same check.
    pub(super) fn enqueue(
        self: &Arc<Self>,
        task_id: String,
        spec: TaskSpec,
        precheck: impl FnOnce() -> Result<()>,
    ) -> Result<Ticket> {
        let mut state = self.lock();
        if let Some(existing) = state.all().find(|s| s.spec.duplicates(&spec)) {
            tracing::info!(
                target: "bananium_api::task",
                kind = <&str>::from(spec.kind),
                instance = spec.instance.as_deref().unwrap_or("-"),
                project = spec.project.as_deref().unwrap_or("-"),
                "task rejected: already queued as {}",
                existing.task_id
            );
            return Err(Error::AlreadyQueued(existing.spec.label.clone()));
        }
        precheck()?;
        let slot = Slot {
            task_id: task_id.clone(),
            spec,
            phase: None,
            queued_at: Instant::now(),
        };
        let position = state.waiting.len() as u32 + 1;
        tracing::info!(
            target: "bananium_api::task",
            task = %task_id,
            kind = <&str>::from(slot.spec.kind),
            instance = slot.spec.instance.as_deref().unwrap_or("-"),
            project = slot.spec.project.as_deref().unwrap_or("-"),
            position,
            "task queued: {}",
            slot.spec.label
        );
        self.emit(slot.queued_event(position));
        state.waiting.push_back(slot);
        drop(state);
        // Something may be able to start right away (possibly this).
        self.notify.notify_waiters();
        Ok(Ticket {
            queue: self.clone(),
            task_id,
        })
    }

    /// Refuse to touch `instance` while a task is working on it (and, with
    /// `include_queued`, while one is waiting to): removing or renaming an
    /// instance under a queued install would make that install fail, and
    /// toggling a mod mid-install races the install's lockfile writes.
    pub(super) fn ensure_idle(&self, instance: &str, include_queued: bool) -> Result<()> {
        let state = self.lock();
        let mut slots: Box<dyn Iterator<Item = &Slot>> = if include_queued {
            Box::new(state.all())
        } else {
            Box::new(state.running.iter())
        };
        match slots.find(|s| s.spec.instance.as_deref() == Some(instance)) {
            Some(slot) => {
                tracing::info!(
                    target: "bananium_api::task",
                    instance,
                    "refused: instance busy with {} ({})",
                    slot.task_id,
                    slot.spec.label
                );
                Err(Error::InstanceBusy {
                    instance: instance.to_string(),
                    task: slot.spec.label.clone(),
                })
            }
            None => Ok(()),
        }
    }

    /// Every queued and running task.
    pub(super) fn list(&self) -> Vec<TaskInfo> {
        let state = self.lock();
        state
            .running
            .iter()
            .map(|s| s.info(TaskState::Running, None))
            .chain(
                state
                    .waiting
                    .iter()
                    .enumerate()
                    .map(|(i, s)| s.info(TaskState::Queued, Some(i as u32 + 1))),
            )
            .collect()
    }

    /// Take a *queued* task out of the line; its caller then fails with
    /// [`Error::Cancelled`]. A running task can't be cancelled.
    pub(super) fn cancel(&self, task_id: &str) -> Result<()> {
        let mut state = self.lock();
        let Some(i) = state.waiting.iter().position(|s| s.task_id == task_id) else {
            return Err(if state.running.iter().any(|s| s.task_id == task_id) {
                Error::TaskRunning(task_id.to_string())
            } else {
                Error::TaskNotFound(task_id.to_string())
            });
        };
        let slot = state.waiting.remove(i).expect("index just found");
        tracing::info!(
            target: "bananium_api::task",
            task = task_id,
            "task cancelled while queued at position {} ({})",
            i + 1,
            slot.spec.label
        );
        self.reannounce_positions(&state, i);
        drop(state);
        self.emit(Event::TaskCancelled {
            task_id: task_id.to_string(),
        });
        self.notify.notify_waiters();
        Ok(())
    }

    /// Record the step a running task is on. Returns whether it changed, so
    /// the caller logs each step once rather than every progress tick.
    pub(super) fn note_phase(&self, task_id: &str, phase: &str) -> bool {
        let mut state = self.lock();
        let Some(slot) = state.running.iter_mut().find(|s| s.task_id == task_id) else {
            return false;
        };
        if slot.phase.as_deref() == Some(phase) {
            return false;
        }
        slot.phase = Some(phase.to_string());
        true
    }

    /// Re-send `TaskQueued` for every waiting task from index `from` on,
    /// whose position just moved up.
    fn reannounce_positions(&self, state: &QueueState, from: usize) {
        for (i, slot) in state.waiting.iter().enumerate().skip(from) {
            self.emit(slot.queued_event(i as u32 + 1));
        }
    }
}

/// A task's place in the queue, from `enqueue` until it's dropped. Dropping
/// it — the task finished, failed, or the command was abandoned mid-wait —
/// always frees the place, so nothing can leave a stuck entry behind.
pub(super) struct Ticket {
    queue: Arc<TaskQueue>,
    task_id: String,
}

impl Ticket {
    pub(super) fn task_id(&self) -> &str {
        &self.task_id
    }

    /// Wait until the scheduling rules let this task run, then mark it
    /// running and announce `Event::TaskStarted`. Fails with
    /// [`Error::Cancelled`] if it was cancelled while waiting.
    pub(super) async fn wait_turn(&self) -> Result<()> {
        loop {
            // Registered before checking, so a wake-up between the check
            // and the await can't be missed.
            let notified = self.queue.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut state = self.queue.lock();
                let Some(mine) = state.waiting.iter().position(|s| s.task_id == self.task_id)
                else {
                    return Err(Error::Cancelled);
                };
                if state.next_eligible() == Some(mine) {
                    let slot = state.waiting.remove(mine).expect("index just found");
                    let waited = slot.queued_at.elapsed().as_secs_f32();
                    tracing::info!(
                        target: "bananium_api::task",
                        task = %self.task_id,
                        "task started after waiting {waited:.1}s"
                    );
                    state.running.push(slot);
                    self.queue.reannounce_positions(&state, mine);
                    drop(state);
                    self.queue.emit(Event::TaskStarted {
                        task_id: self.task_id.clone(),
                    });
                    // Another task may be eligible too (a free slot left).
                    self.queue.notify.notify_waiters();
                    return Ok(());
                }
            }
            notified.await;
        }
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let mut state = self.queue.lock();
        if let Some(i) = state.running.iter().position(|s| s.task_id == self.task_id) {
            state.running.remove(i);
        } else if let Some(i) = state.waiting.iter().position(|s| s.task_id == self.task_id) {
            state.waiting.remove(i);
            self.queue.reannounce_positions(&state, i);
        }
        drop(state);
        self.queue.notify.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn queue() -> (Arc<TaskQueue>, broadcast::Receiver<Event>) {
        let (tx, rx) = broadcast::channel(64);
        (Arc::new(TaskQueue::new(tx)), rx)
    }

    fn content(instance: &str, project: &str) -> TaskSpec {
        TaskSpec::new(
            TaskKind::ContentInstall,
            format!("{project} into {instance}"),
        )
        .instance(instance)
        .project(project)
    }

    fn ok() -> Result<()> {
        Ok(())
    }

    /// Whether `ticket` gets its turn within a short wait.
    async fn starts(ticket: &Ticket) -> bool {
        tokio::time::timeout(Duration::from_millis(50), ticket.wait_turn())
            .await
            .is_ok_and(|r| r.is_ok())
    }

    #[tokio::test]
    async fn an_identical_request_is_rejected_while_the_first_is_pending() {
        let (q, _rx) = queue();
        let first = q
            .enqueue("t1".into(), content("cob", "sodium"), ok)
            .unwrap();
        assert!(matches!(
            q.enqueue("t2".into(), content("cob", "sodium"), ok),
            Err(Error::AlreadyQueued(_))
        ));
        // A different project, or the same one elsewhere, is fine.
        let _other = q.enqueue("t3".into(), content("cob", "iris"), ok).unwrap();
        let _elsewhere = q
            .enqueue("t4".into(), content("main", "sodium"), ok)
            .unwrap();
        drop(first);
        assert!(q.enqueue("t5".into(), content("cob", "sodium"), ok).is_ok());
    }

    #[tokio::test]
    async fn a_pending_new_instance_owns_its_name() {
        let (q, _rx) = queue();
        let _pack = q
            .enqueue(
                "m1".into(),
                TaskSpec::new(TaskKind::ModpackInstall, "COB").instance("COB"),
                ok,
            )
            .unwrap();
        let plain = TaskSpec::new(TaskKind::Install, "COB").instance("COB");
        assert!(matches!(
            q.enqueue("i1".into(), plain, ok),
            Err(Error::AlreadyQueued(_))
        ));
    }

    #[tokio::test]
    async fn precheck_failure_queues_nothing() {
        let (q, _rx) = queue();
        let spec = TaskSpec::new(TaskKind::ModpackInstall, "COB").instance("COB");
        assert!(q
            .enqueue("m1".into(), spec, || Err(Error::Cancelled))
            .is_err());
        assert!(q.list().is_empty());
    }

    #[tokio::test]
    async fn one_instance_runs_its_tasks_one_at_a_time_in_order() {
        let (q, _rx) = queue();
        let a = q.enqueue("a".into(), content("cob", "a"), ok).unwrap();
        let b = q.enqueue("b".into(), content("cob", "b"), ok).unwrap();
        let c = q.enqueue("c".into(), content("cob", "c"), ok).unwrap();
        assert!(!starts(&b).await, "b must wait for a, which is ahead of it");
        assert!(starts(&a).await);
        assert!(!starts(&c).await, "c must wait behind b");
        assert!(!starts(&b).await, "b must wait while a runs");
        drop(a);
        assert!(starts(&b).await);
        drop(b);
        assert!(starts(&c).await);
    }

    #[tokio::test]
    async fn different_instances_run_in_parallel_up_to_the_cap() {
        let (q, _rx) = queue();
        let a = q.enqueue("a".into(), content("one", "x"), ok).unwrap();
        let b = q.enqueue("b".into(), content("two", "x"), ok).unwrap();
        let c = q.enqueue("c".into(), content("three", "x"), ok).unwrap();
        assert!(starts(&a).await);
        assert!(starts(&b).await);
        assert!(!starts(&c).await, "only {MAX_PARALLEL_TASKS} at once");
        drop(a);
        assert!(starts(&c).await);
    }

    #[tokio::test]
    async fn cancelling_a_queued_task_fails_its_wait_and_moves_the_line_up() {
        let (q, mut rx) = queue();
        let a = q.enqueue("a".into(), content("cob", "a"), ok).unwrap();
        let b = q.enqueue("b".into(), content("cob", "b"), ok).unwrap();
        let _c = q.enqueue("c".into(), content("cob", "c"), ok).unwrap();
        assert!(starts(&a).await);
        while rx.try_recv().is_ok() {}

        q.cancel("b").unwrap();
        assert!(matches!(b.wait_turn().await, Err(Error::Cancelled)));
        assert!(matches!(q.cancel("a"), Err(Error::TaskRunning(_))));
        let events: Vec<Event> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(events.iter().any(
            |e| matches!(e, Event::TaskQueued { task_id, position: 1, .. } if task_id == "c")
        ));
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::TaskCancelled { task_id } if task_id == "b")));
        let listed: Vec<(String, TaskState)> =
            q.list().into_iter().map(|t| (t.task_id, t.state)).collect();
        assert_eq!(
            listed,
            [
                ("a".to_string(), TaskState::Running),
                ("c".to_string(), TaskState::Queued)
            ]
        );
    }

    #[tokio::test]
    async fn a_busy_instance_refuses_direct_changes() {
        let (q, _rx) = queue();
        let a = q.enqueue("a".into(), content("cob", "a"), ok).unwrap();
        // Queued only: direct changes are fine, removal isn't.
        assert!(q.ensure_idle("cob", false).is_ok());
        assert!(q.ensure_idle("cob", true).is_err());
        assert!(starts(&a).await);
        assert!(matches!(
            q.ensure_idle("cob", false),
            Err(Error::InstanceBusy { .. })
        ));
        assert!(q.ensure_idle("main", true).is_ok());
    }
}
