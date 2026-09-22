//! Hits real Mojang endpoints. Skips (rather than fails) if the network is
//! unavailable, since CI environments vary; this is a sanity check against
//! the live schema, not the offline-gate test.

use bananium_core::Paths;
use bananium_meta::{MetaClient, Platform};
use bananium_net::HttpClient;

#[tokio::test]
async fn parses_the_real_1_21_1_profile() {
    let http = match HttpClient::new("bananium-tests/0.1 (test@example.invalid)") {
        Ok(h) => h,
        Err(_) => return,
    };
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::at(dir.path());
    let meta = MetaClient::new(http, paths);

    let Ok(manifest) = meta.version_manifest().await else {
        return;
    };
    let entry = manifest
        .find("1.21.1")
        .expect("1.21.1 should be a listed release");
    let profile = meta
        .version_profile(entry)
        .await
        .expect("profile should parse");

    assert_eq!(profile.id, "1.21.1");
    assert!(profile.is_modern_arguments());
    assert_eq!(
        profile.java_version.as_ref().unwrap().component,
        "java-runtime-delta"
    );
    assert!(!profile.libraries.is_empty());

    let platform = Platform {
        os_name: "linux".into(),
        arch: "x86_64".into(),
        os_version: String::new(),
    };
    let features = Default::default();
    let applicable: Vec<_> = profile.applicable_libraries(&platform, &features).collect();
    assert!(applicable
        .iter()
        .any(|l| l.name.starts_with("org.lwjgl:lwjgl:")));
    // macOS-only libraries must not apply on Linux.
    assert!(!applicable
        .iter()
        .any(|l| l.name.starts_with("ca.weblite:java-objc-bridge")));

    let asset_index = meta
        .asset_index(&profile.asset_index)
        .await
        .expect("asset index should parse");
    assert!(!asset_index.objects.is_empty());
}
