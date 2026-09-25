//! Classpath/arg construction, JVM profiles, process supervision, and crash analysis.

pub mod classpath;
pub mod error;
pub mod natives;
pub mod offline;
pub mod plan;

pub use classpath::{resolve_libraries, NativesEntry, ResolvedArtifact, ResolvedLibraries};
pub use error::{Error, Result};
pub use natives::{extract_natives, natives_cache_key};
pub use offline::{is_valid_player_name, offline_uuid, LocalProfile, ProfileStore};
pub use plan::{build_launch_plan, LaunchContext, LaunchPlan};
