//! Fabric loader metadata (`meta.fabricmc.net/v2`) and merging a Fabric
//! launcher profile into the vanilla [`VersionProfile`] it inherits from.
//!
//! A Fabric profile is a thin overlay: a different `mainClass` (Knot), a
//! handful of extra libraries (the loader, intermediary mappings, ASM,
//! mixin), and a few extra arguments. Everything else — assets, natives,
//! the client jar — comes from the vanilla profile, so the merge is just
//! "vanilla, with those three things replaced or appended".

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::client::MetaClient;
use crate::error::{Error, Result};
use crate::profile::{Argument, Arguments, DownloadArtifact, Library, LibraryDownloads};
use crate::VersionProfile;

/// Base URL of Fabric's metadata API.
pub const FABRIC_META_URL: &str = "https://meta.fabricmc.net/v2";

/// One Fabric loader release, as listed for a given Minecraft version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FabricLoaderVersion {
    pub version: String,
    /// Fabric marks every loader release it considers production-ready as
    /// stable; "latest" in Bananium means the newest stable one.
    pub stable: bool,
}

/// `GET /versions/loader/{mc}` returns one of these per loader release,
/// newest first. Only the loader part is needed.
#[derive(Debug, Deserialize)]
struct LoaderListEntry {
    loader: FabricLoaderVersion,
}

/// A Fabric launcher profile (`/versions/loader/{mc}/{loader}/profile/json`)
/// after [`MetaClient::fabric_profile`] has filled in any missing hashes —
/// which is also the exact shape cached on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FabricProfile {
    pub id: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    #[serde(default)]
    pub arguments: FabricArguments,
    pub libraries: Vec<FabricLibrary>,
}

/// Extra arguments, kept as raw JSON so they round-trip through the cache
/// verbatim and parse with the same [`Argument`] logic as vanilla's.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FabricArguments {
    #[serde(default)]
    pub game: Vec<serde_json::Value>,
    #[serde(default)]
    pub jvm: Vec<serde_json::Value>,
}

/// A maven-coordinate library hosted at `url` (a maven repo base). Fabric
/// includes `sha1`/`size` for some libraries but not all — notably not for
/// the loader or intermediary jars.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FabricLibrary {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

impl FabricLibrary {
    /// Maven-layout path for `group:artifact:version[:classifier]`, e.g.
    /// `net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar`.
    pub fn maven_path(&self) -> Option<String> {
        let mut parts = self.name.split(':');
        let group = parts.next()?;
        let artifact = parts.next()?;
        let version = parts.next()?;
        let classifier = parts.next();
        let file = match classifier {
            Some(c) => format!("{artifact}-{version}-{c}.jar"),
            None => format!("{artifact}-{version}.jar"),
        };
        Some(format!(
            "{}/{artifact}/{version}/{file}",
            group.replace('.', "/")
        ))
    }

    /// Absolute download URL: the repo base plus [`FabricLibrary::maven_path`].
    pub fn download_url(&self) -> Option<String> {
        let base = self.url.trim_end_matches('/');
        Some(format!("{base}/{}", self.maven_path()?))
    }
}

impl MetaClient {
    /// Every Fabric loader release compatible with `mc_version`, newest
    /// first. Network-first with cache fallback, like all Mojang metadata.
    pub async fn fabric_loaders(&self, mc_version: &str) -> Result<Vec<FabricLoaderVersion>> {
        let url = format!("{FABRIC_META_URL}/versions/loader/{mc_version}");
        let cache = self
            .paths()
            .meta_dir()
            .join("fabric")
            .join("loaders")
            .join(format!("{mc_version}.json"));
        let entries: Vec<LoaderListEntry> = self
            .fetch_cached("fabric loader list", &url, &cache)
            .await?;
        Ok(entries.into_iter().map(|e| e.loader).collect())
    }

    /// Resolve `"latest"` to the newest stable loader for `mc_version`;
    /// any other value is returned unchanged.
    pub async fn resolve_fabric_loader(&self, mc_version: &str, requested: &str) -> Result<String> {
        if requested != "latest" {
            return Ok(requested.to_string());
        }
        let loaders = self.fabric_loaders(mc_version).await?;
        loaders
            .iter()
            .find(|l| l.stable)
            .or(loaders.first())
            .map(|l| l.version.clone())
            .ok_or_else(|| Error::NoFabricLoader(mc_version.to_string()))
    }

    /// The Fabric profile for one loader version on one Minecraft version,
    /// with every library's `sha1` filled in.
    ///
    /// Unlike the other metadata here this is **cache-first**: a published
    /// loader-version profile never changes, and resolving it costs one
    /// request per hash-less library, so there's nothing to gain from
    /// re-fetching it. Missing hashes come from the `.sha1` file Fabric's
    /// maven publishes beside every jar; the store is content-addressed by
    /// sha1, so a library can't be placed there without one.
    pub async fn fabric_profile(&self, mc_version: &str, loader: &str) -> Result<FabricProfile> {
        let cache = self
            .paths()
            .meta_dir()
            .join("fabric")
            .join("profiles")
            .join(format!("{mc_version}-{loader}.json"));
        if cache.is_file() {
            let bytes = tokio::fs::read(&cache).await?;
            if let Ok(profile) = serde_json::from_slice(&bytes) {
                return Ok(profile);
            }
        }

        let url = format!("{FABRIC_META_URL}/versions/loader/{mc_version}/{loader}/profile/json");
        let mut profile: FabricProfile = self.http().get_json(&url).await?;
        for lib in &mut profile.libraries {
            if lib.sha1.is_some() {
                continue;
            }
            let jar_url = lib
                .download_url()
                .ok_or_else(|| Error::BadMavenCoordinate(lib.name.clone()))?;
            let body = self.http().get_bytes(&format!("{jar_url}.sha1")).await?;
            // Some repos append the file name after the hash; keep the hash.
            let hash = String::from_utf8_lossy(&body)
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error::BadChecksum(format!("{jar_url}.sha1")));
            }
            lib.sha1 = Some(hash);
        }

        if let Some(parent) = cache.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let bytes = serde_json::to_vec_pretty(&profile).map_err(|source| Error::Parse {
            what: "fabric profile",
            url: url.clone(),
            source,
        })?;
        tokio::fs::write(&cache, bytes).await?;
        Ok(profile)
    }
}

/// Overlay `fabric` onto `vanilla`: Fabric's main class and id win, its
/// libraries replace any vanilla library with the same `group:artifact`
/// (e.g. ASM — two versions of one library on the classpath is exactly the
/// kind of conflict Fabric's own launcher avoids the same way), and its
/// arguments are appended after vanilla's.
pub fn merge_fabric(vanilla: &VersionProfile, fabric: &FabricProfile) -> Result<VersionProfile> {
    let mut merged = vanilla.clone();
    merged.id = fabric.id.clone();
    merged.main_class = fabric.main_class.clone();

    let mut fabric_libs = Vec::with_capacity(fabric.libraries.len());
    for lib in &fabric.libraries {
        let (Some(url), Some(path), Some(sha1)) = (lib.download_url(), lib.maven_path(), &lib.sha1)
        else {
            return Err(Error::BadMavenCoordinate(lib.name.clone()));
        };
        fabric_libs.push(Library {
            name: lib.name.clone(),
            downloads: Some(LibraryDownloads {
                artifact: Some(DownloadArtifact {
                    sha1: sha1.clone(),
                    // 0 = unknown; download specs treat it as "no size check".
                    size: lib.size.unwrap_or(0),
                    url,
                    path: Some(path),
                }),
                classifiers: Default::default(),
            }),
            natives: None,
            extract: None,
            rules: Vec::new(),
            url: None,
        });
    }

    let overridden: HashSet<&str> = fabric_libs.iter().map(Library::group_artifact).collect();
    merged
        .libraries
        .retain(|lib| !overridden.contains(lib.group_artifact()));
    merged.libraries.extend(fabric_libs);

    let parse = |values: &[serde_json::Value]| -> Result<Vec<Argument>> {
        values
            .iter()
            .map(|v| {
                serde_json::from_value(v.clone()).map_err(|source| Error::Parse {
                    what: "fabric argument",
                    url: fabric.id.clone(),
                    source,
                })
            })
            .collect()
    };
    let extra_game = parse(&fabric.arguments.game)?;
    let extra_jvm = parse(&fabric.arguments.jvm)?;
    let args = merged.arguments.get_or_insert_with(Arguments::default);
    args.game.extend(extra_game);
    args.jvm.extend(extra_jvm);

    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vanilla() -> VersionProfile {
        serde_json::from_str(
            r#"{
                "id": "1.21.1",
                "type": "release",
                "mainClass": "net.minecraft.client.main.Main",
                "arguments": {"game": ["--username", "${auth_player_name}"], "jvm": ["-cp", "${classpath}"]},
                "assetIndex": {"id":"17","sha1":"aaaa","size":1,"url":"http://example/17.json"},
                "assets": "17",
                "downloads": {"client": {"sha1":"bbbb","size":1,"url":"http://example/client.jar"}},
                "libraries": [
                    {"name":"org.ow2.asm:asm:9.6","downloads":{"artifact":{"path":"a","sha1":"1111111111111111111111111111111111111111","size":1,"url":"http://x/a"}}},
                    {"name":"com.mojang:brigadier:1.3.10","downloads":{"artifact":{"path":"b","sha1":"2222222222222222222222222222222222222222","size":1,"url":"http://x/b"}}}
                ]
            }"#,
        )
        .unwrap()
    }

    fn fabric() -> FabricProfile {
        serde_json::from_str(
            r#"{
                "id": "fabric-loader-0.16.9-1.21.1",
                "inheritsFrom": "1.21.1",
                "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
                "arguments": {"game": [], "jvm": ["-DFabricMcEmu= net.minecraft.client.main.Main "]},
                "libraries": [
                    {"name":"org.ow2.asm:asm:9.7.1","url":"https://maven.fabricmc.net/","sha1":"3333333333333333333333333333333333333333","size":10},
                    {"name":"net.fabricmc:fabric-loader:0.16.9","url":"https://maven.fabricmc.net/","sha1":"4444444444444444444444444444444444444444"}
                ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn maven_path_and_url_follow_maven_layout() {
        let lib = &fabric().libraries[1];
        assert_eq!(
            lib.maven_path().unwrap(),
            "net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar"
        );
        assert_eq!(
            lib.download_url().unwrap(),
            "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar"
        );
    }

    #[test]
    fn merge_overrides_main_class_libraries_and_appends_args() {
        let merged = merge_fabric(&vanilla(), &fabric()).unwrap();
        assert_eq!(
            merged.main_class,
            "net.fabricmc.loader.impl.launch.knot.KnotClient"
        );
        assert_eq!(merged.id, "fabric-loader-0.16.9-1.21.1");
        // Vanilla asm 9.6 is replaced by Fabric's 9.7.1, not kept alongside it.
        let names: Vec<&str> = merged.libraries.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "com.mojang:brigadier:1.3.10",
                "org.ow2.asm:asm:9.7.1",
                "net.fabricmc:fabric-loader:0.16.9"
            ]
        );
        let args = merged.arguments.unwrap();
        assert_eq!(args.jvm.len(), 3);
        assert_eq!(args.game.len(), 2);
        // Vanilla-only fields survive untouched.
        assert_eq!(merged.assets, "17");
        assert_eq!(merged.downloads.client.sha1, "bbbb");
    }

    #[test]
    fn merge_rejects_a_library_without_a_hash() {
        let mut profile = fabric();
        profile.libraries[1].sha1 = None;
        assert!(matches!(
            merge_fabric(&vanilla(), &profile),
            Err(Error::BadMavenCoordinate(_))
        ));
    }
}
