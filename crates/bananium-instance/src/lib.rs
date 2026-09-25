//! Instance model, lockfile, mod graph, and import/export formats.
//!
//! M1 only needs a minimal slice of this: a vanilla instance is just a
//! version pin and a game directory. `instance new|clone|rm|...`, groups,
//! and the mod lockfile arrive in M3/M4.

pub mod content;
pub mod error;
pub mod presets;

use std::path::PathBuf;

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
}

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
    pub fn running_pids(&self, slug: &str) -> Result<Vec<u32>> {
        let path = self.paths.instance_running_toml(slug);
        let state: RunningState = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_default(),
            Err(_) => RunningState::default(),
        };
        let alive: Vec<u32> = state
            .pids
            .into_iter()
            .filter(|&pid| is_pid_alive(pid))
            .collect();
        Ok(alive)
    }

    /// Whether any pid recorded for `slug` is still alive — the
    /// one-instance-online-at-a-time gate `Session::launch` checks before
    /// spawning a new process for it.
    pub fn is_running(&self, slug: &str) -> Result<bool> {
        Ok(!self.running_pids(slug)?.is_empty())
    }

    /// Record `pid` as running `slug`, first pruning any pids that are no
    /// longer alive (so a crashed-and-relaunched instance doesn't
    /// accumulate stale entries forever).
    pub fn mark_running(&self, slug: &str, pid: u32) -> Result<()> {
        let mut pids = self.running_pids(slug)?;
        pids.push(pid);
        self.write_running(slug, pids)
    }

    /// Forget `pid` for `slug` once its process is known to have exited.
    /// Called by a long-lived frontend (one `Session` that outlives the
    /// game) so the running state is exact immediately, rather than waiting
    /// for the lazy sweep in [`InstanceStore::running_pids`] — which also
    /// guards against the OS reusing the pid for an unrelated process.
    pub fn mark_exited(&self, slug: &str, pid: u32) -> Result<()> {
        if !self.paths.instance_dir(slug).is_dir() {
            // Deleted (or renamed) while running; nothing left to update.
            return Ok(());
        }
        let pids = self
            .running_pids(slug)?
            .into_iter()
            .filter(|&p| p != pid)
            .collect();
        self.write_running(slug, pids)
    }

    fn write_running(&self, slug: &str, pids: Vec<u32>) -> Result<()> {
        let path = self.paths.instance_running_toml(slug);
        std::fs::create_dir_all(self.paths.instance_dir(slug))?;
        std::fs::write(&path, toml::to_string_pretty(&RunningState { pids })?)?;
        Ok(())
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
#[derive(Debug, Default, Serialize, Deserialize)]
struct RunningState {
    #[serde(default)]
    pids: Vec<u32>,
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
        store.mark_running(&slug, u32::MAX).unwrap();
        assert!(!store.is_running(&slug).unwrap());
    }

    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn own_process_is_alive_and_mark_exited_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = InstanceStore::new(Paths::at(dir.path()));
        let slug = store.create_named("1.21.1", Some("main")).unwrap();

        let me = std::process::id();
        assert!(is_pid_alive(me));
        store.mark_running(&slug, me).unwrap();
        assert!(store.is_running(&slug).unwrap());

        store.mark_exited(&slug, me).unwrap();
        assert!(!store.is_running(&slug).unwrap());
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
