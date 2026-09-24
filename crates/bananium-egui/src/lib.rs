//! egui/eframe frontend — a full graphical alternative to `bananium-tui`,
//! not a stripped-down companion to it: instance management (list, launch,
//! dry-run preview, RAM/JVM-arg editing, a live log tail), install with a
//! real progress bar wired to `Event::OverallProgress`, and a config
//! viewer, all reachable without touching a terminal.
//!
//! Depends only on `bananium-api` plus its own UI libraries (`egui`,
//! `eframe`) — a CI check enforces that (see PLAN.md's frontend contract).
//! `eframe`'s renderer links straight against whatever GPU driver is already
//! on the system, the same way any native Rust binary does — there's no
//! bundled webview process or Node/Electron runtime to ship alongside it,
//! which is the whole point of reaching for egui here instead of a
//! web-view-based UI toolkit.
//!
//! `eframe`'s *default* renderer is `wgpu`, which on Windows means it opens
//! a DirectX 12 (or Vulkan) device — descriptor heaps and committed
//! GPU-visible allocations that cost a real, measured ~230 MB of private
//! memory (RSS ~330 MB) just to paint egui's immediate-mode 2D UI, nothing
//! this app's actual rendering needs justifies. [`run`] instead requests
//! `NativeOptions::renderer = Renderer::Glow` (`Cargo.toml` enables the
//! `glow` feature alongside the crate's own default features so the variant
//! exists to request), which uses a plain OpenGL context instead —
//! confirmed on this project's real Windows dev box to fall to ~49 MB
//! private / ~104 MB RSS for the exact same window, a >5x reduction with no
//! visible behavior change, since none of egui's rendering needs anything
//! wgpu offers over glow (no 3D, no compute shaders, no WebGPU portability
//! requirement). Revisit only if a real feature actually needs `wgpu`'s
//! capabilities, and re-measure before assuming it's still worth the RAM.
//!
//! `Session::dispatch` is `async`, but `eframe::App::ui` is a plain
//! synchronous callback driven by `winit`'s event loop, which must own the
//! calling thread. So this crate never runs its own async runtime on the
//! UI thread: `worker::Worker` owns a background multi-thread Tokio
//! runtime on its own OS thread, `ui` sends it `Command`s and polls for
//! results/`Event`s over plain `std::sync::mpsc` channels each frame. See
//! `worker`'s module doc comment for the full shape.

mod app;
mod worker;

use bananium_api::Session;

/// This crate's own error type: window/graphics-context setup failures
/// (from `eframe`) are a different domain than `bananium_api::Error` (a
/// dispatched `Command` failing), so callers get a single type that can
/// represent either rather than this crate forcing one into the other's
/// shape.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to start the graphical interface: {0}")]
    Eframe(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Run the egui frontend to completion — blocks the calling thread until
/// the window is closed. `session` should be freshly built (same as any
/// other frontend's entry point) since this takes ownership of it for the
/// whole app lifetime.
pub fn run(session: Session) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 640.0])
            .with_min_inner_size([720.0, 480.0])
            .with_title("Bananium"),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "Bananium",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, session)))),
    )
    .map_err(|err| Error::Eframe(err.to_string()))
}
