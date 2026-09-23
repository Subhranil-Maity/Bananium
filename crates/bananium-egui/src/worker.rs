//! Bridges `eframe`'s synchronous, single-threaded UI loop to `Session`'s
//! `async` API: a dedicated OS thread owns a background Tokio runtime and
//! the `Session` itself, and the UI thread only ever talks to it over
//! plain `std::sync::mpsc` channels — `try_recv` from `App::update` is
//! non-blocking, so a slow `Command` (a multi-gigabyte `install`) never
//! stalls a single frame.
//!
//! Commands are serialized through one channel into one loop, same as
//! every other frontend's single `Session` reference implies (a `Launch`
//! and an `InstanceSet` for the same instance can't race). `Event`s are
//! forwarded from a separate task on the same runtime, spawned once up
//! front, so progress from a job started via `dispatch` is never missed
//! while `block_on` is busy driving that same job to completion.

use std::sync::mpsc;

use bananium_api::{Command, CommandOutput, Event, Session};
use tokio::sync::broadcast;

/// One in-flight request: the `Command` to run, and where to send its
/// eventual result. A fresh one-shot-style `mpsc` pair per call rather than
/// a shared reply channel keeps each caller's `try_recv` loop from ever
/// seeing a result meant for someone else's request.
struct Job {
    command: Command,
    reply: mpsc::Sender<bananium_api::Result<CommandOutput>>,
}

/// A handle to the background worker thread. Cheap to clone (an `mpsc`
/// sender), so every part of the UI that needs to dispatch a `Command` can
/// hold its own copy.
#[derive(Clone)]
pub struct Worker {
    jobs: mpsc::Sender<Job>,
}

impl Worker {
    /// Spawn the worker thread, taking ownership of `session` for the rest
    /// of the app's lifetime. Returns the handle to dispatch commands
    /// through, plus the receiving end of the forwarded `Event` stream.
    pub fn spawn(session: Session) -> (Worker, mpsc::Receiver<Event>) {
        let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
        let (events_tx, events_rx) = mpsc::channel::<Event>();

        std::thread::Builder::new()
            .name("bananium-egui-worker".to_string())
            .spawn(move || worker_main(session, jobs_rx, events_tx))
            .expect("failed to spawn bananium-egui worker thread");

        (Worker { jobs: jobs_tx }, events_rx)
    }

    /// Submit `command` to the worker and return the receiving end its
    /// result will arrive on. Never blocks: the send only waits on a
    /// channel the worker thread is always looping on.
    pub fn dispatch(
        &self,
        command: Command,
    ) -> mpsc::Receiver<bananium_api::Result<CommandOutput>> {
        let (reply_tx, reply_rx) = mpsc::channel();
        // The worker thread outliving every `Worker` handle for the app's
        // whole lifetime means this send can only fail if the thread
        // itself panicked, which `expect`s during setup, not per-command;
        // dropping the reply silently would leave a caller waiting
        // forever, so this is deliberately not swallowed.
        self.jobs
            .send(Job {
                command,
                reply: reply_tx,
            })
            .expect("bananium-egui worker thread is gone");
        reply_rx
    }
}

/// The worker thread's whole body: one background Tokio runtime, one task
/// forwarding `Session::events()` for the runtime's lifetime, and a loop
/// that runs each incoming `Command` to completion in turn.
fn worker_main(session: Session, jobs: mpsc::Receiver<Job>, events_tx: mpsc::Sender<Event>) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to start bananium-egui worker's Tokio runtime");

    // Subscribed once, up front, on the same runtime `dispatch` below runs
    // on — matching `bananium-cli`'s `run_with_progress`, this must happen
    // before any `Command` is dispatched so no early `Progress` event (one
    // fired the instant a download starts) is missed.
    let mut events = session.events();
    runtime.spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if events_tx.send(event).is_err() {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    });

    for job in jobs {
        let result = runtime.block_on(session.dispatch(job.command));
        let _ = job.reply.send(result);
    }
}
