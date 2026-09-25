//! Discord Rich Presence types a frontend sees: the connection status, the
//! launcher page it reports, and the preview it can render. The presence
//! logic itself lives in `session/presence.rs`.

use bananium_core::StatusDisplay;
use serde::{Deserialize, Serialize};

/// The Discord application Bananium presents as: its name ("Bananium") is
/// the "Playing …" headline, and everything else is set per activity.
pub const DISCORD_APP_ID: &str = "1553055181044715520";

/// Where the connection to the local Discord client stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PresenceStatus {
    /// Turned off in settings, or never started by this frontend.
    Disabled,
    /// First attempt to reach Discord (at start-up, or right after the
    /// connection dropped).
    Connecting,
    Connected,
    /// Discord couldn't be reached (usually: it isn't running). Bananium
    /// keeps trying every [`RETRY_INTERVAL_SECS`] seconds, so this turns
    /// into `Connected` by itself once Discord is opened;
    /// `Command::PresenceReconnect` tries again right away. `error` is why
    /// the last attempt failed.
    Waiting {
        error: String,
    },
}

/// How often Bananium retries reaching Discord while it's not connected.
pub const RETRY_INTERVAL_SECS: u64 = 5;

/// Which launcher page the user is on, reported by the frontend with
/// `Command::PresenceSetView` so an idle presence can say what they're
/// doing ("Browsing mods", "Looking at Sodium").
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "view", rename_all = "snake_case")]
pub enum LauncherView {
    #[default]
    Library,
    /// One instance's page.
    Instance {
        instance: String,
    },
    /// Searching Modrinth. `kind` is `"mod"`, `"resource_pack"`, `"shader"`
    /// or `"modpack"`.
    Browse {
        kind: String,
    },
    /// One Modrinth project's page. Only `https://` icon/page URLs are
    /// ever shown on Discord.
    Project {
        title: String,
        #[serde(default)]
        author: Option<String>,
        #[serde(default)]
        icon_url: Option<String>,
        #[serde(default)]
        url: Option<String>,
    },
    Presets,
    Screenshots,
    Accounts,
    Settings,
    About,
    /// Anywhere else: shown as plain "In the launcher".
    Other,
}

/// A canned situation for previewing presence in settings, so every toggle
/// can be tried without actually launching or installing anything.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PreviewScenario {
    /// Whatever is really going on right now.
    #[default]
    Live,
    Idle,
    Browsing,
    Installing,
    Playing,
}

/// An activity exactly as Discord will show it (already clamped to
/// Discord's limits), for a frontend to render a look-alike card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PresencePreview {
    /// The "Playing …" headline: the Discord application's name.
    pub app_name: String,
    pub details: Option<String>,
    pub state: Option<String>,
    /// Image URLs.
    pub large_image: Option<String>,
    pub large_text: Option<String>,
    pub small_image: Option<String>,
    pub small_text: Option<String>,
    /// Unix milliseconds: "elapsed" counts from `start_ms`; with `end_ms`
    /// too, it's a progress bar.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub start_ms: Option<u64>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub end_ms: Option<u64>,
    pub buttons: Vec<PreviewButton>,
    /// Which line the member list shows.
    pub status_display: StatusDisplay,
}

/// A link button under the activity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PreviewButton {
    pub label: String,
    pub url: String,
}
