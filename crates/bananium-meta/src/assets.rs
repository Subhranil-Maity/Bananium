use std::collections::HashMap;

use serde::Deserialize;

/// The full contents of one asset index (fetched from a
/// [`crate::profile::AssetIndexRef`]): every sound/texture/lang-file
/// Minecraft needs for that version, keyed by its logical resource path.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndex {
    /// Logical resource path (e.g. `"minecraft/sounds/random/click.ogg"`) -> object.
    pub objects: HashMap<String, AssetObject>,
    /// Pre-1.7.10 "virtual" layout: objects must also be materialized under
    /// a named-file tree (`assets/virtual/<id>/...`) rather than only the
    /// hash-addressed `objects/` tree.
    #[serde(default, rename = "virtual")]
    pub is_virtual: bool,
    /// Very old layout where the resources tree must additionally be mapped
    /// straight into the instance's `resources/` directory.
    #[serde(default)]
    pub map_to_resources: bool,
}

/// One asset's content hash and size, addressed the same way as everything
/// else in the content store: SHA-1 hex, first two chars as a directory
/// prefix.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

impl AssetObject {
    /// The `<hash[0..2]>/<hash>` path this object lives at, both in
    /// Mojang's `resources.download.minecraft.net` and under
    /// `Paths::assets_objects_dir()`.
    pub fn object_path(&self) -> String {
        format!("{}/{}", &self.hash[0..2], self.hash)
    }
}
