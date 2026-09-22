use std::sync::Arc;

use bananium_core::{Config, Paths};
use bananium_instance::InstanceStore;
use bananium_launch::{
    build_launch_plan, extract_natives, resolve_libraries, LaunchContext, ProfileStore,
};
use bananium_meta::{FeatureFlags, MetaClient, Platform};
use bananium_net::{DownloadSpec, Downloader, HttpClient};
use bananium_store::BlobStore;
use tokio::sync::broadcast;

use crate::command::Command;
use crate::error::{Error, Result};
use crate::event::Event;
use crate::output::{CommandOutput, InstanceSummary, ResolvedPaths};

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
    config: Config,
    http: HttpClient,
    events_tx: broadcast::Sender<Event>,
}

impl Session {
    /// Ensure `paths`' directories exist and build a `Session` ready to
    /// `dispatch`. One `Session` per frontend process; its `HttpClient` and
    /// event channel are shared across every command it runs.
    pub fn new(paths: Paths, config: Config) -> Result<Self> {
        paths.ensure_dirs()?;
        let http = HttpClient::new(USER_AGENT)?;
        let (events_tx, _rx) = broadcast::channel(1024);
        Ok(Self {
            paths,
            config,
            http,
            events_tx,
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
            Command::Install { version, name } => self.install(&version, name.as_deref()).await,
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
            } => self.instance_set(&instance, ram_mb, jvm_args),
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
            config: self.config.clone(),
        })
    }

    /// `Command::Install`: resolve the version, download the client jar +
    /// every applicable library/natives jar + every asset object (skipping
    /// anything already verified in the store), materialize the assets
    /// tree into the Mojang-shaped layout the JVM expects, and create an
    /// instance for it named `name` (a fresh random name when `None`) if
    /// one doesn't already exist under that name.
    ///
    /// Downloads use [`resolve_libraries`] — the *same* library-resolution
    /// logic `launch` uses to build the classpath — so what gets fetched
    /// here is exactly what a later launch will need; nothing is fetched
    /// speculatively and nothing needed is skipped.
    async fn install(&self, version: &str, name: Option<&str>) -> Result<CommandOutput> {
        let meta = MetaClient::new(self.http.clone(), self.paths.clone());
        let entry = meta.resolve_version(version).await?;
        let profile = meta.version_profile(&entry).await?;
        let asset_index = meta.asset_index(&profile.asset_index).await?;

        let platform = Platform::current();
        let features = FeatureFlags::default();
        let resolved = resolve_libraries(&profile, &platform, &features);

        let mut specs = vec![DownloadSpec {
            url: profile.downloads.client.url.clone(),
            dest: self.paths.store_blob(&profile.downloads.client.sha1),
            expected_sha1: Some(profile.downloads.client.sha1.clone()),
            expected_size: Some(profile.downloads.client.size),
            task_id: "install:client".to_string(),
            label: format!("Minecraft {} client", profile.id),
        }];

        for artifact in &resolved.classpath {
            specs.push(DownloadSpec {
                url: artifact.url.clone(),
                dest: self.paths.store_blob(&artifact.sha1),
                expected_sha1: Some(artifact.sha1.clone()),
                expected_size: Some(artifact.size),
                task_id: format!("install:lib:{}", artifact.sha1),
                label: artifact.name.clone(),
            });
        }
        for entry in &resolved.natives {
            specs.push(DownloadSpec {
                url: entry.artifact.url.clone(),
                dest: self.paths.store_blob(&entry.artifact.sha1),
                expected_sha1: Some(entry.artifact.sha1.clone()),
                expected_size: Some(entry.artifact.size),
                task_id: format!("install:natives:{}", entry.artifact.sha1),
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
                task_id: format!("install:asset:{}", object.hash),
                label: name.clone(),
            });
        }

        let total = specs.len();
        // Every spec above sets `expected_size` from Mojang-published
        // metadata, so the whole job's total size is known up front —
        // aggregate progress doesn't need to wait for responses to trickle
        // in to report an accurate `bytes_total`.
        let overall_total: u64 = specs.iter().filter_map(|s| s.expected_size).sum();
        let overall_label = format!("Minecraft {}", profile.id);

        let downloader = Downloader::new(
            self.http.inner().clone(),
            self.config.max_concurrent_downloads,
        );
        let tx = self.events_tx.clone();
        // Per-file bytes-done, keyed by task_id, so the aggregate below can
        // be recomputed as a simple sum on every update without any file's
        // contribution being double-counted across retries/resumes.
        let file_progress: Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>> =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
        let files_completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let install_started = std::time::Instant::now();
        let on_progress: bananium_net::ProgressFn = {
            let file_progress = file_progress.clone();
            let files_completed = files_completed.clone();
            Arc::new(move |p: bananium_net::Progress| {
                let just_completed = p.bytes_total.is_some_and(|t| p.bytes_done >= t);
                let aggregate_done = {
                    let mut progress = file_progress.lock().expect("progress mutex poisoned");
                    let was_complete = progress
                        .get(&p.task_id)
                        .is_some_and(|&done| p.bytes_total.is_some_and(|t| done >= t));
                    progress.insert(p.task_id.clone(), p.bytes_done);
                    if just_completed && !was_complete {
                        files_completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    progress.values().sum::<u64>()
                };
                let elapsed = install_started.elapsed().as_secs_f64();
                let overall_bytes_per_sec = if elapsed > 0.0 {
                    aggregate_done as f64 / elapsed
                } else {
                    0.0
                };
                let _ = tx.send(Event::OverallProgress {
                    task_id: "install".to_string(),
                    label: overall_label.clone(),
                    bytes_done: aggregate_done,
                    bytes_total: Some(overall_total),
                    bytes_per_sec: overall_bytes_per_sec,
                    files_done: files_completed.load(std::sync::atomic::Ordering::Relaxed),
                    files_total: total,
                });
                let _ = tx.send(Event::from(p));
            })
        };
        let results = downloader.download_all(specs, on_progress).await;

        let failures: Vec<String> = results
            .iter()
            .filter_map(|r| r.as_ref().err().map(|e| e.to_string()))
            .collect();
        if let Some(first) = failures.first() {
            return Err(Error::DownloadsFailed(failures.len(), total, first.clone()));
        }
        self.emit(Event::TaskCompleted {
            task_id: "install".to_string(),
        });

        // Materialize the assets tree into the shape the JVM expects.
        let store = BlobStore::new(self.paths.clone());
        for (name, object) in &asset_index.objects {
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

        let instances = InstanceStore::new(self.paths.clone());
        let slug = instances.create_named(&profile.id, name)?;

        Ok(CommandOutput::Installed {
            instance: slug,
            mc_version: profile.id,
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
        let entry = meta.resolve_version(&instance_cfg.mc_version).await?;
        let profile = meta.version_profile(&entry).await?;

        let platform = Platform::current();
        let features = FeatureFlags::default();
        let resolved = resolve_libraries(&profile, &platform, &features);
        let natives_dir = extract_natives(&self.paths, &resolved.natives)?;

        let mut classpath = vec![self.paths.store_blob(&profile.downloads.client.sha1)];
        classpath.extend(
            resolved
                .classpath
                .iter()
                .map(|a| self.paths.store_blob(&a.sha1)),
        );

        let profiles = ProfileStore::new(self.paths.clone());
        let local_profile = match profile_name {
            Some(name) => profiles.get_or_create(name)?,
            None => profiles.default_profile()?,
        };

        let java = bananium_java::find_java(
            instance_cfg
                .java_path
                .as_deref()
                .or(self.config.java_path.as_deref()),
        )?;

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

        let plan = build_launch_plan(&profile, &platform, &features, &ctx, java.path);

        if dry_run {
            return Ok(CommandOutput::LaunchPlanned {
                instance: slug,
                command_line: plan.command_line(),
            });
        }

        std::fs::create_dir_all(&ctx.game_directory)?;
        let mut cmd = tokio::process::Command::from(plan.to_command());
        let mut child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);
        instances.mark_running(&slug, pid)?;
        // M1 launches attached-but-not-awaited: the caller gets the pid back
        // immediately. `--detach`/log-supervision is M3 scope. Nothing here
        // removes the pid from `running.toml` on exit — see
        // `InstanceStore::running_pids`'s doc comment on why that's a
        // lazy, re-checked-on-read design rather than an active one.
        tokio::spawn(async move {
            let _ = child.wait().await;
        });

        Ok(CommandOutput::Launched {
            instance: slug,
            pid,
        })
    }

    /// `Command::InstanceList`: every installed instance plus its current
    /// running status, for a frontend's instance list (the TUI's, for one).
    fn instance_list(&self) -> Result<CommandOutput> {
        let instances = InstanceStore::new(self.paths.clone());
        let mut summaries = Vec::new();
        for (slug, cfg) in instances.list_configs()? {
            let running = instances.is_running(&slug)?;
            summaries.push(InstanceSummary {
                slug,
                name: cfg.name,
                mc_version: cfg.mc_version,
                ram_mb: cfg.ram_mb,
                jvm_args: cfg.jvm_args,
                running,
            });
        }
        Ok(CommandOutput::InstanceListed {
            instances: summaries,
        })
    }

    /// `Command::InstanceSet`: see the field-level doc comments on
    /// `Command::InstanceSet` for the "`None`/absent means unchanged"
    /// convention this follows.
    fn instance_set(
        &self,
        instance: &str,
        ram_mb: Option<u32>,
        jvm_args: Option<Vec<String>>,
    ) -> Result<CommandOutput> {
        let instances = InstanceStore::new(self.paths.clone());
        let mut cfg = instances.load(instance)?;
        if let Some(ram_mb) = ram_mb {
            cfg.ram_mb = if ram_mb == 0 { None } else { Some(ram_mb) };
        }
        if let Some(jvm_args) = jvm_args {
            cfg.jvm_args = jvm_args;
        }
        instances.save(instance, &cfg)?;
        Ok(CommandOutput::InstanceUpdated {
            instance: instance.to_string(),
        })
    }
}
