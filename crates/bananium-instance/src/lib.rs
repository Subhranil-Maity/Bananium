//! Instance model, lockfile, mod graph, and import/export formats.
//!
//! M1 only needs a minimal slice of this: a vanilla instance is just a
//! version pin and a game directory. `instance new|clone|rm|...`, groups,
//! and the mod lockfile arrive in M3/M4.

pub mod content;
pub mod error;
pub mod presets;

use std::path::{Path, PathBuf};

use bananium_core::Paths;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use content::{ContentEntry, ContentKind, ContentStore};
pub use error::{Error, Result};
pub use presets::{Preset, PresetEntry, PresetStore};

/// Which mod loader an instance uses. Quilt and Forge/NeoForge aren't
/// supported (see PLAN.md M5/M6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Loader {
    #[default]
    Vanilla,
    /// Fabric; the loader release is [`InstanceConfig::loader_version`].
    Fabric,
}

/// The contents of an instance's `instance.toml`: enough to know what
/// version to launch and how. The mod lockfile (`bananium.lock.toml`) is a
/// separate file, owned by M4's mod-management code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceConfig {
    pub name: String,
    pub mc_version: String,
    #[serde(default)]
    pub loader: Loader,
    /// The pinned loader release (e.g. Fabric `0.16.9`); `None` for
    /// vanilla. A flat field rather than data on [`Loader::Fabric`] so that
    /// `loader = "vanilla"` in existing `instance.toml` files still parses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    /// Per-instance Java override; falls back to `Config::java_path`, then
    /// auto-detection, when absent.
    #[serde(default)]
    pub java_path: Option<PathBuf>,
    /// `-Xmx<ram_mb>M` heap cap; `None` leaves the JVM's own default in
    /// place. There's no auto-sizer yet (that's M3's RAM-percentage-of-
    /// system logic) — this is only ever set explicitly by the user.
    #[serde(default)]
    pub ram_mb: Option<u32>,
    /// Extra JVM arguments appended after every profile-generated (and
    /// `ram_mb`-derived) JVM argument, never inserted anywhere else — see
    /// `bananium_launch::build_launch_plan`'s doc comment for why append-
    /// only is the only placement that keeps these arguments interpreted as
    /// JVM flags rather than game arguments.
    #[serde(default)]
    pub jvm_args: Vec<String>,
    /// User-chosen group the instance is filed under in a frontend's
    /// library ("Modded", "Servers", ...); `None` is ungrouped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

/// The contents of an instance's `stats.toml`: play history, recorded by
/// launches rather than edited by the user.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceStats {
    /// When the instance was last launched (Unix seconds).
    #[serde(default)]
    pub last_played_unix: Option<u64>,
    /// Total time the game has been running, summed over every launch.
    #[serde(default)]
    pub playtime_secs: u64,
}

/// Image extensions accepted by [`InstanceStore::set_icon`] — the formats a
/// webview can display directly.
const ICON_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];

/// True if `name` is safe to use both as a display name and, after
/// [`InstanceStore::slugify`], as an instance directory name: non-empty and
/// only letters, digits, `-`, and `_`. Exposed so every frontend can
/// validate a user-typed name before round-tripping it through `Session`
/// (see `bananium_api`'s re-export), and enforced again in
/// [`InstanceStore::create_named`] as the actual source of truth.
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Reads and writes `instance.toml` files under `Paths::instances_dir()`.
pub struct InstanceStore {
    paths: Paths,
}

impl InstanceStore {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    /// A filesystem-safe slug for a raw Minecraft version id like "1.21.1".
    pub fn slugify(mc_version: &str) -> String {
        mc_version
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    /// Create a vanilla instance for `mc_version`, named either the
    /// caller-given `name` or a fresh random id when `name` is `None` —
    /// unlike a version-derived slug, this lets several instances share the
    /// same Minecraft version, each under its own name. Idempotent only
    /// when `name` is given and already names an instance on the *same*
    /// `mc_version` (a re-run of `install` for something you already have);
    /// re-using a name against a different version, or an invalid name, is
    /// an error rather than silently repurposing or renaming anything.
    pub fn create_named(&self, mc_version: &str, name: Option<&str>) -> Result<String> {
        let display_name = match name {
            Some(name) => {
                if !is_valid_name(name) {
                    return Err(Error::InvalidName(name.to_string()));
                }
                name.to_string()
            }
            None => Uuid::new_v4().to_string(),
        };
        let slug = Self::slugify(&display_name);

        if self.paths.instance_toml(&slug).is_file() {
            let existing = self.load(&slug)?;
            if existing.mc_version != mc_version {
                return Err(Error::NameInUse(display_name));
            }
            std::fs::create_dir_all(self.paths.instance_minecraft_dir(&slug))?;
            return Ok(slug);
        }

        let cfg = InstanceConfig {
            name: display_name,
            mc_version: mc_version.to_string(),
            loader: Loader::Vanilla,
            loader_version: None,
            java_path: None,
            ram_mb: None,
            jvm_args: Vec::new(),
            group: None,
        };
        self.save(&slug, &cfg)?;
        std::fs::create_dir_all(self.paths.instance_minecraft_dir(&slug))?;
        Ok(slug)
    }

    /// Write `cfg` to `<slug>/instance.toml`, creating the instance
    /// directory if it doesn't exist yet.
    pub fn save(&self, slug: &str, cfg: &InstanceConfig) -> Result<()> {
        std::fs::create_dir_all(self.paths.instance_dir(slug))?;
        std::fs::write(self.paths.instance_toml(slug), toml::to_string_pretty(cfg)?)?;
        Ok(())
    }

    /// Read `<slug>/instance.toml`, erroring with [`Error::NotFound`] if it
    /// doesn't exist (rather than a raw io error).
    pub fn load(&self, slug: &str) -> Result<InstanceConfig> {
        let path = self.paths.instance_toml(slug);
        let text = std::fs::read_to_string(&path).map_err(|_| Error::NotFound(slug.to_string()))?;
        Ok(toml::from_str(&text)?)
    }

    /// Every installed instance's slug, sorted.
    pub fn list(&self) -> Result<Vec<String>> {
        let dir = self.paths.instances_dir();
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut slugs = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    slugs.push(name.to_string());
                }
            }
        }
        slugs.sort();
        Ok(slugs)
    }

    /// Resolve an optional CLI-supplied instance name: the name itself if
    /// given, otherwise the sole installed instance, otherwise an error
    /// asking the caller to disambiguate.
    pub fn resolve(&self, requested: Option<&str>) -> Result<String> {
        match requested {
            Some(slug) => {
                if self.paths.instance_toml(slug).is_file() {
                    Ok(slug.to_string())
                } else {
                    Err(Error::NotFound(slug.to_string()))
                }
            }
            None => {
                let mut slugs = self.list()?;
                match slugs.len() {
                    0 => Err(Error::NotFound("<none installed yet>".into())),
                    1 => Ok(slugs.remove(0)),
                    _ => Err(Error::Ambiguous(slugs)),
                }
            }
        }
    }

    /// Every installed instance's full config, in [`InstanceStore::list`]
    /// order — the shape a frontend's instance list actually wants, rather
    /// than making every caller re-`load` each slug itself.
    pub fn list_configs(&self) -> Result<Vec<(String, InstanceConfig)>> {
        self.list()?
            .into_iter()
            .map(|slug| {
                let cfg = self.load(&slug)?;
                Ok((slug, cfg))
            })
            .collect()
    }

    /// Pids Bananium believes are currently running `slug`, pruned of any
    /// that are no longer alive. Nothing long-lived watches for the game
    /// process to exit and cleans this up proactively — the CLI is a fresh
    /// process per command, so there's nothing to watch with — instead
    /// staleness is re-checked and swept here on every call, which is the
    /// only point that actually needs an accurate answer.
    ///
    /// Sweeping a dead run also credits its playtime. Nobody saw the exit
    /// happen, so the end time is estimated as the launch log's last write:
    /// the JVM's stdout/stderr go there until the process dies. Removing
    /// the run from `running.toml` is what makes each run count once — a
    /// long-lived session's exact [`InstanceStore::mark_exited`] credits
    /// only runs that are still listed, and so does this sweep.
    pub fn running_pids(&self, slug: &str) -> Result<Vec<u32>> {
        let runs = self.read_running(slug);
        let (alive, dead): (Vec<Run>, Vec<Run>) =
            runs.into_iter().partition(|run| is_pid_alive(run.pid));
        if !dead.is_empty() && self.paths.instance_dir(slug).is_dir() {
            let swept: u64 = dead.iter().map(Run::estimated_secs).sum();
            self.write_running(slug, &alive)?;
            if swept > 0 {
                self.add_playtime(slug, swept)?;
            }
        }
        Ok(alive.into_iter().map(|run| run.pid).collect())
    }

    /// Whether any pid recorded for `slug` is still alive — the
    /// one-instance-online-at-a-time gate `Session::launch` checks before
    /// spawning a new process for it.
    pub fn is_running(&self, slug: &str) -> Result<bool> {
        Ok(!self.running_pids(slug)?.is_empty())
    }

    /// Record `pid` as running `slug`, first pruning any pids that are no
    /// longer alive (so a crashed-and-relaunched instance doesn't
    /// accumulate stale entries forever). Also stamps the instance's
    /// last-played time. `log` is the file the process's output goes to,
    /// which lets a later sweep estimate when it exited.
    pub fn mark_running(&self, slug: &str, pid: u32, log: Option<&Path>) -> Result<()> {
        self.running_pids(slug)?;
        let mut runs = self.read_running(slug);
        let now = now_unix();
        runs.push(Run {
            pid,
            started_unix: Some(now),
            log: log.map(Path::to_path_buf),
        });
        self.write_running(slug, &runs)?;
        let mut stats = self.stats(slug);
        stats.last_played_unix = Some(now);
        self.save_stats(slug, &stats)
    }

    /// Forget `pid` for `slug` once its process is known to have exited,
    /// crediting the exact time it ran. Called by a long-lived frontend
    /// (one `Session` that outlives the game) so the running state is exact
    /// immediately, rather than waiting for the lazy sweep in
    /// [`InstanceStore::running_pids`] — which also guards against the OS
    /// reusing the pid for an unrelated process.
    pub fn mark_exited(&self, slug: &str, pid: u32) -> Result<()> {
        if !self.paths.instance_dir(slug).is_dir() {
            // Deleted (or renamed) while running; nothing left to update.
            return Ok(());
        }
        let mut runs = self.read_running(slug);
        let Some(index) = runs.iter().position(|run| run.pid == pid) else {
            // Already swept (and credited) by `running_pids`.
            return Ok(());
        };
        let run = runs.remove(index);
        self.write_running(slug, &runs)?;
        if let Some(started) = run.started_unix {
            self.add_playtime(slug, now_unix().saturating_sub(started))?;
        }
        Ok(())
    }

    /// Every recorded run, dead or alive; unreadable state reads as none.
    fn read_running(&self, slug: &str) -> Vec<Run> {
        let path = self.paths.instance_running_toml(slug);
        let state: RunningState = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_default(),
            Err(_) => RunningState::default(),
        };
        state.into_runs()
    }

    fn write_running(&self, slug: &str, runs: &[Run]) -> Result<()> {
        let path = self.paths.instance_running_toml(slug);
        std::fs::create_dir_all(self.paths.instance_dir(slug))?;
        let state = RunningState {
            pids: Vec::new(),
            runs: runs.to_vec(),
        };
        std::fs::write(&path, toml::to_string_pretty(&state)?)?;
        Ok(())
    }

    /// `slug`'s play history; a missing or unreadable `stats.toml` reads
    /// as never played rather than an error, since it's only ever display
    /// data.
    pub fn stats(&self, slug: &str) -> InstanceStats {
        std::fs::read_to_string(self.paths.instance_stats_toml(slug))
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save_stats(&self, slug: &str, stats: &InstanceStats) -> Result<()> {
        std::fs::create_dir_all(self.paths.instance_dir(slug))?;
        std::fs::write(
            self.paths.instance_stats_toml(slug),
            toml::to_string_pretty(stats)?,
        )?;
        Ok(())
    }

    /// Add `secs` to `slug`'s total playtime.
    pub fn add_playtime(&self, slug: &str, secs: u64) -> Result<()> {
        let mut stats = self.stats(slug);
        stats.playtime_secs = stats.playtime_secs.saturating_add(secs);
        self.save_stats(slug, &stats)
    }

    /// The instance's custom icon, if one is set. Stored as
    /// `icon-<unix millis>.<ext>` so that each change gets a new path: a
    /// webview caches images by URL and would otherwise keep showing the
    /// old one.
    pub fn icon(&self, slug: &str) -> Option<PathBuf> {
        std::fs::read_dir(self.paths.instance_dir(slug))
            .ok()?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .find(|path| is_icon_file(path))
    }

    /// Replace `slug`'s icon with a copy of the image at `source`, or clear
    /// it when `source` is `None`.
    pub fn set_icon(&self, slug: &str, source: Option<&Path>) -> Result<()> {
        self.resolve(Some(slug))?;
        let new_icon = match source {
            Some(source) => {
                let ext = source
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase)
                    .filter(|e| ICON_EXTENSIONS.contains(&e.as_str()))
                    .ok_or_else(|| Error::UnsupportedIcon(source.display().to_string()))?;
                let dest = self
                    .paths
                    .instance_dir(slug)
                    .join(format!("icon-{}.{ext}", now_millis()));
                std::fs::copy(source, &dest)?;
                Some(dest)
            }
            None => None,
        };
        for entry in std::fs::read_dir(self.paths.instance_dir(slug))? {
            let path = entry?.path();
            if is_icon_file(&path) && Some(&path) != new_icon.as_ref() {
                std::fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    /// Enabled mods in `slug`'s `mods/` folder, counted straight from the
    /// directory — cheap enough for every row of an instance list, unlike a
    /// full [`ContentStore`] listing.
    pub fn mod_count(&self, slug: &str) -> u32 {
        let Ok(entries) = std::fs::read_dir(self.paths.instance_minecraft_dir(slug).join("mods"))
        else {
            return 0;
        };
        entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.to_ascii_lowercase().ends_with(".jar"))
            })
            .count() as u32
    }

    /// Delete an instance and everything in it (worlds, mods, logs).
    /// Refused while it's running, so a live game never has its directory
    /// pulled out from under it.
    pub fn remove(&self, slug: &str) -> Result<()> {
        self.resolve(Some(slug))?;
        if self.is_running(slug)? {
            return Err(Error::Running(slug.to_string()));
        }
        std::fs::remove_dir_all(self.paths.instance_dir(slug))?;
        Ok(())
    }

    /// Rename an instance: its display name and its directory/slug both
    /// change. Returns the new slug. Refused while running (the game's
    /// working directory would move) or if `new_name` is taken.
    pub fn rename(&self, slug: &str, new_name: &str) -> Result<String> {
        let mut cfg = self.load(slug)?;
        if !is_valid_name(new_name) {
            return Err(Error::InvalidName(new_name.to_string()));
        }
        if self.is_running(slug)? {
            return Err(Error::Running(slug.to_string()));
        }
        let new_slug = Self::slugify(new_name);
        if new_slug != slug && self.paths.instance_dir(&new_slug).exists() {
            return Err(Error::AlreadyExists(new_name.to_string()));
        }
        if new_slug != slug {
            std::fs::rename(
                self.paths.instance_dir(slug),
                self.paths.instance_dir(&new_slug),
            )?;
        }
        cfg.name = new_name.to_string();
        self.save(&new_slug, &cfg)?;
        Ok(new_slug)
    }

    /// Copy an instance (config, worlds, mods, resource packs — the whole
    /// game directory) under a new name. Launch logs and running state are
    /// deliberately not copied: they describe the source's history, not
    /// the copy's. Returns the new slug.
    pub fn clone_instance(&self, slug: &str, new_name: &str) -> Result<String> {
        let mut cfg = self.load(slug)?;
        if !is_valid_name(new_name) {
            return Err(Error::InvalidName(new_name.to_string()));
        }
        let new_slug = Self::slugify(new_name);
        if self.paths.instance_dir(&new_slug).exists() {
            return Err(Error::AlreadyExists(new_name.to_string()));
        }
        copy_dir_recursive(
            &self.paths.instance_minecraft_dir(slug),
            &self.paths.instance_minecraft_dir(&new_slug),
        )?;
        let lock = self.paths.instance_lock(slug);
        if lock.is_file() {
            std::fs::copy(&lock, self.paths.instance_lock(&new_slug))?;
        }
        if let Some(icon) = self.icon(slug) {
            if let Some(file_name) = icon.file_name() {
                std::fs::copy(&icon, self.paths.instance_dir(&new_slug).join(file_name))?;
            }
        }
        cfg.name = new_name.to_string();
        self.save(&new_slug, &cfg)?;
        Ok(new_slug)
    }
}

/// Recursively copy `from` into `to` (created if missing). A missing
/// `from` copies nothing — a never-launched instance may not have one yet.
fn copy_dir_recursive(from: &std::path::Path, to: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    if !from.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

/// On-disk shape of `running.toml` — see [`InstanceStore::running_pids`].
/// `pids` is the older format (bare pids, no start time), still read so an
/// upgrade mid-game doesn't lose track of a running instance; it's never
/// written any more.
#[derive(Debug, Default, Serialize, Deserialize)]
struct RunningState {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pids: Vec<u32>,
    #[serde(default)]
    runs: Vec<Run>,
}

impl RunningState {
    fn into_runs(self) -> Vec<Run> {
        let legacy = self.pids.into_iter().map(|pid| Run {
            pid,
            started_unix: None,
            log: None,
        });
        self.runs.into_iter().chain(legacy).collect()
    }
}

/// One launched game process.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Run {
    pid: u32,
    #[serde(default)]
    started_unix: Option<u64>,
    /// Where the process's stdout/stderr go.
    #[serde(default)]
    log: Option<PathBuf>,
}

impl Run {
    /// How long a run that exited unobserved probably lasted: from its
    /// start to its log's last write. Zero when either is unknown.
    fn estimated_secs(&self) -> u64 {
        let (Some(started), Some(log)) = (self.started_unix, &self.log) else {
            return 0;
        };
        std::fs::metadata(log)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs().saturating_sub(started))
            .unwrap_or(0)
    }
}

fn now_unix() -> u64 {
    now_millis() / 1000
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// Whether `path` is an icon written by [`InstanceStore::set_icon`].
fn is_icon_file(path: &Path) -> bool {
    let name_ok = path
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.starts_with("icon-"));
    let ext_ok = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ICON_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()));
    name_ok && ext_ok && path.is_file()
}

/// Whether `pid` currently identifies a live process. Linux checks
/// `/proc/<pid>`; Windows opens the process and asks whether it has an exit
/// code yet. On any other target this conservatively reports `false`
/// rather than guessing, which just disables the one-instance-online-at-a-
/// time guard there instead of misbehaving.
#[cfg(target_os = "linux")]
fn is_pid_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(windows)]
fn is_pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: plain Win32 calls on a handle we open and always close here;
    // a null handle (no such pid, or access denied) is checked first.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut code) != 0;
        CloseHandle(handle);
        // A process that genuinely exits with code 259 is misreported as
        // alive; the JVM never does, so this well-known Win32 ambiguity is
        // acceptable here.
        ok && code == STILL_ACTIVE as u32
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
fn is_pid_alive(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_named_is_idempotent_for_the_same_name_and_version() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths.clone());

        let slug = store.create_named("1.21.1", Some("main")).unwrap();
        assert_eq!(slug, "main");
        assert!(paths.instance_minecraft_dir(&slug).is_dir());

        let again = store.create_named("1.21.1", Some("main")).unwrap();
        assert_eq!(again, slug);
        let cfg = store.load(&slug).unwrap();
        assert_eq!(cfg.mc_version, "1.21.1");
        assert_eq!(cfg.name, "main");
        assert_eq!(cfg.loader, Loader::Vanilla);
    }

    #[test]
    fn create_named_allows_two_instances_of_the_same_version() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths);

        let a = store.create_named("1.21.1", Some("survival")).unwrap();
        let b = store.create_named("1.21.1", Some("creative")).unwrap();
        assert_ne!(a, b);
        assert_eq!(store.load(&a).unwrap().mc_version, "1.21.1");
        assert_eq!(store.load(&b).unwrap().mc_version, "1.21.1");
    }

    #[test]
    fn create_named_rejects_reusing_a_name_for_a_different_version() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths);

        store.create_named("1.20.1", Some("main")).unwrap();
        assert!(matches!(
            store.create_named("1.21.1", Some("main")),
            Err(Error::NameInUse(_))
        ));
    }

    #[test]
    fn create_named_rejects_invalid_characters() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths);

        assert!(matches!(
            store.create_named("1.21.1", Some("my instance")),
            Err(Error::InvalidName(_))
        ));
        assert!(matches!(
            store.create_named("1.21.1", Some("")),
            Err(Error::InvalidName(_))
        ));
    }

    #[test]
    fn create_named_defaults_to_a_random_name_when_none_given() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths);

        let a = store.create_named("1.21.1", None).unwrap();
        let b = store.create_named("1.21.1", None).unwrap();
        assert_ne!(a, b, "two unnamed installs must not collide");
    }

    #[test]
    fn resolve_picks_the_only_instance() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths);
        store.create_named("1.20.1", Some("solo")).unwrap();
        assert_eq!(store.resolve(None).unwrap(), "solo");
    }

    #[test]
    fn resolve_is_ambiguous_with_multiple_instances() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths);
        store.create_named("1.20.1", Some("a")).unwrap();
        store.create_named("1.21.1", Some("b")).unwrap();
        assert!(matches!(store.resolve(None), Err(Error::Ambiguous(_))));
        assert_eq!(store.resolve(Some("a")).unwrap(), "a");
    }

    #[test]
    fn mark_running_is_reported_by_is_running_and_pruned_once_dead() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths);
        let slug = store.create_named("1.21.1", Some("main")).unwrap();

        assert!(!store.is_running(&slug).unwrap());

        // A pid far past Linux's real pid_max can never correspond to a
        // live process, so this exercises the "recorded but dead" path
        // deterministically instead of racing a real child process.
        store.mark_running(&slug, u32::MAX, None).unwrap();
        assert!(!store.is_running(&slug).unwrap());
        assert!(store.stats(&slug).last_played_unix.is_some());
    }

    #[test]
    fn legacy_running_toml_still_parses() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths.clone());
        let slug = store.create_named("1.21.1", Some("main")).unwrap();
        let me = std::process::id();
        std::fs::write(
            paths.instance_running_toml(&slug),
            format!("pids = [{me}]\n"),
        )
        .unwrap();
        let runs = store.read_running(&slug);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].pid, me);
        assert_eq!(runs[0].started_unix, None);
    }

    #[test]
    fn sweeping_a_dead_run_credits_playtime_up_to_its_logs_last_write() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths.clone());
        let slug = store.create_named("1.21.1", Some("main")).unwrap();
        let log = paths.instance_dir(&slug).join("launch.log");
        std::fs::write(&log, "done").unwrap();
        let log_written = now_unix();
        let run = Run {
            pid: u32::MAX,
            started_unix: Some(log_written - 600),
            log: Some(log),
        };
        store.write_running(&slug, &[run]).unwrap();

        assert!(store.running_pids(&slug).unwrap().is_empty());
        let played = store.stats(&slug).playtime_secs;
        assert!((600..=602).contains(&played), "played {played}s");

        // Swept runs are gone, so a second sweep (or a late `mark_exited`)
        // can't count the same run again.
        store.running_pids(&slug).unwrap();
        store.mark_exited(&slug, u32::MAX).unwrap();
        assert_eq!(store.stats(&slug).playtime_secs, played);
    }

    #[test]
    fn stats_round_trip_and_default_to_never_played() {
        let dir = tempfile::tempdir().unwrap();
        let store = InstanceStore::new(Paths::at(dir.path()));
        let slug = store.create_named("1.21.1", Some("main")).unwrap();
        assert_eq!(store.stats(&slug), InstanceStats::default());
        store.add_playtime(&slug, 90).unwrap();
        store.add_playtime(&slug, 30).unwrap();
        assert_eq!(store.stats(&slug).playtime_secs, 120);
    }

    #[test]
    fn set_icon_replaces_and_clears_and_follows_a_clone() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths.clone());
        let slug = store.create_named("1.21.1", Some("main")).unwrap();
        let png = dir.path().join("pic.PNG");
        std::fs::write(&png, "png").unwrap();

        assert!(store.icon(&slug).is_none());
        store.set_icon(&slug, Some(&png)).unwrap();
        let icon = store.icon(&slug).unwrap();
        assert_eq!(icon.extension().unwrap(), "png");

        let copy = store.clone_instance(&slug, "copy").unwrap();
        assert!(store.icon(&copy).is_some());

        let txt = dir.path().join("notes.txt");
        std::fs::write(&txt, "x").unwrap();
        assert!(matches!(
            store.set_icon(&slug, Some(&txt)),
            Err(Error::UnsupportedIcon(_))
        ));
        assert_eq!(store.icon(&slug), Some(icon));

        store.set_icon(&slug, None).unwrap();
        assert!(store.icon(&slug).is_none());
    }

    #[test]
    fn mod_count_counts_enabled_jars_only() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths.clone());
        let slug = store.create_named("1.21.1", Some("main")).unwrap();
        assert_eq!(store.mod_count(&slug), 0);
        let mods = paths.instance_minecraft_dir(&slug).join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("a.jar"), "").unwrap();
        std::fs::write(mods.join("b.jar.disabled"), "").unwrap();
        assert_eq!(store.mod_count(&slug), 1);
    }

    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn own_process_is_alive_and_mark_exited_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = InstanceStore::new(Paths::at(dir.path()));
        let slug = store.create_named("1.21.1", Some("main")).unwrap();

        let me = std::process::id();
        assert!(is_pid_alive(me));
        store.mark_running(&slug, me, None).unwrap();
        assert!(store.is_running(&slug).unwrap());

        store.mark_exited(&slug, me).unwrap();
        assert!(!store.is_running(&slug).unwrap());
        // Credited exactly once, by `mark_exited`.
        store.mark_exited(&slug, me).unwrap();
        assert!(store.stats(&slug).playtime_secs < 5);
    }

    #[test]
    fn rename_moves_the_directory_and_updates_the_name() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths.clone());
        store.create_named("1.21.1", Some("old")).unwrap();
        std::fs::write(paths.instance_minecraft_dir("old").join("options.txt"), "x").unwrap();

        let new_slug = store.rename("old", "new").unwrap();
        assert_eq!(new_slug, "new");
        assert!(!paths.instance_dir("old").exists());
        assert!(paths
            .instance_minecraft_dir("new")
            .join("options.txt")
            .is_file());
        assert_eq!(store.load("new").unwrap().name, "new");

        store.create_named("1.21.1", Some("other")).unwrap();
        assert!(matches!(
            store.rename("new", "other"),
            Err(Error::AlreadyExists(_))
        ));
    }

    #[test]
    fn clone_copies_the_game_dir_but_not_logs() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths.clone());
        store.create_named("1.21.1", Some("src")).unwrap();
        let mods = paths.instance_minecraft_dir("src").join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("a.jar"), "jar").unwrap();
        std::fs::create_dir_all(paths.instance_logs_dir("src")).unwrap();
        std::fs::write(paths.instance_logs_dir("src").join("launch-1.log"), "log").unwrap();

        let slug = store.clone_instance("src", "copy").unwrap();
        assert!(paths
            .instance_minecraft_dir(&slug)
            .join("mods/a.jar")
            .is_file());
        assert!(!paths.instance_logs_dir(&slug).exists());
        assert_eq!(store.load(&slug).unwrap().mc_version, "1.21.1");
    }

    #[test]
    fn remove_deletes_the_instance() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths.clone());
        store.create_named("1.21.1", Some("gone")).unwrap();
        store.remove("gone").unwrap();
        assert!(!paths.instance_dir("gone").exists());
        assert!(matches!(store.remove("gone"), Err(Error::NotFound(_))));
    }
}
