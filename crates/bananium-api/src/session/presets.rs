//! The `Preset*` commands over `bananium_instance::PresetStore`.

use std::path::Path;

use bananium_instance::{ContentKind, Loader, Preset, PresetStore};

use super::content::Pin;
use super::Session;
use crate::error::{Error, Result};
use crate::output::{CommandOutput, SkippedEntry};

impl Session {
    fn presets(&self) -> PresetStore {
        PresetStore::new(self.paths.clone())
    }

    /// `Command::PresetList`.
    pub(super) fn preset_list(&self) -> Result<CommandOutput> {
        Ok(CommandOutput::PresetListed {
            presets: self.presets().list()?,
        })
    }

    /// `Command::PresetSave`: capture an instance's chosen Modrinth content.
    pub(super) fn preset_save(
        &self,
        instance: &str,
        name: &str,
        kinds: &[ContentKind],
    ) -> Result<CommandOutput> {
        let cfg = self.instances().load(instance)?;
        let content = self.content().sync(instance)?;
        let kinds = if kinds.is_empty() {
            &ContentKind::ALL[..]
        } else {
            kinds
        };
        let (preset, skipped_local) =
            Preset::capture(name, &cfg.mc_version, cfg.loader, &content, kinds);
        self.presets().save(&preset)?;
        Ok(CommandOutput::PresetSaved {
            preset,
            skipped_local: skipped_local as u32,
        })
    }

    /// `Command::PresetApply` as one task. Unlike a plain install, one
    /// project without a usable version doesn't fail the whole preset: it's
    /// reported in `skipped` and everything else still installs.
    pub(super) async fn preset_apply(&self, name: &str, instance: &str) -> Result<CommandOutput> {
        let preset = self.presets().get(name)?;
        let target = self.target(instance)?;
        let installed = self.content().installed_projects(instance)?;

        let mut roots = Vec::new();
        let mut skipped = Vec::new();
        for entry in preset.entries {
            let skip = |reason: String| SkippedEntry {
                title: entry.title.clone(),
                reason,
            };
            if installed.contains(&entry.project_id) {
                skipped.push(skip("already installed".into()));
                continue;
            }
            if entry.kind != ContentKind::ResourcePack && target.loader == Loader::Vanilla {
                skipped.push(skip("needs a Fabric instance".into()));
                continue;
            }
            let pin = entry.version_id.clone().map_or(Pin::Newest, Pin::Prefer);
            match self
                .choose_version(&entry.project_id, &pin, entry.kind, &target)
                .await
            {
                Ok(v) => roots.push((entry.project_id, Some(v.id), entry.kind)),
                Err(Error::NoCompatibleVersion { mc_version, .. }) => {
                    skipped.push(skip(format!("no version for Minecraft {mc_version}")));
                }
                Err(other) => return Err(other),
            }
        }

        let applied = if roots.is_empty() {
            Vec::new()
        } else {
            let task_id = self.new_task_id("preset");
            self.tracked(&task_id, self.content_install(&task_id, instance, roots))
                .await?
        };
        Ok(CommandOutput::PresetApplied {
            instance: instance.to_string(),
            applied,
            skipped,
        })
    }

    /// `Command::PresetDelete`.
    pub(super) fn preset_delete(&self, name: &str) -> Result<CommandOutput> {
        self.presets().delete(name)?;
        Ok(CommandOutput::PresetDeleted {
            name: name.to_string(),
        })
    }

    /// `Command::PresetRename`.
    pub(super) fn preset_rename(&self, name: &str, new_name: &str) -> Result<CommandOutput> {
        self.presets().rename(name, new_name)?;
        Ok(CommandOutput::PresetRenamed {
            name: new_name.to_string(),
        })
    }

    /// `Command::PresetExport`.
    pub(super) fn preset_export(&self, name: &str, path: &Path) -> Result<CommandOutput> {
        self.presets().export(name, path)?;
        Ok(CommandOutput::PresetExported {
            path: path.to_path_buf(),
        })
    }

    /// `Command::PresetImport`.
    pub(super) fn preset_import(&self, path: &Path) -> Result<CommandOutput> {
        Ok(CommandOutput::PresetImported {
            preset: self.presets().import(path)?,
        })
    }
}
