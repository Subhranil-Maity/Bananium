//! Modrinth modpacks (`.mrpack`): browse them, read one's manifest, and
//! turn one into a new instance.
//!
//! An `.mrpack` is a zip holding `modrinth.index.json` — the Minecraft and
//! loader versions plus a list of files to download, each with hashes — and
//! `overrides/` / `client-overrides/` trees copied over the game directory
//! verbatim (configs, resource packs, options). Installing one is an
//! ordinary `install` of the pinned Minecraft + Fabric versions, then every
//! listed file through the content-addressed store (verified by SHA-1, and
//! shared with other instances like any other download), then the overrides.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bananium_instance::{Error as InstanceError, InstanceStore};
use bananium_modrinth::{Facet, FacetsBuilder, VersionsFilter};
use bananium_net::{DownloadSpec, Downloader};
use bananium_store::BlobStore;
use serde::Deserialize;

use super::files::join_relative;
use super::Session;
use crate::command::{ModpackSource, SearchSort};
use crate::error::{Error, Result};
use crate::output::{CommandOutput, ModpackSummary, ModrinthVersion};

const INDEX_FILE: &str = "modrinth.index.json";
/// Loaders a pack may list that Bananium can't run.
const UNSUPPORTED_LOADERS: &[(&str, &str)] = &[
    ("forge", "Forge"),
    ("neoforge", "NeoForge"),
    ("quilt-loader", "Quilt"),
];

/// `modrinth.index.json` (format version 1).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackIndex {
    format_version: u32,
    game: String,
    version_id: String,
    name: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    files: Vec<PackFile>,
    dependencies: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackFile {
    path: String,
    hashes: HashMap<String, String>,
    #[serde(default)]
    env: Option<PackEnv>,
    downloads: Vec<String>,
    #[serde(default)]
    file_size: u64,
}

#[derive(Debug, Deserialize)]
struct PackEnv {
    #[serde(default)]
    client: String,
}

impl PackFile {
    /// Server-only files (dedicated-server tweaks) are skipped client-side.
    fn wanted_on_client(&self) -> bool {
        self.env.as_ref().is_none_or(|e| e.client != "unsupported")
    }
}

impl PackIndex {
    /// What the pack needs, or why Bananium can't run it.
    fn summary(&self) -> ModpackSummary {
        let mc_version = self
            .dependencies
            .get("minecraft")
            .cloned()
            .unwrap_or_default();
        let fabric = self.dependencies.get("fabric-loader").cloned();
        let unsupported = if self.game != "minecraft" || self.format_version != 1 {
            Some("This isn't a Minecraft modpack Bananium understands.".to_string())
        } else if mc_version.is_empty() {
            Some("The pack doesn't say which Minecraft version it needs.".to_string())
        } else {
            UNSUPPORTED_LOADERS
                .iter()
                .find(|(key, _)| self.dependencies.contains_key(*key))
                .map(|(_, name)| format!("This pack needs {name}; Bananium only supports Fabric."))
        };
        ModpackSummary {
            name: self.name.clone(),
            version_id: self.version_id.clone(),
            summary: self.summary.clone(),
            mc_version,
            loader: if fabric.is_some() {
                "fabric"
            } else {
                "vanilla"
            }
            .to_string(),
            loader_version: fabric,
            file_count: self.files.iter().filter(|f| f.wanted_on_client()).count() as u32,
            unsupported,
        }
    }
}

fn read_index(pack: &Path) -> Result<PackIndex> {
    let file = std::fs::File::open(pack)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| Error::BadModpack(e.to_string()))?;
    let mut entry = zip
        .by_name(INDEX_FILE)
        .map_err(|_| Error::BadModpack(format!("no {INDEX_FILE} inside")))?;
    let mut text = String::new();
    entry.read_to_string(&mut text)?;
    serde_json::from_str(&text).map_err(|e| Error::BadModpack(e.to_string()))
}

/// Copy `overrides/` and then `client-overrides/` from the pack into
/// `game_dir` (client overrides win). Entry names that would escape the
/// game directory are skipped.
fn extract_overrides(pack: &Path, game_dir: &Path) -> Result<usize> {
    let file = std::fs::File::open(pack)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| Error::BadModpack(e.to_string()))?;
    let mut written = 0;
    for prefix in ["overrides/", "client-overrides/"] {
        for i in 0..zip.len() {
            let mut entry = zip
                .by_index(i)
                .map_err(|e| Error::BadModpack(e.to_string()))?;
            let name = entry.name().replace('\\', "/");
            let Some(rel) = name.strip_prefix(prefix) else {
                continue;
            };
            let Some(dest) = join_relative(game_dir, rel) else {
                continue;
            };
            if entry.is_dir() {
                std::fs::create_dir_all(&dest)?;
                continue;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&dest)?;
            std::io::copy(&mut entry, &mut out)?;
            written += 1;
        }
    }
    Ok(written)
}

/// A valid instance name from a pack's display name: disallowed characters
/// become `-`, and a numeric suffix avoids names already taken.
fn instance_name_for(pack_name: &str, store: &InstanceStore) -> Result<String> {
    let base: String = pack_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = if base.is_empty() {
        "modpack".to_string()
    } else {
        base
    };
    let taken: HashSet<String> = store.list()?.into_iter().collect();
    if !taken.contains(&InstanceStore::slugify(&base)) {
        return Ok(base);
    }
    Ok((2..)
        .map(|n| format!("{base}-{n}"))
        .find(|n| !taken.contains(&InstanceStore::slugify(n)))
        .expect("an unused suffix exists"))
}

impl Session {
    /// `Command::ModpackSearch`: Modrinth modpacks Bananium can run (Fabric).
    pub(super) async fn modpack_search(
        &self,
        query: &str,
        categories: &[String],
        sort: SearchSort,
        offset: u32,
        limit: u32,
    ) -> Result<CommandOutput> {
        let mut facets = FacetsBuilder::new()
            .and(Facet::project_type("modpack"))
            .and(Facet::loader("fabric"));
        for c in categories {
            facets = facets.and(Facet::category(c));
        }
        self.run_search(facets, query, sort, offset, limit, &HashSet::new())
            .await
    }

    /// `Command::ModpackVersions`: every version of a modpack project;
    /// `compatible` marks those Bananium can install (Fabric or vanilla).
    pub(super) async fn modpack_versions(&self, project: &str) -> Result<CommandOutput> {
        let versions = self
            .modrinth
            .project_versions(project, &VersionsFilter::default())
            .await?
            .into_iter()
            .map(|v| ModrinthVersion {
                compatible: v.loaders.iter().all(|l| l == "fabric" || l == "minecraft"),
                id: v.id,
                name: v.name,
                version_number: v.version_number,
                version_type: v.version_type,
                date_published: v.date_published,
                downloads: v.downloads,
                game_versions: v.game_versions,
                loaders: v.loaders,
            })
            .collect();
        Ok(CommandOutput::ModrinthVersionsListed { versions })
    }

    /// `Command::ModpackInspect`: read a local `.mrpack`'s manifest.
    pub(super) fn modpack_inspect(&self, path: &Path) -> Result<CommandOutput> {
        Ok(CommandOutput::ModpackInspected {
            pack: read_index(path)?.summary(),
        })
    }

    /// `Command::ModpackInstall`, reported as one task.
    pub(super) async fn modpack_install_tracked(
        &self,
        source: &ModpackSource,
        name: Option<&str>,
        group: Option<&str>,
    ) -> Result<CommandOutput> {
        let task_id = self.new_task_id("modpack");
        self.tracked(
            &task_id,
            self.modpack_install(&task_id, source, name, group),
        )
        .await
    }

    /// Download (for a Modrinth source) and install a modpack as a new
    /// instance. The name must be unused: unlike `install`, an existing
    /// instance is never reused, since a pack overwrites its files.
    async fn modpack_install(
        &self,
        task_id: &str,
        source: &ModpackSource,
        name: Option<&str>,
        group: Option<&str>,
    ) -> Result<CommandOutput> {
        let store = self.instances();
        if let Some(name) = name {
            if store.resolve(Some(&InstanceStore::slugify(name))).is_ok() {
                return Err(InstanceError::AlreadyExists(name.to_string()).into());
            }
        }

        let (pack_path, icon_url) = match source {
            ModpackSource::File { path } => (path.clone(), None),
            ModpackSource::Modrinth { project, version } => {
                self.fetch_modrinth_pack(task_id, project, version.as_deref())
                    .await?
            }
        };
        let index = read_index(&pack_path)?;
        let summary = index.summary();
        if let Some(reason) = summary.unsupported {
            return Err(Error::UnsupportedModpack(reason));
        }

        let name = match name {
            Some(n) => n.to_string(),
            None => instance_name_for(&index.name, &store)?,
        };
        let out = self
            .install(
                task_id,
                &summary.mc_version,
                Some(&name),
                summary.loader_version.as_deref(),
            )
            .await?;
        let CommandOutput::Installed { instance: slug, .. } = &out else {
            unreachable!("install always reports Installed");
        };
        let game_dir = self.paths.instance_minecraft_dir(slug);

        // Pack files go through the store like any other download.
        let mut specs = Vec::new();
        let mut placements = Vec::new();
        for f in index.files.iter().filter(|f| f.wanted_on_client()) {
            let dest = join_relative(&game_dir, &f.path)
                .ok_or_else(|| Error::BadModpack(format!("unsafe file path {:?}", f.path)))?;
            let sha1 = f
                .hashes
                .get("sha1")
                .ok_or_else(|| Error::BadModpack(format!("{} has no sha1", f.path)))?
                .to_lowercase();
            let url = f
                .downloads
                .first()
                .ok_or_else(|| Error::BadModpack(format!("{} has no download", f.path)))?;
            specs.push(DownloadSpec {
                url: url.clone(),
                dest: self.paths.store_blob(&sha1),
                expected_sha1: Some(sha1.clone()),
                expected_size: (f.file_size > 0).then_some(f.file_size),
                task_id: format!("{task_id}/file:{sha1}"),
                label: f.path.clone(),
            });
            placements.push((sha1, dest));
        }
        self.download_tracked(task_id, &format!("{} files", index.name), specs)
            .await?;
        let blobs = BlobStore::new(self.paths.clone());
        let mut last = None;
        let total = placements.len();
        for (i, (sha1, dest)) in placements.iter().enumerate() {
            self.phase_progress(task_id, "Placing modpack files", i + 1, total, &mut last);
            blobs.materialize(sha1, dest)?;
        }
        // The remaining steps each announce themselves, so a frontend never
        // sits on a stale label from an earlier phase while they run.
        let finishing = |step: &str, done: usize| {
            self.phase_progress(task_id, step, done, 3, &mut None);
        };
        finishing("Applying pack configs", 0);
        extract_overrides(&pack_path, &game_dir)?;

        if let Some(group) = group.filter(|g| !g.trim().is_empty()) {
            let mut cfg = store.load(slug)?;
            cfg.group = Some(group.trim().to_string());
            store.save(slug, &cfg)?;
        }
        if let Some(url) = icon_url {
            finishing("Setting the pack icon", 1);
            // Cosmetic: a failed icon download doesn't fail the install.
            if let Err(err) = self.fetch_icon(slug, &url).await {
                tracing::warn!("couldn't set modpack icon: {err}");
            }
        }
        // Record Modrinth metadata for the pack's mods/packs so they show
        // up properly and can be updated; offline this just finds nothing.
        finishing("Identifying mods on Modrinth", 2);
        if let Err(err) = self.identify(slug).await {
            tracing::warn!("couldn't identify modpack content: {err}");
        }
        finishing("Done", 3);
        Ok(out)
    }

    /// Download a Modrinth modpack version's `.mrpack` into the store,
    /// returning its path and the project's icon URL.
    async fn fetch_modrinth_pack(
        &self,
        task_id: &str,
        project: &str,
        version_id: Option<&str>,
    ) -> Result<(PathBuf, Option<String>)> {
        let info = self.modrinth.project(project).await?;
        let mut versions = self
            .modrinth
            .project_versions(project, &VersionsFilter::default())
            .await?
            .into_iter();
        // Modrinth lists versions newest first.
        let version = match version_id {
            Some(id) => versions
                .find(|v| v.id == id)
                .ok_or_else(|| Error::VersionNotFound(id.to_string()))?,
            None => versions
                .find(|v| v.version_type == "release" && v.loaders.iter().any(|l| l == "fabric"))
                .ok_or_else(|| Error::OnlyPrereleases {
                    project: info.title.clone(),
                    mc_version: "any Minecraft version".to_string(),
                })?,
        };
        let version_id = version.id.as_str();
        let file = version
            .files
            .iter()
            .find(|f| f.primary && f.filename.ends_with(".mrpack"))
            .or_else(|| {
                version
                    .files
                    .iter()
                    .find(|f| f.filename.ends_with(".mrpack"))
            })
            .ok_or_else(|| Error::NoFile(version_id.to_string()))?;
        let sha1 = file
            .hashes
            .sha1
            .clone()
            .ok_or_else(|| Error::NoFile(version_id.to_string()))?;
        let dest = self.paths.store_blob(&sha1);
        self.download_tracked(
            task_id,
            &format!("{} {}", info.title, version.version_number),
            vec![DownloadSpec {
                url: file.url.clone(),
                dest: dest.clone(),
                expected_sha1: Some(sha1.clone()),
                expected_size: Some(file.size),
                task_id: format!("{task_id}/pack"),
                label: file.filename.clone(),
            }],
        )
        .await?;
        Ok((dest, info.icon_url))
    }

    /// Download `url` and make it `slug`'s icon.
    async fn fetch_icon(&self, slug: &str, url: &str) -> Result<()> {
        let ext = url
            .rsplit('.')
            .next()
            .filter(|e| {
                ["png", "jpg", "jpeg", "gif", "webp"].contains(&e.to_ascii_lowercase().as_str())
            })
            .unwrap_or("png");
        let tmp = self
            .paths
            .instance_dir(slug)
            .join(format!("pack-icon.{ext}"));
        let spec = DownloadSpec {
            url: url.to_string(),
            dest: tmp.clone(),
            expected_sha1: None,
            expected_size: None,
            task_id: "modpack-icon".to_string(),
            label: "icon".to_string(),
        };
        // A few KB with no published size: downloaded without progress
        // events (the caller's "Setting the pack icon" step covers it), so
        // it never shows up as a sizeless 0% bar.
        let downloader = Downloader::new(self.http.inner().clone(), 1);
        let result = downloader
            .download(&spec, Arc::new(|_: bananium_net::Progress| {}))
            .await
            .map_err(Error::from)
            .and_then(|()| Ok(self.instances().set_icon(slug, Some(&tmp))?));
        let _ = std::fs::remove_file(&tmp);
        result
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use bananium_core::Paths;

    use super::*;

    fn write_pack(dir: &Path, index: &str, extra: &[(&str, &str)]) -> PathBuf {
        let path = dir.join("pack.mrpack");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file(INDEX_FILE, opts).unwrap();
        zip.write_all(index.as_bytes()).unwrap();
        for (name, body) in extra {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    const FABRIC_INDEX: &str = r#"{
        "formatVersion": 1, "game": "minecraft", "versionId": "1.2.0",
        "name": "Cool Pack!", "summary": "fast",
        "files": [
            {"path": "mods/a.jar", "hashes": {"sha1": "AB"}, "downloads": ["https://x/a.jar"], "fileSize": 3},
            {"path": "mods/server.jar", "hashes": {"sha1": "cd"}, "env": {"client": "unsupported", "server": "required"}, "downloads": ["https://x/s.jar"], "fileSize": 3}
        ],
        "dependencies": {"minecraft": "1.21.1", "fabric-loader": "0.16.9"}
    }"#;

    #[test]
    fn summary_reads_versions_and_skips_server_only_files() {
        let dir = tempfile::tempdir().unwrap();
        let pack = write_pack(dir.path(), FABRIC_INDEX, &[]);
        let s = read_index(&pack).unwrap().summary();
        assert_eq!(s.name, "Cool Pack!");
        assert_eq!(s.mc_version, "1.21.1");
        assert_eq!(s.loader, "fabric");
        assert_eq!(s.loader_version.as_deref(), Some("0.16.9"));
        assert_eq!(s.file_count, 1);
        assert!(s.unsupported.is_none());
    }

    #[test]
    fn forge_packs_are_reported_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let index = FABRIC_INDEX.replace("\"fabric-loader\": \"0.16.9\"", "\"forge\": \"47.2.0\"");
        let s = read_index(&write_pack(dir.path(), &index, &[]))
            .unwrap()
            .summary();
        assert!(s.unsupported.unwrap().contains("Forge"));
    }

    #[test]
    fn overrides_are_extracted_with_client_overrides_winning_and_escapes_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let pack = write_pack(
            dir.path(),
            FABRIC_INDEX,
            &[
                ("overrides/config/a.txt", "base"),
                ("overrides/options.txt", "base"),
                ("client-overrides/options.txt", "client"),
                ("overrides/../evil.txt", "nope"),
            ],
        );
        let game = dir.path().join("game");
        extract_overrides(&pack, &game).unwrap();
        assert_eq!(
            std::fs::read_to_string(game.join("config/a.txt")).unwrap(),
            "base"
        );
        assert_eq!(
            std::fs::read_to_string(game.join("options.txt")).unwrap(),
            "client"
        );
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn instance_names_are_sanitised_and_made_unique() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = InstanceStore::new(paths.clone());
        assert_eq!(
            instance_name_for("Cool Pack!", &store).unwrap(),
            "Cool-Pack"
        );
        store.create_named("1.21.1", Some("Cool-Pack")).unwrap();
        assert_eq!(
            instance_name_for("Cool Pack!", &store).unwrap(),
            "Cool-Pack-2"
        );
    }
}
