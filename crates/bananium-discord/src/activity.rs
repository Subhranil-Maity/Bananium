//! The Rich Presence activity payload for `SET_ACTIVITY`.
//!
//! Discord rejects the *whole* update if any one field breaks its rules
//! (a 1-character `state`, a 200-character `details`, an `http://` button),
//! so [`Activity::sanitize`] clamps everything to what Discord accepts
//! instead of letting one long instance name blank the presence.

use serde::{Serialize, Serializer};

/// Text fields must be 2–128 characters.
const TEXT_MIN: usize = 2;
const TEXT_MAX: usize = 128;
/// Image keys/URLs and the `*_url` links.
const URL_MAX: usize = 256;
const BUTTON_LABEL_MAX: usize = 32;
const BUTTON_URL_MAX: usize = 512;
const BUTTONS_MAX: usize = 2;

/// The verb Discord shows before the app name. `SET_ACTIVITY` only accepts
/// these four.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ActivityType {
    #[default]
    Playing = 0,
    Listening = 2,
    Watching = 3,
    Competing = 5,
}

impl Serialize for ActivityType {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(*self as u8)
    }
}

/// Which text the member list shows next to a user's name ("Playing
/// Bananium" vs. the activity's `state` or `details` line).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StatusDisplayType {
    /// The application's name (Discord's default).
    #[default]
    Name = 0,
    State = 1,
    Details = 2,
}

impl Serialize for StatusDisplayType {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(*self as u8)
    }
}

/// Unix timestamps in **milliseconds**. `start` alone shows "elapsed";
/// `start` with `end` shows a progress bar / time remaining.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Timestamps {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<u64>,
}

/// The large image and the small badge in its corner. Each image is either
/// an art-asset key uploaded to the Discord application or an `https://`
/// URL, which Discord proxies (see [`crate::art`]). `*_text` is the hover
/// tooltip; `*_url` makes the image a link.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Assets {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub large_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub large_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub large_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_url: Option<String>,
}

impl Assets {
    fn is_empty(&self) -> bool {
        self == &Assets::default()
    }
}

/// A link button under the activity. Other people see it; Discord hides
/// your own buttons from you.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Button {
    pub label: String,
    pub url: String,
}

/// One Rich Presence activity. Build it with the chained setters, then
/// hand it to [`crate::DiscordClient::set_activity`], which sanitizes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Activity {
    #[serde(rename = "type")]
    pub kind: ActivityType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_display_type: Option<StatusDisplayType>,
    /// First line under the app name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details_url: Option<String>,
    /// Second line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamps: Option<Timestamps>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assets: Option<Assets>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<Button>,
}

impl Activity {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn details(mut self, text: impl Into<String>) -> Self {
        self.details = Some(text.into());
        self
    }

    pub fn details_url(mut self, url: impl Into<String>) -> Self {
        self.details_url = Some(url.into());
        self
    }

    pub fn state(mut self, text: impl Into<String>) -> Self {
        self.state = Some(text.into());
        self
    }

    pub fn state_url(mut self, url: impl Into<String>) -> Self {
        self.state_url = Some(url.into());
        self
    }

    pub fn status_display(mut self, kind: StatusDisplayType) -> Self {
        self.status_display_type = Some(kind);
        self
    }

    /// Show "elapsed" counting from `start_ms`.
    pub fn started_at(mut self, start_ms: u64) -> Self {
        self.timestamps
            .get_or_insert_with(Timestamps::default)
            .start = Some(start_ms);
        self
    }

    /// With [`Activity::started_at`], turns "elapsed" into a progress bar
    /// ending at `end_ms`.
    pub fn ends_at(mut self, end_ms: u64) -> Self {
        self.timestamps.get_or_insert_with(Timestamps::default).end = Some(end_ms);
        self
    }

    pub fn large_image(mut self, image: impl Into<String>, text: Option<String>) -> Self {
        let assets = self.assets.get_or_insert_with(Assets::default);
        assets.large_image = Some(image.into());
        assets.large_text = text;
        self
    }

    pub fn small_image(mut self, image: impl Into<String>, text: Option<String>) -> Self {
        let assets = self.assets.get_or_insert_with(Assets::default);
        assets.small_image = Some(image.into());
        assets.small_text = text;
        self
    }

    /// Adds a button; ones past Discord's limit of two are dropped by
    /// [`Activity::sanitize`].
    pub fn button(mut self, label: impl Into<String>, url: impl Into<String>) -> Self {
        self.buttons.push(Button {
            label: label.into(),
            url: url.into(),
        });
        self
    }

    /// A copy Discord will accept: text trimmed, padded to 2 and truncated
    /// to 128 characters; links and image URLs that aren't `https://` (or
    /// are too long) dropped; at most two valid buttons; an `end` before
    /// `start` dropped; empty groups removed.
    pub fn sanitize(&self) -> Activity {
        let mut a = self.clone();
        a.details = text(a.details);
        a.state = text(a.state);
        a.details_url = link(a.details_url, URL_MAX);
        a.state_url = link(a.state_url, URL_MAX);

        if let Some(ts) = &mut a.timestamps {
            if let (Some(start), Some(end)) = (ts.start, ts.end) {
                if end < start {
                    ts.end = None;
                }
            }
        }
        if a.timestamps.as_ref() == Some(&Timestamps::default()) {
            a.timestamps = None;
        }

        if let Some(assets) = &mut a.assets {
            assets.large_image = image(assets.large_image.take());
            assets.small_image = image(assets.small_image.take());
            // Hover text only means something on an image that's shown.
            assets.large_text = assets
                .large_image
                .as_ref()
                .and_then(|_| text(assets.large_text.take()));
            assets.small_text = assets
                .small_image
                .as_ref()
                .and_then(|_| text(assets.small_text.take()));
            assets.large_url = link(assets.large_url.take(), URL_MAX);
            assets.small_url = link(assets.small_url.take(), URL_MAX);
        }
        if a.assets.as_ref().is_some_and(Assets::is_empty) {
            a.assets = None;
        }

        a.buttons = a
            .buttons
            .into_iter()
            .filter_map(|b| {
                let label = truncate(b.label.trim(), BUTTON_LABEL_MAX);
                let url = link(Some(b.url), BUTTON_URL_MAX)?;
                (!label.is_empty()).then_some(Button { label, url })
            })
            .take(BUTTONS_MAX)
            .collect();
        a
    }
}

/// Discord counts characters, so this cuts on `char`s (never mid-UTF-8)
/// and marks the cut with an ellipsis.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

fn text(s: Option<String>) -> Option<String> {
    let s = s?;
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut out = truncate(s, TEXT_MAX);
    // A 1-character field is rejected outright. Pad with the Hangul filler,
    // which renders blank but isn't whitespace, so Discord doesn't trim it
    // back to one character.
    while out.chars().count() < TEXT_MIN {
        out.push('\u{3164}');
    }
    Some(out)
}

fn link(url: Option<String>, max: usize) -> Option<String> {
    let url = url?.trim().to_string();
    (url.starts_with("https://") && url.len() <= max).then_some(url)
}

/// An image is either an `https://` URL or an uploaded asset key (no
/// scheme at all); `http://` and other schemes are dropped.
fn image(img: Option<String>) -> Option<String> {
    let img = img?.trim().to_string();
    if img.is_empty() || img.len() > URL_MAX {
        return None;
    }
    if img.contains("://") && !img.starts_with("https://") {
        return None;
    }
    Some(img)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_only_set_fields_with_numeric_enums() {
        let a = Activity::new()
            .details("Playing Minecraft 1.21.1")
            .state("Fabric · 49 mods")
            .status_display(StatusDisplayType::Details)
            .started_at(1_700_000_000_000)
            .large_image("https://cdn.modrinth.com/icon.png", Some("Pack".into()))
            .button(
                "Get Bananium",
                "https://github.com/Subhranil-Maity/Bananium",
            );
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            json!({
                "type": 0,
                "status_display_type": 2,
                "details": "Playing Minecraft 1.21.1",
                "state": "Fabric · 49 mods",
                "timestamps": {"start": 1_700_000_000_000u64},
                "assets": {"large_image": "https://cdn.modrinth.com/icon.png", "large_text": "Pack"},
                "buttons": [{"label": "Get Bananium", "url": "https://github.com/Subhranil-Maity/Bananium"}],
            })
        );
    }

    #[test]
    fn sanitize_clamps_text_lengths() {
        let long = "é".repeat(300);
        let a = Activity::new().details(long).state("x").sanitize();
        let details = a.details.unwrap();
        assert_eq!(details.chars().count(), TEXT_MAX);
        assert!(details.ends_with('…'));
        assert_eq!(a.state.unwrap().chars().count(), TEXT_MIN);

        let blank = Activity::new().details("   ").sanitize();
        assert_eq!(blank.details, None);
    }

    #[test]
    fn sanitize_drops_bad_links_images_and_extra_buttons() {
        let a = Activity::new()
            .details_url("http://insecure.example")
            .large_image("http://insecure.example/a.png", Some("hover".into()))
            .small_image("bananium", Some("via Bananium".into()))
            .button("One", "https://a.example")
            .button("Bad", "ftp://b.example")
            .button("Two", "https://c.example")
            .button("Three", "https://d.example")
            .sanitize();
        assert_eq!(a.details_url, None);
        let assets = a.assets.unwrap();
        assert_eq!(assets.large_image, None);
        assert_eq!(assets.large_text, None, "hover text without an image");
        assert_eq!(assets.small_image.as_deref(), Some("bananium"));
        let labels: Vec<_> = a.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["One", "Two"]);
    }

    #[test]
    fn sanitize_drops_end_before_start_and_empty_groups() {
        let a = Activity::new().started_at(100).ends_at(50).sanitize();
        assert_eq!(
            a.timestamps,
            Some(Timestamps {
                start: Some(100),
                end: None
            })
        );

        let a = Activity::new()
            .large_image("http://nope.example/a.png", None)
            .sanitize();
        assert_eq!(a.assets, None);
        assert_eq!(serde_json::to_value(&a).unwrap(), json!({"type": 0}));
    }

    #[test]
    fn long_button_labels_are_truncated() {
        let a = Activity::new()
            .button("A".repeat(40), "https://a.example")
            .sanitize();
        assert_eq!(a.buttons[0].label.chars().count(), BUTTON_LABEL_MAX);
    }
}
