//! Offline accounts: the `Profile*` commands over `bananium_launch::ProfileStore`.

use bananium_launch::ProfileStore;

use super::Session;
use crate::error::Result;
use crate::output::{CommandOutput, ProfileSummary};

impl Session {
    fn profiles(&self) -> ProfileStore {
        ProfileStore::new(self.paths.clone())
    }

    /// `Command::ProfileList`: every saved offline profile, flagging the
    /// one a launch without an explicit `profile` would use.
    pub(super) fn profile_list(&self) -> Result<CommandOutput> {
        let store = self.profiles();
        let default = store.default_name()?;
        let profiles = store
            .list()?
            .into_iter()
            .map(|p| ProfileSummary {
                is_default: default.as_deref() == Some(p.name.as_str()),
                uuid: p.uuid.to_string(),
                name: p.name,
            })
            .collect();
        Ok(CommandOutput::ProfileListed { profiles })
    }

    /// `Command::ProfileAdd`: validated per `is_valid_player_name`.
    pub(super) fn profile_add(&self, name: &str) -> Result<CommandOutput> {
        let store = self.profiles();
        let profile = store.add(name)?;
        let is_default = store.default_name()?.as_deref() == Some(profile.name.as_str());
        Ok(CommandOutput::ProfileAdded {
            profile: ProfileSummary {
                is_default,
                uuid: profile.uuid.to_string(),
                name: profile.name,
            },
        })
    }

    /// `Command::ProfileRemove`.
    pub(super) fn profile_remove(&self, name: &str) -> Result<CommandOutput> {
        self.profiles().remove(name)?;
        Ok(CommandOutput::ProfileRemoved {
            name: name.to_string(),
        })
    }

    /// `Command::ProfileSetDefault`.
    pub(super) fn profile_set_default(&self, name: &str) -> Result<CommandOutput> {
        self.profiles().set_default(name)?;
        Ok(CommandOutput::ProfileDefaultSet {
            name: name.to_string(),
        })
    }
}
