//! The whole app: three tabs (Instances, Install, Config) over the exact
//! `Command`/`Event` surface `bananium-api` exposes — nothing here reaches
//! outside it, per the frontend contract. See `crate::worker` for how an
//! `async` `Session` gets driven from this synchronous `eframe::App`.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use bananium_api::{
    Command, CommandOutput, Config, Event, InstanceSummary, ResolvedPaths, Session,
};

use crate::worker::Worker;

/// How often the instance list (and with it, every instance's running
/// status) is re-fetched in the background, same idea as the TUI's timed
/// poll — there's no `Event` push for a process exiting on its own yet.
const INSTANCE_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
/// How often the selected instance's log file is re-read for the live tail.
const LOG_TAIL_INTERVAL: Duration = Duration::from_millis(500);
/// Only the last chunk of a log file is ever shown — a multi-hour session's
/// log can be large, and nothing here needs more than recent context.
const LOG_TAIL_MAX_BYTES: u64 = 64 * 1024;

#[derive(PartialEq, Eq, Clone, Copy)]
enum Tab {
    Instances,
    Install,
    Config,
}

/// What a currently in-flight worker reply, once it arrives, should update
/// in `App`'s state. Threaded through explicitly rather than inferred from
/// the returned `CommandOutput` alone, since e.g. `Launch` and `InstanceSet`
/// after a save both eventually trigger the same `InstanceList` refresh.
enum Pending {
    RefreshInstances,
    Launch { instance: String },
    DryRun { instance: String },
    SaveEdit { instance: String },
    Install,
    RefreshConfig,
}

struct InFlight {
    kind: Pending,
    reply: mpsc::Receiver<bananium_api::Result<CommandOutput>>,
}

/// Live state for the install tab's progress bar, rebuilt from
/// `Event::Progress`/`Event::OverallProgress` as they arrive.
#[derive(Default)]
struct InstallProgress {
    current_file: String,
    bytes_done: u64,
    bytes_total: Option<u64>,
    bytes_per_sec: f64,
    files_done: usize,
    files_total: usize,
}

pub struct App {
    worker: Worker,
    events: mpsc::Receiver<Event>,

    tab: Tab,
    status: String,
    in_flight: Option<InFlight>,

    instances: Vec<InstanceSummary>,
    selected: Option<String>,
    filter: String,
    edit_ram: String,
    edit_jvm: String,
    dry_run_output: Option<String>,
    last_log_path: Option<PathBuf>,
    log_tail: String,
    log_tail_read_at: Option<Instant>,
    last_instance_refresh: Instant,

    install_version: String,
    install_name: String,
    install_progress: Option<InstallProgress>,
    install_log: Vec<String>,

    config: Option<(ResolvedPaths, Config)>,
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>, session: Session) -> Self {
        let (worker, events) = Worker::spawn(session);
        let mut app = Self {
            worker,
            events,
            tab: Tab::Instances,
            status: String::new(),
            in_flight: None,
            instances: Vec::new(),
            selected: None,
            filter: String::new(),
            edit_ram: String::new(),
            edit_jvm: String::new(),
            dry_run_output: None,
            last_log_path: None,
            log_tail: String::new(),
            log_tail_read_at: None,
            last_instance_refresh: Instant::now() - INSTANCE_REFRESH_INTERVAL,
            install_version: String::new(),
            install_name: String::new(),
            install_progress: None,
            install_log: Vec::new(),
            config: None,
        };
        app.start(Pending::RefreshInstances, Command::InstanceList);
        app.start(Pending::RefreshConfig, Command::ConfigShow);
        app
    }

    fn start(&mut self, kind: Pending, command: Command) {
        // Only one request in flight at a time keeps `Pending` unambiguous
        // and matches the worker's own single-command-at-a-time loop — a
        // second `start` while one's running replaces the first rather
        // than queuing, so a stale background refresh never blocks a
        // user-initiated action from starting immediately.
        let reply = self.worker.dispatch(command);
        self.in_flight = Some(InFlight { kind, reply });
    }

    fn selected_instance(&self) -> Option<&InstanceSummary> {
        let slug = self.selected.as_deref()?;
        self.instances.iter().find(|i| i.slug == slug)
    }

    fn select(&mut self, slug: String) {
        if let Some(instance) = self.instances.iter().find(|i| i.slug == slug) {
            self.edit_ram = instance.ram_mb.map(|mb| mb.to_string()).unwrap_or_default();
            self.edit_jvm = instance.jvm_args.join(" ");
        }
        self.selected = Some(slug);
        self.dry_run_output = None;
    }

    fn launch(&mut self, instance: String) {
        self.status = format!("launching {instance}...");
        self.start(
            Pending::Launch {
                instance: instance.clone(),
            },
            Command::Launch {
                instance: Some(instance),
                profile: None,
                dry_run: false,
            },
        );
    }

    fn dry_run(&mut self, instance: String) {
        self.start(
            Pending::DryRun {
                instance: instance.clone(),
            },
            Command::Launch {
                instance: Some(instance),
                profile: None,
                dry_run: true,
            },
        );
    }

    /// Validate and submit `edit_ram`/`edit_jvm` via `Command::InstanceSet`.
    /// An empty RAM field clears the cap back to the JVM default (`Some(0)`,
    /// per `Command::InstanceSet`'s sentinel convention); a non-empty field
    /// that fails to parse as `u32` is rejected without dispatching.
    fn save_edit(&mut self, instance: String) {
        let ram_mb = if self.edit_ram.trim().is_empty() {
            Some(0)
        } else {
            match self.edit_ram.trim().parse::<u32>() {
                Ok(mb) => Some(mb),
                Err(_) => {
                    self.status = format!("invalid RAM value {:?}, not saved", self.edit_ram);
                    return;
                }
            }
        };
        let jvm_args = Some(
            self.edit_jvm
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>(),
        );
        self.start(
            Pending::SaveEdit {
                instance: instance.clone(),
            },
            Command::InstanceSet {
                instance,
                ram_mb,
                jvm_args,
            },
        );
    }

    fn start_install(&mut self) {
        let version = self.install_version.trim().to_string();
        let name = if self.install_name.trim().is_empty() {
            None
        } else {
            Some(self.install_name.trim().to_string())
        };
        self.install_log.clear();
        self.install_progress = Some(InstallProgress::default());
        self.status = format!("installing {version}...");
        self.start(Pending::Install, Command::Install { version, name });
    }

    fn drain_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Progress { label, .. } => {
                    self.install_progress
                        .get_or_insert_with(InstallProgress::default)
                        .current_file = label;
                }
                Event::OverallProgress {
                    bytes_done,
                    bytes_total,
                    bytes_per_sec,
                    files_done,
                    files_total,
                    ..
                } => {
                    let progress = self
                        .install_progress
                        .get_or_insert_with(InstallProgress::default);
                    progress.bytes_done = bytes_done;
                    progress.bytes_total = bytes_total;
                    progress.bytes_per_sec = bytes_per_sec;
                    progress.files_done = files_done;
                    progress.files_total = files_total;
                }
                Event::TaskCompleted { task_id } => {
                    self.install_log.push(format!("{task_id}: completed"));
                }
                Event::TaskFailed { task_id, error } => {
                    self.install_log
                        .push(format!("{task_id}: failed — {error}"));
                }
                Event::Log { level, message } => {
                    self.install_log.push(format!("[{level}] {message}"));
                }
            }
        }
    }

    fn poll_in_flight(&mut self) {
        let received = match &self.in_flight {
            Some(in_flight) => in_flight.reply.try_recv(),
            None => return,
        };
        match received {
            Ok(result) => {
                let kind = self.in_flight.take().expect("just matched Some").kind;
                self.handle_result(kind, result);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.in_flight = None;
                self.status = "internal error: worker thread is gone".to_string();
            }
        }
    }

    fn handle_result(&mut self, kind: Pending, result: bananium_api::Result<CommandOutput>) {
        match kind {
            Pending::RefreshInstances => match result {
                Ok(CommandOutput::InstanceListed { instances }) => {
                    self.instances = instances;
                    self.last_instance_refresh = Instant::now();
                    if let Some(selected) = &self.selected {
                        if !self.instances.iter().any(|i| &i.slug == selected) {
                            self.selected = None;
                        }
                    }
                }
                Ok(_) => {}
                Err(err) => self.status = format!("error refreshing instances: {err}"),
            },
            Pending::Launch { instance } => match result {
                Ok(CommandOutput::Launched { pid, log_path, .. }) => {
                    self.status = format!("launched {instance} (pid {pid})");
                    self.last_log_path = Some(log_path);
                    self.log_tail.clear();
                    self.log_tail_read_at = None;
                    self.start(Pending::RefreshInstances, Command::InstanceList);
                }
                Ok(_) => {}
                Err(err) => self.status = format!("error launching {instance}: {err}"),
            },
            Pending::DryRun { instance } => match result {
                Ok(CommandOutput::LaunchPlanned { command_line, .. }) => {
                    self.dry_run_output = Some(command_line);
                    self.status = format!("dry run planned for {instance}");
                }
                Ok(_) => {}
                Err(err) => self.status = format!("error planning launch for {instance}: {err}"),
            },
            Pending::SaveEdit { instance } => match result {
                Ok(CommandOutput::InstanceUpdated { .. }) => {
                    self.status = format!("saved {instance}");
                    self.start(Pending::RefreshInstances, Command::InstanceList);
                }
                Ok(_) => {}
                Err(err) => self.status = format!("error saving {instance}: {err}"),
            },
            Pending::Install => match result {
                Ok(CommandOutput::Installed {
                    instance,
                    mc_version,
                }) => {
                    self.status = format!("installed {mc_version} as instance {instance:?}");
                    self.install_progress = None;
                    self.start(Pending::RefreshInstances, Command::InstanceList);
                }
                Ok(_) => {}
                Err(err) => {
                    self.status = format!("install failed: {err}");
                    self.install_progress = None;
                }
            },
            Pending::RefreshConfig => match result {
                Ok(CommandOutput::ConfigShown { paths, config }) => {
                    self.config = Some((paths, config));
                }
                Ok(_) => {}
                Err(err) => self.status = format!("error loading config: {err}"),
            },
        }
    }

    fn maybe_auto_refresh_instances(&mut self) {
        if self.in_flight.is_some() {
            return;
        }
        if self.last_instance_refresh.elapsed() >= INSTANCE_REFRESH_INTERVAL {
            self.start(Pending::RefreshInstances, Command::InstanceList);
        }
    }

    fn maybe_tail_log(&mut self) {
        let Some(path) = self.last_log_path.clone() else {
            return;
        };
        let due = self
            .log_tail_read_at
            .is_none_or(|t| t.elapsed() >= LOG_TAIL_INTERVAL);
        if !due {
            return;
        }
        self.log_tail_read_at = Some(Instant::now());
        self.log_tail = match read_tail(&path, LOG_TAIL_MAX_BYTES) {
            Ok(text) => text,
            Err(err) => format!("(could not read log: {err})"),
        };
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_events();
        self.poll_in_flight();
        self.maybe_auto_refresh_instances();
        self.maybe_tail_log();

        egui::Panel::top("tabs").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Instances, "Instances");
                ui.selectable_value(&mut self.tab, Tab::Install, "Install");
                ui.selectable_value(&mut self.tab, Tab::Config, "Config");
                if self.in_flight.is_some() {
                    ui.add_space(8.0);
                    ui.spinner();
                }
            });
            ui.add_space(4.0);
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
            });
        });

        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Instances => self.instances_tab(ui),
            Tab::Install => self.install_tab(ui),
            Tab::Config => self.config_tab(ui),
        });

        // Nothing here reacts to OS input events (progress ticks, the log
        // tail, an instance exiting on its own) — request steady repaints
        // while any of that could be moving so state updates actually land
        // on screen instead of waiting for the user to touch the window.
        let anything_live = self.in_flight.is_some() || self.instances.iter().any(|i| i.running);
        if anything_live {
            ui.ctx().request_repaint_after(Duration::from_millis(150));
        }
    }
}

impl App {
    fn instances_tab(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("instance_list")
            .resizable(true)
            .default_size(260.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("filter:");
                    ui.text_edit_singleline(&mut self.filter);
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.instances.is_empty() {
                        ui.weak("no instances installed yet — use the Install tab");
                    }
                    let filter = self.filter.to_lowercase();
                    let matches: Vec<(String, String)> = self
                        .instances
                        .iter()
                        .filter(|i| {
                            filter.is_empty()
                                || i.name.to_lowercase().contains(&filter)
                                || i.slug.to_lowercase().contains(&filter)
                        })
                        .map(|i| {
                            let marker = if i.running { "\u{25cf}" } else { "\u{25cb}" };
                            (
                                i.slug.clone(),
                                format!("{marker} {} ({})", i.name, i.mc_version),
                            )
                        })
                        .collect();
                    for (slug, label) in matches {
                        let is_selected = self.selected.as_deref() == Some(slug.as_str());
                        if ui.selectable_label(is_selected, label).clicked() {
                            self.select(slug);
                        }
                    }
                });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            let Some(instance) = self.selected_instance().cloned() else {
                ui.weak("select an instance on the left");
                return;
            };

            ui.heading(&instance.name);
            ui.label(format!(
                "slug: {}    version: {}",
                instance.slug, instance.mc_version
            ));
            ui.label(if instance.running {
                "status: running"
            } else {
                "status: stopped"
            });
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("RAM cap (MB, blank = JVM default):");
                ui.text_edit_singleline(&mut self.edit_ram);
            });
            ui.label("extra JVM args (space-separated):");
            ui.text_edit_multiline(&mut self.edit_jvm);

            let busy = self.in_flight.is_some();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new("Save changes"))
                    .clicked()
                {
                    self.save_edit(instance.slug.clone());
                }
                if ui
                    .add_enabled(!busy && !instance.running, egui::Button::new("Launch"))
                    .clicked()
                {
                    self.launch(instance.slug.clone());
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("Dry run"))
                    .clicked()
                {
                    self.dry_run(instance.slug.clone());
                }
            });

            if let Some(command_line) = &self.dry_run_output {
                ui.separator();
                ui.label("dry-run command line:");
                let mut text = command_line.as_str();
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .desired_rows(3)
                        .font(egui::TextStyle::Monospace),
                );
            }

            if let Some(log_path) = &self.last_log_path {
                ui.separator();
                ui.label(format!("log: {}", log_path.display()));
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.log_tail)
                                .desired_width(f32::INFINITY)
                                .font(egui::TextStyle::Monospace)
                                .interactive(false),
                        );
                    });
            }
        });
    }

    fn install_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Install a version");
        ui.horizontal(|ui| {
            ui.label("Minecraft version:");
            ui.text_edit_singleline(&mut self.install_version);
        });
        ui.horizontal(|ui| {
            ui.label("instance name (optional, blank = random):");
            ui.text_edit_singleline(&mut self.install_name);
        });

        let busy = self.in_flight.is_some();
        if ui
            .add_enabled(
                !busy && !self.install_version.trim().is_empty(),
                egui::Button::new("Install"),
            )
            .clicked()
        {
            self.start_install();
        }

        if let Some(progress) = &self.install_progress {
            ui.separator();
            let fraction = progress
                .bytes_total
                .filter(|&t| t > 0)
                .map(|t| progress.bytes_done as f32 / t as f32)
                .unwrap_or(0.0);
            ui.add(egui::ProgressBar::new(fraction).show_percentage());
            ui.label(format!(
                "{} / {} files    {} / {}    {}/s",
                progress.files_done,
                progress.files_total,
                format_bytes(progress.bytes_done),
                progress
                    .bytes_total
                    .map(format_bytes)
                    .unwrap_or_else(|| "?".to_string()),
                format_bytes(progress.bytes_per_sec.round() as u64),
            ));
            if !progress.current_file.is_empty() {
                ui.weak(&progress.current_file);
            }
        }

        ui.separator();
        ui.label("log:");
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &self.install_log {
                    ui.monospace(line);
                }
            });
    }

    fn config_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Configuration");
        if ui.button("Refresh").clicked() {
            self.start(Pending::RefreshConfig, Command::ConfigShow);
        }
        ui.separator();

        let Some((paths, config)) = &self.config else {
            ui.weak("loading...");
            return;
        };

        ui.label("paths");
        egui::Grid::new("config_paths")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label("home");
                ui.monospace(paths.home.display().to_string());
                ui.end_row();
                ui.label("config.toml");
                ui.monospace(paths.config_toml.display().to_string());
                ui.end_row();
                ui.label("store");
                ui.monospace(paths.store_dir.display().to_string());
                ui.end_row();
                ui.label("instances");
                ui.monospace(paths.instances_dir.display().to_string());
                ui.end_row();
                ui.label("java");
                ui.monospace(paths.java_dir.display().to_string());
                ui.end_row();
                ui.label("assets");
                ui.monospace(paths.assets_dir.display().to_string());
                ui.end_row();
            });

        ui.add_space(12.0);
        ui.label("settings");
        egui::Grid::new("config_values")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label("max_concurrent_downloads");
                ui.label(config.max_concurrent_downloads.to_string());
                ui.end_row();
                ui.label("theme");
                ui.label(&config.theme);
                ui.end_row();
                ui.label("java_path");
                ui.label(
                    config
                        .java_path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "<auto>".to_string()),
                );
                ui.end_row();
            });
    }
}

/// Read at most the last `max_bytes` of the file at `path`, for a bounded-
/// cost "tail -f"-style view that never has to load an entire, possibly
/// long-running instance's log into memory.
fn read_tail(path: &std::path::Path, max_bytes: u64) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Human-readable byte count (`1536` -> `"1.5 KB"`); mirrors
/// `bananium-cli`'s own copy since sharing it would need a dependency
/// neither frontend crate is allowed to take on the other.
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
