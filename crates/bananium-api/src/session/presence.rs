//! Discord Rich Presence: what to show, and keeping Discord in sync.
//!
//! [`Presence`] holds a [`Model`] of what's going on — running games,
//! tasks in progress, the launcher page — fed by this session's own
//! events plus hints from the frontend. [`compose`] is a pure function of
//! that model and the user's [`DiscordConfig`] to one [`Activity`]; a
//! background task ([`run`]) keeps trying to reach Discord every few
//! seconds until it can (so opening Discord later just works) and sends
//! the composed activity whenever it changes, no faster than Discord's
//! rate limit allows.
//!
//! Only frontends that call `Session::start_presence` (the desktop app)
//! get any of this; for everyone else it's inert bookkeeping.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bananium_core::{DiscordConfig, Paths, StatusDisplay};
use bananium_discord::art::Art;
use bananium_discord::{Activity, DiscordClient, Error as DiscordError, StatusDisplayType};
use bananium_instance::{InstanceStore, Loader};
use tokio::sync::{broadcast, Notify};
use tokio::time::Instant;

use crate::event::Event;
use crate::presence::{
    LauncherView, PresencePreview, PresenceStatus, PreviewButton, PreviewScenario, DISCORD_APP_ID,
    RETRY_INTERVAL_SECS,
};

const APP_NAME: &str = "Bananium";
const REPO_URL: &str = "https://github.com/Subhranil-Maity/Bananium";
/// Discord allows about five activity updates per 20 seconds; one per five
/// keeps well inside that even while an install's progress ticks.
const MIN_UPDATE_GAP: Duration = Duration::from_secs(5);
/// Pause between attempts to reach Discord while not connected.
const RETRY_INTERVAL: Duration = Duration::from_secs(RETRY_INTERVAL_SECS);

/// What presence needs to know about one instance, snapshotted from its
/// `instance.toml` when it's launched or its page is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstanceInfo {
    pub name: String,
    pub mc_version: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
    pub mod_count: u32,
    /// The modpack's Modrinth icon URL, when installed from one.
    pub icon_url: Option<String>,
    /// The modpack's Modrinth slug, when installed from one.
    pub modrinth_project: Option<String>,
    pub hidden: bool,
}

impl InstanceInfo {
    pub(crate) fn load(store: &InstanceStore, slug: &str) -> Option<Self> {
        let cfg = store.load(slug).ok()?;
        Some(Self {
            mod_count: store.mod_count(slug),
            name: cfg.name,
            mc_version: cfg.mc_version,
            loader: cfg.loader,
            loader_version: cfg.loader_version,
            icon_url: cfg.modrinth_icon_url,
            modrinth_project: cfg.modrinth_project,
            hidden: cfg.discord_hidden,
        })
    }
}

/// A game this session launched and is still running.
#[derive(Debug, Clone)]
struct Game {
    slug: String,
    started_ms: u64,
    player: String,
    info: InstanceInfo,
}

/// One long-running task, from its `OverallProgress` events.
#[derive(Debug, Clone)]
struct Task {
    id: String,
    started_ms: u64,
    label: String,
    bytes_done: u64,
    bytes_total: Option<u64>,
    bytes_per_sec: f64,
    files_done: usize,
    files_total: usize,
}

impl Task {
    /// `"install-3"` -> `"install"`: the kind `Session::new_task_id` was
    /// given.
    fn kind(&self) -> &str {
        self.id.split('-').next().unwrap_or_default()
    }

    fn fraction(&self) -> Option<f64> {
        match self.bytes_total {
            Some(total) if total > 0 => Some(self.bytes_done as f64 / total as f64),
            _ if self.files_total > 0 => Some(self.files_done as f64 / self.files_total as f64),
            _ => None,
        }
    }
}

/// The launcher page, with an instance page's data already looked up.
#[derive(Debug, Clone, Default)]
enum View {
    #[default]
    Library,
    Instance(InstanceInfo),
    Browse(String),
    Project {
        title: String,
        author: Option<String>,
        icon_url: Option<String>,
        url: Option<String>,
    },
    Presets,
    Screenshots,
    Accounts,
    Settings,
    About,
    Other,
}

/// Everything [`compose`] looks at.
#[derive(Debug, Clone, Default)]
struct Model {
    launcher_started_ms: u64,
    instance_count: usize,
    view: View,
    /// In launch order: the last one is shown.
    games: Vec<Game>,
    /// In start order: the first one is shown.
    tasks: Vec<Task>,
    /// Titles and icons tasks were given by `describe_task` (keyed by task
    /// id), which can arrive before the task's first progress event.
    task_titles: HashMap<String, (String, Option<String>)>,
}

/// Shared between the `Session` and the background task.
pub(crate) struct Presence {
    model: Mutex<Model>,
    config: Mutex<DiscordConfig>,
    status: Mutex<PresenceStatus>,
    /// Something changed: recompose, and maybe reconnect.
    wake: Notify,
    reconnect: AtomicBool,
    started: AtomicBool,
}

impl Presence {
    pub(crate) fn new(config: DiscordConfig) -> Self {
        Self {
            model: Mutex::new(Model {
                launcher_started_ms: now_ms(),
                ..Model::default()
            }),
            config: Mutex::new(config),
            status: Mutex::new(PresenceStatus::Disabled),
            wake: Notify::new(),
            reconnect: AtomicBool::new(false),
            started: AtomicBool::new(false),
        }
    }

    /// Spawn the background task (once; later calls do nothing). Must run
    /// inside a Tokio runtime.
    pub(crate) fn start(self: &Arc<Self>, paths: Paths, events: broadcast::Sender<Event>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let store = InstanceStore::new(paths);
        self.lock_model().instance_count = instance_count(&store);
        tokio::spawn(run(self.clone(), store, events.subscribe(), events));
    }

    pub(crate) fn status(&self) -> PresenceStatus {
        self.status
            .lock()
            .expect("presence status poisoned")
            .clone()
    }

    pub(crate) fn update_config(&self, config: DiscordConfig) {
        *self.config.lock().expect("presence config poisoned") = config;
        self.wake.notify_one();
    }

    /// Give a task a human title ("Installing Fabulously Optimized") and an
    /// icon, instead of a generic one derived from its kind.
    pub(crate) fn describe_task(&self, task_id: &str, title: String, icon_url: Option<String>) {
        self.lock_model()
            .task_titles
            .insert(task_id.to_string(), (title, icon_url));
        self.wake.notify_one();
    }

    pub(crate) fn set_view(&self, view: LauncherView, store: &InstanceStore) {
        let view = match view {
            LauncherView::Library => View::Library,
            LauncherView::Instance { instance } => match InstanceInfo::load(store, &instance) {
                Some(info) => View::Instance(info),
                None => View::Other,
            },
            LauncherView::Browse { kind } => View::Browse(kind),
            LauncherView::Project {
                title,
                author,
                icon_url,
                url,
            } => View::Project {
                title,
                author,
                icon_url,
                url,
            },
            LauncherView::Presets => View::Presets,
            LauncherView::Screenshots => View::Screenshots,
            LauncherView::Accounts => View::Accounts,
            LauncherView::Settings => View::Settings,
            LauncherView::About => View::About,
            LauncherView::Other => View::Other,
        };
        let mut model = self.lock_model();
        if matches!(view, View::Library) {
            model.instance_count = instance_count(store);
        }
        model.view = view;
        drop(model);
        self.wake.notify_one();
    }

    /// An instance's settings changed (e.g. hidden from Discord): refresh
    /// anything presence has cached about it.
    pub(crate) fn instance_changed(&self, store: &InstanceStore, slug: &str) {
        let Some(info) = InstanceInfo::load(store, slug) else {
            return;
        };
        let mut model = self.lock_model();
        for game in model.games.iter_mut().filter(|g| g.slug == slug) {
            game.info = info.clone();
        }
        drop(model);
        self.wake.notify_one();
    }

    pub(crate) fn request_reconnect(&self) {
        self.reconnect.store(true, Ordering::SeqCst);
        self.wake.notify_one();
    }

    /// The activity for `scenario` under `config` (the saved config when
    /// `None`, so settings can preview a change before saving it).
    pub(crate) fn preview(
        &self,
        scenario: PreviewScenario,
        config: Option<DiscordConfig>,
    ) -> Option<PresencePreview> {
        let config = config.unwrap_or_else(|| self.config.lock().expect("config poisoned").clone());
        let now = now_ms();
        let model = match scenario {
            PreviewScenario::Live => self.lock_model().clone(),
            other => sample_model(other, now),
        };
        compose(&model, &config, now).map(|a| to_preview(&a.sanitize(), config.status_display))
    }

    fn lock_model(&self) -> std::sync::MutexGuard<'_, Model> {
        self.model.lock().expect("presence model poisoned")
    }

    fn set_status(&self, status: PresenceStatus, events: &broadcast::Sender<Event>) {
        let mut current = self.status.lock().expect("presence status poisoned");
        if *current != status {
            // Waiting -> Waiting (a new retry time) isn't worth a line.
            if std::mem::discriminant(&*current) != std::mem::discriminant(&status) {
                tracing::info!("discord presence: {status:?}");
            }
            *current = status.clone();
            let _ = events.send(Event::PresenceStatusChanged { status });
        }
    }

    /// Fold one session event into the model; returns whether it mattered.
    fn apply(&self, event: Event, store: &InstanceStore) -> bool {
        let mut model = self.lock_model();
        match event {
            Event::InstanceLaunched {
                instance,
                player,
                started_unix,
                ..
            } => {
                let Some(info) = InstanceInfo::load(store, &instance) else {
                    return false;
                };
                model.games.retain(|g| g.slug != instance);
                model.games.push(Game {
                    slug: instance,
                    started_ms: started_unix * 1000,
                    player,
                    info,
                });
            }
            Event::InstanceExited { instance, .. } => {
                model.games.retain(|g| g.slug != instance);
            }
            Event::OverallProgress {
                task_id,
                label,
                bytes_done,
                bytes_total,
                bytes_per_sec,
                files_done,
                files_total,
                ..
            } => {
                let index = match model.tasks.iter().position(|t| t.id == task_id) {
                    Some(i) => i,
                    None => {
                        model.tasks.push(Task {
                            id: task_id,
                            started_ms: now_ms(),
                            label: String::new(),
                            bytes_done: 0,
                            bytes_total: None,
                            bytes_per_sec: 0.0,
                            files_done: 0,
                            files_total: 0,
                        });
                        model.tasks.len() - 1
                    }
                };
                let task = &mut model.tasks[index];
                task.label = label;
                task.bytes_done = bytes_done;
                task.bytes_total = bytes_total;
                task.bytes_per_sec = bytes_per_sec;
                task.files_done = files_done;
                task.files_total = files_total;
            }
            Event::TaskCompleted { task_id } | Event::TaskFailed { task_id, .. } => {
                model.tasks.retain(|t| t.id != task_id);
                model.task_titles.remove(&task_id);
                model.instance_count = instance_count(store);
            }
            Event::Log { .. } | Event::Progress { .. } | Event::PresenceStatusChanged { .. } => {
                return false
            }
        }
        true
    }
}

/// The background task: keep Discord showing [`compose`]'s activity.
///
/// While not connected it tries to reach Discord every
/// [`RETRY_INTERVAL`], indefinitely: closing Discord and opening it again
/// later just works, without the user doing anything.
async fn run(
    presence: Arc<Presence>,
    store: InstanceStore,
    mut events: broadcast::Receiver<Event>,
    events_tx: broadcast::Sender<Event>,
) {
    let mut client: Option<DiscordClient> = None;
    let mut last_sent: Option<Option<Activity>> = None;
    let mut next_update = Instant::now();
    let mut next_attempt = Instant::now();

    loop {
        if presence.reconnect.swap(false, Ordering::SeqCst) {
            if let Some(c) = client.take() {
                let _ = c.close().await;
            }
            last_sent = None;
            next_attempt = Instant::now();
            presence.set_status(PresenceStatus::Connecting, &events_tx);
        }

        let config = presence.config.lock().expect("config poisoned").clone();
        let mut wait_until = None;
        if !config.enabled {
            if let Some(mut c) = client.take() {
                let _ = c.clear().await;
                let _ = c.close().await;
            }
            last_sent = None;
            next_attempt = Instant::now();
            presence.set_status(PresenceStatus::Disabled, &events_tx);
        } else if client.is_none() {
            if Instant::now() >= next_attempt {
                // "Connecting" only for a fresh start; while retrying,
                // stay on "Waiting" instead of flickering every 5 seconds.
                if !matches!(presence.status(), PresenceStatus::Waiting { .. }) {
                    presence.set_status(PresenceStatus::Connecting, &events_tx);
                }
                match DiscordClient::connect(DISCORD_APP_ID).await {
                    Ok(c) => {
                        client = Some(c);
                        last_sent = None;
                        presence.set_status(PresenceStatus::Connected, &events_tx);
                    }
                    Err(err) => {
                        tracing::debug!("Discord not reachable, retrying: {err}");
                        next_attempt = Instant::now() + RETRY_INTERVAL;
                        presence.set_status(
                            PresenceStatus::Waiting {
                                error: err.to_string(),
                            },
                            &events_tx,
                        );
                    }
                }
            }
            if client.is_none() {
                wait_until = Some(next_attempt);
            }
        }

        // Send the current activity if it changed, rate limit permitting.
        if let Some(c) = client.as_mut() {
            let desired = {
                let model = presence.lock_model();
                compose(&model, &config, now_ms())
                    .map(|a| a.status_display(display_type(config.status_display)))
            };
            if last_sent.as_ref() != Some(&desired) {
                if Instant::now() < next_update {
                    wait_until = Some(next_update);
                } else {
                    next_update = Instant::now() + MIN_UPDATE_GAP;
                    match c.set_activity(desired.as_ref()).await {
                        Ok(()) => last_sent = Some(desired),
                        Err(err @ DiscordError::Rejected { .. }) => {
                            // Retrying the same payload won't help; wait
                            // for the next change instead of looping.
                            tracing::warn!("Discord rejected the activity: {err}");
                            last_sent = Some(desired);
                        }
                        Err(err) => {
                            // The connection dropped (Discord quit?): back
                            // to retrying every few seconds.
                            tracing::info!("lost the Discord connection: {err}");
                            client = None;
                            last_sent = None;
                            next_attempt = Instant::now();
                            presence.set_status(PresenceStatus::Connecting, &events_tx);
                            continue;
                        }
                    }
                }
            }
        }

        tokio::select! {
            event = events.recv() => match event {
                Ok(event) => {
                    presence.apply(event, &store);
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return,
            },
            _ = presence.wake.notified() => {}
            _ = sleep_until(wait_until) => {}
        }
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d).await,
        None => std::future::pending().await,
    }
}

fn display_type(display: StatusDisplay) -> StatusDisplayType {
    match display {
        StatusDisplay::Name => StatusDisplayType::Name,
        StatusDisplay::Details => StatusDisplayType::Details,
        StatusDisplay::State => StatusDisplayType::State,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn instance_count(store: &InstanceStore) -> usize {
    store.list_configs().map(|v| v.len()).unwrap_or_default()
}

/// What to show right now, or `None` for no presence at all. Priority: a
/// running game, then a task in progress, then the launcher page.
fn compose(model: &Model, config: &DiscordConfig, now: u64) -> Option<Activity> {
    let mut activity = if let Some(game) = model.games.last() {
        playing(game, model.games.len() - 1, config)
    } else if !config.show_in_launcher {
        return None;
    } else if let Some(task) = model.tasks.first().filter(|_| config.show_tasks) {
        working(task, model, now)
    } else {
        launcher(model, config)
    };
    if config.show_buttons {
        activity = activity.button("Get Bananium", REPO_URL);
    } else {
        activity.buttons.clear();
    }
    if !config.show_elapsed {
        activity.timestamps = None;
    }
    Some(activity)
}

fn playing(game: &Game, others: usize, config: &DiscordConfig) -> Activity {
    let info = &game.info;
    let more = if others > 0 {
        format!(" (+{others} more)")
    } else {
        String::new()
    };
    let mut a = Activity::new().started_at(game.started_ms);
    if info.hidden {
        return a
            .details(format!("Playing Minecraft{more}"))
            .large_image(Art::Minecraft.url(), Some("Minecraft".into()));
    }

    let version = format!("Minecraft {}", info.mc_version);
    let named = config.instance_name_in_status && config.show_instance_name;
    a = a.details(if named {
        format!("Playing {}", info.name)
    } else if config.show_version {
        format!("Playing {version}")
    } else {
        "Playing Minecraft".to_string()
    });

    let mut parts = Vec::new();
    if named && config.show_version {
        parts.push(version.clone());
    }
    if config.show_loader {
        parts.push(loader_label(info, config.show_loader_version));
    }
    if config.show_mod_count && info.mod_count > 0 {
        parts.push(plural(info.mod_count as usize, "mod"));
    }
    if config.show_username && !game.player.is_empty() {
        parts.push(format!("as {}", game.player));
    }
    if !parts.is_empty() || !more.is_empty() {
        a = a.state(format!("{}{more}", parts.join(" · ")));
    }

    let tooltip = if config.show_instance_name {
        info.name.clone()
    } else {
        version.clone()
    };
    a = a.large_image(large_art(info, config), Some(tooltip));
    let (badge, badge_text) = small_art(info, config);
    a = a.small_image(badge, Some(badge_text));
    if let (Some(project), true) = (&info.modrinth_project, config.show_buttons) {
        a = a.button(
            "View modpack",
            format!("https://modrinth.com/modpack/{project}"),
        );
    }
    a
}

fn working(task: &Task, model: &Model, now: u64) -> Activity {
    let (title, icon) = match model.task_titles.get(&task.id) {
        Some((title, icon)) => (title.clone(), icon.clone()),
        None => (default_task_title(task.kind()).to_string(), None),
    };
    let more = model.tasks.len() - 1;
    let pct = task.fraction().map(|f| (f * 100.0).round() as u32);
    let mut state = match (task.bytes_total, pct) {
        (Some(_), Some(p)) if task.files_total > 1 => format!(
            "Downloading {} / {} files · {p}%",
            task.files_done, task.files_total
        ),
        (Some(_), Some(p)) => format!("Downloading · {p}%"),
        (None, Some(p)) => format!("{} · {p}%", task.label),
        _ => task.label.clone(),
    };
    if more > 0 {
        state.push_str(&format!(" (+{more} more)"));
    }

    let mut a = Activity::new()
        .details(title)
        .state(state)
        .started_at(task.started_ms);
    // A progress bar when the remaining time can be estimated.
    if let (Some(total), true) = (task.bytes_total, task.bytes_per_sec > 0.0) {
        let remaining = total.saturating_sub(task.bytes_done) as f64 / task.bytes_per_sec;
        a = a.ends_at(now + (remaining * 1000.0) as u64);
    }
    let badge = if task.kind() == "java" {
        (Art::Java, "Setting up Java")
    } else {
        (Art::Download, "Downloading")
    };
    a.large_image(
        icon.unwrap_or_else(|| Art::Bananium.url()),
        Some(launcher_label()),
    )
    .small_image(badge.0.url(), Some(badge.1.to_string()))
}

fn launcher(model: &Model, config: &DiscordConfig) -> Activity {
    let base = Activity::new()
        .started_at(model.launcher_started_ms)
        .large_image(Art::Bananium.url(), Some(launcher_label()));
    let idle = |state: &str| base.clone().details("In the launcher").state(state);
    match &model.view {
        View::Browse(kind) if config.show_browsing => base
            .clone()
            .details(format!("Browsing {}", kind_label(kind)))
            .state("on Modrinth")
            .small_image(Art::Modrinth.url(), Some("Modrinth".into())),
        View::Project {
            title,
            author,
            icon_url,
            url,
        } if config.show_browsing => {
            let mut a = base
                .clone()
                .details(format!("Looking at {title}"))
                .state(match author {
                    Some(author) => format!("by {author}"),
                    None => "on Modrinth".to_string(),
                })
                .large_image(
                    icon_url.clone().unwrap_or_else(|| Art::Modrinth.url()),
                    Some(title.clone()),
                )
                .small_image(Art::Modrinth.url(), Some("Modrinth".into()));
            if let Some(url) = url {
                a = a.button("View on Modrinth", url.clone());
            }
            a
        }
        View::Instance(info) if !info.hidden => {
            let details = if config.show_instance_name {
                format!("Tweaking {}", info.name)
            } else {
                "Managing an instance".to_string()
            };
            let mut parts = Vec::new();
            if config.show_version {
                parts.push(format!("Minecraft {}", info.mc_version));
            }
            if config.show_loader {
                parts.push(loader_label(info, false));
            }
            let mut a = base.clone().details(details);
            if !parts.is_empty() {
                a = a.state(parts.join(" · "));
            }
            let (badge, badge_text) = small_art(info, config);
            a.large_image(large_art(info, config), Some(info.name.clone()))
                .small_image(badge, Some(badge_text))
        }
        View::Library => idle(&plural(model.instance_count, "instance")),
        View::Presets => idle("Managing presets"),
        View::Screenshots => idle("Looking at screenshots"),
        View::Accounts => idle("Managing accounts"),
        View::Settings => idle("Adjusting settings"),
        View::About => idle("Reading about Bananium"),
        _ => base.clone().details("In the launcher"),
    }
}

fn default_task_title(kind: &str) -> &'static str {
    match kind {
        "install" => "Installing Minecraft",
        "modpack" => "Installing a modpack",
        "content" => "Adding content",
        "preset" => "Applying a preset",
        "java" => "Setting up Java",
        _ => "Working",
    }
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "resource_pack" => "resource packs",
        "shader" => "shaders",
        "modpack" => "modpacks",
        _ => "mods",
    }
}

/// The large image for an instance: its modpack's own icon, else the
/// grass block (Minecraft itself).
fn large_art(info: &InstanceInfo, config: &DiscordConfig) -> String {
    info.icon_url
        .clone()
        .filter(|_| config.show_modpack_icon)
        .unwrap_or_else(|| Art::Minecraft.url())
}

/// The small badge for an instance, with its tooltip: the mod loader's
/// logo, or Bananium's for vanilla (or when the loader is hidden).
fn small_art(info: &InstanceInfo, config: &DiscordConfig) -> (String, String) {
    match info.loader {
        Loader::Fabric if config.show_loader => (
            Art::Fabric.url(),
            loader_label(info, config.show_loader_version),
        ),
        _ => (Art::Bananium.url(), launcher_label()),
    }
}

fn loader_label(info: &InstanceInfo, with_version: bool) -> String {
    match (info.loader, &info.loader_version) {
        (Loader::Vanilla, _) => "Vanilla".to_string(),
        (Loader::Fabric, Some(v)) if with_version => format!("Fabric {v}"),
        (Loader::Fabric, _) => "Fabric".to_string(),
    }
}

fn launcher_label() -> String {
    format!("{APP_NAME} v{}", env!("CARGO_PKG_VERSION"))
}

fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

fn to_preview(a: &Activity, display: StatusDisplay) -> PresencePreview {
    let assets = a.assets.clone().unwrap_or_default();
    PresencePreview {
        app_name: APP_NAME.to_string(),
        details: a.details.clone(),
        state: a.state.clone(),
        large_image: assets.large_image,
        large_text: assets.large_text,
        small_image: assets.small_image,
        small_text: assets.small_text,
        start_ms: a.timestamps.as_ref().and_then(|t| t.start),
        end_ms: a.timestamps.as_ref().and_then(|t| t.end),
        buttons: a
            .buttons
            .iter()
            .map(|b| PreviewButton {
                label: b.label.clone(),
                url: b.url.clone(),
            })
            .collect(),
        status_display: display,
    }
}

/// Stand-in data for the settings preview's canned scenarios.
fn sample_model(scenario: PreviewScenario, now: u64) -> Model {
    let pack = InstanceInfo {
        name: "Fabulously Optimized".into(),
        mc_version: "1.21.1".into(),
        loader: Loader::Fabric,
        loader_version: Some("0.16.9".into()),
        mod_count: 49,
        icon_url: Some(
            "https://cdn.modrinth.com/data/1KVo5zza/d8152911f8fd5d7e9a8c499fe89045af81fe816e_96.webp"
                .into(),
        ),
        modrinth_project: Some("fabulously-optimized".into()),
        hidden: false,
    };
    let mut model = Model {
        launcher_started_ms: now - 12 * 60 * 1000,
        instance_count: 6,
        ..Model::default()
    };
    match scenario {
        PreviewScenario::Live | PreviewScenario::Idle => {}
        PreviewScenario::Browsing => model.view = View::Browse("mod".into()),
        PreviewScenario::Installing => {
            model.tasks.push(Task {
                id: "modpack-1".into(),
                started_ms: now - 20_000,
                label: "Fabulously Optimized files".into(),
                bytes_done: 36_000_000,
                bytes_total: Some(100_000_000),
                bytes_per_sec: 2_000_000.0,
                files_done: 18,
                files_total: 49,
            });
            model.task_titles.insert(
                "modpack-1".into(),
                (
                    "Installing Fabulously Optimized".into(),
                    pack.icon_url.clone(),
                ),
            );
        }
        PreviewScenario::Playing => model.games.push(Game {
            slug: "fabulously-optimized".into(),
            started_ms: now - 42 * 60 * 1000,
            player: "Steve".into(),
            info: pack,
        }),
    }
    model
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    fn cfg() -> DiscordConfig {
        DiscordConfig::default()
    }

    fn fabric_game(icon: bool) -> Game {
        Game {
            slug: "smp".into(),
            started_ms: NOW - 1000,
            player: "Steve".into(),
            info: InstanceInfo {
                name: "Survival SMP".into(),
                mc_version: "1.21.1".into(),
                loader: Loader::Fabric,
                loader_version: Some("0.16.9".into()),
                mod_count: 49,
                icon_url: icon.then(|| "https://cdn.modrinth.com/icon.png".to_string()),
                modrinth_project: icon.then(|| "fo".to_string()),
                hidden: false,
            },
        }
    }

    fn playing_model(game: Game) -> Model {
        Model {
            games: vec![game],
            ..Model::default()
        }
    }

    #[test]
    fn playing_shows_version_loader_mods_and_username() {
        let a = compose(&playing_model(fabric_game(false)), &cfg(), NOW).unwrap();
        assert_eq!(a.details.as_deref(), Some("Playing Minecraft 1.21.1"));
        assert_eq!(
            a.state.as_deref(),
            Some("Fabric 0.16.9 · 49 mods · as Steve")
        );
        let assets = a.assets.unwrap();
        assert_eq!(assets.large_image, Some(Art::Minecraft.url()));
        assert_eq!(assets.large_text.as_deref(), Some("Survival SMP"));
        assert_eq!(assets.small_image, Some(Art::Fabric.url()));
        assert_eq!(assets.small_text.as_deref(), Some("Fabric 0.16.9"));
        assert_eq!(a.timestamps.unwrap().start, Some(NOW - 1000));
        assert_eq!(a.buttons.len(), 1);
    }

    #[test]
    fn modpack_uses_its_icon_and_gets_a_view_button() {
        let a = compose(&playing_model(fabric_game(true)), &cfg(), NOW).unwrap();
        let assets = a.assets.clone().unwrap();
        assert_eq!(
            assets.large_image.as_deref(),
            Some("https://cdn.modrinth.com/icon.png")
        );
        assert_eq!(assets.small_image, Some(Art::Fabric.url()));
        let labels: Vec<_> = a.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["View modpack", "Get Bananium"]);
        assert_eq!(a.buttons[0].url, "https://modrinth.com/modpack/fo");
    }

    #[test]
    fn every_detail_can_be_switched_off() {
        let config = DiscordConfig {
            show_version: false,
            show_loader: false,
            show_mod_count: false,
            show_username: false,
            show_instance_name: false,
            show_modpack_icon: false,
            show_elapsed: false,
            show_buttons: false,
            ..cfg()
        };
        let a = compose(&playing_model(fabric_game(true)), &config, NOW).unwrap();
        assert_eq!(a.details.as_deref(), Some("Playing Minecraft"));
        assert_eq!(a.state, None);
        assert_eq!(a.timestamps, None);
        assert!(a.buttons.is_empty());
        let assets = a.assets.unwrap();
        assert_eq!(assets.large_image, Some(Art::Minecraft.url()));
        assert_eq!(assets.large_text.as_deref(), Some("Minecraft 1.21.1"));
        // With the loader hidden, the badge mustn't give Fabric away.
        assert_eq!(assets.small_image, Some(Art::Bananium.url()));
    }

    #[test]
    fn instance_name_can_lead_the_status() {
        let config = DiscordConfig {
            instance_name_in_status: true,
            ..cfg()
        };
        let a = compose(&playing_model(fabric_game(false)), &config, NOW).unwrap();
        assert_eq!(a.details.as_deref(), Some("Playing Survival SMP"));
        assert!(a.state.unwrap().starts_with("Minecraft 1.21.1 · Fabric"));
    }

    #[test]
    fn hidden_instance_reveals_nothing() {
        let mut game = fabric_game(true);
        game.info.hidden = true;
        let a = compose(&playing_model(game), &cfg(), NOW).unwrap();
        assert_eq!(a.details.as_deref(), Some("Playing Minecraft"));
        assert_eq!(a.state, None);
        assert_eq!(a.assets.unwrap().large_image, Some(Art::Minecraft.url()));
        let labels: Vec<_> = a.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["Get Bananium"]);
    }

    #[test]
    fn vanilla_uses_grass_block_and_mentions_extra_games() {
        let mut first = fabric_game(false);
        first.slug = "a".into();
        let mut second = fabric_game(false);
        second.info.loader = Loader::Vanilla;
        second.info.mod_count = 0;
        let model = Model {
            games: vec![first, second],
            ..Model::default()
        };
        let a = compose(&model, &cfg(), NOW).unwrap();
        assert_eq!(a.state.as_deref(), Some("Vanilla · as Steve (+1 more)"));
        assert_eq!(a.assets.unwrap().large_image, Some(Art::Minecraft.url()));
    }

    #[test]
    fn launcher_states_follow_the_view_and_can_be_hidden() {
        let mut model = Model {
            launcher_started_ms: NOW - 5000,
            instance_count: 3,
            ..Model::default()
        };
        let a = compose(&model, &cfg(), NOW).unwrap();
        assert_eq!(a.details.as_deref(), Some("In the launcher"));
        assert_eq!(a.state.as_deref(), Some("3 instances"));

        model.view = View::Browse("shader".into());
        let a = compose(&model, &cfg(), NOW).unwrap();
        assert_eq!(a.details.as_deref(), Some("Browsing shaders"));

        let no_browsing = DiscordConfig {
            show_browsing: false,
            ..cfg()
        };
        let a = compose(&model, &no_browsing, NOW).unwrap();
        assert_eq!(a.details.as_deref(), Some("In the launcher"));

        model.view = View::Project {
            title: "Sodium".into(),
            author: Some("jellysquid3".into()),
            icon_url: Some("https://cdn.modrinth.com/sodium.png".into()),
            url: Some("https://modrinth.com/mod/sodium".into()),
        };
        let a = compose(&model, &cfg(), NOW).unwrap();
        assert_eq!(a.details.as_deref(), Some("Looking at Sodium"));
        assert_eq!(a.state.as_deref(), Some("by jellysquid3"));
        assert_eq!(a.buttons[0].label, "View on Modrinth");

        let game_only = DiscordConfig {
            show_in_launcher: false,
            ..cfg()
        };
        assert_eq!(compose(&model, &game_only, NOW), None);
        model.games.push(fabric_game(false));
        assert!(compose(&model, &game_only, NOW).is_some());
    }

    #[test]
    fn tasks_show_progress_bar_and_described_title() {
        let mut model = sample_model(PreviewScenario::Installing, NOW);
        let a = compose(&model, &cfg(), NOW).unwrap();
        assert_eq!(
            a.details.as_deref(),
            Some("Installing Fabulously Optimized")
        );
        assert_eq!(a.state.as_deref(), Some("Downloading 18 / 49 files · 36%"));
        let ts = a.timestamps.unwrap();
        // 64 MB left at 2 MB/s.
        assert_eq!(ts.end, Some(NOW + 32_000));

        model.task_titles.clear();
        model.tasks[0].id = "java-9".into();
        let a = compose(&model, &cfg(), NOW).unwrap();
        assert_eq!(a.details.as_deref(), Some("Setting up Java"));
        assert_eq!(a.assets.unwrap().small_image, Some(Art::Java.url()));

        let no_tasks = DiscordConfig {
            show_tasks: false,
            ..cfg()
        };
        let a = compose(&model, &no_tasks, NOW).unwrap();
        assert_eq!(a.details.as_deref(), Some("In the launcher"));
    }

    #[test]
    fn every_scenario_previews_within_discord_limits() {
        let presence = Presence::new(cfg());
        for scenario in [
            PreviewScenario::Live,
            PreviewScenario::Idle,
            PreviewScenario::Browsing,
            PreviewScenario::Installing,
            PreviewScenario::Playing,
        ] {
            let p = presence.preview(scenario, None).unwrap();
            assert_eq!(p.app_name, "Bananium");
            assert!(p.details.is_some(), "{scenario:?}");
            assert!(p.buttons.len() <= 2);
        }
    }

    #[test]
    fn events_update_games_and_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths);
        store.create_named("1.21.1", Some("main")).unwrap();
        let presence = Presence::new(cfg());

        assert!(presence.apply(
            Event::InstanceLaunched {
                instance: "main".into(),
                pid: 1,
                started_unix: 100,
                player: "Alex".into(),
            },
            &store,
        ));
        let preview = presence.preview(PreviewScenario::Live, None).unwrap();
        assert_eq!(preview.details.as_deref(), Some("Playing Minecraft 1.21.1"));
        assert_eq!(preview.state.as_deref(), Some("Vanilla · as Alex"));
        assert_eq!(preview.start_ms, Some(100_000));

        presence.apply(
            Event::InstanceExited {
                instance: "main".into(),
                exit_code: Some(0),
            },
            &store,
        );
        presence.describe_task("content-4", "Adding mods to main".into(), None);
        presence.apply(
            Event::OverallProgress {
                task_id: "content-4".into(),
                label: "Sodium".into(),
                current_file: None,
                bytes_done: 1,
                bytes_total: Some(2),
                bytes_per_sec: 0.0,
                files_done: 0,
                files_total: 1,
            },
            &store,
        );
        let preview = presence.preview(PreviewScenario::Live, None).unwrap();
        assert_eq!(preview.details.as_deref(), Some("Adding mods to main"));
        assert_eq!(preview.state.as_deref(), Some("Downloading · 50%"));

        presence.apply(
            Event::TaskCompleted {
                task_id: "content-4".into(),
            },
            &store,
        );
        let preview = presence.preview(PreviewScenario::Live, None).unwrap();
        assert_eq!(preview.details.as_deref(), Some("In the launcher"));
    }
}
