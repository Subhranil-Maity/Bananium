//! Per-instance content — mods, resource packs, shader packs — and the
//! `bananium.lock.toml` that records where each file came from.
//!
//! The files in the game directory are the source of truth for *what is
//! installed*: a jar dropped into `mods/` by hand is installed, and one
//! deleted by hand is gone. The lockfile only adds metadata the files
//! can't carry (which Modrinth project/version a file is, its title and
//! icon). [`ContentStore::sync`] reconciles the two on every listing.
//! Disabling a file renames it to `<name>.disabled`, which is also how
//! other launchers do it, so the convention survives moving an instance.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use bananium_core::Paths;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::error::{Error, Result};

const DISABLED_SUFFIX: &str = ".disabled";

/// What kind of content a file is, which decides the folder it lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Mod,
    ResourcePack,
    Shader,
}

impl ContentKind {
    pub const ALL: [ContentKind; 3] = [
        ContentKind::Mod,
        ContentKind::ResourcePack,
        ContentKind::Shader,
    ];

    /// Folder under the game directory the game loads this kind from.
    pub fn dir_name(self) -> &'static str {
        match self {
            ContentKind::Mod => "mods",
            ContentKind::ResourcePack => "resourcepacks",
            ContentKind::Shader => "shaderpacks",
        }
    }

    /// Modrinth's `project_type` for this kind.
    pub fn modrinth_type(self) -> &'static str {
        match self {
            ContentKind::Mod => "mod",
            ContentKind::ResourcePack => "resourcepack",
            ContentKind::Shader => "shader",
        }
    }

    /// Whether a file name in this kind's folder is content at all (as
    /// opposed to e.g. a mod's stray config file). Resource and shader
    /// packs may also be plain directories, handled by the caller.
    fn accepts(self, file_name: &str) -> bool {
        let name = file_name.strip_suffix(DISABLED_SUFFIX).unwrap_or(file_name);
        match self {
            ContentKind::Mod => name.ends_with(".jar"),
            ContentKind::ResourcePack | ContentKind::Shader => name.ends_with(".zip"),
        }
    }
}

/// One installed piece of content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ContentEntry {
    pub kind: ContentKind,
    /// File (or, for an unzipped pack, directory) name without any
    /// `.disabled` suffix — the stable key within `kind`'s folder.
    pub filename: String,
    pub enabled: bool,
    /// Display name: the Modrinth project title, or the file name for
    /// untracked files.
    pub title: String,
    /// Modrinth project id; `None` for a file not (yet) identified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    /// Human-readable version, e.g. `"mc1.21.1-0.6.0"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    /// `None` for a directory pack, which has no single hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    /// Installed automatically as another project's required dependency
    /// rather than chosen directly.
    #[serde(default)]
    pub dependency: bool,
}

/// On-disk shape of `bananium.lock.toml`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct LockFile {
    #[serde(default, rename = "content")]
    entries: Vec<ContentEntry>,
}

/// Reads and writes an instance's content folders and lockfile.
pub struct ContentStore {
    paths: Paths,
}

impl ContentStore {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    /// The folder `kind` lives in for instance `slug`.
    pub fn dir(&self, slug: &str, kind: ContentKind) -> PathBuf {
        self.paths
            .instance_minecraft_dir(slug)
            .join(kind.dir_name())
    }

    /// Where `entry`'s file currently is on disk (with `.disabled` when off).
    pub fn file_path(&self, slug: &str, entry: &ContentEntry) -> PathBuf {
        let dir = self.dir(slug, entry.kind);
        if entry.enabled {
            dir.join(&entry.filename)
        } else {
            dir.join(format!("{}{DISABLED_SUFFIX}", entry.filename))
        }
    }

    fn load(&self, slug: &str) -> Result<LockFile> {
        match std::fs::read_to_string(self.paths.instance_lock(slug)) {
            Ok(text) => Ok(toml::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(LockFile::default()),
            Err(e) => Err(e.into()),
        }
    }

    fn save(&self, slug: &str, lock: &LockFile) -> Result<()> {
        std::fs::write(
            self.paths.instance_lock(slug),
            toml::to_string_pretty(lock)?,
        )?;
        Ok(())
    }

    /// Reconcile the lockfile with the content folders and return every
    /// entry, sorted by kind then title: files that vanished are dropped,
    /// files that appeared are added as untracked local entries (hashed so
    /// they can later be identified on Modrinth), and each entry's
    /// `enabled` follows its file's `.disabled` suffix.
    pub fn sync(&self, slug: &str) -> Result<Vec<ContentEntry>> {
        if !self.paths.instance_toml(slug).is_file() {
            return Err(Error::NotFound(slug.to_string()));
        }
        let old = self.load(slug)?;
        let mut entries = Vec::new();
        for kind in ContentKind::ALL {
            let dir = self.dir(slug, kind);
            let Ok(read) = std::fs::read_dir(&dir) else {
                continue;
            };
            for item in read {
                let item = item?;
                let raw = item.file_name().to_string_lossy().into_owned();
                let is_dir = item.file_type()?.is_dir();
                // Unzipped packs are directories; mods never are.
                if !(kind.accepts(&raw) || (is_dir && kind != ContentKind::Mod)) {
                    continue;
                }
                let enabled = !raw.ends_with(DISABLED_SUFFIX);
                let filename = raw
                    .strip_suffix(DISABLED_SUFFIX)
                    .unwrap_or(&raw)
                    .to_string();
                let known = old
                    .entries
                    .iter()
                    .find(|e| e.kind == kind && e.filename == filename);
                let entry = match known {
                    Some(e) => ContentEntry {
                        enabled,
                        ..e.clone()
                    },
                    None => ContentEntry {
                        kind,
                        title: filename.clone(),
                        sha1: if is_dir {
                            None
                        } else {
                            Some(sha1_file(&item.path())?)
                        },
                        filename,
                        enabled,
                        project_id: None,
                        version_id: None,
                        version_number: None,
                        icon_url: None,
                        dependency: false,
                    },
                };
                entries.push(entry);
            }
        }
        entries.sort_by(|a, b| {
            (a.kind as u8, a.title.to_lowercase()).cmp(&(b.kind as u8, b.title.to_lowercase()))
        });
        let lock = LockFile { entries };
        if lock.entries != old.entries {
            self.save(slug, &lock)?;
        }
        Ok(lock.entries)
    }

    /// Record `entry`, whose file the caller has already placed at
    /// [`ContentStore::file_path`]. A previous entry for the same Modrinth
    /// project (an older version being replaced) is removed along with its
    /// file, so an update never leaves two copies of one mod behind.
    pub fn upsert(&self, slug: &str, entry: ContentEntry) -> Result<()> {
        let mut lock = LockFile {
            entries: self.sync(slug)?,
        };
        let mut replaced = Vec::new();
        lock.entries.retain(|e| {
            let same_file = e.kind == entry.kind && e.filename == entry.filename;
            let same_project = e.project_id.is_some() && e.project_id == entry.project_id;
            if same_project && !same_file {
                replaced.push(e.clone());
            }
            !(same_file || same_project)
        });
        for old in replaced {
            remove_path(&self.file_path(slug, &old))?;
        }
        lock.entries.push(entry);
        self.save(slug, &lock)
    }

    /// Update the metadata of the existing entry for `kind`/`filename` in
    /// place (used when an untracked file is identified on Modrinth).
    pub fn update_metadata(
        &self,
        slug: &str,
        kind: ContentKind,
        filename: &str,
        apply: impl FnOnce(&mut ContentEntry),
    ) -> Result<()> {
        let mut entries = self.sync(slug)?;
        let entry = entries
            .iter_mut()
            .find(|e| e.kind == kind && e.filename == filename)
            .ok_or_else(|| Error::ContentNotFound(filename.to_string()))?;
        apply(entry);
        self.save(slug, &LockFile { entries })
    }

    /// Delete a piece of content (file and entry).
    pub fn remove(&self, slug: &str, kind: ContentKind, filename: &str) -> Result<()> {
        let mut entries = self.sync(slug)?;
        let idx = entries
            .iter()
            .position(|e| e.kind == kind && e.filename == filename)
            .ok_or_else(|| Error::ContentNotFound(filename.to_string()))?;
        let entry = entries.remove(idx);
        remove_path(&self.file_path(slug, &entry))?;
        self.save(slug, &LockFile { entries })
    }

    /// Enable or disable a piece of content by renaming its file.
    pub fn set_enabled(
        &self,
        slug: &str,
        kind: ContentKind,
        filename: &str,
        enabled: bool,
    ) -> Result<()> {
        let mut entries = self.sync(slug)?;
        let entry = entries
            .iter_mut()
            .find(|e| e.kind == kind && e.filename == filename)
            .ok_or_else(|| Error::ContentNotFound(filename.to_string()))?;
        if entry.enabled != enabled {
            let from = self.file_path(slug, entry);
            entry.enabled = enabled;
            std::fs::rename(from, self.file_path(slug, entry))?;
        }
        self.save(slug, &LockFile { entries })
    }

    /// Modrinth project ids already installed in `slug`.
    pub fn installed_projects(&self, slug: &str) -> Result<HashSet<String>> {
        Ok(self
            .sync(slug)?
            .into_iter()
            .filter_map(|e| e.project_id)
            .collect())
    }
}

fn remove_path(path: &Path) -> Result<()> {
    let result = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match result {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Lowercase hex SHA-1 of a file, streamed rather than read whole.
pub fn sha1_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InstanceStore;

    fn setup() -> (tempfile::TempDir, Paths, ContentStore) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        InstanceStore::new(paths.clone())
            .create_named("1.21.1", Some("main"))
            .unwrap();
        let store = ContentStore::new(paths.clone());
        (dir, paths, store)
    }

    fn put(store: &ContentStore, kind: ContentKind, name: &str, body: &str) {
        let dir = store.dir("main", kind);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn sync_picks_up_untracked_files_and_ignores_non_content() {
        let (_d, _p, store) = setup();
        put(&store, ContentKind::Mod, "sodium.jar", "a");
        put(&store, ContentKind::Mod, "notes.txt", "x");
        put(
            &store,
            ContentKind::ResourcePack,
            "faithful.zip.disabled",
            "b",
        );
        std::fs::create_dir_all(store.dir("main", ContentKind::Shader).join("Unzipped")).unwrap();

        let entries = store.sync("main").unwrap();
        let names: Vec<(&str, bool)> = entries
            .iter()
            .map(|e| (e.filename.as_str(), e.enabled))
            .collect();
        assert_eq!(
            names,
            [
                ("sodium.jar", true),
                ("faithful.zip", false),
                ("Unzipped", true)
            ]
        );
        assert_eq!(
            entries[0].sha1.as_deref(),
            Some("86f7e437faa5a7fce15d1ddcb9eaeaea377667b8")
        );
        assert_eq!(entries[2].sha1, None);
    }

    #[test]
    fn sync_keeps_metadata_and_drops_vanished_files() {
        let (_d, _p, store) = setup();
        put(&store, ContentKind::Mod, "a.jar", "a");
        put(&store, ContentKind::Mod, "b.jar", "b");
        store
            .update_metadata("main", ContentKind::Mod, "a.jar", |e| {
                e.project_id = Some("AANobbMI".into());
                e.title = "Sodium".into();
            })
            .unwrap();
        std::fs::remove_file(store.dir("main", ContentKind::Mod).join("b.jar")).unwrap();

        let entries = store.sync("main").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Sodium");
        assert_eq!(entries[0].project_id.as_deref(), Some("AANobbMI"));
    }

    #[test]
    fn toggle_renames_and_remove_deletes() {
        let (_d, _p, store) = setup();
        put(&store, ContentKind::Mod, "a.jar", "a");
        store
            .set_enabled("main", ContentKind::Mod, "a.jar", false)
            .unwrap();
        let dir = store.dir("main", ContentKind::Mod);
        assert!(dir.join("a.jar.disabled").is_file());
        assert!(!dir.join("a.jar").exists());
        assert!(!store.sync("main").unwrap()[0].enabled);

        store
            .set_enabled("main", ContentKind::Mod, "a.jar", true)
            .unwrap();
        assert!(dir.join("a.jar").is_file());

        store.remove("main", ContentKind::Mod, "a.jar").unwrap();
        assert!(!dir.join("a.jar").exists());
        assert!(store.sync("main").unwrap().is_empty());
    }

    #[test]
    fn upsert_replaces_an_older_version_of_the_same_project() {
        let (_d, _p, store) = setup();
        let entry = |file: &str, version: &str| ContentEntry {
            kind: ContentKind::Mod,
            filename: file.into(),
            enabled: true,
            title: "Sodium".into(),
            project_id: Some("AANobbMI".into()),
            version_id: Some(version.into()),
            version_number: None,
            icon_url: None,
            sha1: None,
            dependency: false,
        };
        put(&store, ContentKind::Mod, "sodium-1.jar", "1");
        store.upsert("main", entry("sodium-1.jar", "v1")).unwrap();
        put(&store, ContentKind::Mod, "sodium-2.jar", "2");
        store.upsert("main", entry("sodium-2.jar", "v2")).unwrap();

        let entries = store.sync("main").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].version_id.as_deref(), Some("v2"));
        assert!(!store
            .dir("main", ContentKind::Mod)
            .join("sodium-1.jar")
            .exists());
    }
}
