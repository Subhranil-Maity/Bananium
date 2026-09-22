//! Mojang piston-meta, asset indexes, library rules/natives, and loader metadata.

pub mod assets;
pub mod client;
pub mod error;
pub mod manifest;
pub mod platform;
pub mod profile;
pub mod rules;

pub use assets::{AssetIndex, AssetObject};
pub use client::MetaClient;
pub use error::{Error, Result};
pub use manifest::{VersionManifest, VersionManifestEntry};
pub use platform::Platform;
pub use profile::{compare_versions, Argument, Arguments, Library, VersionProfile};
pub use rules::{evaluate_rules, FeatureFlags, Rule};
