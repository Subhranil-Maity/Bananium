mod content;
mod download;
mod files;
mod instances;
mod java;
mod logs;
mod modpacks;
mod presets;
mod profiles;
mod system;
mod versions;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use bananium_core::{Config, Paths};
use bananium_instance::{InstanceStore, Loader};
use bananium_launch::{
    build_launch_plan, extract_natives, resolve_libraries, LaunchContext, ProfileStore,
};
use bananium_meta::{merge_fabric, FeatureFlags, MetaClient, Platform, VersionProfile};
use bananium_modrinth::ModrinthClient;
use bananium_net::{DownloadSpec, HttpClient};
use bananium_store::BlobStore;
use tokio::sync::{broadcast, oneshot};

use crate::command::Command;
use crate::error::{Error, Result};
use crate::event::Event;
use crate::output::{CommandOutput, ResolvedPaths};

/// Sent on every outgoing request. Mojang doesn't require this, but
/// Modrinth's API (M4) rate-limits generic/missing agents — setting a
/// compliant one from the start avoids a churn point later.
const USER_AGENT: &str = concat!(
    "bananium/",
    env!("CARGO_PKG_VERSION"),
    " (github.com/bananium/bananium)"
);

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
}

impl Session {
    /// Ensure `paths`' directories exist and build a `Session` ready to
    /// `dispatch`. One `Session` per frontend process; its `HttpClient` and
    /// event channel are shared across every command it runs.
    pub fn new(paths: Paths, config: Config) -> Result<Self> {
        paths.ensure_dirs()?;
        let http = HttpClient::new(USER_AGENT)?;
        let modrinth = ModrinthClient::new(USER_AGENT)?;
        // Progress is throttled at the source (see `download_tracked`), so
        // this only needs headroom for bursts, not for per-file floods.
        let (events_tx, _rx) = broadcast::channel(4096);
        Ok(Self {
            paths,
            config: RwLock::new(config),
            http,
            modrinth,
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

    fn emit(&self, event: Event) {
        // No receivers is a perfectly normal state (e.g. `--format json`
        // frontends that only care about the final result).
        let _ = self.events_tx.send(event);
    }

    /// Run one `Command` to completion, returning its typed result. This is
    /// the *entire* surface every frontend talks to — see PLAN.md's
    /// "frontend contract."
    pub async fn dispatch(&self, command: Command) -> Result<CommandOutput> {
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
            Command::InstanceRemove { instance } => self.instance_remove(&instance),
            Command::InstanceRename { instance, new_name } => {
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
            } => self.file_write(&instance, &path, &text, create_new),
            Command::FileCreateDir { instance, path } => self.file_create_dir(&instance, &path),
            Command::FileRename { instance, from, to } => self.file_rename(&instance, &from, &to),
            Command::FileDelete { instance, path } => self.file_delete(&instance, &path),
            Command::FileImport {
                instance,
                path,
                sources,
            } => self.file_import(&instance, &path, &sources),
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
                self.content_install_tracked(&instance, vec![(project, version, kind)])
                    .await
            }
            Command::ContentRemove {
                instance,
                kind,
                filename,
            } => self.content_remove(&instance, kind, &filename),
            Command::ContentToggle {
                instance,
                kind,
                filename,
                enabled,
            } => self.content_toggle(&instance, kind, &filename, enabled),
            Command::ContentImport {
                instance,
                kind,
                path,
            } => self.content_import(&instance, kind, &path).await,
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
        let task_id = self.new_task_id("install");
        self.tracked(
            &task_id,
            self.install(&task_id, version, name, fabric_loader),
        )
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
        self.download_tracked(task_id, &label, specs).await?;

        // Materialize the assets tree into the shape the JVM expects.
        // Thousands of small files; reported so the task doesn't look stuck
        // at 100% while this runs.
        let store = BlobStore::new(self.paths.clone());
        let asset_total = asset_index.objects.len();
        let mut last_emit = None;
        for (i, (name, object)) in asset_index.objects.iter().enumerate() {
            self.phase_progress(
                task_id,
                "Preparing game assets",
                i + 1,
                asset_total,
                &mut last_emit,
            );
            let object_dest = self.paths.assets_objects_dir().join(object.object_path());
            store.materialize(&object.hash, &object_dest)?;
            if asset_index.is_virtual {
                let virtual_dest = self.paths.assets_virtual_dir(&profile.assets).join(name);
                store.materialize(&object.hash, &virtual_dest)?;
            }
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

        let plan = build_launch_plan(&profile, &platform, &features, &ctx, java_path);

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
        let mut child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);
        instances.mark_running(&slug, pid, Some(&log_path))?;

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
        tokio::spawn(async move {
            let status = tokio::select! {
                status = child.wait() => status,
                Ok(()) = kill_rx => {
                    let _ = child.start_kill();
                    child.wait().await
                }
            };
            kill_switches
                .lock()
                .expect("kill switch mutex poisoned")
                .remove(&exit_slug);
            let _ = InstanceStore::new(exit_paths).mark_exited(&exit_slug, pid);
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
