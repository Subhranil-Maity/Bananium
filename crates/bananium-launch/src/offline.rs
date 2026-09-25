use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use bananium_core::Paths;

use crate::error::{Error, Result};

/// Whether `name` is a username vanilla accepts: 3–16 ASCII letters,
/// digits, or `_`. Servers (even offline-mode ones) kick names outside
/// this set, so it's enforced when a profile is added rather than
/// discovered at join time. Profiles that predate this check and were
/// created implicitly by `get_or_create` aren't retroactively rejected.
pub fn is_valid_player_name(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

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
    /// Name of the profile a launch uses when it doesn't name one. Absent
    /// (older files, or never set) means "the first profile".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default: Option<String>,
    #[serde(default)]
    profile: Vec<LocalProfile>,
}

impl ProfilesFile {
    /// The effective default: the named `default` if it still exists,
    /// otherwise the first profile.
    fn default_profile(&self) -> Option<&LocalProfile> {
        self.default
            .as_deref()
            .and_then(|name| self.profile.iter().find(|p| p.name == name))
            .or_else(|| self.profile.first())
    }
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

    /// The default profile used when a launch doesn't name one: the one
    /// set via [`ProfileStore::set_default`], else the first saved profile,
    /// else a freshly created "Player".
    pub fn default_profile(&self) -> Result<LocalProfile> {
        let file = self.load()?;
        match file.default_profile() {
            Some(p) => Ok(p.clone()),
            None => self.get_or_create("Player"),
        }
    }

    /// Name of the effective default profile, if any profile exists. Unlike
    /// [`ProfileStore::default_profile`] this never creates one — it's for
    /// display, not for launching.
    pub fn default_name(&self) -> Result<Option<String>> {
        Ok(self.load()?.default_profile().map(|p| p.name.clone()))
    }

    /// Add a new profile, validated with [`is_valid_player_name`]. Names
    /// are unique case-insensitively, since vanilla treats `Steve` and
    /// `steve` as the same player.
    pub fn add(&self, name: &str) -> Result<LocalProfile> {
        if !is_valid_player_name(name) {
            return Err(Error::InvalidPlayerName(name.to_string()));
        }
        let mut file = self.load()?;
        if file
            .profile
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(name))
        {
            return Err(Error::ProfileExists(name.to_string()));
        }
        let profile = LocalProfile {
            name: name.to_string(),
            uuid: offline_uuid(name),
        };
        file.profile.push(profile.clone());
        self.save(&file)?;
        Ok(profile)
    }

    /// Delete a profile. If it was the explicit default, the default falls
    /// back to the first remaining profile.
    pub fn remove(&self, name: &str) -> Result<()> {
        let mut file = self.load()?;
        let before = file.profile.len();
        file.profile.retain(|p| p.name != name);
        if file.profile.len() == before {
            return Err(Error::ProfileNotFound(name.to_string()));
        }
        if file.default.as_deref() == Some(name) {
            file.default = None;
        }
        self.save(&file)
    }

    /// Make `name` the profile launches use when they don't name one.
    pub fn set_default(&self, name: &str) -> Result<()> {
        let mut file = self.load()?;
        if !file.profile.iter().any(|p| p.name == name) {
            return Err(Error::ProfileNotFound(name.to_string()));
        }
        file.default = Some(name.to_string());
        self.save(&file)
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
    fn player_name_validation_matches_vanilla_rules() {
        assert!(is_valid_player_name("Steve"));
        assert!(is_valid_player_name("a_b"));
        assert!(is_valid_player_name("abcdefghijklmnop")); // 16
        assert!(!is_valid_player_name("ab"));
        assert!(!is_valid_player_name("abcdefghijklmnopq")); // 17
        assert!(!is_valid_player_name("has space"));
        assert!(!is_valid_player_name("dash-name"));
        assert!(!is_valid_player_name("ünï"));
    }

    #[test]
    fn add_rejects_invalid_and_duplicate_names() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProfileStore::new(Paths::at(dir.path()));
        store.add("Steve").unwrap();
        assert!(matches!(store.add("steve"), Err(Error::ProfileExists(_))));
        assert!(matches!(store.add("x"), Err(Error::InvalidPlayerName(_))));
    }

    #[test]
    fn set_default_and_remove_fall_back_to_first() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProfileStore::new(Paths::at(dir.path()));
        store.add("Alex").unwrap();
        store.add("Steve").unwrap();
        assert_eq!(store.default_profile().unwrap().name, "Alex");

        store.set_default("Steve").unwrap();
        assert_eq!(store.default_profile().unwrap().name, "Steve");

        store.remove("Steve").unwrap();
        assert_eq!(store.default_profile().unwrap().name, "Alex");
        assert!(matches!(
            store.remove("Steve"),
            Err(Error::ProfileNotFound(_))
        ));
        assert!(matches!(
            store.set_default("Nobody"),
            Err(Error::ProfileNotFound(_))
        ));
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
