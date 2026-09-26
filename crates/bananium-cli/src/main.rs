//! clap frontend. Depends only on `bananium-api` plus its own UI library
//! (clap) — `scripts/check_frontend_deps.py` enforces that
//! (see the frontend contract in CONTRIBUTING.md).

use std::io::{IsTerminal, Write};

use bananium_api::{
    is_valid_instance_name, Command, CommandOutput, Config, ConfigOverrides, ContentKind, Event,
    ModpackSource, Paths, Session,
};
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
        /// Instance name (letters, digits, '-', '_' only). Lets several
        /// instances share the same version. Skips the interactive prompt
        /// when given; omit and run in a real terminal to be prompted, with
        /// a blank answer defaulting to a random name.
        #[arg(long)]
        name: Option<String>,
        /// Also install the Fabric mod loader: a loader version, or
        /// "latest" for the newest stable one.
        #[arg(long)]
        fabric: Option<String>,
        /// Library group to file the new instance under.
        #[arg(long)]
        group: Option<String>,
    },
    /// List installable Minecraft versions.
    Versions {
        /// Include snapshots, betas, and alphas.
        #[arg(long)]
        all: bool,
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
    /// Search Modrinth.
    Search {
        query: String,
        /// mod, resource_pack, or shader.
        #[arg(long, default_value = "mod", value_parser = parse_kind)]
        kind: ContentKind,
        /// Only show results this instance can use.
        #[arg(long)]
        instance: Option<String>,
    },
    /// Manage an instance's mods, resource packs, and shader packs.
    Content {
        #[command(subcommand)]
        action: ContentAction,
    },
    /// Save and apply content presets.
    Preset {
        #[command(subcommand)]
        action: PresetAction,
    },
    /// List every JVM detected on this machine.
    Java,
    /// List screenshots from every instance.
    Screenshots,
    /// Manage offline profiles (the usernames you can play as).
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// Modrinth modpacks (.mrpack).
    Modpack {
        #[command(subcommand)]
        action: ModpackAction,
    },
}

#[derive(Subcommand)]
enum ModpackAction {
    /// Show what a local .mrpack needs.
    Info { file: std::path::PathBuf },
    /// Create an instance from a local .mrpack or a Modrinth modpack
    /// (project id or slug).
    Install {
        /// Path to a .mrpack file, or a Modrinth project id/slug.
        source: String,
        /// Modrinth version id; defaults to the newest stable release.
        #[arg(long)]
        version: Option<String>,
        /// Instance name; defaults to the pack's name.
        #[arg(long)]
        name: Option<String>,
        /// Library group for the new instance.
        #[arg(long)]
        group: Option<String>,
    },
}

#[derive(Subcommand)]
enum ContentAction {
    /// List everything installed.
    Ls { instance: String },
    /// Install a Modrinth project (id or slug) and its required dependencies.
    Add {
        instance: String,
        project: String,
        #[arg(long, default_value = "mod", value_parser = parse_kind)]
        kind: ContentKind,
    },
    /// Remove an installed file.
    Rm {
        instance: String,
        filename: String,
        #[arg(long, default_value = "mod", value_parser = parse_kind)]
        kind: ContentKind,
    },
    /// Show available updates.
    Updates { instance: String },
    /// Look unidentified files up on Modrinth (files Modrinth doesn't know
    /// are marked and logged, and not checked again).
    Identify { instance: String },
}

fn parse_kind(s: &str) -> Result<ContentKind, String> {
    match s {
        "mod" => Ok(ContentKind::Mod),
        "resource_pack" | "resourcepack" => Ok(ContentKind::ResourcePack),
        "shader" => Ok(ContentKind::Shader),
        _ => Err("expected mod, resource_pack, or shader".into()),
    }
}

#[derive(Subcommand)]
enum PresetAction {
    /// List presets.
    Ls,
    /// Save an instance's Modrinth content as a preset.
    Save { instance: String, name: String },
    /// Install a preset's content into an instance.
    Apply { preset: String, instance: String },
    /// Delete a preset.
    Rm { name: String },
}

#[derive(Subcommand)]
enum ProfileAction {
    /// List saved profiles; `*` marks the default.
    Ls,
    /// Add a profile (3-16 letters, digits, or '_').
    Add { name: String },
    /// Remove a profile.
    Rm { name: String },
    /// Make a profile the default for launches without `--profile`.
    Default { name: String },
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
        /// Library group; pass "" to ungroup.
        #[arg(long)]
        group: Option<String>,
    },
    /// Delete an instance and everything in it (worlds included).
    Rm { instance: String },
    /// Rename an instance.
    Rename { instance: String, new_name: String },
    /// Copy an instance, worlds and mods included, under a new name.
    Clone { instance: String, new_name: String },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Print resolved configuration and paths.
    Show,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // Parse first: `--help` and usage errors exit inside `parse` and
    // shouldn't leave a log that looks like a crashed run.
    let cli = Cli::parse();
    if let Ok(paths) = Paths::resolve() {
        bananium_api::init_logging(
            &paths,
            bananium_api::LogOptions {
                frontend: "cli",
                stderr: true,
            },
        );
    }
    let code = run_cli(cli.command, cli.format_json).await;
    bananium_api::log_shutdown("command finished");
    code
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
        Cmd::Install {
            version,
            name,
            fabric,
            group,
        } => {
            let name = name.or_else(prompt_instance_name);
            Command::Install {
                version,
                name,
                fabric_loader: fabric,
                group,
            }
        }
        Cmd::Versions { all } => Command::VersionList {
            include_snapshots: all,
        },
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
                group,
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
                java_path: None,
                group,
            },
            InstanceAction::Rm { instance } => Command::InstanceRemove { instance },
            InstanceAction::Rename { instance, new_name } => {
                Command::InstanceRename { instance, new_name }
            }
            InstanceAction::Clone { instance, new_name } => {
                Command::InstanceClone { instance, new_name }
            }
        },
        Cmd::Search {
            query,
            kind,
            instance,
        } => Command::ModrinthSearch {
            query,
            kind,
            instance,
            categories: Vec::new(),
            sort: Default::default(),
            offset: 0,
            limit: 20,
        },
        Cmd::Content { action } => match action {
            ContentAction::Ls { instance } => Command::ContentList { instance },
            ContentAction::Add {
                instance,
                project,
                kind,
            } => Command::ContentInstall {
                instance,
                kind,
                project,
                version: None,
            },
            ContentAction::Rm {
                instance,
                filename,
                kind,
            } => Command::ContentRemove {
                instance,
                kind,
                filename,
            },
            ContentAction::Updates { instance } => Command::ContentCheckUpdates { instance },
            ContentAction::Identify { instance } => Command::ContentIdentify { instance },
        },
        Cmd::Preset { action } => match action {
            PresetAction::Ls => Command::PresetList,
            PresetAction::Save { instance, name } => Command::PresetSave {
                instance,
                name,
                kinds: Vec::new(),
            },
            PresetAction::Apply { preset, instance } => Command::PresetApply { preset, instance },
            PresetAction::Rm { name } => Command::PresetDelete { name },
        },
        Cmd::Java => Command::JavaList,
        Cmd::Screenshots => Command::ScreenshotList { instance: None },
        Cmd::Profile { action } => match action {
            ProfileAction::Ls => Command::ProfileList,
            ProfileAction::Add { name } => Command::ProfileAdd { name },
            ProfileAction::Rm { name } => Command::ProfileRemove { name },
            ProfileAction::Default { name } => Command::ProfileSetDefault { name },
        },
        Cmd::Modpack { action } => match action {
            ModpackAction::Info { file } => Command::ModpackInspect { path: file },
            ModpackAction::Install {
                source,
                version,
                name,
                group,
            } => {
                let path = std::path::PathBuf::from(&source);
                let source = if path.is_file() {
                    ModpackSource::File { path }
                } else {
                    ModpackSource::Modrinth {
                        project: source,
                        version,
                    }
                };
                Command::ModpackInstall {
                    source,
                    name,
                    group,
                }
            }
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

/// Stderr progress renderer for a `run_with_progress` event stream, driven
/// by the aggregate `OverallProgress` events `Session` computes for the
/// whole job (total downloaded, total size, speed, current file).
#[derive(Default)]
struct ProgressPrinter {
    current_file: String,
    rendered_anything: bool,
}

impl ProgressPrinter {
    fn handle(&mut self, event: Event) {
        match event {
            Event::Progress { label, .. } => {
                self.current_file = label;
            }
            Event::OverallProgress {
                label,
                current_file,
                bytes_done,
                bytes_total,
                bytes_per_sec,
                files_done,
                files_total,
                ..
            } => {
                // `Session` already throttles these to ~10/s and always
                // sends an exact final one, so every event is rendered.
                self.rendered_anything = true;
                if let Some(file) = current_file {
                    self.current_file = file;
                }
                if bytes_done == 0 && bytes_total.is_none() {
                    // A non-download phase: just a step count.
                    eprint!("\r\x1b[K{label} {files_done}/{files_total}");
                    let _ = std::io::stderr().flush();
                    return;
                }
                let pct = bytes_total
                    .filter(|&t| t > 0)
                    .map(|t| (bytes_done as f64 / t as f64 * 100.0).min(100.0))
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
            // The CLI exits right after `launch` returns, long before any
            // game does, so it never observes this.
            Event::InstanceExited { .. } => {}
            // `launch` prints the pid from its result; the CLI never starts
            // Discord Rich Presence.
            Event::InstanceLaunched { .. } | Event::PresenceStatusChanged { .. } => {}
            Event::TaskQueued {
                label, position, ..
            } if position > 1 => {
                eprintln!("{label}: queued ({} ahead)", position - 1);
            }
            Event::TaskRetrying {
                attempt,
                max_attempts,
                reason,
                ..
            }
            | Event::ServiceRetrying {
                attempt,
                max_attempts,
                reason,
                ..
            } => {
                if self.rendered_anything {
                    eprintln!();
                    self.rendered_anything = false;
                }
                eprintln!("{reason} — retrying ({attempt}/{max_attempts})");
            }
            Event::TaskCancelled { .. } => eprintln!("cancelled"),
            Event::TaskQueued { .. } | Event::TaskStarted { .. } => {}
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
                let group = i.group.as_deref().unwrap_or("-");
                let played = format!(
                    "{}h{:02}m",
                    i.playtime_secs / 3600,
                    i.playtime_secs / 60 % 60
                );
                println!(
                    "{:<20} {:<12} [{status}] group={group} played={played} ram={ram} java_args={:?}",
                    i.slug, i.mc_version, i.jvm_args
                );
            }
        }
        CommandOutput::InstanceUpdated { instance } => {
            println!("updated instance {instance:?}");
        }
        CommandOutput::ModrinthSearched {
            hits, total_hits, ..
        } => {
            for h in hits {
                let mark = if h.installed { "*" } else { " " };
                println!(
                    "{mark} {:<28} {:<10} {}",
                    h.slug.as_deref().unwrap_or(&h.project_id),
                    h.downloads,
                    h.title
                );
            }
            println!("({total_hits} total)");
        }
        CommandOutput::ModrinthProjectShown { project } => {
            println!("{} — {}", project.title, project.description);
        }
        CommandOutput::ModrinthVersionsListed { versions } => {
            for v in versions {
                println!("{:<24} {:<8} {}", v.version_number, v.version_type, v.id);
            }
        }
        CommandOutput::ContentListed { entries, .. } => {
            if entries.is_empty() {
                println!("nothing installed");
            }
            for e in entries {
                let state = if e.enabled { " " } else { "x" };
                println!(
                    "{state} {:<14} {:<32} {}",
                    format!("{:?}", e.kind),
                    e.title,
                    e.version_number.as_deref().unwrap_or("local")
                );
            }
        }
        CommandOutput::ContentInstalled { installed, .. } => {
            for e in installed {
                let dep = if e.dependency { " (dependency)" } else { "" };
                println!(
                    "installed {} {}{dep}",
                    e.title,
                    e.version_number.as_deref().unwrap_or_default()
                );
            }
        }
        CommandOutput::ContentRemoved { filename, .. } => println!("removed {filename}"),
        CommandOutput::ContentToggled {
            filename, enabled, ..
        } => {
            let state = if *enabled { "enabled" } else { "disabled" };
            println!("{state} {filename}");
        }
        CommandOutput::ContentImported { entry, .. } => println!("imported {}", entry.title),
        CommandOutput::ContentIdentified {
            identified,
            not_found,
            ..
        } => {
            println!("identified {identified} file(s); {not_found} not on Modrinth");
        }
        CommandOutput::ContentUpdatesFound { updates, .. } => {
            if updates.is_empty() {
                println!("everything is up to date");
            }
            for u in updates {
                println!(
                    "{:<32} {} -> {}",
                    u.title,
                    u.current_version.as_deref().unwrap_or("?"),
                    u.new_version_number
                );
            }
        }
        CommandOutput::JavaListed { installs } => {
            for j in installs {
                let origin = match &j.component {
                    Some(c) => format!("mojang:{c}"),
                    None => j.source.clone(),
                };
                println!(
                    "java {:<4} {:<28} {}",
                    j.major_version,
                    origin,
                    j.path.display()
                );
            }
        }
        CommandOutput::JavaRuntimeRemoved { component } => {
            println!("removed Mojang runtime {component}")
        }
        CommandOutput::InstanceJavaShown {
            instance,
            component,
            major_version,
            installed_version,
            path,
            ..
        } => {
            let major = major_version
                .map(|m| format!(" (Java {m})"))
                .unwrap_or_default();
            let state = match installed_version {
                Some(v) => format!("installed {v}"),
                None => "downloads before first launch".to_string(),
            };
            println!("{instance}: {component}{major}, {state}");
            println!("  {}", path.display());
        }
        CommandOutput::ScreenshotListed { screenshots } => {
            for s in screenshots {
                println!("{:<20} {}", s.instance, s.path.display());
            }
        }
        CommandOutput::ScreenshotDeleted { path } => println!("deleted {}", path.display()),
        CommandOutput::PresetListed { presets } => {
            for p in presets {
                println!(
                    "{:<24} {} {:?}, {} item(s)",
                    p.name,
                    p.mc_version,
                    p.loader,
                    p.entries.len()
                );
            }
        }
        CommandOutput::PresetSaved {
            preset,
            skipped_local,
        } => {
            println!(
                "saved preset {:?} with {} item(s)",
                preset.name,
                preset.entries.len()
            );
            if *skipped_local > 0 {
                println!("({skipped_local} local file(s) not included)");
            }
        }
        CommandOutput::PresetApplied {
            applied, skipped, ..
        } => {
            for e in applied {
                println!("installed {}", e.title);
            }
            for s in skipped {
                println!("skipped {}: {}", s.title, s.reason);
            }
        }
        CommandOutput::PresetDeleted { name } => println!("deleted preset {name:?}"),
        CommandOutput::PresetRenamed { name } => println!("renamed preset to {name:?}"),
        CommandOutput::PresetExported { path } => println!("exported to {}", path.display()),
        CommandOutput::PresetImported { preset } => println!("imported preset {:?}", preset.name),
        CommandOutput::VersionListed { versions, .. } => {
            for v in versions {
                println!("{:<24} {}", v.id, v.kind);
            }
        }
        CommandOutput::FabricLoaderListed { loaders, .. } => {
            for l in loaders {
                let stable = if l.stable { "stable" } else { "" };
                println!("{:<16} {stable}", l.version);
            }
        }
        CommandOutput::InstanceRemoved { instance } => {
            println!("removed instance {instance:?}");
        }
        CommandOutput::InstanceRenamed { old, instance } => {
            println!("renamed instance {old:?} to {instance:?}");
        }
        CommandOutput::InstanceCloned { source, instance } => {
            println!("cloned instance {source:?} as {instance:?}");
        }
        CommandOutput::InstanceKilled { instance } => {
            println!("stopped instance {instance:?}");
        }
        CommandOutput::LogListed { logs, .. } => {
            for log in logs {
                println!("{:<32} {:>10}", log.name, format_bytes(log.size));
            }
        }
        CommandOutput::LogChunk { text, .. } => {
            print!("{text}");
        }
        CommandOutput::LauncherLogListed { logs, .. } => {
            for log in logs {
                println!(
                    "{:<28} {:>10}  {}",
                    log.name,
                    format_bytes(log.size),
                    log.reason.as_deref().unwrap_or("running")
                );
            }
        }
        CommandOutput::LauncherLogChunk { text, .. } => {
            print!("{text}");
        }
        CommandOutput::LauncherLastSession { log } => match log {
            Some(log) => println!(
                "{}: {}",
                log.name,
                log.reason.as_deref().unwrap_or("running")
            ),
            None => println!("no previous session"),
        },
        CommandOutput::Logged => {}
        CommandOutput::FileListed { entries, .. } => {
            for e in entries {
                let size = if e.is_dir {
                    "<dir>".to_string()
                } else {
                    format_bytes(e.size)
                };
                println!("{:>10}  {}", size, e.path);
            }
        }
        CommandOutput::FileContents { text, .. } => print!("{text}"),
        CommandOutput::FileWritten { path } => println!("wrote {path}"),
        CommandOutput::FileDeleted { path } => println!("deleted {path}"),
        CommandOutput::FileImported { count } => println!("imported {count} item(s)"),
        CommandOutput::ModpackInspected { pack } => {
            println!("{} {}", pack.name, pack.version_id);
            let loader = match &pack.loader_version {
                Some(v) => format!("Fabric {v}"),
                None => "vanilla".to_string(),
            };
            println!(
                "Minecraft {} · {loader} · {} files",
                pack.mc_version, pack.file_count
            );
            if let Some(reason) = &pack.unsupported {
                println!("can't install: {reason}");
            }
        }
        CommandOutput::ProfileListed { profiles } => {
            if profiles.is_empty() {
                println!("no profiles yet (\"Player\" is created on first launch)");
            }
            for p in profiles {
                let marker = if p.is_default { "*" } else { " " };
                println!("{marker} {:<16} {}", p.name, p.uuid);
            }
        }
        CommandOutput::ProfileAdded { profile } => {
            println!("added profile {:?} ({})", profile.name, profile.uuid);
        }
        CommandOutput::ProfileRemoved { name } => {
            println!("removed profile {name:?}");
        }
        CommandOutput::ProfileDefaultSet { name } => {
            println!("default profile is now {name:?}");
        }
        // Rich Presence runs only in the desktop app; the CLI has no
        // subcommands that produce these.
        CommandOutput::PresenceStatusShown { .. }
        | CommandOutput::PresencePreviewed { .. }
        | CommandOutput::PresenceViewSet => {}
        // A CLI process only ever has its own one command in flight, so it
        // has no queue to list or cancel from.
        CommandOutput::TaskListed { .. } | CommandOutput::TaskCancelled { .. } => {}
    }
}
