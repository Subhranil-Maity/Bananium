use std::path::Path;

use bananium_core::Paths;
use bananium_net::HttpClient;
use serde::de::DeserializeOwned;

use crate::assets::AssetIndex;
use crate::error::{Error, Result};
use crate::manifest::{VersionManifest, VersionManifestEntry, VERSION_MANIFEST_URL};
use crate::profile::{AssetIndexRef, VersionProfile};

/// Fetches Mojang metadata and mirrors every response verbatim under
/// `~/.bananium/meta/`. A network failure falls back to whatever was cached
/// last time, which is what makes browsing/installing/launching work with
/// the interface down as long as the metadata has been seen before.
pub struct MetaClient {
    http: HttpClient,
    paths: Paths,
}

impl MetaClient {
    pub fn new(http: HttpClient, paths: Paths) -> Self {
        Self { http, paths }
    }

    pub(crate) fn http(&self) -> &HttpClient {
        &self.http
    }

    pub(crate) fn paths(&self) -> &Paths {
        &self.paths
    }

    /// Fetch (or, offline, load the cached) version manifest.
    pub async fn version_manifest(&self) -> Result<VersionManifest> {
        let cache_path = self.paths.meta_dir().join("version_manifest_v2.json");
        self.fetch_cached("version manifest", VERSION_MANIFEST_URL, &cache_path)
            .await
    }

    /// Look up a version by id in the manifest, erroring if it isn't listed.
    pub async fn resolve_version(&self, id: &str) -> Result<VersionManifestEntry> {
        let manifest = self.version_manifest().await?;
        manifest
            .find(id)
            .cloned()
            .ok_or_else(|| Error::UnknownVersion(id.to_string()))
    }

    /// Fetch (or, offline, load the cached) full profile for a manifest entry.
    pub async fn version_profile(&self, entry: &VersionManifestEntry) -> Result<VersionProfile> {
        let cache_path = self
            .paths
            .meta_dir()
            .join("versions")
            .join(format!("{}.json", entry.id));
        self.fetch_cached("version profile", &entry.url, &cache_path)
            .await
    }

    /// Fetch (or, offline, load the cached) asset index a profile points at.
    pub async fn asset_index(&self, asset_index_ref: &AssetIndexRef) -> Result<AssetIndex> {
        let cache_path = self
            .paths
            .meta_dir()
            .join("asset_indexes")
            .join(format!("{}.json", asset_index_ref.id));
        self.fetch_cached("asset index", &asset_index_ref.url, &cache_path)
            .await
    }

    /// The shared fetch-or-fallback logic behind every method above: try
    /// the network first (so metadata never silently goes stale while
    /// online), write whatever comes back to `cache_path` verbatim, and
    /// only on a network *error* fall back to the last-cached bytes at that
    /// path. This ordering — network first, cache as fallback rather than
    /// cache-first — is what the offline gate actually depends on: it must
    /// degrade gracefully, not silently prefer stale data while connected.
    /// Public so other Mojang metadata (e.g. the java-runtime manifests,
    /// whose types live in `bananium-java`) gets the same behavior.
    pub async fn fetch_cached<T: DeserializeOwned>(
        &self,
        what: &'static str,
        url: &str,
        cache_path: &Path,
    ) -> Result<T> {
        match self.http.get_bytes(url).await {
            Ok(bytes) => {
                if let Some(parent) = cache_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::write(cache_path, &bytes).await?;
                serde_json::from_slice(&bytes).map_err(|source| Error::Parse {
                    what,
                    url: url.to_string(),
                    source,
                })
            }
            Err(net_err) => {
                if cache_path.is_file() {
                    let bytes = tokio::fs::read(cache_path).await?;
                    serde_json::from_slice(&bytes).map_err(|source| Error::Parse {
                        what,
                        url: url.to_string(),
                        source,
                    })
                } else {
                    Err(net_err.into())
                }
            }
        }
    }
}
