//! Tauri desktop frontend. Depends only on `bananium-api` plus its own UI
//! libraries (tauri and its plugins) — `scripts/check_frontend_deps.py`
//! enforces that (see the frontend contract in CONTRIBUTING.md).
//!
//! This crate is deliberately a thin bridge: the React webview sends
//! serialized `Command`s through the single `dispatch` Tauri command, and
//! every `Event` from `Session::events()` is re-emitted to the webview as
//! [`EVENT_CHANNEL`]. All UI lives in `desktop/src`; all business logic
//! lives behind `Session`.

// Hide the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use bananium_api::{Command, CommandOutput, Config, ConfigOverrides, Paths, Session};
use tauri::{Emitter, Manager};
use tokio::sync::broadcast::error::RecvError;

/// Name of the webview event every `bananium_api::Event` is forwarded on.
/// Mirrored by `EVENT_CHANNEL` in `desktop/src/lib/api.ts`.
const EVENT_CHANNEL: &str = "bananium://event";

/// Run one `Command` against the shared `Session`. Errors are flattened to
/// their `Display` string: the webview only ever shows them to the user,
/// and `bananium_api::Error` wraps non-serializable sources (`io::Error`
/// etc.) that couldn't cross the IPC boundary as structured data anyway.
#[tauri::command]
async fn dispatch(
    session: tauri::State<'_, Arc<Session>>,
    cmd: Command,
) -> Result<CommandOutput, String> {
    session.dispatch(cmd).await.map_err(|e| e.to_string())
}

/// Resolve `BANANIUM_HOME`, load layered config from it, and build the one
/// `Session` the whole app shares — same resolution the CLI uses. Also
/// returns the instances directory, which the asset protocol is scoped to.
fn build_session() -> bananium_api::Result<(Session, PathBuf)> {
    let paths = Paths::resolve()?;
    let instances_dir = paths.instances_dir();
    let config = Config::load(&paths, ConfigOverrides::default())?;
    Ok((Session::new(paths, config)?, instances_dir))
}

fn main() {
    bananium_api::init_logging();

    let (session, instances_dir) = match build_session() {
        Ok((s, dir)) => (Arc::new(s), dir),
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            // The asset protocol (how the webview displays screenshots) is
            // scoped at runtime rather than in tauri.conf.json because the
            // data directory moves with `BANANIUM_HOME` / portable mode.
            app.asset_protocol_scope()
                .allow_directory(&instances_dir, true)?;

            // Subscribe before the webview can dispatch anything, so no
            // event from the first command is lost (a broadcast receiver
            // only sees events sent after it subscribes).
            let mut events = session.events();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match events.recv().await {
                        Ok(event) => {
                            let _ = handle.emit(EVENT_CHANNEL, event);
                        }
                        // A burst (thousands of asset `Progress` events)
                        // can outrun the buffer; dropped progress ticks are
                        // harmless since the next one supersedes them.
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            });
            // Discord Rich Presence runs for the app's whole lifetime; it
            // needs the Tokio runtime, which `setup` itself isn't inside.
            let presence = session.clone();
            tauri::async_runtime::spawn(async move { presence.start_presence() });
            app.manage(session);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![dispatch])
        .run(tauri::generate_context!())
        .expect("error while running bananium-desktop");
}
