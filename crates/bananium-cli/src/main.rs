//! clap frontend. Depends only on `bananium-api` plus its own UI library
//! (clap) — a CI check enforces that (see PLAN.md's frontend contract).

use std::io::Write;
use std::time::{Duration, Instant};

use bananium_api::{Command, CommandOutput, Config, ConfigOverrides, Event, Paths, Session};
use clap::{Parser, Subcommand};
use tokio::sync::broadcast;

#[derive(Parser)]
#[command(name = "bananium", version, about = "The Banana Launcher")]
struct Cli {
    /// Print machine-readable JSON instead of human text.
    #[arg(long, global = true)]
    format_json: bool,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print resolved configuration and paths.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Download everything needed to launch a version offline afterwards.
    Install {
        /// A Minecraft version id, e.g. "1.21.1".
        version: String,
    },
    /// Launch an installed instance.
    Launch {
        /// Instance slug; omit when exactly one instance is installed.
        instance: Option<String>,
        /// Named local (offline) profile to play as.
        #[arg(long)]
        profile: Option<String>,
        /// Print the exact command line instead of launching.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Print resolved configuration and paths.
    Show,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    bananium_api::init_logging();
    let cli = Cli::parse();

    let session = match build_session() {
        Ok(s) => s,
        Err(err) => {
            eprintln!("error: {err}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let command = match cli.command {
        Cmd::Config {
            action: ConfigAction::Show,
        } => Command::ConfigShow,
        Cmd::Install { version } => Command::Install { version },
        Cmd::Launch {
            instance,
            profile,
            dry_run,
        } => Command::Launch {
            instance,
            profile,
            dry_run,
        },
    };

    let result = run_with_progress(&session, command, !cli.format_json).await;

    match result {
        Ok(output) => {
            print_output(&output, cli.format_json);
            std::process::ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Runs `command` to completion while printing its progress events to
/// stderr as they arrive — the fix for `install` otherwise giving no
/// feedback until it's entirely done. Subscribes to `session.events()`
/// *before* dispatching, so no early event (e.g. the first `Progress` from
/// a download that starts immediately) is missed — a `broadcast::Receiver`
/// only sees events sent after it subscribes.
///
/// Can't just loop `events.recv()` until the channel closes: it never does
/// while `session` (and its `events_tx`) is still alive, which is for the
/// rest of `main`. Instead this polls the dispatch future and the event
/// stream concurrently via `select!`, and once dispatch resolves, drains
/// whatever's left in the broadcast buffer non-blockingly so a final 100%
/// update isn't lost to the race between the two branches.
async fn run_with_progress(
    session: &Session,
    command: Command,
    show_progress: bool,
) -> bananium_api::Result<CommandOutput> {
    let mut events = session.events();
    let mut printer = ProgressPrinter::default();

    let dispatch = session.dispatch(command);
    tokio::pin!(dispatch);

    loop {
        tokio::select! {
            result = &mut dispatch => {
                while let Ok(event) = events.try_recv() {
                    if show_progress {
                        printer.handle(event);
                    }
                }
                return result;
            }
            event = events.recv(), if show_progress => {
                match event {
                    Ok(event) => printer.handle(event),
                    Err(broadcast::error::RecvError::Lagged(_) | broadcast::error::RecvError::Closed) => {}
                }
            }
        }
    }
}

/// Stderr progress renderer for a `run_with_progress` event stream. Per-file
/// `Progress` events are tracked only to show which file is currently in
/// flight; the numbers that matter for a "is this stuck" read — total
/// downloaded, total size, speed — come from the aggregate
/// `OverallProgress` events `Session` computes for the whole job.
#[derive(Default)]
struct ProgressPrinter {
    current_file: String,
    last_rendered: Option<Instant>,
    rendered_anything: bool,
}

impl ProgressPrinter {
    fn handle(&mut self, event: Event) {
        match event {
            Event::Progress { label, .. } => {
                self.current_file = label;
            }
            Event::OverallProgress {
                bytes_done,
                bytes_total,
                bytes_per_sec,
                files_done,
                files_total,
                ..
            } => {
                // Throttle rendering, not the underlying events — a
                // multi-file install can emit hundreds of these a second,
                // far faster than a terminal line is worth repainting.
                // Always render the final (100%) update so the line ends
                // on a completed, not stale, state.
                let is_done = bytes_total.is_some_and(|t| bytes_done >= t);
                let due = self
                    .last_rendered
                    .is_none_or(|t| t.elapsed() >= Duration::from_millis(150));
                if !is_done && !due {
                    return;
                }
                self.last_rendered = Some(Instant::now());
                self.rendered_anything = true;

                let pct = bytes_total
                    .filter(|&t| t > 0)
                    .map(|t| bytes_done as f64 / t as f64 * 100.0)
                    .unwrap_or(0.0);
                let total = bytes_total
                    .map(format_bytes)
                    .unwrap_or_else(|| "?".to_string());
                eprint!(
                    "\r\x1b[Kdownloading {files_done}/{files_total} files — {}/{total} ({pct:.0}%) @ {}/s — {}",
                    format_bytes(bytes_done),
                    format_bytes(bytes_per_sec.round() as u64),
                    self.current_file,
                );
                let _ = std::io::stderr().flush();
            }
            Event::TaskCompleted { .. } => {
                if self.rendered_anything {
                    eprintln!();
                    self.rendered_anything = false;
                }
            }
            Event::TaskFailed { error, .. } => {
                if self.rendered_anything {
                    eprintln!();
                    self.rendered_anything = false;
                }
                eprintln!("error: {error}");
            }
            Event::Log { level, message } => {
                eprintln!("[{level}] {message}");
            }
        }
    }
}

/// Human-readable byte count (`1536` -> `"1.5 KB"`), used both for a raw
/// size and, with a `/s` suffix left to the caller, a transfer speed.
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Resolve `BANANIUM_HOME`, load layered config from it, and build a
/// `Session` — the CLI's one entry point into the frontend contract.
fn build_session() -> bananium_api::Result<Session> {
    let paths = Paths::resolve()?;
    let config = Config::load(&paths, ConfigOverrides::default())?;
    Session::new(paths, config)
}

/// Render a `CommandOutput` either as pretty JSON (`--format-json`, for
/// scripting) or as the human-readable text below.
fn print_output(output: &CommandOutput, as_json: bool) {
    if as_json {
        println!("{}", serde_json::to_string_pretty(output).unwrap());
        return;
    }

    match output {
        CommandOutput::ConfigShown { paths, config } => {
            println!("home:          {}", paths.home.display());
            println!("config.toml:   {}", paths.config_toml.display());
            println!("store:         {}", paths.store_dir.display());
            println!("instances:     {}", paths.instances_dir.display());
            println!("java:          {}", paths.java_dir.display());
            println!("assets:        {}", paths.assets_dir.display());
            println!();
            println!(
                "max_concurrent_downloads: {}",
                config.max_concurrent_downloads
            );
            println!("theme:                    {}", config.theme);
            println!(
                "java_path:                {}",
                config
                    .java_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<auto>".into())
            );
        }
        CommandOutput::Installed {
            instance,
            mc_version,
        } => {
            println!("installed {mc_version} as instance {instance:?}");
        }
        CommandOutput::LaunchPlanned {
            instance,
            command_line,
        } => {
            println!("# instance {instance:?}, dry run — nothing was executed");
            println!("{command_line}");
        }
        CommandOutput::Launched { instance, pid } => {
            println!("launched instance {instance:?} (pid {pid})");
        }
    }
}
