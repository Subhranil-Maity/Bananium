//! User-saved presets: a named list of Modrinth projects (mods, resource
//! packs, shaders) captured from one instance and re-applied to others.
//!
//! A preset stores *projects*, with the version they were saved at as a
//! hint — not files. Applying one to an instance on another Minecraft
//! version picks each project's version for that instance instead, which
//! is the whole point of a preset over copying a `mods/` folder.

use std::path::Path;

use bananium_core::Paths;
use serde::{Deserialize, Serialize};

use crate::content::{ContentEntry, ContentKind};
use crate::error::{Error, Result};
use crate::Loader;

/// One project in a preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PresetEntry {
    pub kind: ContentKind,
    pub project_id: String,
    /// The version installed when the preset was saved; reused when the
    /// target instance can run it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

/// A saved preset (also its exact on-disk TOML shape).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Preset {
    pub name: String,
    /// Minecraft version of the instance it was saved from (informational).
    pub mc_version: String,
    #[cfg_attr(feature = "ts", ts(type = "\"vanilla\" | \"fabric\""))]
    pub loader: Loader,
    #[serde(default)]
    pub entries: Vec<PresetEntry>,
}

impl Preset {
    /// Build a preset from an instance's content: every Modrinth project
    /// of the given kinds that was chosen directly. Dependencies are left
    /// out (they're re-resolved for the target on apply) and so are local
    /// files (there's nothing to download them from). Returns the preset
    /// and how many local files were left out.
    pub fn capture(
        name: &str,
        mc_version: &str,
        loader: Loader,
        content: &[ContentEntry],
        kinds: &[ContentKind],
    ) -> (Preset, usize) {
        let mut local = 0;
        let mut entries = Vec::new();
        for e in content
            .iter()
            .filter(|e| kinds.contains(&e.kind) && !e.dependency)
        {
            match &e.project_id {
                Some(project_id) => entries.push(PresetEntry {
                    kind: e.kind,
                    project_id: project_id.clone(),
                    version_id: e.version_id.clone(),
                    title: e.title.clone(),
                    icon_url: e.icon_url.clone(),
                }),
                None => local += 1,
            }
        }
        let preset = Preset {
            name: name.to_string(),
            mc_version: mc_version.to_string(),
            loader,
            entries,
        };
        (preset, local)
    }
}

/// Whether `name` works as a preset name: 1–64 characters of anything but
/// path separators and control characters (spaces are fine — the file name
/// is derived from it, see [`PresetStore`]).
pub fn is_valid_preset_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed.len() == name.len()
        && name.chars().count() <= 64
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
}

/// Presets live at `Paths::presets_dir()/<file stem>.toml`, where the stem
/// is the name with anything outside `[A-Za-z0-9._-]` replaced by `_`. The
/// real name is stored inside the file, so display names keep their spaces.
pub struct PresetStore {
    paths: Paths,
}

impl PresetStore {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    fn file(&self, name: &str) -> std::path::PathBuf {
        let stem: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.paths
            .presets_dir()
            .join(format!("{}.toml", stem.to_lowercase()))
    }

    /// Every preset, by name.
    pub fn list(&self) -> Result<Vec<Preset>> {
        let mut presets = Vec::new();
        let Ok(dir) = std::fs::read_dir(self.paths.presets_dir()) else {
            return Ok(presets);
        };
        for item in dir {
            let path = item?.path();
            if path.extension().is_some_and(|e| e == "toml") {
                // A hand-mangled file shouldn't hide every other preset.
                if let Ok(preset) = read(&path) {
                    presets.push(preset);
                }
            }
        }
        presets.sort_by_key(|p| p.name.to_lowercase());
        Ok(presets)
    }

    pub fn get(&self, name: &str) -> Result<Preset> {
        let path = self.file(name);
        if !path.is_file() {
            return Err(Error::PresetNotFound(name.to_string()));
        }
        read(&path)
    }

    /// Save `preset`, replacing any preset with the same name.
    pub fn save(&self, preset: &Preset) -> Result<()> {
        if !is_valid_preset_name(&preset.name) {
            return Err(Error::InvalidPresetName(preset.name.clone()));
        }
        std::fs::create_dir_all(self.paths.presets_dir())?;
        std::fs::write(self.file(&preset.name), toml::to_string_pretty(preset)?)?;
        Ok(())
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        let path = self.file(name);
        if !path.is_file() {
            return Err(Error::PresetNotFound(name.to_string()));
        }
        std::fs::remove_file(path)?;
        Ok(())
    }

    /// Rename, refusing to overwrite a different existing preset.
    pub fn rename(&self, name: &str, new_name: &str) -> Result<()> {
        let mut preset = self.get(name)?;
        if !is_valid_preset_name(new_name) {
            return Err(Error::InvalidPresetName(new_name.to_string()));
        }
        let (old_file, new_file) = (self.file(name), self.file(new_name));
        if new_file != old_file && new_file.exists() {
            return Err(Error::PresetExists(new_name.to_string()));
        }
        preset.name = new_name.to_string();
        std::fs::remove_file(old_file)?;
        self.save(&preset)
    }

    /// Write a preset to an arbitrary path, for sharing.
    pub fn export(&self, name: &str, to: &Path) -> Result<()> {
        std::fs::write(to, toml::to_string_pretty(&self.get(name)?)?)?;
        Ok(())
    }

    /// Read a shared preset file and save it (replacing a same-named one).
    pub fn import(&self, from: &Path) -> Result<Preset> {
        let preset = read(from)?;
        self.save(&preset)?;
        Ok(preset)
    }
}

fn read(path: &Path) -> Result<Preset> {
    Ok(toml::from_str(&std::fs::read_to_string(path)?)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: ContentKind, project: Option<&str>, dependency: bool) -> ContentEntry {
        ContentEntry {
            kind,
            filename: format!("{project:?}.jar"),
            enabled: true,
            title: project.unwrap_or("local").into(),
            project_id: project.map(Into::into),
            version_id: project.map(|p| format!("{p}-v1")),
            version_number: None,
            icon_url: None,
            sha1: None,
            dependency,
        }
    }

    #[test]
    fn capture_keeps_chosen_modrinth_projects_of_selected_kinds() {
        let content = [
            entry(ContentKind::Mod, Some("sodium"), false),
            entry(ContentKind::Mod, Some("fabric-api"), true),
            entry(ContentKind::Mod, None, false),
            entry(ContentKind::Shader, Some("bsl"), false),
            entry(ContentKind::ResourcePack, Some("faithful"), false),
        ];
        let (preset, local) = Preset::capture(
            "Perf",
            "1.21.1",
            Loader::Fabric,
            &content,
            &[ContentKind::Mod, ContentKind::Shader],
        );
        let ids: Vec<&str> = preset
            .entries
            .iter()
            .map(|e| e.project_id.as_str())
            .collect();
        assert_eq!(ids, ["sodium", "bsl"]);
        assert_eq!(local, 1);
        assert_eq!(preset.entries[0].version_id.as_deref(), Some("sodium-v1"));
    }

    #[test]
    fn save_list_rename_delete_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = PresetStore::new(Paths::at(dir.path()));
        let (preset, _) = Preset::capture(
            "My Perf Pack",
            "1.21.1",
            Loader::Fabric,
            &[entry(ContentKind::Mod, Some("sodium"), false)],
            &ContentKind::ALL,
        );
        store.save(&preset).unwrap();
        assert_eq!(store.list().unwrap(), std::slice::from_ref(&preset));
        assert_eq!(store.get("My Perf Pack").unwrap(), preset);

        store.rename("My Perf Pack", "Speedy").unwrap();
        assert!(matches!(
            store.get("My Perf Pack"),
            Err(Error::PresetNotFound(_))
        ));
        assert_eq!(store.get("Speedy").unwrap().entries, preset.entries);

        let shared = dir.path().join("shared.toml");
        store.export("Speedy", &shared).unwrap();
        store.delete("Speedy").unwrap();
        assert!(store.list().unwrap().is_empty());
        assert_eq!(store.import(&shared).unwrap().name, "Speedy");
    }

    #[test]
    fn preset_names_reject_separators_and_blank() {
        assert!(is_valid_preset_name("Performance Pack"));
        assert!(!is_valid_preset_name(""));
        assert!(!is_valid_preset_name("  padded "));
        assert!(!is_valid_preset_name("a/b"));
    }
}
