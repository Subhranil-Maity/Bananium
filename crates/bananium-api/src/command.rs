use std::path::PathBuf;

use bananium_core::DiscordConfig;
use bananium_instance::ContentKind;
use serde::{Deserialize, Serialize};

use crate::presence::{LauncherView, PreviewScenario};

/// Sort order for `Command::ModrinthSearch`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum SearchSort {
    #[default]
    Relevance,
    Downloads,
    Follows,
    Newest,
    Updated,
}

/// Where `Command::ModpackInstall` gets its `.mrpack` from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModpackSource {
    /// A version of a Modrinth modpack project, downloaded on install;
    /// without `version`, its newest stable release Bananium can run.
    Modrinth {
        project: String,
        #[serde(default)]
        version: Option<String>,
    },
    /// A `.mrpack` file on disk.
    File {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        path: PathBuf,
    },
}

/// Every action a frontend can ask for. New variants land milestone by
/// milestone; nothing outside `bananium-api` may add capability that isn't
/// expressed here first (see the frontend contract in CONTRIBUTING.md).
#[derive(Debug, Clone, Serialize, Deserialize, strum::IntoStaticStr)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "command", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Command {
    /// Print the resolved config and paths.
    ConfigShow,
    /// Change settings in `config.toml`; `None` leaves a setting alone.
    /// `java_path: Some("")` clears it back to auto-detection.
    ConfigSet {
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(type = "number | null"))]
        max_concurrent_downloads: Option<usize>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(type = "string | null"))]
        java_path: Option<PathBuf>,
    },
    /// Every usable JVM: Mojang runtimes Bananium downloaded, then every
    /// JVM detected on this machine.
    JavaList,
    /// Delete a downloaded Mojang runtime (e.g. `"java-runtime-delta"`);
    /// it's downloaded again when next needed.
    JavaRuntimeRemove {
        component: String,
    },
    /// Which Mojang runtime an instance uses by default, and whether it's
    /// downloaded yet.
    InstanceJava {
        instance: String,
    },
    /// Screenshots from every instance (or just `instance`), newest first.
    ScreenshotList {
        #[serde(default)]
        instance: Option<String>,
    },
    /// Delete one screenshot (must be inside an instance's `screenshots/`).
    ScreenshotDelete {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        path: PathBuf,
    },
    /// Download everything needed to launch `version` offline afterwards,
    /// creating an instance for it named `name` (or a fresh random name
    /// when omitted) if one doesn't already exist under that name.
    /// Distinct names let several instances share the same `version`.
    ///
    /// `fabric_loader` installs the Fabric mod loader on top: a loader
    /// version such as `"0.16.9"`, or `"latest"` for the newest stable one.
    /// `group` files a newly created instance under that library group.
    Install {
        version: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        fabric_loader: Option<String>,
        #[serde(default)]
        group: Option<String>,
    },
    /// Every Minecraft version Mojang publishes, newest first.
    VersionList {
        #[serde(default)]
        include_snapshots: bool,
    },
    /// Fabric loader releases compatible with `mc_version`, newest first.
    FabricLoaderList {
        mc_version: String,
    },
    /// Launch an instance. `instance` is optional only when exactly one is
    /// installed. `profile` selects a named local (offline) profile,
    /// defaulting to the first one saved (or a freshly created "Player").
    /// Refused (outside `dry_run`) when the instance already has a live
    /// pid recorded — only one process per instance at a time, for now.
    Launch {
        #[serde(default)]
        instance: Option<String>,
        #[serde(default)]
        profile: Option<String>,
        #[serde(default)]
        dry_run: bool,
    },
    /// Every installed instance, for a frontend's instance list.
    InstanceList,
    /// Update an instance's RAM cap and/or extra JVM arguments; either
    /// field left as `None` here leaves that setting untouched.
    InstanceSet {
        instance: String,
        /// `Some(0)` clears the cap back to the JVM default (a real 0 MB
        /// cap isn't meaningful, so it doubles as the "unset" sentinel
        /// rather than needing a nested `Option`); `Some(n>0)` sets it;
        /// `None` leaves the current value alone.
        #[serde(default)]
        ram_mb: Option<u32>,
        /// Replaces the stored extra-JVM-args list entirely when given
        /// (an empty `Vec` clears it); `None` leaves the current list
        /// alone.
        #[serde(default)]
        jvm_args: Option<Vec<String>>,
        /// Per-instance Java executable. `Some("")` clears it back to the
        /// global setting/auto-detection; `None` leaves it alone.
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(type = "string | null"))]
        java_path: Option<PathBuf>,
        /// Library group. `Some("")` ungroups it; `None` leaves it alone.
        #[serde(default)]
        group: Option<String>,
    },
    /// Set an instance's icon to a copy of the image at `path` (PNG, JPEG,
    /// GIF or WebP), or clear it back to the default when `path` is `None`.
    InstanceSetIcon {
        instance: String,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(type = "string | null"))]
        path: Option<PathBuf>,
    },
    /// Delete an instance and everything in it. Refused while running.
    InstanceRemove {
        instance: String,
    },
    /// Rename an instance (its directory/slug changes too). Refused while
    /// running.
    InstanceRename {
        instance: String,
        new_name: String,
    },
    /// Copy an instance, game directory and all, under a new name.
    InstanceClone {
        instance: String,
        new_name: String,
    },
    /// Force-stop a game this session launched.
    InstanceKill {
        instance: String,
    },
    /// Every launch log for an instance, newest first.
    LogList {
        instance: String,
    },
    /// Read a chunk of a launch log (the newest when `file` is `None`)
    /// from byte `offset`. Poll with the returned `next_offset` to tail it.
    LogRead {
        instance: String,
        #[serde(default)]
        file: Option<String>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        offset: u64,
    },
    /// The entries of one folder in an instance's game directory. Every
    /// `File*` path is relative to the game directory and `/`-separated;
    /// `""` is the game directory itself. Anything reaching outside it
    /// (`..`, absolute paths) is refused.
    FileList {
        instance: String,
        #[serde(default)]
        path: String,
    },
    /// A text file's contents for in-app editing (at most 2 MiB, UTF-8).
    FileRead {
        instance: String,
        path: String,
    },
    /// Save `text` to a file, creating it and missing parent folders.
    /// `create_new` refuses to overwrite an existing file.
    FileWrite {
        instance: String,
        path: String,
        text: String,
        #[serde(default)]
        create_new: bool,
    },
    /// Create a folder (and any missing parents).
    FileCreateDir {
        instance: String,
        path: String,
    },
    /// Rename or move a file or folder within the game directory.
    FileRename {
        instance: String,
        from: String,
        to: String,
    },
    /// Delete a file, or a folder and everything in it.
    FileDelete {
        instance: String,
        path: String,
    },
    /// Copy files or folders from anywhere on disk into folder `path`.
    FileImport {
        instance: String,
        #[serde(default)]
        path: String,
        #[cfg_attr(feature = "ts", ts(type = "Array<string>"))]
        sources: Vec<PathBuf>,
    },
    /// Search Modrinth for one kind of content. With `instance`, results
    /// are narrowed to what that instance can use (its Minecraft version,
    /// and Fabric/Iris compatibility) and flagged if already installed.
    ModrinthSearch {
        query: String,
        kind: ContentKind,
        #[serde(default)]
        instance: Option<String>,
        /// Modrinth category slugs, each required (AND).
        #[serde(default)]
        categories: Vec<String>,
        #[serde(default)]
        sort: SearchSort,
        #[serde(default)]
        offset: u32,
        #[serde(default)]
        limit: u32,
    },
    /// Full details of one Modrinth project (id or slug).
    ModrinthProject {
        project: String,
    },
    /// Every version of a Modrinth project, newest first, each flagged
    /// with whether `instance` can use it.
    ModrinthVersions {
        project: String,
        kind: ContentKind,
        #[serde(default)]
        instance: Option<String>,
    },
    /// Search Modrinth modpacks that Bananium can run (Fabric).
    ModpackSearch {
        #[serde(default)]
        query: String,
        /// Modrinth category slugs, each required (AND).
        #[serde(default)]
        categories: Vec<String>,
        #[serde(default)]
        sort: SearchSort,
        #[serde(default)]
        offset: u32,
        #[serde(default)]
        limit: u32,
    },
    /// Every version of a modpack project, newest first; `compatible`
    /// marks the ones Bananium can install.
    ModpackVersions {
        project: String,
    },
    /// Read a local `.mrpack`'s manifest: name, Minecraft and loader
    /// versions, and whether Bananium can run it.
    ModpackInspect {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        path: PathBuf,
    },
    /// Create a new instance from a modpack. `name` defaults to the pack's
    /// own name (made valid and unique); an existing name is refused.
    ModpackInstall {
        source: ModpackSource,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        group: Option<String>,
    },
    /// Everything installed in an instance (all kinds), reconciled with
    /// what's actually in its folders.
    ContentList {
        instance: String,
    },
    /// Install a Modrinth project (newest compatible version, or `version`)
    /// plus its required dependencies. A shader pack also brings in Iris.
    ContentInstall {
        instance: String,
        kind: ContentKind,
        project: String,
        #[serde(default)]
        version: Option<String>,
    },
    /// Delete one installed file.
    ContentRemove {
        instance: String,
        kind: ContentKind,
        filename: String,
    },
    /// Enable or disable one installed file (renames it to/from `.disabled`).
    ContentToggle {
        instance: String,
        kind: ContentKind,
        filename: String,
        enabled: bool,
    },
    /// Copy a file from disk into the instance, then try to identify it on
    /// Modrinth.
    ContentImport {
        instance: String,
        kind: ContentKind,
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        path: PathBuf,
    },
    /// Look up untracked files on Modrinth by hash and record what they are.
    ContentIdentify {
        instance: String,
    },
    /// Which installed Modrinth content has a newer compatible version.
    ContentCheckUpdates {
        instance: String,
    },
    /// Update the given projects to their newest compatible versions.
    ContentUpdate {
        instance: String,
        projects: Vec<String>,
    },
    /// Every saved content preset.
    PresetList,
    /// Save an instance's Modrinth content (of `kinds`; all kinds when
    /// empty) as a preset, replacing any preset with the same name.
    PresetSave {
        instance: String,
        name: String,
        #[serde(default)]
        kinds: Vec<ContentKind>,
    },
    /// Install a preset's projects into an instance, each at a version that
    /// instance can use; ones with no such version are skipped, not fatal.
    PresetApply {
        preset: String,
        instance: String,
    },
    PresetDelete {
        name: String,
    },
    PresetRename {
        name: String,
        new_name: String,
    },
    /// Write a preset to a file, for sharing.
    PresetExport {
        name: String,
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        path: PathBuf,
    },
    /// Load a shared preset file.
    PresetImport {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        path: PathBuf,
    },
    /// Every saved offline (local) profile, i.e. the usernames a launch can
    /// play as.
    ProfileList,
    /// Save a new offline profile. `name` must be a valid vanilla username
    /// (3–16 letters, digits, or `_`) not already taken case-insensitively.
    ProfileAdd {
        name: String,
    },
    /// Delete a saved offline profile.
    ProfileRemove {
        name: String,
    },
    /// Make `name` the profile a `Launch` without an explicit `profile` uses.
    ProfileSetDefault {
        name: String,
    },
    /// Save the Discord Rich Presence settings (`[discord]` in
    /// `config.toml`) and apply them to the live presence immediately.
    DiscordConfigSet {
        config: DiscordConfig,
    },
    /// Hide an instance from Discord (a generic "Playing Minecraft" while
    /// it runs), or show it again.
    InstanceSetDiscord {
        instance: String,
        hidden: bool,
    },
    /// Tell Rich Presence which launcher page the user is on.
    PresenceSetView {
        view: LauncherView,
    },
    /// Where the Discord connection stands.
    PresenceStatus,
    /// The activity Discord would show for `scenario`, under `config` (the
    /// saved settings when omitted) — for a live preview while editing.
    PresencePreview {
        #[serde(default)]
        scenario: PreviewScenario,
        #[serde(default)]
        config: Option<DiscordConfig>,
    },
    /// Try reaching Discord right now instead of at the next retry
    /// (or reconnect, when already connected).
    PresenceReconnect,
    /// Every launcher log (one per run of any frontend), newest first.
    LauncherLogList,
    /// One chunk (at most 256 KiB, trimmed to whole lines) of a launcher
    /// log; `file: None` is this run's log. Never the whole file: logs grow
    /// large and go over IPC.
    ///
    /// - `before: None, offset: 0`: the file's tail.
    /// - `before: None, offset: n`: from byte `n` on — poll with the last
    ///   chunk's `end` to follow a growing log.
    /// - `before: Some(n)`: the chunk ending at byte `n` — pass the first
    ///   chunk's `start` to page backwards.
    LauncherLogRead {
        #[serde(default)]
        file: Option<String>,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        offset: u64,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(type = "number | null"))]
        before: Option<u64>,
    },
    /// How the previous launcher run ended, read from the tail of its log
    /// only, so a frontend can warn after a crash without slowing startup.
    LauncherLastSession,
    /// Write a line from the frontend (e.g. an uncaught webview error) into
    /// the launcher log under the `webview` target. `level` is `error`,
    /// `warn`, `info`, or `debug`.
    LogFrontend {
        level: String,
        message: String,
    },
}
