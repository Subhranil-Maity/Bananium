//! Instance management: list/set/remove/rename/clone/kill.

use std::path::PathBuf;

use bananium_instance::{InstanceStore, Loader};

use super::Session;
use crate::error::{Error, Result};
use crate::output::{CommandOutput, InstanceSummary};

/// The wire name of a loader, matching its `instance.toml` spelling.
pub(super) fn loader_name(loader: Loader) -> &'static str {
    match loader {
        Loader::Vanilla => "vanilla",
        Loader::Fabric => "fabric",
    }
}

impl Session {
    pub(super) fn instances(&self) -> InstanceStore {
        InstanceStore::new(self.paths.clone())
    }

    /// `Command::InstanceList`: every installed instance plus its current
    /// running status, for a frontend's instance list (the TUI's, for one).
    pub(super) fn instance_list(&self) -> Result<CommandOutput> {
        let instances = self.instances();
        let mut summaries = Vec::new();
        for (slug, cfg) in instances.list_configs()? {
            let running = instances.is_running(&slug)?;
            summaries.push(InstanceSummary {
                game_dir: self.paths.instance_minecraft_dir(&slug),
                slug,
                name: cfg.name,
                mc_version: cfg.mc_version,
                loader: loader_name(cfg.loader).to_string(),
                loader_version: cfg.loader_version,
                ram_mb: cfg.ram_mb,
                jvm_args: cfg.jvm_args,
                java_path: cfg.java_path,
                running,
            });
        }
        Ok(CommandOutput::InstanceListed {
            instances: summaries,
        })
    }

    /// `Command::InstanceSet`: see the field-level doc comments on
    /// `Command::InstanceSet` for the "`None`/absent means unchanged"
    /// convention this follows.
    pub(super) fn instance_set(
        &self,
        instance: &str,
        ram_mb: Option<u32>,
        jvm_args: Option<Vec<String>>,
        java_path: Option<PathBuf>,
    ) -> Result<CommandOutput> {
        let instances = self.instances();
        let mut cfg = instances.load(instance)?;
        if let Some(ram_mb) = ram_mb {
            cfg.ram_mb = if ram_mb == 0 { None } else { Some(ram_mb) };
        }
        if let Some(jvm_args) = jvm_args {
            cfg.jvm_args = jvm_args;
        }
        if let Some(java_path) = java_path {
            cfg.java_path = if java_path.as_os_str().is_empty() {
                None
            } else {
                Some(java_path)
            };
        }
        instances.save(instance, &cfg)?;
        Ok(CommandOutput::InstanceUpdated {
            instance: instance.to_string(),
        })
    }

    /// `Command::InstanceRemove`.
    pub(super) fn instance_remove(&self, instance: &str) -> Result<CommandOutput> {
        self.instances().remove(instance)?;
        Ok(CommandOutput::InstanceRemoved {
            instance: instance.to_string(),
        })
    }

    /// `Command::InstanceRename`.
    pub(super) fn instance_rename(&self, instance: &str, new_name: &str) -> Result<CommandOutput> {
        let slug = self.instances().rename(instance, new_name)?;
        Ok(CommandOutput::InstanceRenamed {
            old: instance.to_string(),
            instance: slug,
        })
    }

    /// `Command::InstanceClone`.
    pub(super) fn instance_clone(&self, instance: &str, new_name: &str) -> Result<CommandOutput> {
        let slug = self.instances().clone_instance(instance, new_name)?;
        Ok(CommandOutput::InstanceCloned {
            source: instance.to_string(),
            instance: slug,
        })
    }

    /// `Command::InstanceKill`: force-stop a game this `Session` launched.
    /// The launch's own wait task observes the exit and emits
    /// `Event::InstanceExited` as usual.
    pub(super) fn instance_kill(&self, instance: &str) -> Result<CommandOutput> {
        let kill = self
            .kill_switches
            .lock()
            .expect("kill switch mutex poisoned")
            .remove(instance);
        match kill {
            Some(tx) => {
                let _ = tx.send(());
                Ok(CommandOutput::InstanceKilled {
                    instance: instance.to_string(),
                })
            }
            None => Err(Error::NotLaunchedHere(instance.to_string())),
        }
    }
}
