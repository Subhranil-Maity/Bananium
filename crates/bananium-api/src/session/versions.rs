//! Listing installable Minecraft versions and Fabric loaders.

use bananium_meta::MetaClient;

use super::Session;
use crate::error::Result;
use crate::output::{CommandOutput, FabricLoaderSummary, VersionSummary};

impl Session {
    fn meta(&self) -> MetaClient {
        MetaClient::new(self.http.clone(), self.paths.clone())
    }

    /// `Command::VersionList`: every Minecraft version, newest first, as
    /// Mojang's manifest orders them. Works offline once the manifest has
    /// been fetched once.
    pub(super) async fn version_list(&self, include_snapshots: bool) -> Result<CommandOutput> {
        let manifest = self.meta().version_manifest().await?;
        let versions = manifest
            .versions
            .into_iter()
            .filter(|v| include_snapshots || v.version_type == "release")
            .map(|v| VersionSummary {
                id: v.id,
                kind: v.version_type,
                release_time: v.release_time,
            })
            .collect();
        Ok(CommandOutput::VersionListed {
            latest_release: manifest.latest.release,
            versions,
        })
    }

    /// `Command::FabricLoaderList`: Fabric loader releases for `mc_version`,
    /// newest first. Empty when Fabric doesn't support that version.
    pub(super) async fn fabric_loader_list(&self, mc_version: &str) -> Result<CommandOutput> {
        let loaders = self
            .meta()
            .fabric_loaders(mc_version)
            .await?
            .into_iter()
            .map(|l| FabricLoaderSummary {
                version: l.version,
                stable: l.stable,
            })
            .collect();
        Ok(CommandOutput::FabricLoaderListed {
            mc_version: mc_version.to_string(),
            loaders,
        })
    }
}
