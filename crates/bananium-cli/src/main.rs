//! clap frontend. Depends only on `bananium-api` plus its own UI library
//! (clap) — a CI check enforces that (see PLAN.md's frontend contract).

use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

use bananium_api::{
    is_valid_instance_name, Command, CommandOutput, Config, ConfigOverrides, Event, Paths, Session,
};
use clap::{Parser, Subcommand};
use tokio::sync::broadcast;

#[derive(Parser)]
#[command(name = "bananium", version, about = "The Banana Launcher")]
struct Cli {
    /// Print machine-readable JSON instead of human text.
    #[arg(long, global = true)]
    format_json: bool,

    /// Launch the graphical (egui) interface instead of running a
    /// subcommand. Takes over the whole process — no subcommand may be
    /// given alongside it.
    #[arg(long, global = true)]
    gui: bool,

    #[command(subcommand)]
    command: Option<Cmd>,
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
        /// Instance name (letters, digits, '-', '_' only). Lets several
        /// instances share the same version. Skips the interactive prompt
        /// when given; omit and run in a real terminal to be prompted, with
        /// a blank answer defaulting to a random name.
        #[arg(long)]
        name: Option<String>,
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
    /// Manage installed instances.
    Instance {
        #[command(subcommand)]
        action: InstanceAction,
    },
}

#[derive(Subcommand)]
enum InstanceAction {
    /// List every installed instance and whether it's currently running.
    Ls,
    /// Change an instance's RAM cap and/or extra JVM arguments.
    Set {
        /// Instance slug (see `bananium instance ls`).
        instance: String,
        /// `-Xmx` heap cap in MB. Pass `0` to clear it back to the JVM
        /// default.
        #[arg(long)]
        ram_mb: Option<u32>,
        /// Extra JVM argument, appended after every other JVM argument at
        /// launch time; repeat the flag for more than one. Replaces the
        /// instance's entire current list. Omit entirely to leave the
        /// current list untouched. `allow_hyphen_values` because almost
        /// every real JVM flag starts with `-` (`-Xmx...`, `-XX:...`),
        /// which clap would otherwise try to parse as one of *its* flags.
        #[arg(long = "java-arg", allow_hyphen_values = true)]
        java_arg: Vec<String>,
        /// Clear the instance's extra JVM arguments back to none. Needed
        /// because omitting `--java-arg` means "leave unchanged", not
        /// "clear" — this is how you actually empty the list.
        #[arg(long)]
        clear_java_args: bool,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Print resolved configuration and paths.
    Show,
}

/// Plain (non-`#[tokio::main]`) entry point: `--gui` must own the main
/// thread for `eframe`/`winit`'s native event loop, which can't run inside
/// an already-started Tokio runtime the way the rest of this binary's
/// subcommands need one. So the runtime is only ever built on the branch
/// that actually needs it — `run_cli` — never up front in `main` itself.
fn main() -> std::process::ExitCode {
    bananium_api::init_logging();
    let cli = Cli::parse();

    if cli.gui {
        if cli.command.is_some() {
            eprintln!("error: --gui cannot be combined with a subcommand");
            return std::process::ExitCode::FAILURE;
        }
        return run_gui();
    }

    let Some(command) = cli.command else {
        use clap::CommandFactory;
        let _ = Cli::command().print_help();
        println!();
        return std::process::ExitCode::FAILURE;
    };

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(err) => {
            eprintln!("error: failed to start async runtime: {err}");
            return std::process::ExitCode::FAILURE;
        }
    };
    runtime.block_on(run_cli(command, cli.format_json))
}

/// `--gui`: hand the already-built `Session` straight to `bananium-egui`,
/// blocking this thread (the window's event loop) until the window closes.
fn run_gui() -> std::process::ExitCode {
    let session = match build_session() {
        Ok(s) => s,
        Err(err) => {
            eprintln!("error: {err}");
            return std::process::ExitCode::FAILURE;
        }
    };
    match bananium_egui::run(session) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run_cli(command: Cmd, format_json: bool) -> std::process::ExitCode {
    let session = match build_session() {
        Ok(s) => s,
        Err(err) => {
            eprintln!("error: {err}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let command = match command {
        Cmd::Config {
            action: ConfigAction::Show,
        } => Command::ConfigShow,
        Cmd::Install { version, name } => {
            let name = name.or_else(prompt_instance_name);
            Command::Install { version, name }
        }
        Cmd::Launch {
            instance,
            profile,
            dry_run,
        } => Command::Launch {
            instance,
            profile,
            dry_run,
        },
        Cmd::Instance { action } => match action {
            InstanceAction::Ls => Command::InstanceList,
            InstanceAction::Set {
                instance,
                ram_mb,
                java_arg,
                clear_java_args,
            } => Command::InstanceSet {
                instance,
                ram_mb,
                jvm_args: if clear_java_args {
                    Some(Vec::new())
                } else if java_arg.is_empty() {
                    None
                } else {
                    Some(java_arg)
                },
            },
        },
    };

    let result = run_with_progress(&session, command, !format_json).await;

    match result {
        Ok(output) => {
            print_output(&output, format_json);
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

/// Interactively ask for an instance name when `install` wasn't given
/// `--name` and stdin is actually a terminal (never blocks a piped/scripted
/// invocation — those just get `None`, which `Session` turns into a random
/// name). Loops on an invalid answer instead of erroring, since a wasted
/// keystroke is cheap and re-dispatching an entire failed `install` isn't.
/// Validated with `is_valid_instance_name` — the same check
/// `InstanceStore::create_named` re-applies server-side — so a typo is
/// caught here, before any network call, rather than after one.
fn prompt_instance_name() -> Option<String> {
    if !std::io::stdin().is_terminal() {
        return None;
    }
    loop {
        eprint!("instance name (letters, digits, '-', '_'; blank = random): ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() {
            return None;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        if is_valid_instance_name(trimmed) {
            return Some(trimmed.to_string());
        }
        eprintln!("invalid name: only letters, digits, '-', and '_' are allowed");
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
        CommandOutput::Launched {
            instance,
            pid,
            log_path,
        } => {
            println!("launched instance {instance:?} (pid {pid})");
            println!("output logged to {}", log_path.display());
        }
        CommandOutput::InstanceListed { instances } => {
            if instances.is_empty() {
                println!("no instances installed yet");
            }
            for i in instances {
                let status = if i.running { "running" } else { "stopped" };
                let ram = i
                    .ram_mb
                    .map(|m| format!("{m} MB"))
                    .unwrap_or_else(|| "default".to_string());
                println!(
                    "{:<20} {:<12} [{status}] ram={ram} java_args={:?}",
                    i.slug, i.mc_version, i.jvm_args
                );
            }
        }
        CommandOutput::InstanceUpdated { instance } => {
            println!("updated instance {instance:?}");
        }
    }
}
