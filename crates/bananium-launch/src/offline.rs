use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use bananium_core::Paths;

use crate::error::Result;

/// Vanilla's offline-mode UUID: `UUID.nameUUIDFromBytes(("OfflinePlayer:" +
/// name).getBytes(UTF_8))`. Note this is *not* RFC 4122 UUIDv3 over a
/// namespace — Java hashes the raw bytes directly, with no namespace UUID
/// prepended — so it has to be reproduced by hand rather than via the
/// `uuid` crate's `new_v3`.
pub fn offline_uuid(player_name: &str) -> Uuid {
    let mut hasher = Md5::new();
    hasher.update(format!("OfflinePlayer:{player_name}").as_bytes());
    let mut bytes: [u8; 16] = hasher.finalize().into();
    bytes[6] = (bytes[6] & 0x0f) | 0x30; // version 3
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
    Uuid::from_bytes(bytes)
}

/// A saved offline player identity: a display name and its derived
/// [`offline_uuid`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalProfile {
    pub name: String,
    pub uuid: Uuid,
}

/// The on-disk shape of `profiles.toml`: a TOML array of tables under the
/// `[[profile]]` key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ProfilesFile {
    #[serde(default)]
    profile: Vec<LocalProfile>,
}

/// Several named offline profiles, stored at `~/.bananium/profiles.toml`,
/// selectable per launch.
pub struct ProfileStore {
    paths: Paths,
}

impl ProfileStore {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    /// Every saved profile, in file order.
    pub fn list(&self) -> Result<Vec<LocalProfile>> {
        Ok(self.load()?.profile)
    }

    /// The named profile if it exists, otherwise a freshly minted (and
    /// persisted) one — so `bananium launch` never needs a prior
    /// `profile add` step to work.
    pub fn get_or_create(&self, name: &str) -> Result<LocalProfile> {
        let mut file = self.load()?;
        if let Some(existing) = file.profile.iter().find(|p| p.name == name) {
            return Ok(existing.clone());
        }
        let profile = LocalProfile {
            name: name.to_string(),
            uuid: offline_uuid(name),
        };
        file.profile.push(profile.clone());
        self.save(&file)?;
        Ok(profile)
    }

    /// The default profile used when a launch doesn't name one: the first
    /// saved profile, or a freshly created "Player".
    pub fn default_profile(&self) -> Result<LocalProfile> {
        let file = self.load()?;
        match file.profile.first() {
            Some(p) => Ok(p.clone()),
            None => self.get_or_create("Player"),
        }
    }

    /// Load `profiles.toml`, or an empty set if it doesn't exist yet (not
    /// an error — the file is created lazily on first
    /// [`ProfileStore::get_or_create`]).
    fn load(&self) -> Result<ProfilesFile> {
        let path = self.paths.profiles_toml();
        if !path.is_file() {
            return Ok(ProfilesFile::default());
        }
        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }

    fn save(&self, file: &ProfilesFile) -> Result<()> {
        if let Some(parent) = self.paths.profiles_toml().parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(self.paths.profiles_toml(), toml::to_string_pretty(file)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_known_vanilla_values() {
        // Cross-checked against an independent Python re-implementation of
        // Java's UUID.nameUUIDFromBytes("OfflinePlayer:<name>").
        assert_eq!(
            offline_uuid("Notch").to_string(),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
        assert_eq!(
            offline_uuid("Player").to_string(),
            "a01e3843-e521-3998-958a-f459800e4d11"
        );
    }

    #[test]
    fn offline_uuid_has_version_3_and_variant_bits_set() {
        let id = offline_uuid("anyone");
        assert_eq!(id.get_version_num(), 3);
        let bytes = id.as_bytes();
        assert_eq!(bytes[8] & 0xc0, 0x80);
    }

    #[test]
    fn get_or_create_persists_across_loads() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let store = ProfileStore::new(paths.clone());
        let first = store.get_or_create("Steve").unwrap();

        let reopened = ProfileStore::new(paths);
        let second = reopened.get_or_create("Steve").unwrap();
        assert_eq!(first.uuid, second.uuid);
    }

    #[test]
    fn default_profile_creates_player_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProfileStore::new(Paths::at(dir.path()));
        let profile = store.default_profile().unwrap();
        assert_eq!(profile.name, "Player");
        assert_eq!(profile.uuid, offline_uuid("Player"));
    }
}
