//! ratatui frontend — the primary interface per PLAN.md. This is a first
//! slice, not the full M2 scope: an instance list, running-status display,
//! and launch. The log pane, task tray, and command palette PLAN.md
//! describes for M2 are separate, later pieces — this exists to prove the
//! frontend contract holds through a real UI, not a stub.
//!
//! Depends only on `bananium-api` plus its own UI libraries (`ratatui`,
//! `crossterm`) — a CI check enforces that (see PLAN.md's frontend
//! contract).

use std::io::{self, Stdout};
use std::time::Duration;

use bananium_api::{
    Command, CommandOutput, Config, ConfigOverrides, InstanceSummary, Paths, Session,
};
use crossterm::event::{self, Event as CtEvent, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};

type Term = Terminal<CrosstermBackend<Stdout>>;

/// Single-threaded runtime: this app has exactly one thing to do at a time
/// (poll input or await a dispatch), so a multi-thread runtime would only
/// cost RAM against PLAN.md's `< 40 MB` TUI budget for nothing in return.
#[tokio::main(flavor = "current_thread")]
async fn main() {
    bananium_api::init_logging();

    let session = match build_session() {
        Ok(session) => session,
        Err(err) => {
            eprintln!("error: {err}");
            return;
        }
    };

    let mut terminal = match setup_terminal() {
        Ok(terminal) => terminal,
        Err(err) => {
            eprintln!("error: failed to start terminal UI: {err}");
            return;
        }
    };

    let result = run(&mut terminal, &session).await;
    // Always try to restore the terminal, even if `run` errored, so a
    // crash doesn't leave the user's shell in raw/alternate-screen mode.
    let _ = restore_terminal(&mut terminal);

    if let Err(err) = result {
        eprintln!("error: {err}");
    }
}

/// Resolve `BANANIUM_HOME`, load layered config from it, and build a
/// `Session` — the TUI's one entry point into the frontend contract, same
/// as every other frontend.
fn build_session() -> bananium_api::Result<Session> {
    let paths = Paths::resolve()?;
    let config = Config::load(&paths, ConfigOverrides::default())?;
    Session::new(paths, config)
}

fn setup_terminal() -> io::Result<Term> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut Term) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()
}

/// Everything the instance-list screen needs to redraw itself — the whole
/// app's state, for now.
struct App {
    instances: Vec<InstanceSummary>,
    selected: usize,
    /// One-line footer message: the outcome of the last launch attempt, or
    /// blank. Not an `Event` subscription yet (that's real M2 work, per the
    /// module doc comment) — `Command::Launch`'s own `Result` is enough
    /// feedback for this first slice.
    status: String,
}

impl App {
    fn selected_slug(&self) -> Option<&str> {
        self.instances.get(self.selected).map(|i| i.slug.as_str())
    }
}

async fn run(terminal: &mut Term, session: &Session) -> bananium_api::Result<()> {
    let mut app = App {
        instances: Vec::new(),
        selected: 0,
        status: String::new(),
    };
    refresh(&mut app, session).await?;

    loop {
        terminal.draw(|frame| draw(frame, &app)).ok();

        // A timed poll (rather than a blocking read) is what lets the list
        // eventually pick up a running instance exiting on its own without
        // the user pressing a key — there's no `Event` push for that yet.
        if event::poll(Duration::from_millis(500)).unwrap_or(false) {
            if let Ok(CtEvent::Key(key)) = event::read() {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Down | KeyCode::Char('j') => select_next(&mut app),
                    KeyCode::Up | KeyCode::Char('k') => select_prev(&mut app),
                    KeyCode::Char('r') => refresh(&mut app, session).await?,
                    KeyCode::Enter => launch_selected(&mut app, terminal, session).await?,
                    _ => {}
                }
            }
        } else {
            refresh(&mut app, session).await?;
        }
    }
}

fn select_next(app: &mut App) {
    if !app.instances.is_empty() {
        app.selected = (app.selected + 1) % app.instances.len();
    }
}

fn select_prev(app: &mut App) {
    if !app.instances.is_empty() {
        app.selected = app
            .selected
            .checked_sub(1)
            .unwrap_or(app.instances.len() - 1);
    }
}

/// Launch the selected instance, redrawing immediately with an in-flight
/// status message first since `Command::Launch` can take a moment (a
/// version-profile fetch even when everything's already installed).
async fn launch_selected(
    app: &mut App,
    terminal: &mut Term,
    session: &Session,
) -> bananium_api::Result<()> {
    let Some(slug) = app.selected_slug().map(str::to_string) else {
        return Ok(());
    };
    app.status = format!("launching {slug}...");
    terminal.draw(|frame| draw(frame, app)).ok();

    let command = Command::Launch {
        instance: Some(slug.clone()),
        profile: None,
        dry_run: false,
    };
    app.status = match session.dispatch(command).await {
        Ok(CommandOutput::Launched { pid, .. }) => format!("launched {slug} (pid {pid})"),
        Ok(_) => format!("launched {slug}"),
        Err(err) => format!("error launching {slug}: {err}"),
    };
    refresh(app, session).await
}

/// Re-fetch the instance list (and with it, every instance's running
/// status) via `Command::InstanceList` — the only source of truth, kept on
/// `Session`'s side of the frontend contract rather than this crate poking
/// at pid files itself.
async fn refresh(app: &mut App, session: &Session) -> bananium_api::Result<()> {
    if let CommandOutput::InstanceListed { instances } =
        session.dispatch(Command::InstanceList).await?
    {
        app.instances = instances;
        if app.selected >= app.instances.len() {
            app.selected = app.instances.len().saturating_sub(1);
        }
    }
    Ok(())
}

fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let items: Vec<ListItem> = if app.instances.is_empty() {
        vec![ListItem::new(
            "no instances installed yet — run `bananium install <version>`",
        )]
    } else {
        app.instances
            .iter()
            .map(|instance| {
                let status = if instance.running {
                    Span::styled(" [running]", Style::default().fg(Color::Green))
                } else {
                    Span::styled(" [stopped]", Style::default().fg(Color::DarkGray))
                };
                ListItem::new(Line::from(vec![
                    Span::raw(format!("{} ", instance.name)),
                    Span::styled(
                        format!("({})", instance.mc_version),
                        Style::default().fg(Color::Yellow),
                    ),
                    status,
                ]))
            })
            .collect()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Bananium — Instances "),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");

    let mut list_state = ListState::default();
    if !app.instances.is_empty() {
        list_state.select(Some(app.selected));
    }
    frame.render_stateful_widget(list, chunks[0], &mut list_state);

    frame.render_widget(Paragraph::new(app.status.as_str()), chunks[1]);
    frame.render_widget(
        Paragraph::new("up/down or j/k: select   enter: launch   r: refresh   q: quit"),
        chunks[2],
    );
}
