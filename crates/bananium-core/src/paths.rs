use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Resolves every on-disk location Bananium touches, rooted at a single
/// `BANANIUM_HOME` directory. Resolution order:
///
/// 1. `BANANIUM_HOME` environment variable, if set.
/// 2. A `bananium/` directory beside the running executable, if it exists
///    (portable install).
/// 3. `~/.bananium` (or the platform equivalent of the home directory).
#[derive(Debug, Clone)]
pub struct Paths {
    home: PathBuf,
}

impl Paths {
    /// Resolve `BANANIUM_HOME` using the real environment and executable
    /// location (see the resolution order documented on the type). This is
    /// the constructor every binary frontend should call; use [`Paths::at`]
    /// instead when you want a sandboxed root (tests, dry-run previews).
    pub fn resolve() -> Result<Self> {
        if let Ok(dir) = std::env::var("BANANIUM_HOME") {
            return Ok(Self {
                home: PathBuf::from(dir),
            });
        }

        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                let portable = exe_dir.join("bananium");
                if portable.is_dir() {
                    return Ok(Self { home: portable });
                }
            }
        }

        let base = dirs::home_dir().ok_or(Error::NoHomeDir)?;
        Ok(Self {
            home: base.join(".bananium"),
        })
    }

    /// Build a `Paths` rooted at an arbitrary directory. Used by tests and by
    /// anything that wants an isolated sandbox rather than the real home.
    pub fn at(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }

    /// The resolved `BANANIUM_HOME` root itself.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// `config.toml`: the layered-config file (see `bananium_core::config`).
    pub fn config_toml(&self) -> PathBuf {
        self.home.join("config.toml")
    }

    /// `index.db`: instances, playtime, and the Modrinth FTS5 mirror (from
    /// M4 onward — unused so far).
    pub fn index_db(&self) -> PathBuf {
        self.home.join("index.db")
    }

    /// The root of the content-addressed blob store; see [`Paths::store_blob`].
    pub fn store_dir(&self) -> PathBuf {
        self.home.join("store")
    }

    /// Content-addressed path for a blob identified by its lowercase hex SHA-1.
    pub fn store_blob(&self, sha1_hex: &str) -> PathBuf {
        self.store_dir().join(&sha1_hex[0..2]).join(sha1_hex)
    }

    /// Root of the Mojang java-runtime install tree; see [`Paths::java_component_dir`].
    pub fn java_dir(&self) -> PathBuf {
        self.home.join("java")
    }

    /// Where a specific Mojang java-runtime component (e.g. `jre-legacy`,
    /// `java-runtime-delta`) is materialized once provisioned.
    pub fn java_component_dir(&self, component: &str) -> PathBuf {
        self.java_dir().join(component)
    }

    /// Cached Mojang + loader metadata (version manifest, version profiles,
    /// asset indexes), mirrored verbatim so it's usable offline. See
    /// `bananium_meta::MetaClient`.
    pub fn meta_dir(&self) -> PathBuf {
        self.home.join("meta")
    }

    /// ETag/Last-Modified HTTP cache root (reserved; not yet used by the M1
    /// download engine, which caches by content hash instead).
    pub fn http_cache_dir(&self) -> PathBuf {
        self.home.join("cache").join("http")
    }

    /// Not in the top-level layout diagram in PLAN.md, but required to give
    /// Mojang's asset objects the exact on-disk shape the JVM expects
    /// (`objects/<hash[0..2]>/<hash>`, `indexes/<id>.json`, and legacy
    /// `virtual/<id>/...`) while still deduplicating the underlying bytes in
    /// `store/`. Assets are hardlinked here from the content store.
    pub fn assets_dir(&self) -> PathBuf {
        self.home.join("assets")
    }

    /// The Mojang-shaped `objects/<hash[0..2]>/<hash>` tree, populated by
    /// hardlinking blobs out of the content store.
    pub fn assets_objects_dir(&self) -> PathBuf {
        self.assets_dir().join("objects")
    }

    /// Where cached asset-index JSON files live, one per index id; see
    /// [`Paths::assets_index_json`].
    pub fn assets_indexes_dir(&self) -> PathBuf {
        self.assets_dir().join("indexes")
    }

    /// The cached asset-index JSON for a specific index id (e.g. `"17"`),
    /// mirrored here from `meta_dir()` so `--assetsDir` points at a
    /// self-contained tree.
    pub fn assets_index_json(&self, asset_index_id: &str) -> PathBuf {
        self.assets_indexes_dir()
            .join(format!("{asset_index_id}.json"))
    }

    /// Pre-1.7.10 "virtual" asset layout: a named-file tree (rather than
    /// the hash-addressed `objects/` tree) that some legacy versions read
    /// resources from directly. See `AssetIndex::is_virtual`.
    pub fn assets_virtual_dir(&self, asset_index_id: &str) -> PathBuf {
        self.assets_dir().join("virtual").join(asset_index_id)
    }

    /// Root directory holding every instance; see [`Paths::instance_dir`].
    pub fn instances_dir(&self) -> PathBuf {
        self.home.join("instances")
    }

    /// An instance's own directory, named by its slug (e.g. the Minecraft
    /// version id for the minimal M1 instances `bananium install` creates).
    pub fn instance_dir(&self, slug: &str) -> PathBuf {
        self.instances_dir().join(slug)
    }

    /// `instance.toml`: name, MC version, loader, Java/JVM overrides. See
    /// `bananium_instance::InstanceConfig`.
    pub fn instance_toml(&self, slug: &str) -> PathBuf {
        self.instance_dir(slug).join("instance.toml")
    }

    /// `bananium.lock.toml`: the resolved mod list (M4+; unused so far).
    pub fn instance_lock(&self, slug: &str) -> PathBuf {
        self.instance_dir(slug).join("bananium.lock.toml")
    }

    /// `running.toml`: pids Bananium has spawned for this instance,
    /// deliberately separate from `instance.toml` — it's ephemeral runtime
    /// state, not declarative config, and gets pruned/rewritten on every
    /// liveness check rather than only on user edits. See
    /// `bananium_instance::InstanceStore::running_pids`.
    pub fn instance_running_toml(&self, slug: &str) -> PathBuf {
        self.instance_dir(slug).join("running.toml")
    }

    /// `stats.toml`: last-played time and accumulated playtime. Kept apart
    /// from `instance.toml` so recording a launch never rewrites the user's
    /// own config. See `bananium_instance::InstanceStats`.
    pub fn instance_stats_toml(&self, slug: &str) -> PathBuf {
        self.instance_dir(slug).join("stats.toml")
    }

    /// The actual game directory passed to the JVM as `--gameDir` /
    /// `${game_directory}` (holds `mods/`, `saves/`, `config/`, ...).
    pub fn instance_minecraft_dir(&self, slug: &str) -> PathBuf {
        self.instance_dir(slug).join("minecraft")
    }

    /// Per-instance launch logs (distinct from the game's own
    /// `minecraft/logs/`, which Minecraft itself writes to).
    pub fn instance_logs_dir(&self, slug: &str) -> PathBuf {
        self.instance_dir(slug).join("logs")
    }

    /// `profiles.toml`: the saved local (offline) player profiles. See
    /// `bananium_launch::ProfileStore`.
    pub fn profiles_toml(&self) -> PathBuf {
        self.home.join("profiles.toml")
    }

    /// Native library extraction cache, keyed by a hash of the sorted set of
    /// native-jar SHA-1s that were extracted into it, so the same jar set is
    /// never re-extracted twice and a change in the library set (e.g. a
    /// version bump) gets a fresh directory automatically.
    pub fn natives_cache_dir(&self, key_hex: &str) -> PathBuf {
        self.home.join("cache").join("natives").join(key_hex)
    }

    /// User-saved content presets, one TOML file each.
    pub fn presets_dir(&self) -> PathBuf {
        self.home.join("presets")
    }

    /// A `.jar`-named view (reflink/hardlink) of the store blob `sha1`, for
    /// the launch classpath. Store blobs are extensionless, and Fabric's
    /// remapper (tiny-remapper) silently skips classpath inputs that don't
    /// end in `.jar` — it then fails with "Generated deobfuscated JARs
    /// contain no classes". Vanilla doesn't care, but every entry gets the
    /// same treatment so there's one classpath shape to reason about.
    pub fn jar_link(&self, sha1_hex: &str) -> PathBuf {
        let prefix = &sha1_hex[..2.min(sha1_hex.len())];
        self.home
            .join("cache")
            .join("jars")
            .join(prefix)
            .join(format!("{sha1_hex}.jar"))
    }

    /// Create every top-level directory Bananium expects to find on startup.
    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [
            self.home.clone(),
            self.store_dir(),
            self.java_dir(),
            self.meta_dir(),
            self.http_cache_dir(),
            self.assets_dir(),
            self.assets_objects_dir(),
            self.assets_indexes_dir(),
            self.instances_dir(),
        ] {
            std::fs::create_dir_all(&dir)?;
        }
        Ok(())
    }
}
