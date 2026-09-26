use std::path::PathBuf;

use bananium_core::Config;
use bananium_instance::{ContentEntry, ContentKind, Preset};
use serde::Serialize;

use crate::presence::{PresencePreview, PresenceStatus};
use crate::task::TaskInfo;

/// The subset of `Paths` surfaced to frontends, for `bananium config show`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ResolvedPaths {
    pub home: PathBuf,
    pub config_toml: PathBuf,
    pub store_dir: PathBuf,
    pub instances_dir: PathBuf,
    pub java_dir: PathBuf,
    pub assets_dir: PathBuf,
    /// The launcher's own logs, one per run.
    pub logs_dir: PathBuf,
}

/// One instance's summary, as surfaced by `Command::InstanceList` — enough
/// for a frontend's instance list without a separate lookup per row.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct InstanceSummary {
    pub slug: String,
    pub name: String,
    pub mc_version: String,
    /// `"vanilla"` or `"fabric"`.
    pub loader: String,
    /// The pinned loader release, for a modded instance.
    pub loader_version: Option<String>,
    pub ram_mb: Option<u32>,
    pub jvm_args: Vec<String>,
    /// Per-instance Java override, if any.
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub java_path: Option<PathBuf>,
    /// The game directory (`--gameDir`): saves, mods, resource packs,
    /// screenshots — what "open folder" should open.
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub game_dir: PathBuf,
    /// Whether `bananium_instance::InstanceStore::is_running` currently
    /// sees a live pid recorded for this instance.
    pub running: bool,
    /// User-chosen library group; `None` is ungrouped.
    pub group: Option<String>,
    /// Custom icon image, displayable through the desktop app's asset
    /// protocol. `None` means the frontend draws its own placeholder.
    #[cfg_attr(feature = "ts", ts(type = "string | null"))]
    pub icon_path: Option<PathBuf>,
    /// Last launch time (Unix seconds); `None` if never played.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub last_played_unix: Option<u64>,
    /// Total time played across every launch.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub playtime_secs: u64,
    /// Enabled mods in `mods/`.
    pub mod_count: u32,
    /// Hidden from Discord Rich Presence (see `Command::InstanceSetDiscord`).
    pub discord_hidden: bool,
    /// Slug of the Modrinth modpack it was installed from, if any.
    pub modrinth_project: Option<String>,
}

/// One Modrinth search result.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ModrinthHit {
    pub project_id: String,
    pub slug: Option<String>,
    pub title: String,
    pub description: String,
    pub author: String,
    pub icon_url: Option<String>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub downloads: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub follows: u64,
    pub categories: Vec<String>,
    pub date_modified: String,
    /// Already installed in the instance the search targeted.
    pub installed: bool,
}

/// One gallery image of a Modrinth project.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct GalleryItem {
    pub url: String,
    pub title: Option<String>,
}

/// A Modrinth project's full details, for a project page.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ModrinthProject {
    pub id: String,
    pub slug: Option<String>,
    pub title: String,
    pub description: String,
    /// Long description, markdown.
    pub body: String,
    pub project_type: String,
    pub icon_url: Option<String>,
    pub gallery: Vec<GalleryItem>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub downloads: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub followers: u64,
    pub categories: Vec<String>,
    pub license: Option<String>,
    pub updated: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub source_url: Option<String>,
    pub issues_url: Option<String>,
    pub wiki_url: Option<String>,
    pub discord_url: Option<String>,
}

/// One version of a Modrinth project.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ModrinthVersion {
    pub id: String,
    pub name: String,
    pub version_number: String,
    /// `release`, `beta`, or `alpha`.
    pub version_type: String,
    pub date_published: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub downloads: u64,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    /// Usable by the instance the listing targeted (always `true` without one).
    pub compatible: bool,
}

/// An installed Modrinth project with a newer compatible version available.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ContentUpdateInfo {
    pub project_id: String,
    pub kind: ContentKind,
    pub filename: String,
    pub title: String,
    pub current_version: Option<String>,
    pub new_version_id: String,
    pub new_version_number: String,
}

/// A usable JVM: a Mojang runtime Bananium downloaded, or one detected on
/// the machine.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct JavaInstall {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub path: PathBuf,
    pub major_version: u32,
    /// `"mojang"` (downloaded into `java/`) or `"system"` (detected).
    pub source: String,
    /// Mojang runtime component, e.g. `"java-runtime-delta"`.
    pub component: Option<String>,
    /// Mojang runtime version, e.g. `"21.0.7"`.
    pub version: Option<String>,
    /// Disk used by a Mojang runtime.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub size_bytes: Option<u64>,
}

/// One screenshot file.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Screenshot {
    pub instance: String,
    pub instance_name: String,
    pub file_name: String,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub path: PathBuf,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub size: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub taken_unix: u64,
}

/// A preset entry that wasn't applied, and why.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SkippedEntry {
    pub title: String,
    pub reason: String,
}

/// One Minecraft version from Mojang's manifest.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct VersionSummary {
    pub id: String,
    /// `release`, `snapshot`, `old_beta`, or `old_alpha`.
    pub kind: String,
    /// ISO-8601 release timestamp.
    pub release_time: String,
}

/// One Fabric loader release.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FabricLoaderSummary {
    pub version: String,
    pub stable: bool,
}

/// One launch log file, as listed by `Command::LogList`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct LogFile {
    /// File name; pass back as `LogRead::file`.
    pub name: String,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub path: PathBuf,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub size: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub modified_unix: u64,
}

/// How a launcher run ended, read back from its log file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum LogStatus {
    /// This process's own log.
    Running,
    /// Ended with the clean-shutdown marker.
    Closed,
    /// Ended with a panic.
    Crashed,
    /// Ended with neither: killed, power loss, or a crash that couldn't be
    /// logged.
    Unclean,
}

/// One launcher log file, as listed by `Command::LauncherLogList`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct LauncherLogFile {
    /// File name (`YYYY-MM-DD_HH-MM-SS[_n].log`); pass back as
    /// `LauncherLogRead::file`.
    pub name: String,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub path: PathBuf,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub size: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub modified_unix: u64,
    pub status: LogStatus,
    /// One line on how it ended ("Closed: ...", "Crashed: ...", ...);
    /// `None` while running.
    pub reason: Option<String>,
}

/// What a modpack needs, read from its `modrinth.index.json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ModpackSummary {
    pub name: String,
    /// The pack's own version, e.g. `"5.3.0"`.
    pub version_id: String,
    pub summary: Option<String>,
    pub mc_version: String,
    /// `"vanilla"` or `"fabric"`.
    pub loader: String,
    pub loader_version: Option<String>,
    /// Files downloaded on install (server-only files excluded).
    pub file_count: u32,
    /// Why Bananium can't install this pack (e.g. it needs Forge); `None`
    /// when it can.
    pub unsupported: Option<String>,
}

/// One entry of a folder in an instance's game directory, as listed by
/// `Command::FileList`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FileEntry {
    pub name: String,
    /// Relative to the game directory, `/`-separated; pass back to the
    /// other `File*` commands.
    pub path: String,
    pub is_dir: bool,
    /// Bytes; 0 for folders.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub size: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub modified_unix: u64,
    /// The real path, for "open with the system" / "show in folder".
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub abs_path: PathBuf,
}

/// The result of a successfully dispatched `Command`. One variant per
/// `Command` variant.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum CommandOutput {
    ConfigShown {
        paths: ResolvedPaths,
        config: Config,
    },
    Installed {
        instance: String,
        mc_version: String,
    },
    /// `--dry-run`: nothing was executed, this is just the rendering.
    LaunchPlanned {
        instance: String,
        command_line: String,
    },
    Launched {
        instance: String,
        pid: u32,
        /// Where this run's JVM stdout/stderr were redirected — the process
        /// is spawned detached from the frontend's own stdio (see
        /// `Session::launch`'s doc comment), so this is how a caller finds
        /// the output after the fact.
        log_path: PathBuf,
    },
    InstanceListed {
        instances: Vec<InstanceSummary>,
    },
    InstanceUpdated {
        instance: String,
    },
    VersionListed {
        latest_release: String,
        versions: Vec<VersionSummary>,
    },
    FabricLoaderListed {
        mc_version: String,
        loaders: Vec<FabricLoaderSummary>,
    },
    InstanceRemoved {
        instance: String,
    },
    InstanceRenamed {
        old: String,
        /// The new slug.
        instance: String,
    },
    InstanceCloned {
        source: String,
        /// The new instance's slug.
        instance: String,
    },
    InstanceKilled {
        instance: String,
    },
    LogListed {
        instance: String,
        logs: Vec<LogFile>,
    },
    FileListed {
        instance: String,
        /// The listed folder, normalised (no leading/trailing `/`).
        path: String,
        entries: Vec<FileEntry>,
    },
    FileContents {
        path: String,
        text: String,
    },
    /// A file or folder was written, created, or renamed to `path`.
    FileWritten {
        path: String,
    },
    FileDeleted {
        path: String,
    },
    FileImported {
        count: u32,
    },
    LauncherLogListed {
        /// This run's log file name, if it has one.
        current: Option<String>,
        logs: Vec<LauncherLogFile>,
    },
    LauncherLogChunk {
        /// Which log was read; `None` when there is none.
        file: Option<String>,
        text: String,
        /// Byte range of `text` in the file: pass `end` back as `offset`
        /// to follow the log, `start` as `before` to page backwards.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        start: u64,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        end: u64,
        /// The file's size when it was read.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        size: u64,
    },
    /// `Command::LauncherLastSession`. `log` is `None` when this is the
    /// first run (or the previous log was deleted).
    LauncherLastSession {
        log: Option<LauncherLogFile>,
    },
    /// `Command::LogFrontend` was written.
    Logged,
    LogChunk {
        /// Which log was read; `None` when the instance has no logs yet.
        file: Option<String>,
        text: String,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        next_offset: u64,
    },
    ModrinthSearched {
        hits: Vec<ModrinthHit>,
        offset: u32,
        total_hits: u32,
    },
    ModrinthProjectShown {
        project: ModrinthProject,
    },
    ModrinthVersionsListed {
        versions: Vec<ModrinthVersion>,
    },
    ModpackInspected {
        pack: ModpackSummary,
    },
    ContentListed {
        instance: String,
        entries: Vec<ContentEntry>,
    },
    /// Everything that was installed, dependencies included.
    ContentInstalled {
        instance: String,
        installed: Vec<ContentEntry>,
    },
    ContentRemoved {
        instance: String,
        filename: String,
    },
    ContentToggled {
        instance: String,
        filename: String,
        enabled: bool,
    },
    ContentImported {
        instance: String,
        entry: ContentEntry,
    },
    ContentIdentified {
        instance: String,
        /// Files matched to a Modrinth project this time.
        identified: u32,
        /// Files looked up that Modrinth doesn't know (each is logged by
        /// name); they're marked and not checked again.
        not_found: u32,
    },
    ContentUpdatesFound {
        instance: String,
        updates: Vec<ContentUpdateInfo>,
    },
    JavaListed {
        installs: Vec<JavaInstall>,
    },
    JavaRuntimeRemoved {
        component: String,
    },
    /// The Mojang runtime an instance uses unless a Java is chosen for it.
    InstanceJavaShown {
        instance: String,
        component: String,
        major_version: Option<u32>,
        /// Installed runtime version; `None` if not downloaded yet (it
        /// downloads before the first launch).
        installed_version: Option<String>,
        /// Where its `java` executable is (or will be).
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        path: PathBuf,
        /// Whether Mojang publishes runtimes for this platform at all.
        available: bool,
    },
    ScreenshotListed {
        screenshots: Vec<Screenshot>,
    },
    ScreenshotDeleted {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        path: PathBuf,
    },
    PresetListed {
        presets: Vec<Preset>,
    },
    PresetSaved {
        preset: Preset,
        /// Untracked local files that couldn't be included.
        skipped_local: u32,
    },
    PresetApplied {
        instance: String,
        /// Everything installed, dependencies included.
        applied: Vec<ContentEntry>,
        skipped: Vec<SkippedEntry>,
    },
    PresetDeleted {
        name: String,
    },
    PresetRenamed {
        name: String,
    },
    PresetExported {
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        path: PathBuf,
    },
    PresetImported {
        preset: Preset,
    },
    ProfileListed {
        profiles: Vec<ProfileSummary>,
    },
    ProfileAdded {
        profile: ProfileSummary,
    },
    ProfileRemoved {
        name: String,
    },
    ProfileDefaultSet {
        name: String,
    },
    PresenceStatusShown {
        status: PresenceStatus,
    },
    /// `None` when nothing would be shown (e.g. launcher activity is off
    /// and no game is running).
    PresencePreviewed {
        preview: Option<PresencePreview>,
    },
    PresenceViewSet,
    TaskListed {
        tasks: Vec<TaskInfo>,
    },
    TaskCancelled {
        task_id: String,
    },
}

/// One saved offline profile (a username to play as).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ProfileSummary {
    pub name: String,
    /// The offline UUID vanilla derives from `name` (hyphenated form).
    pub uuid: String,
    /// Whether a launch that doesn't name a profile would use this one.
    pub is_default: bool,
}
