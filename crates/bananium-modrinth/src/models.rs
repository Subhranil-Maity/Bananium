//! JSON shapes returned (or, for request bodies, accepted) by the Modrinth
//! v2 API. Open-ended fields whose value set Modrinth can extend over time
//! (`status`, `version_type`, `environment`, `dependency_type`, ...) are
//! kept as plain `String` rather than closed Rust enums, so a new value
//! Modrinth starts returning tomorrow deserializes fine today instead of
//! hard-failing this client. Fields absent from this crate weren't needed
//! by anything in scope here (see the crate root doc comment for what is);
//! unknown JSON fields are ignored by `serde` by default, so growth on
//! Modrinth's side is forward-compatible without any special handling.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// `GET /search` response envelope.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchResponse {
    /// The page of matching projects.
    pub hits: Vec<SearchHit>,
    /// Echoes the request's `offset`.
    pub offset: u32,
    /// Echoes the request's `limit`.
    pub limit: u32,
    /// Total matches across all pages, for computing page count.
    pub total_hits: u32,
}

/// One project summary row in a [`SearchResponse`] — a condensed view of a
/// [`Project`], not the full project (no `body`, license details, etc.).
#[derive(Debug, Clone, Deserialize)]
pub struct SearchHit {
    pub project_id: String,
    pub project_type: String,
    #[serde(default)]
    pub slug: Option<String>,
    pub title: String,
    pub description: String,
    pub author: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub display_categories: Vec<String>,
    #[serde(default)]
    pub versions: Vec<String>,
    pub downloads: u64,
    pub follows: u64,
    #[serde(default)]
    pub icon_url: Option<String>,
    pub date_created: String,
    pub date_modified: String,
    #[serde(default)]
    pub latest_version: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub gallery: Vec<String>,
    #[serde(default)]
    pub color: Option<i64>,
}

/// The full `GET /project/{id|slug}` response.
#[derive(Debug, Clone, Deserialize)]
pub struct Project {
    /// Base62-encoded project id.
    pub id: String,
    /// Id of the team with ownership rights over this project.
    pub team: String,
    #[serde(default)]
    pub slug: Option<String>,
    pub title: String,
    /// Short one-to-two sentence summary.
    pub description: String,
    /// Full long-form project description (usually markdown).
    pub body: String,
    /// Moderation state, e.g. `"approved"`, `"archived"`, `"draft"`.
    pub status: String,
    /// `"mod"`, `"modpack"`, `"resourcepack"`, or `"shader"`.
    pub project_type: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub additional_categories: Vec<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    /// Version ids belonging to this project — fetch full [`Version`]
    /// objects via [`crate::ModrinthClient::project_versions`].
    #[serde(default)]
    pub versions: Vec<String>,
    #[serde(default)]
    pub license: Option<License>,
    /// ISO-8601 creation timestamp.
    pub published: String,
    /// ISO-8601 timestamp of the most recent version.
    pub updated: String,
    pub downloads: u64,
    pub followers: u64,
    #[serde(default)]
    pub gallery: Vec<GalleryImage>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub issues_url: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub wiki_url: Option<String>,
    #[serde(default)]
    pub discord_url: Option<String>,
    #[serde(default)]
    pub donation_urls: Vec<DonationUrl>,
    #[serde(default)]
    pub organization: Option<String>,
    /// Deprecated by Modrinth in favor of `environment`-style fields on
    /// [`Version`], but still present on the wire — kept here rather than
    /// dropped so callers reading an older field don't hit a silent gap.
    #[serde(default)]
    pub client_side: Option<String>,
    #[serde(default)]
    pub server_side: Option<String>,
}

/// SPDX-ish license identification on a [`Project`].
#[derive(Debug, Clone, Deserialize)]
pub struct License {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
}

/// One image in a project's gallery.
#[derive(Debug, Clone, Deserialize)]
pub struct GalleryImage {
    pub url: String,
    #[serde(default)]
    pub featured: bool,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub ordering: i64,
}

/// A donation platform link on a [`Project`].
#[derive(Debug, Clone, Deserialize)]
pub struct DonationUrl {
    pub id: String,
    pub platform: String,
    pub url: String,
}

/// A single published version of a project, as returned by
/// `GET /project/{id|slug}/version`, `POST /version_files`, and
/// `POST /version_files/update`.
#[derive(Debug, Clone, Deserialize)]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub author_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version_number: String,
    #[serde(default)]
    pub changelog: Option<String>,
    /// ISO-8601 publish timestamp.
    pub date_published: String,
    pub downloads: u64,
    /// `"release"`, `"beta"`, or `"alpha"`.
    #[serde(default)]
    pub version_type: String,
    /// `"listed"`, `"archived"`, `"draft"`, `"unlisted"`, `"scheduled"`.
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub featured: bool,
    /// The downloadable files for this version — usually one, but modpacks
    /// or split-jar releases can have more than one.
    #[serde(default)]
    pub files: Vec<VersionFile>,
    #[serde(default)]
    pub dependencies: Vec<VersionDependency>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub environment: Option<String>,
}

/// One downloadable file attached to a [`Version`].
#[derive(Debug, Clone, Deserialize)]
pub struct VersionFile {
    pub hashes: FileHashes,
    pub url: String,
    pub filename: String,
    /// True for the file a client should install when a version has more
    /// than one (e.g. a "sources" jar alongside the real one).
    #[serde(default)]
    pub primary: bool,
    pub size: u64,
    #[serde(default)]
    pub file_type: Option<String>,
}

/// The hashes Modrinth publishes for a [`VersionFile`] — either may be
/// absent depending on when the file was uploaded, which is why both are
/// `Option` rather than required.
#[derive(Debug, Clone, Deserialize)]
pub struct FileHashes {
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub sha512: Option<String>,
}

/// One dependency edge from a [`Version`] to another project or version.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionDependency {
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    /// `"required"`, `"optional"`, `"incompatible"`, or `"embedded"`.
    pub dependency_type: String,
}

/// Which hash algorithm a bulk hash lookup's `hashes` array uses. Modrinth
/// requires this alongside the hashes themselves rather than inferring it
/// from hash length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HashAlgorithm {
    Sha1,
    Sha512,
}

/// `POST /version_files` response: a map from each requested hash to the
/// [`Version`] it identifies. Hashes with no match are simply absent from
/// the map rather than mapped to `null`.
pub type VersionFilesResponse = HashMap<String, Version>;
