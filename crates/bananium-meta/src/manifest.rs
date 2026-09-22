use serde::Deserialize;

/// Mojang's top-level list of every Minecraft version ever released.
pub const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// The full response from [`VERSION_MANIFEST_URL`].
#[derive(Debug, Clone, Deserialize)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<VersionManifestEntry>,
}

/// The current latest release and snapshot version ids.
#[derive(Debug, Clone, Deserialize)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

/// One entry in [`VersionManifest::versions`] — enough to locate and verify
/// that version's full profile (fetched separately via `url`).
#[derive(Debug, Clone, Deserialize)]
pub struct VersionManifestEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    /// Where to fetch the full `VersionProfile` for this version.
    pub url: String,
    /// SHA-1 of the profile JSON at `url`, and also the value embedded in
    /// that URL's own path (Mojang serves version profiles at a
    /// content-addressed location).
    pub sha1: String,
    pub time: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
}

impl VersionManifest {
    /// Look up a version by its exact id (e.g. `"1.21.1"`).
    pub fn find(&self, id: &str) -> Option<&VersionManifestEntry> {
        self.versions.iter().find(|v| v.id == id)
    }
}
