mod content;
mod download;
mod files;
mod instances;
mod java;
mod logs;
mod modpacks;
mod presence;
mod presets;
mod profiles;
mod system;
mod tasks;
mod versions;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use bananium_core::{Config, Paths};
use bananium_instance::{InstanceStore, Loader};
use bananium_launch::{
    build_launch_plan, extract_natives, resolve_libraries, LaunchContext, ProfileStore,
};
use bananium_meta::{merge_fabric, FeatureFlags, MetaClient, Platform, VersionProfile};
use bananium_modrinth::{ModrinthClient, RetryNotice};
use bananium_net::{DownloadSpec, HttpClient};
use bananium_store::BlobStore;
use tokio::sync::{broadcast, oneshot};

use crate::command::Command;
use crate::error::{Error, Result};
use crate::event::Event;
use crate::output::{CommandOutput, ResolvedPaths};
use crate::task::TaskKind;
use presence::Presence;
use tasks::{TaskQueue, TaskSpec, CURRENT_TASK};

/// Sent on every outgoing request. Mojang doesn't require this, but
/// Modrinth's API (M4) rate-limits generic/missing agents — setting a
/// compliant one from the start avoids a churn point later.
const USER_AGENT: &str = concat!(
    "bananium/",
    env!("CARGO_PKG_VERSION"),
    " (github.com/Subhranil-Maity/Bananium)"
);

/// Win32 `CREATE_NO_WINDOW` process-creation flag: start a console program
/// without allocating a console window for it.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The frontend facade. Every frontend — TUI, CLI, RPC, anything else —
/// talks to exactly this: `dispatch(Command) -> Result<CommandOutput>`, and
/// `events()` for a live `Event` stream while that command runs.
pub struct Session {
    paths: Paths,
    /// Behind a lock because Command::ConfigSet changes it in place.
    config: RwLock<Config>,
    http: HttpClient,
    /// One client for the session's lifetime, so its rate-limit tracking
    /// (Modrinth's `X-Ratelimit-*` headers) spans every command.
    modrinth: ModrinthClient,
    events_tx: broadcast::Sender<Event>,
    /// Instance slug -> a trigger that force-kills the game this `Session`
    /// launched for it. Only processes spawned by *this* `Session` can be
    /// stopped: a pid alone isn't safe to kill (the OS may have reused it),
    /// but the `Child` handle held by the launch's wait task is.
    kill_switches: Arc<Mutex<HashMap<String, oneshot::Sender<()>>>>,
    /// Discord Rich Presence. Inert until a frontend calls
    /// [`Session::start_presence`].
    presence: Arc<Presence>,
    /// Every long-running command waits its turn here — see
    /// `session/tasks.rs` for the rules.
    tasks: Arc<TaskQueue>,
}

impl Session {
    /// Ensure `paths`' directories exist and build a `Session` ready to
    /// `dispatch`. One `Session` per frontend process; its `HttpClient` and
    /// event channel are shared across every command it runs.
    pub fn new(paths: Paths, config: Config) -> Result<Self> {
        paths.ensure_dirs()?;
        let http = HttpClient::new(USER_AGENT)?;
        // Progress is throttled at the source (see `download_tracked`), so
        // this only needs headroom for bursts, not for per-file floods.
        let (events_tx, _rx) = broadcast::channel(4096);
        let modrinth = ModrinthClient::new(USER_AGENT)?
            .with_retry_observer(Arc::new(retry_reporter(events_tx.clone())));
        let presence = Arc::new(Presence::new(config.discord.clone()));
        Ok(Self {
            paths,
            presence,
            config: RwLock::new(config),
            http,
            modrinth,
            tasks: Arc::new(TaskQueue::new(events_tx.clone())),
            events_tx,
            kill_switches: Arc::default(),
        })
    }

    /// Subscribe to this session's `Event` stream. Each call creates an
    /// independent `broadcast::Receiver` — multiple frontends (or a TUI
    /// with several panes) can subscribe without stealing events from one
    /// another. Events sent before a given `subscribe()` call are not
    /// replayed to it.
    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.events_tx.subscribe()
    }

    /// Start Discord Rich Presence for this session: connect to the local
    /// Discord client — retrying every few seconds for as long as it isn't
    /// running ([`crate::PresenceStatus::Waiting`]) — and keep it showing
    /// what's going on until the session ends. Only a
    /// long-lived frontend should call this — the desktop app does; the
    /// one-shot CLI and the TUI don't. Calling it again does nothing.
    ///
    /// Must be called from inside a Tokio runtime.
    pub fn start_presence(&self) {
        self.presence
            .start(self.paths.clone(), self.events_tx.clone());
    }

    fn emit(&self, event: Event) {
        // No receivers is a perfectly normal state (e.g. `--format json`
        // frontends that only care about the final result).
        let _ = self.events_tx.send(event);
    }

    /// Run one `Command` to completion, returning its typed result. This is
    /// the *entire* surface every frontend talks to — see the frontend
    /// contract in CONTRIBUTING.md.
    ///
    /// Every command is logged by name (never by content — some carry file
    /// text) with its duration, and every failure with its error. Commands a
    /// frontend polls are logged at `trace` only: the console polls the
    /// launcher log every second, and logging each read would feed itself.
    pub async fn dispatch(&self, command: Command) -> Result<CommandOutput> {
        let kind: &'static str = (&command).into();
        let polled = matches!(
            command,
            Command::LauncherLogRead { .. }
                | Command::LauncherLogList
                | Command::LogRead { .. }
                | Command::LogList { .. }
                | Command::PresenceStatus
                | Command::LogFrontend { .. }
                | Command::TaskList
        );
        let started = std::time::Instant::now();
        let result = self.dispatch_inner(command).await;
        let ms = started.elapsed().as_millis();
        match &result {
            Ok(_) if polled => tracing::trace!(command = kind, ms, "command done"),
            Ok(_) => tracing::debug!(command = kind, ms, "command done"),
            Err(err) => tracing::warn!(command = kind, ms, "command failed: {err}"),
        }
        result
    }

    async fn dispatch_inner(&self, command: Command) -> Result<CommandOutput> {
        match command {
            Command::ConfigShow => self.config_show(),
            Command::ConfigSet {
                max_concurrent_downloads,
                java_path,
            } => self.config_set(max_concurrent_downloads, java_path),
            Command::JavaList => self.java_list().await,
            Command::JavaRuntimeRemove { component } => self.java_runtime_remove(&component),
            Command::InstanceJava { instance } => self.instance_java(&instance).await,
            Command::ScreenshotList { instance } => self.screenshot_list(instance.as_deref()),
            Command::ScreenshotDelete { path } => self.screenshot_delete(&path),
            Command::Install {
                version,
                name,
                fabric_loader,
                group,
            } => {
                let out = self
                    .install_tracked(&version, name.as_deref(), fabric_loader.as_deref())
                    .await?;
                if let (Some(group), CommandOutput::Installed { instance, .. }) = (group, &out) {
                    self.instance_set(instance, None, None, None, Some(group))?;
                }
                Ok(out)
            }
            Command::VersionList { include_snapshots } => {
                self.version_list(include_snapshots).await
            }
            Command::FabricLoaderList { mc_version } => self.fabric_loader_list(&mc_version).await,
            Command::Launch {
                instance,
                profile,
                dry_run,
            } => {
                self.launch(instance.as_deref(), profile.as_deref(), dry_run)
                    .await
            }
            Command::InstanceList => self.instance_list(),
            Command::InstanceSet {
                instance,
                ram_mb,
                jvm_args,
                java_path,
                group,
            } => self.instance_set(&instance, ram_mb, jvm_args, java_path, group),
            Command::InstanceSetIcon { instance, path } => {
                self.instance_set_icon(&instance, path.as_deref())
            }
            Command::InstanceRemove { instance } => {
                self.tasks.ensure_idle(&instance, true)?;
                self.instance_remove(&instance)
            }
            Command::InstanceRename { instance, new_name } => {
                self.tasks.ensure_idle(&instance, true)?;
                self.instance_rename(&instance, &new_name)
            }
            Command::InstanceClone { instance, new_name } => {
                self.instance_clone(&instance, &new_name)
            }
            Command::InstanceKill { instance } => self.instance_kill(&instance),
            Command::LogList { instance } => self.log_list(&instance),
            Command::LogRead {
                instance,
                file,
                offset,
            } => self.log_read(&instance, file.as_deref(), offset),
            Command::FileList { instance, path } => self.file_list(&instance, &path),
            Command::FileRead { instance, path } => self.file_read(&instance, &path),
            Command::FileWrite {
                instance,
                path,
                text,
                create_new,
            } => {
                self.tasks.ensure_idle(&instance, false)?;
                self.file_write(&instance, &path, &text, create_new)
            }
            Command::FileCreateDir { instance, path } => {
                self.tasks.ensure_idle(&instance, false)?;
                self.file_create_dir(&instance, &path)
            }
            Command::FileRename { instance, from, to } => {
                self.tasks.ensure_idle(&instance, false)?;
                self.file_rename(&instance, &from, &to)
            }
            Command::FileDelete { instance, path } => {
                self.tasks.ensure_idle(&instance, false)?;
                self.file_delete(&instance, &path)
            }
            Command::FileImport {
                instance,
                path,
                sources,
            } => {
                self.tasks.ensure_idle(&instance, false)?;
                self.file_import(&instance, &path, &sources)
            }
            Command::ModrinthSearch {
                query,
                kind,
                instance,
                categories,
                sort,
                offset,
                limit,
            } => {
                self.modrinth_search(
                    &query,
                    kind,
                    instance.as_deref(),
                    &categories,
                    sort,
                    offset,
                    limit,
                )
                .await
            }
            Command::ModrinthProject { project } => self.modrinth_project(&project).await,
            Command::ModpackSearch {
                query,
                categories,
                sort,
                offset,
                limit,
            } => {
                self.modpack_search(&query, &categories, sort, offset, limit)
                    .await
            }
            Command::ModpackVersions { project } => self.modpack_versions(&project).await,
            Command::ModpackInspect { path } => self.modpack_inspect(&path),
            Command::ModpackInstall {
                source,
                name,
                group,
            } => {
                self.modpack_install_tracked(&source, name.as_deref(), group.as_deref())
                    .await
            }
            Command::ModrinthVersions {
                project,
                kind,
                instance,
            } => {
                self.modrinth_versions(&project, kind, instance.as_deref())
                    .await
            }
            Command::ContentList { instance } => self.content_list(&instance),
            Command::ContentInstall {
                instance,
                kind,
                project,
                version,
            } => {
                self.content_install_tracked(&instance, project, version, kind)
                    .await
            }
            Command::ContentRemove {
                instance,
                kind,
                filename,
            } => {
                self.tasks.ensure_idle(&instance, false)?;
                self.content_remove(&instance, kind, &filename)
            }
            Command::ContentToggle {
                instance,
                kind,
                filename,
                enabled,
            } => {
                self.tasks.ensure_idle(&instance, false)?;
                self.content_toggle(&instance, kind, &filename, enabled)
            }
            Command::ContentImport {
                instance,
                kind,
                path,
            } => {
                self.tasks.ensure_idle(&instance, false)?;
                self.content_import(&instance, kind, &path).await
            }
            Command::ContentIdentify { instance } => self.content_identify(&instance).await,
            Command::ContentCheckUpdates { instance } => {
                self.content_check_updates(&instance).await
            }
            Command::ContentUpdate { instance, projects } => {
                self.content_update(&instance, &projects).await
            }
            Command::PresetList => self.preset_list(),
            Command::PresetSave {
                instance,
                name,
                kinds,
            } => self.preset_save(&instance, &name, &kinds),
            Command::PresetApply { preset, instance } => {
                self.preset_apply(&preset, &instance).await
            }
            Command::PresetDelete { name } => self.preset_delete(&name),
            Command::PresetRename { name, new_name } => self.preset_rename(&name, &new_name),
            Command::PresetExport { name, path } => self.preset_export(&name, &path),
            Command::PresetImport { path } => self.preset_import(&path),
            Command::ProfileList => self.profile_list(),
            Command::ProfileAdd { name } => self.profile_add(&name),
            Command::ProfileRemove { name } => self.profile_remove(&name),
            Command::ProfileSetDefault { name } => self.profile_set_default(&name),
            Command::DiscordConfigSet { config } => self.discord_config_set(config),
            Command::InstanceSetDiscord { instance, hidden } => {
                self.instance_set_discord(&instance, hidden)
            }
            Command::PresenceSetView { view } => self.presence_set_view(view),
            Command::PresenceStatus => Ok(CommandOutput::PresenceStatusShown {
                status: self.presence.status(),
            }),
            Command::PresencePreview { scenario, config } => Ok(CommandOutput::PresencePreviewed {
                preview: self.presence.preview(scenario, config),
            }),
            Command::PresenceReconnect => {
                self.presence.request_reconnect();
                Ok(CommandOutput::PresenceStatusShown {
                    status: self.presence.status(),
                })
            }
            Command::LauncherLogList => self.launcher_log_list().await,
            Command::LauncherLogRead {
                file,
                offset,
                before,
            } => self.launcher_log_read(file, offset, before).await,
            Command::LauncherLastSession => self.launcher_last_session().await,
            Command::LogFrontend { level, message } => self.log_frontend(&level, &message),
            Command::TaskList => Ok(CommandOutput::TaskListed {
                tasks: self.tasks.list(),
            }),
            Command::TaskCancel { task_id } => {
                self.tasks.cancel(&task_id)?;
                Ok(CommandOutput::TaskCancelled { task_id })
            }
        }
    }

    fn config_show(&self) -> Result<CommandOutput> {
        Ok(CommandOutput::ConfigShown {
            paths: ResolvedPaths {
                home: self.paths.home().to_path_buf(),
                config_toml: self.paths.config_toml(),
                store_dir: self.paths.store_dir(),
                instances_dir: self.paths.instances_dir(),
                java_dir: self.paths.java_dir(),
                assets_dir: self.paths.assets_dir(),
                logs_dir: self.paths.logs_dir(),
            },
            config: self.config(),
        })
    }

    /// The launch profile for `mc_version` with `loader` applied: the plain
    /// vanilla profile, or vanilla merged with the pinned Fabric loader
    /// profile. `install` and `launch` both go through this, so what gets
    /// downloaded is exactly what a later launch puts on the classpath.
    /// Returns `(vanilla, effective)` — install needs vanilla's id (the
    /// instance's Minecraft version) and asset index either way.
    async fn resolve_profile(
        &self,
        meta: &MetaClient,
        mc_version: &str,
        loader: Loader,
        loader_version: Option<&str>,
    ) -> Result<(VersionProfile, VersionProfile)> {
        let entry = meta.resolve_version(mc_version).await?;
        let vanilla = meta.version_profile(&entry).await?;
        let effective = match (loader, loader_version) {
            (Loader::Vanilla, _) => vanilla.clone(),
            (Loader::Fabric, Some(v)) => {
                let fabric = meta.fabric_profile(&vanilla.id, v).await?;
                merge_fabric(&vanilla, &fabric)?
            }
            (Loader::Fabric, None) => return Err(Error::MissingLoaderVersion),
        };
        Ok((vanilla, effective))
    }

    /// `Command::Install`, reported as one task: see [`Session::install`].
    async fn install_tracked(
        &self,
        version: &str,
        name: Option<&str>,
        fabric_loader: Option<&str>,
    ) -> Result<CommandOutput> {
        let label = format!(
            "Installing {}",
            name.unwrap_or(&format!("Minecraft {version}"))
        );
        let mut spec = TaskSpec::new(TaskKind::Install, label.clone());
        if let Some(name) = name {
            spec = spec.instance(InstanceStore::slugify(name));
        }
        let ticket = self.enqueue_task("install", spec, || Ok(()))?;
        let task_id = ticket.task_id().to_string();
        self.presence.describe_task(&task_id, label, None);
        self.tracked(ticket, self.install(&task_id, version, name, fabric_loader))
            .await
    }

    /// `Command::Install`: resolve the version (and Fabric loader, if
    /// asked for), download the client jar + every applicable library/
    /// natives jar + every asset object (skipping anything already verified
    /// in the store), materialize the assets tree into the Mojang-shaped
    /// layout the JVM expects, and create an instance for it named `name`
    /// (a fresh random name when `None`) if one doesn't already exist under
    /// that name. Re-running it on an existing vanilla instance with a
    /// loader converts that instance to the loader.
    ///
    /// Downloads use [`resolve_libraries`] — the *same* library-resolution
    /// logic `launch` uses to build the classpath — so what gets fetched
    /// here is exactly what a later launch will need; nothing is fetched
    /// speculatively and nothing needed is skipped.
    async fn install(
        &self,
        task_id: &str,
        version: &str,
        name: Option<&str>,
        fabric_loader: Option<&str>,
    ) -> Result<CommandOutput> {
        tracing::info!(
            "installing Minecraft {version} (fabric: {})",
            fabric_loader.unwrap_or("none")
        );
        let meta = MetaClient::new(self.http.clone(), self.paths.clone());
        let (loader, loader_version) = match fabric_loader {
            Some(requested) => {
                let entry = meta.resolve_version(version).await?;
                let v = meta.resolve_fabric_loader(&entry.id, requested).await?;
                (Loader::Fabric, Some(v))
            }
            None => (Loader::Vanilla, None),
        };
        let (vanilla, profile) = self
            .resolve_profile(&meta, version, loader, loader_version.as_deref())
            .await?;
        let asset_index = meta.asset_index(&profile.asset_index).await?;

        let platform = Platform::current();
        let features = FeatureFlags::default();
        let resolved = resolve_libraries(&profile, &platform, &features);

        let mut specs = vec![DownloadSpec {
            url: profile.downloads.client.url.clone(),
            dest: self.paths.store_blob(&profile.downloads.client.sha1),
            expected_sha1: Some(profile.downloads.client.sha1.clone()),
            expected_size: Some(profile.downloads.client.size),
            task_id: format!("{task_id}/client"),
            label: format!("Minecraft {} client", vanilla.id),
        }];
        // A size of 0 means the metadata didn't publish one (Fabric's
        // loader/intermediary jars); verify by sha1 alone.
        let known = |size: u64| (size > 0).then_some(size);
        for artifact in &resolved.classpath {
            specs.push(DownloadSpec {
                url: artifact.url.clone(),
                dest: self.paths.store_blob(&artifact.sha1),
                expected_sha1: Some(artifact.sha1.clone()),
                expected_size: known(artifact.size),
                task_id: format!("{task_id}/lib:{}", artifact.sha1),
                label: artifact.name.clone(),
            });
        }
        for entry in &resolved.natives {
            specs.push(DownloadSpec {
                url: entry.artifact.url.clone(),
                dest: self.paths.store_blob(&entry.artifact.sha1),
                expected_sha1: Some(entry.artifact.sha1.clone()),
                expected_size: known(entry.artifact.size),
                task_id: format!("{task_id}/natives:{}", entry.artifact.sha1),
                label: entry.artifact.name.clone(),
            });
        }
        for (name, object) in &asset_index.objects {
            specs.push(DownloadSpec {
                url: format!(
                    "https://resources.download.minecraft.net/{}",
                    object.object_path()
                ),
                dest: self.paths.store_blob(&object.hash),
                expected_sha1: Some(object.hash.clone()),
                expected_size: Some(object.size),
                task_id: format!("{task_id}/asset:{}", object.hash),
                label: name.clone(),
            });
        }

        let label = match &loader_version {
            Some(v) => format!("Minecraft {} + Fabric {v}", vanilla.id),
            None => format!("Minecraft {}", vanilla.id),
        };
        tracing::info!(
            "resolved {label}: {} libraries, {} natives, {} assets ({} files to check)",
            resolved.classpath.len(),
            resolved.natives.len(),
            asset_index.objects.len(),
            specs.len()
        );
        self.download_tracked(task_id, &label, specs).await?;

        // Materialize the assets tree into the shape the JVM expects.
        // Thousands of small files: done in batches off the async runtime
        // (so other commands and events keep flowing), reported between
        // batches so the task doesn't look stuck at 100% while this runs.
        let asset_total = asset_index.objects.len();
        let mut links: Vec<(String, std::path::PathBuf)> = Vec::new();
        for (name, object) in &asset_index.objects {
            links.push((
                object.hash.clone(),
                self.paths.assets_objects_dir().join(object.object_path()),
            ));
            if asset_index.is_virtual {
                links.push((
                    object.hash.clone(),
                    self.paths.assets_virtual_dir(&profile.assets).join(name),
                ));
            }
        }
        let mut last_emit = None;
        let mut done = 0;
        for batch in links.chunks(256) {
            let store = BlobStore::new(self.paths.clone());
            let owned = batch.to_vec();
            blocking(move || {
                for (hash, dest) in &owned {
                    store.materialize(hash, dest)?;
                }
                Ok(())
            })
            .await?;
            done += batch.len();
            self.phase_progress(
                task_id,
                "Preparing game assets",
                done,
                links.len(),
                &mut last_emit,
            );
        }
        let index_cache_path = self
            .paths
            .meta_dir()
            .join("asset_indexes")
            .join(format!("{}.json", profile.assets));
        if index_cache_path.is_file() {
            std::fs::create_dir_all(self.paths.assets_indexes_dir())?;
            std::fs::copy(
                &index_cache_path,
                self.paths.assets_index_json(&profile.assets),
            )?;
        }

        // The Java runtime Mojang's profile names, so the instance is ready
        // to launch — offline, even — the moment it exists. `None` just means
        // Mojang has no runtime for this platform; launch handles that.
        tracing::debug!("assets materialized ({asset_total} objects)");
        let (component, _) = java::required_runtime(&profile);
        self.ensure_runtime(task_id, &component).await?;

        let instances = InstanceStore::new(self.paths.clone());
        let slug = instances.create_named(&vanilla.id, name)?;
        if loader != Loader::Vanilla {
            let mut cfg = instances.load(&slug)?;
            cfg.loader = loader;
            cfg.loader_version = loader_version;
            instances.save(&slug, &cfg)?;
        }
        tracing::info!("installed {label} as instance {slug:?}");

        Ok(CommandOutput::Installed {
            instance: slug,
            mc_version: vanilla.id,
        })
    }

    /// `Command::Launch`: resolve the instance and its version profile
    /// (both from cache if offline), rebuild the same [`resolve_libraries`]
    /// result `install` used to fetch everything, extract natives,
    /// assemble the classpath from store paths (no filesystem copying
    /// needed — see `bananium-store`'s doc comment on why), and build a
    /// [`bananium_launch::LaunchPlan`]. `dry_run` stops right there and
    /// returns the rendered command line instead of spawning it.
    async fn launch(
        &self,
        instance: Option<&str>,
        profile_name: Option<&str>,
        dry_run: bool,
    ) -> Result<CommandOutput> {
        let instances = InstanceStore::new(self.paths.clone());
        let slug = instances.resolve(instance)?;
        let instance_cfg = instances.load(&slug)?;

        // Checked before doing any of the (potentially slow, network-
        // touching) work below, and skipped for `dry_run` since that never
        // actually spawns anything. Only a real launch needs to actually
        // record a pid, so only a real launch needs to check for one first.
        if !dry_run && instances.is_running(&slug)? {
            return Err(Error::InstanceAlreadyRunning(slug));
        }
        // Launching mid-install would start a half-built game.
        if !dry_run {
            self.tasks.ensure_idle(&slug, false)?;
        }
        tracing::info!(
            instance = %slug,
            mc_version = %instance_cfg.mc_version,
            loader = ?instance_cfg.loader,
            dry_run,
            "preparing launch"
        );

        let meta = MetaClient::new(self.http.clone(), self.paths.clone());
        let (_, profile) = self
            .resolve_profile(
                &meta,
                &instance_cfg.mc_version,
                instance_cfg.loader,
                instance_cfg.loader_version.as_deref(),
            )
            .await?;

        let platform = Platform::current();
        let features = FeatureFlags::default();
        let resolved = resolve_libraries(&profile, &platform, &features);
        let natives_dir = extract_natives(&self.paths, &resolved.natives, &platform)?;

        // `.jar`-named links rather than raw store paths — see
        // `Paths::jar_link` for why the extension matters to Fabric.
        let blobs = BlobStore::new(self.paths.clone());
        let jar = |sha1: &str| -> Result<std::path::PathBuf> {
            let link = self.paths.jar_link(sha1);
            blobs.materialize(sha1, &link)?;
            Ok(link)
        };
        let mut classpath = vec![jar(&profile.downloads.client.sha1)?];
        for artifact in &resolved.classpath {
            classpath.push(jar(&artifact.sha1)?);
        }

        let profiles = ProfileStore::new(self.paths.clone());
        let local_profile = match profile_name {
            Some(name) => profiles.get_or_create(name)?,
            None => profiles.default_profile()?,
        };

        // Mojang's runtime for this version unless the user chose a Java;
        // downloaded here, before anything is spawned, if it's missing.
        let java_path = self.resolve_java(&instance_cfg, &profile, dry_run).await?;

        let ctx = LaunchContext {
            player_name: local_profile.name,
            player_uuid: local_profile.uuid,
            access_token: "0".to_string(),
            user_type: "legacy".to_string(),
            game_directory: self.paths.instance_minecraft_dir(&slug),
            assets_root: self.paths.assets_dir(),
            natives_directory: natives_dir,
            classpath,
            launcher_name: "bananium".to_string(),
            launcher_version: env!("CARGO_PKG_VERSION").to_string(),
            ram_mb: instance_cfg.ram_mb,
            extra_jvm_args: instance_cfg.jvm_args.clone(),
        };

        tracing::info!(
            instance = %slug,
            java = %java_path.display(),
            player = %ctx.player_name,
            ram_mb = ?ctx.ram_mb,
            "launch resolved"
        );
        let plan = build_launch_plan(&profile, &platform, &features, &ctx, java_path);
        tracing::debug!(instance = %slug, "command line: {}", plan.command_line());

        if dry_run {
            return Ok(CommandOutput::LaunchPlanned {
                instance: slug,
                command_line: plan.command_line(),
            });
        }

        std::fs::create_dir_all(&ctx.game_directory)?;

        // The JVM's stdout/stderr must never inherit the frontend's own —
        // for the TUI that's raw-mode/alternate-screen terminal state, and
        // Minecraft's log spam would otherwise get interleaved with (and
        // corrupt) the redrawn UI on every frame. Redirect both to a
        // per-launch file under `instance_logs_dir` instead; stdin is
        // closed outright since nothing here ever feeds this process input.
        let logs_dir = self.paths.instance_logs_dir(&slug);
        std::fs::create_dir_all(&logs_dir)?;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let log_path = logs_dir.join(format!("launch-{timestamp}.log"));
        let stdout_log = std::fs::File::create(&log_path)?;
        let stderr_log = stdout_log.try_clone()?;

        let mut cmd = tokio::process::Command::from(plan.to_command());
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::from(stdout_log))
            .stderr(std::process::Stdio::from(stderr_log));
        // `java.exe` is a console program: spawned from a GUI-subsystem
        // parent (the release desktop build has no console), Windows would
        // pop up a fresh, empty console window for it. All stdio is already
        // redirected above, so suppress that window outright.
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        let mut child = cmd.spawn().inspect_err(|err| {
            tracing::error!(instance = %slug, "failed to start the game: {err}");
        })?;
        let pid = child.id().unwrap_or(0);
        let started = std::time::SystemTime::now();
        tracing::info!(
            instance = %slug,
            pid,
            log = %log_path.display(),
            "game started"
        );
        instances.mark_running(&slug, pid, Some(&log_path))?;
        self.emit(Event::InstanceLaunched {
            instance: slug.clone(),
            pid,
            started_unix: timestamp,
            player: ctx.player_name.clone(),
        });

        // The caller gets the pid back immediately; this task outlives the
        // command. For a long-lived frontend (the desktop app, the TUI) it
        // clears the pid and announces the exit the moment the game quits.
        // A one-shot CLI process exits before the game does, dropping this
        // task — which is why `InstanceStore::running_pids` still also
        // sweeps dead pids lazily on every read.
        let (kill_tx, kill_rx) = oneshot::channel();
        self.kill_switches
            .lock()
            .expect("kill switch mutex poisoned")
            .insert(slug.clone(), kill_tx);
        let kill_switches = self.kill_switches.clone();
        let events_tx = self.events_tx.clone();
        let exit_paths = self.paths.clone();
        let exit_slug = slug.clone();
        let game_dir = ctx.game_directory.clone();
        let game_log = log_path.clone();
        tokio::spawn(async move {
            let mut killed = false;
            let status = tokio::select! {
                status = child.wait() => status,
                Ok(()) = kill_rx => {
                    killed = true;
                    let _ = child.start_kill();
                    child.wait().await
                }
            };
            kill_switches
                .lock()
                .expect("kill switch mutex poisoned")
                .remove(&exit_slug);
            log_game_exit(&exit_slug, &status, killed, started, &game_dir, &game_log);
            if let Err(err) = InstanceStore::new(exit_paths).mark_exited(&exit_slug, pid) {
                tracing::warn!(instance = %exit_slug, "couldn't record the game's exit: {err}");
            }
            let _ = events_tx.send(Event::InstanceExited {
                instance: exit_slug,
                exit_code: status.ok().and_then(|s| s.code()),
            });
        });

        Ok(CommandOutput::Launched {
            instance: slug,
            pid,
            log_path,
        })
    }
}

/// The Modrinth client's retry observer: report each retry against the task
/// that made the request (read from [`CURRENT_TASK`]), or as a service-wide
/// notice when no task did — a search or a project page the UI is loading.
fn retry_reporter(events: broadcast::Sender<Event>) -> impl Fn(&RetryNotice) + Send + Sync {
    move |notice: &RetryNotice| {
        let mut reason = notice.reason.to_string();
        if notice.wait.as_secs() >= 1 {
            reason = format!("{reason}; waiting {}s", notice.wait.as_secs());
        }
        let event = match CURRENT_TASK.try_with(Clone::clone) {
            Ok(task_id) => {
                tracing::warn!(
                    target: "bananium_api::task",
                    task = %task_id,
                    endpoint = %notice.endpoint,
                    "task retrying: attempt {}/{}: {reason}",
                    notice.attempt,
                    notice.max_attempts
                );
                Event::TaskRetrying {
                    task_id,
                    attempt: notice.attempt,
                    max_attempts: notice.max_attempts,
                    reason,
                }
            }
            Err(_) => Event::ServiceRetrying {
                service: "modrinth".to_string(),
                attempt: notice.attempt,
                max_attempts: notice.max_attempts,
                reason,
            },
        };
        let _ = events.send(event);
    }
}

/// Run blocking file work (hashing, zip extraction, thousands of small
/// file operations) off the async runtime, so it can't stall other commands
/// and the event stream while it runs.
pub(super) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| Error::Io(std::io::Error::other(e)))?
}

/// Log how a launched game ended. A non-zero exit (other than one we asked
/// for with `InstanceKill`) is logged as a crash, naming the files that
/// explain it: the game's output log, and any crash report or JVM fatal
/// error log written since it started.
fn log_game_exit(
    slug: &str,
    status: &std::io::Result<std::process::ExitStatus>,
    killed: bool,
    started: std::time::SystemTime,
    game_dir: &std::path::Path,
    game_log: &std::path::Path,
) {
    let secs = started.elapsed().map(|d| d.as_secs()).unwrap_or_default();
    match status {
        Err(err) => tracing::error!(instance = %slug, "lost track of the game process: {err}"),
        Ok(_) if killed => {
            tracing::info!(instance = %slug, "game stopped by the user after {secs}s")
        }
        Ok(s) if s.success() => {
            tracing::info!(instance = %slug, "game exited normally after {secs}s")
        }
        Ok(s) => {
            let code = s
                .code()
                .map_or_else(|| "none (killed by a signal)".into(), |c| c.to_string());
            tracing::error!(
                instance = %slug,
                "game crashed: exit code {code} after {secs}s; game log: {}",
                game_log.display()
            );
            for report in crash_artifacts(game_dir, started) {
                tracing::error!(instance = %slug, "crash report: {}", report.display());
            }
        }
    }
}

/// Minecraft crash reports (`crash-reports/*.txt`) and JVM fatal error logs
/// (`hs_err_pid*.log`) in `game_dir` modified at or after `since`.
fn crash_artifacts(
    game_dir: &std::path::Path,
    since: std::time::SystemTime,
) -> Vec<std::path::PathBuf> {
    let recent = |e: &std::fs::DirEntry| {
        e.metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| t >= since)
    };
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(game_dir.join("crash-reports")) {
        found.extend(
            entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "txt") && recent(e))
                .map(|e| e.path()),
        );
    }
    if let Ok(entries) = std::fs::read_dir(game_dir) {
        found.extend(
            entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("hs_err_pid") && recent(e))
                .map(|e| e.path()),
        );
    }
    found
}
