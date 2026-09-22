//! clap frontend. Depends only on `bananium-api` plus its own UI library
//! (clap) — a CI check enforces that (see PLAN.md's frontend contract).

use bananium_api::{Command, CommandOutput, Config, ConfigOverrides, Paths, Session};
use clap::{Parser, Subcommand};

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

    match session.dispatch(command).await {
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
