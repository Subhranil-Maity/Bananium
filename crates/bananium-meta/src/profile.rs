use std::cmp::Ordering;
use std::collections::HashMap;

use serde::Deserialize;

use crate::platform::Platform;
use crate::rules::{evaluate_rules, FeatureFlags, Rule};

/// A single Minecraft version's full launch metadata, as fetched from the
/// URL in its [`crate::VersionManifestEntry`]. Covers both the modern
/// (1.13+) and legacy (pre-1.13) shapes — see [`VersionProfile::is_modern_arguments`].
#[derive(Debug, Clone, Deserialize)]
pub struct VersionProfile {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    /// Fully-qualified JVM entry point, e.g. `net.minecraft.client.main.Main`.
    #[serde(rename = "mainClass")]
    pub main_class: String,
    /// Legacy (pre-1.13) single-string game arguments. `None` on modern
    /// profiles, which use [`VersionProfile::arguments`] instead.
    #[serde(default, rename = "minecraftArguments")]
    pub minecraft_arguments: Option<String>,
    /// Modern (1.13+) rule-array arguments. `None` on legacy profiles.
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(rename = "assetIndex")]
    pub asset_index: AssetIndexRef,
    /// The asset index id (e.g. `"17"`), used as `${assets_index_name}`.
    pub assets: String,
    pub downloads: Downloads,
    /// Which Mojang java-runtime component this version needs (e.g.
    /// `java-runtime-delta`). Absent on some very old profiles, which fall
    /// back to `jre-legacy` (M3 concern; not yet implemented here).
    #[serde(default, rename = "javaVersion")]
    pub java_version: Option<JavaVersionRef>,
    pub libraries: Vec<Library>,
    /// Log4j2 config reference; unused until crash-log analysis lands (M3).
    #[serde(default)]
    pub logging: Option<Logging>,
}

impl VersionProfile {
    /// This profile uses the modern (1.13+) `arguments.game`/`arguments.jvm`
    /// rule-array shape rather than the legacy `minecraftArguments` string.
    pub fn is_modern_arguments(&self) -> bool {
        self.arguments.is_some()
    }

    /// Libraries whose `rules` (if any) apply on the given platform.
    pub fn applicable_libraries<'a>(
        &'a self,
        platform: &'a Platform,
        features: &'a FeatureFlags,
    ) -> impl Iterator<Item = &'a Library> {
        self.libraries
            .iter()
            .filter(move |lib| lib.is_applicable(platform, features))
    }
}

/// The modern (1.13+) argument shape: two flat arrays whose entries can
/// each be conditional on `rules`. See [`Argument`].
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
}

/// One entry in a modern `arguments.game`/`arguments.jvm` array: either a
/// bare literal token, or a `{rules, value}` object whose `value` (one
/// token or several) only applies when `rules` matches. Deserialized
/// `#[serde(untagged)]` so both JSON shapes parse into this one type.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    Conditional {
        #[serde(default)]
        rules: Vec<Rule>,
        #[serde(deserialize_with = "one_or_many")]
        value: Vec<String>,
    },
}

impl Argument {
    /// The literal tokens this argument contributes, given the current
    /// platform/features, or `None` if its rules don't apply.
    pub fn resolve(&self, platform: &Platform, features: &FeatureFlags) -> Option<&[String]> {
        match self {
            Argument::Plain(s) => Some(std::slice::from_ref(s)),
            Argument::Conditional { rules, value } => {
                if evaluate_rules(rules, platform, features) {
                    Some(value)
                } else {
                    None
                }
            }
        }
    }
}

/// Deserialize a JSON field that's either a bare string or an array of
/// strings into a `Vec<String>` either way — used for `Argument::value`,
/// which Mojang sometimes writes as `"--demo"` and sometimes as
/// `["-XstartOnFirstThread"]`.
fn one_or_many<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
    })
}

/// Points at the asset index JSON for this version (fetched separately via
/// `url`); not the index's contents themselves — see [`crate::AssetIndex`].
#[derive(Debug, Clone, Deserialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    /// Sum of every asset object's size; informational only.
    #[serde(default, rename = "totalSize")]
    pub total_size: Option<u64>,
    pub url: String,
}

/// The client (and, for server-hosting versions, server) jar downloads.
#[derive(Debug, Clone, Deserialize)]
pub struct Downloads {
    pub client: DownloadArtifact,
    #[serde(default)]
    pub server: Option<DownloadArtifact>,
}

/// A single downloadable file: enough to fetch it, verify it, and (for
/// library entries) know where its maven layout expects it.
#[derive(Debug, Clone, Deserialize)]
pub struct DownloadArtifact {
    pub sha1: String,
    pub size: u64,
    pub url: String,
    /// Maven-layout relative path (e.g. `org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar`).
    /// Present on library artifacts, absent on the client/server jar entries.
    #[serde(default)]
    pub path: Option<String>,
}

/// Which Mojang java-runtime component a version needs, and its major
/// version (informational — the component name is what actually drives
/// provisioning, per PLAN.md's "Java the Mojang way" section).
#[derive(Debug, Clone, Deserialize)]
pub struct JavaVersionRef {
    pub component: String,
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

/// One dependency: a classpath jar, a natives jar, or (pre-1.19) both via
/// `natives`/`downloads.classifiers`. See `resolve_libraries` in
/// `bananium-launch` for how these get sorted into "classpath" vs.
/// "extract into the natives directory."
#[derive(Debug, Clone, Deserialize)]
pub struct Library {
    /// Maven coordinate, e.g. `"org.lwjgl:lwjgl:3.3.3"` or, for a modern
    /// per-platform natives jar, `"org.lwjgl:lwjgl:3.3.3:natives-linux"`.
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    /// Legacy (pre-~1.19) natives classifier map, e.g. `{"linux": "natives-linux"}`.
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
    /// Legacy extraction excludes (e.g. `META-INF/`); modern profiles don't
    /// carry this even for their natives-jar library entries.
    #[serde(default)]
    pub extract: Option<ExtractRules>,
    /// Platform/feature conditions gating whether this library applies at
    /// all; see [`Library::is_applicable`].
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// Some legacy entries give a maven repo base URL instead of a concrete
    /// `downloads.artifact`; resolving those is out of scope for 1.13+ but
    /// the field is kept so parsing never fails on older profiles.
    #[serde(default)]
    pub url: Option<String>,
}

/// The concrete downloadable jar(s) for a [`Library`].
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LibraryDownloads {
    /// The main (non-natives) jar, if any.
    #[serde(default)]
    pub artifact: Option<DownloadArtifact>,
    /// Legacy per-platform natives jars, keyed by classifier (e.g.
    /// `"natives-linux"`); see [`Library::natives_artifact`].
    #[serde(default)]
    pub classifiers: HashMap<String, DownloadArtifact>,
}

/// Legacy natives-jar extraction rules.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExtractRules {
    /// Path prefixes to skip when unzipping (e.g. `["META-INF/"]`).
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// Log4j2 configuration reference (unused until crash-log analysis, M3).
#[derive(Debug, Clone, Deserialize)]
pub struct Logging {
    #[serde(default)]
    pub client: Option<LoggingClient>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingClient {
    /// JVM argument template, e.g. `-Dlog4j2.configurationFile=${path}`.
    pub argument: String,
    pub file: DownloadArtifact,
    #[serde(rename = "type")]
    pub log_type: String,
}

impl Library {
    /// Whether this library's `rules` (if any) permit it on `platform`/`features`.
    pub fn is_applicable(&self, platform: &Platform, features: &FeatureFlags) -> bool {
        evaluate_rules(&self.rules, platform, features)
    }

    /// The main (non-natives) artifact to place on the classpath, if any.
    pub fn artifact(&self) -> Option<&DownloadArtifact> {
        self.downloads.as_ref()?.artifact.as_ref()
    }

    /// The legacy natives classifier key for this platform (with `${arch}`
    /// substituted), if this library ships one.
    pub fn natives_classifier(&self, platform: &Platform) -> Option<String> {
        let key = self.natives.as_ref()?.get(&platform.os_name)?;
        Some(key.replace("${arch}", platform.arch_bits()))
    }

    /// The natives jar for this platform, from the legacy
    /// `downloads.classifiers` map, if any.
    pub fn natives_artifact(&self, platform: &Platform) -> Option<&DownloadArtifact> {
        let classifier = self.natives_classifier(platform)?;
        self.downloads.as_ref()?.classifiers.get(&classifier)
    }

    /// Whether this library uses the legacy `natives`/`downloads.classifiers`
    /// shape at all (as opposed to the modern per-platform-entry shape, or
    /// not being a natives library in the first place).
    pub fn has_natives(&self) -> bool {
        self.natives.is_some()
    }

    /// The `group:artifact` portion of the maven coordinate `name`, used to
    /// dedup libraries by keeping only the highest version of each.
    pub fn group_artifact(&self) -> &str {
        match self.name.match_indices(':').nth(1) {
            Some((idx, _)) => &self.name[..idx],
            None => &self.name,
        }
    }

    /// The version portion (3rd `:`-separated segment) of the maven
    /// coordinate `name`.
    pub fn version(&self) -> &str {
        self.name.split(':').nth(2).unwrap_or("")
    }
}

/// Best-effort ordering over maven version strings: numeric-looking
/// dot/dash-separated segments compare numerically, everything else falls
/// back to lexical comparison of that segment.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let split = |v: &str| -> Vec<String> { v.split(['.', '-']).map(str::to_string).collect() };
    let (sa, sb) = (split(a), split(b));
    for (pa, pb) in sa.iter().zip(sb.iter()) {
        let ord = match (pa.parse::<u64>(), pb.parse::<u64>()) {
            (Ok(na), Ok(nb)) => na.cmp(&nb),
            _ => pa.cmp(pb),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    sa.len().cmp(&sb.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_artifact_and_version_split_correctly() {
        let lib: Library = serde_json::from_str(r#"{"name":"org.lwjgl:lwjgl:3.3.3"}"#).unwrap();
        assert_eq!(lib.group_artifact(), "org.lwjgl:lwjgl");
        assert_eq!(lib.version(), "3.3.3");
    }

    #[test]
    fn version_compare_prefers_higher_numeric_segments() {
        assert_eq!(compare_versions("3.3.3", "3.10.0"), Ordering::Less);
        assert_eq!(compare_versions("1.2", "1.2.0"), Ordering::Less);
        assert_eq!(
            compare_versions("2.9.4-nightly-20150209", "2.9.4-nightly-20150210"),
            Ordering::Less
        );
    }

    #[test]
    fn argument_conditional_resolves_by_feature() {
        let args: Vec<Argument> = serde_json::from_str(
            r#"["--username","${auth_player_name}",{"rules":[{"action":"allow","features":{"is_demo_user":true}}],"value":"--demo"}]"#,
        )
        .unwrap();
        let platform = Platform {
            os_name: "linux".into(),
            arch: "x86_64".into(),
            os_version: String::new(),
        };
        assert_eq!(args[2].resolve(&platform, &FeatureFlags::default()), None);
        let demo = FeatureFlags {
            is_demo_user: true,
            ..Default::default()
        };
        assert_eq!(
            args[2].resolve(&platform, &demo),
            Some(&["--demo".to_string()][..])
        );
    }
}
