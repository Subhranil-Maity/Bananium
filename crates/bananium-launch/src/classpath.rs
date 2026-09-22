use std::cmp::Ordering;
use std::collections::HashMap;

use bananium_meta::profile::DownloadArtifact;
use bananium_meta::{compare_versions, FeatureFlags, Library, Platform, VersionProfile};

/// Everything needed to both download and later locate a library jar in the
/// content store: it's identified purely by `sha1` (the store key), with
/// `url`/`size` carried along for the download engine.
#[derive(Debug, Clone)]
pub struct ResolvedArtifact {
    /// Content hash — the key it lives under in the store and the value
    /// verified against after download.
    pub sha1: String,
    /// Where to download it from, if it isn't in the store yet.
    pub url: String,
    /// Expected size, used for the store's fast already-verified check.
    pub size: u64,
}

impl From<&DownloadArtifact> for ResolvedArtifact {
    fn from(a: &DownloadArtifact) -> Self {
        Self {
            sha1: a.sha1.clone(),
            url: a.url.clone(),
            size: a.size,
        }
    }
}

/// A native-library jar that must be extracted into the natives directory
/// rather than placed on the classpath.
#[derive(Debug, Clone)]
pub struct NativesEntry {
    pub artifact: ResolvedArtifact,
    /// Zip-entry path prefixes to skip during extraction (e.g. `META-INF/`);
    /// from the library's legacy `extract.exclude`, empty for modern entries.
    pub exclude: Vec<String>,
    /// Which native-loading subsystem this belongs to — see [`native_component`].
    pub component: &'static str,
}

/// Every subdirectory `extract_natives` always creates under the natives
/// root, regardless of whether any library actually routes there (e.g. JNA
/// self-extracts its own bundled natives into `jna.tmpdir` at runtime, so
/// Bananium never populates that one — it only needs to exist).
pub const NATIVE_COMPONENTS: &[&str] = &["java", "jna", "lwjgl", "netty"];

/// Classify a library's maven `group:artifact` into the native-loading
/// subsystem it belongs to. Newer Mojang profiles (see PLAN.md's note on
/// this being a moving target) pass each of the JVM's four
/// native-library-loading properties a *different* subdirectory of
/// `${natives_directory}` — `-Djava.library.path=.../java`,
/// `-Djna.tmpdir=.../jna`,
/// `-Dorg.lwjgl.system.SharedLibraryExtractPath=.../lwjgl`,
/// `-Dio.netty.native.workdir=.../netty` — rather than the single shared
/// directory older profiles use for all four. The profile itself never
/// states which library belongs to which property; the maven group is the
/// only signal available, so that's what this infers from. Anything that
/// isn't LWJGL or Netty (e.g. Mojang's own `jtracy`) is generic JNI and
/// belongs on plain `java.library.path`.
pub fn native_component(group_artifact: &str) -> &'static str {
    if let Some(group) = group_artifact.split(':').next() {
        if group == "org.lwjgl" {
            return "lwjgl";
        }
        if group == "io.netty" {
            return "netty";
        }
        if group == "net.java.dev.jna" {
            return "jna";
        }
    }
    "java"
}

/// The output of [`resolve_libraries`]: everything needed to both fetch and
/// assemble a version's dependencies for the current platform.
#[derive(Debug, Clone, Default)]
pub struct ResolvedLibraries {
    /// Deduped by maven `group:artifact` (highest version wins), sorted by
    /// SHA-1 for determinism.
    pub classpath: Vec<ResolvedArtifact>,
    /// Jars that must be extracted into the natives directory, not placed
    /// on the classpath.
    pub natives: Vec<NativesEntry>,
}

/// Whether this library's jar must be extracted into the natives directory
/// rather than placed on the classpath — true either for the legacy
/// `natives`/`downloads.classifiers` shape, or the modern (1.19+) shape
/// where the natives jar is its own library entry carrying a `natives-*`
/// maven classifier as the 4th `:`-separated segment of `name`.
fn is_natives_only(lib: &Library) -> bool {
    if lib.has_natives() {
        return true;
    }
    lib.name
        .split(':')
        .nth(3)
        .map(|classifier| classifier.starts_with("natives"))
        .unwrap_or(false)
}

/// Filter a profile's libraries to the ones applicable on `platform`, dedup
/// the classpath entries by `group:artifact` keeping the highest version,
/// and separate out the natives jars that need extraction instead. Pure
/// metadata resolution — no filesystem or network access, so it's usable
/// both to build download specs and (once those files are in the store) to
/// build the classpath.
pub fn resolve_libraries(
    profile: &VersionProfile,
    platform: &Platform,
    features: &FeatureFlags,
) -> ResolvedLibraries {
    let applicable: Vec<&Library> = profile.applicable_libraries(platform, features).collect();

    let mut best: HashMap<&str, &Library> = HashMap::new();
    let mut natives = Vec::new();

    for lib in &applicable {
        if is_natives_only(lib) {
            let artifact = if lib.has_natives() {
                lib.natives_artifact(platform)
            } else {
                lib.artifact()
            };
            if let Some(artifact) = artifact {
                let exclude = lib
                    .extract
                    .as_ref()
                    .map(|e| e.exclude.clone())
                    .unwrap_or_default();
                natives.push(NativesEntry {
                    artifact: artifact.into(),
                    exclude,
                    component: native_component(lib.group_artifact()),
                });
            }
            continue;
        }

        let Some(_) = lib.artifact() else { continue };
        best.entry(lib.group_artifact())
            .and_modify(|current| {
                if compare_versions(lib.version(), current.version()) == Ordering::Greater {
                    *current = lib;
                }
            })
            .or_insert(lib);
    }

    let mut classpath: Vec<ResolvedArtifact> = best
        .values()
        .map(|lib| {
            lib.artifact()
                .expect("dedup map only holds libraries with an artifact")
                .into()
        })
        .collect();
    classpath.sort_by(|a, b| a.sha1.cmp(&b.sha1));

    ResolvedLibraries { classpath, natives }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platform() -> Platform {
        Platform {
            os_name: "linux".into(),
            arch: "x86_64".into(),
            os_version: String::new(),
        }
    }

    fn profile_with(libraries_json: &str) -> VersionProfile {
        let body = format!(
            r#"{{
                "id": "test",
                "type": "release",
                "mainClass": "net.minecraft.client.main.Main",
                "assetIndex": {{"id":"17","sha1":"aaaa","size":1,"url":"http://example/17.json"}},
                "assets": "17",
                "downloads": {{"client": {{"sha1":"bbbb","size":1,"url":"http://example/client.jar"}}}},
                "libraries": {libraries_json}
            }}"#
        );
        serde_json::from_str(&body).unwrap()
    }

    #[test]
    fn dedup_keeps_highest_version_and_separates_natives() {
        let profile = profile_with(
            r#"[
                {"name":"org.lwjgl:lwjgl:3.3.2","downloads":{"artifact":{"path":"a","sha1":"1111111111111111111111111111111111111a","size":1,"url":"http://x/a"}}},
                {"name":"org.lwjgl:lwjgl:3.3.3","downloads":{"artifact":{"path":"b","sha1":"2222222222222222222222222222222222222b","size":1,"url":"http://x/b"}}},
                {"name":"org.lwjgl:lwjgl:3.3.3:natives-linux","rules":[{"action":"allow","os":{"name":"linux"}}],"downloads":{"artifact":{"path":"c","sha1":"3333333333333333333333333333333333333c","size":1,"url":"http://x/c"}}},
                {"name":"org.lwjgl:lwjgl:3.3.3:natives-windows","rules":[{"action":"allow","os":{"name":"windows"}}],"downloads":{"artifact":{"path":"d","sha1":"4444444444444444444444444444444444444d","size":1,"url":"http://x/d"}}}
            ]"#,
        );

        let resolved = resolve_libraries(&profile, &platform(), &FeatureFlags::default());

        assert_eq!(resolved.classpath.len(), 1);
        assert_eq!(
            resolved.classpath[0].sha1,
            "2222222222222222222222222222222222222b"
        );

        assert_eq!(resolved.natives.len(), 1);
        assert_eq!(
            resolved.natives[0].artifact.sha1,
            "3333333333333333333333333333333333333c"
        );
    }

    #[test]
    fn legacy_natives_classifier_map_is_resolved() {
        let profile = profile_with(
            r#"[{
                "name":"net.java.jinput:jinput-platform:2.0.5",
                "natives": {"linux": "natives-linux"},
                "downloads": {
                    "classifiers": {
                        "natives-linux": {"path":"n","sha1":"5555555555555555555555555555555555555e","size":1,"url":"http://x/n"}
                    }
                }
            }]"#,
        );
        let resolved = resolve_libraries(&profile, &platform(), &FeatureFlags::default());
        assert!(resolved.classpath.is_empty());
        assert_eq!(resolved.natives.len(), 1);
        assert_eq!(
            resolved.natives[0].artifact.sha1,
            "5555555555555555555555555555555555555e"
        );
    }
}
