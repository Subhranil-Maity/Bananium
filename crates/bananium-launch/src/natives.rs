use std::path::{Path, PathBuf};

use bananium_core::Paths;
use sha1::{Digest, Sha1};

use crate::classpath::{NativesEntry, NATIVE_COMPONENTS};
use crate::error::Result;

/// Bumped whenever [`extract_jar`]'s output layout changes, and folded into
/// [`natives_cache_key`]. Without this, fixing an extraction bug (like the
/// ones described in `extract_jar`'s doc comment) would silently do
/// nothing for anyone who already has a `.bananium-extracted` marker on
/// disk from the broken version: the key wouldn't change, so
/// `extract_natives` would keep trusting the stale, wrong output forever.
const EXTRACTION_LAYOUT_VERSION: u32 = 4;

/// A stable cache key for a set of native jars: the SHA-1 of their sorted
/// SHA-1s (plus `EXTRACTION_LAYOUT_VERSION`), joined. Deterministic
/// regardless of iteration order, and changes automatically if the library
/// set — or the extraction logic itself — ever changes.
pub fn natives_cache_key(natives: &[NativesEntry]) -> String {
    let mut ids: Vec<&str> = natives.iter().map(|n| n.artifact.sha1.as_str()).collect();
    ids.sort();
    let mut hasher = Sha1::new();
    hasher.update(format!("v{EXTRACTION_LAYOUT_VERSION}:").as_bytes());
    hasher.update(ids.join(",").as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Always skipped during extraction, on top of a library's own
/// `extract.exclude` — jar signing/manifest metadata has no business in a
/// natives directory.
const DEFAULT_EXCLUDES: &[&str] = &["META-INF/"];

/// Extract every native jar (looked up by SHA-1 in the content store) into
/// a shared, hash-keyed cache directory, skipping the work entirely if
/// that exact jar set was already extracted. Returns the natives directory.
///
/// Populates two things per file, to satisfy both native-directory
/// conventions Mojang profiles use (see `native_component`'s doc comment):
/// `<root>/<component>/<basename>` (what newer profiles' per-subsystem
/// `${natives_directory}/<component>` arguments expect) and a hardlinked
/// `<root>/<basename>` (what older profiles' single shared
/// `${natives_directory}` expects). Every [`NATIVE_COMPONENTS`]
/// subdirectory is created up front regardless of whether any library
/// routes there — e.g. JNA self-extracts its own bundled natives into
/// `jna.tmpdir` at runtime, so Bananium never writes into `<root>/jna/`,
/// but the directory still needs to exist.
pub fn extract_natives(paths: &Paths, natives: &[NativesEntry]) -> Result<PathBuf> {
    let key = natives_cache_key(natives);
    let dir = paths.natives_cache_dir(&key);
    let marker = dir.join(".bananium-extracted");
    if marker.is_file() {
        return Ok(dir);
    }

    std::fs::create_dir_all(&dir)?;
    for component in NATIVE_COMPONENTS {
        std::fs::create_dir_all(dir.join(component))?;
    }
    for entry in natives {
        let jar_path = paths.store_blob(&entry.artifact.sha1);
        extract_jar(&jar_path, &dir, entry.component, &entry.exclude)?;
    }
    std::fs::write(&marker, b"")?;
    Ok(dir)
}

/// Unzip every entry of one native-library jar, **flattened to its
/// filename**, and make it discoverable from every location Bananium knows
/// a native-library loader might actually look: `root/<basename>` (the
/// flat layout older profiles' single shared `${natives_directory}`
/// expects) and `root/<component>/<basename>` for *every* entry in
/// [`NATIVE_COMPONENTS`] — not just this file's own classified component.
///
/// That last part is deliberately more paranoid than "put it only where it
/// nominally belongs," and it's paranoid for a concrete, tested reason:
/// `org.lwjgl.system.SharedLibraryExtractPath` turned out, empirically, to
/// be a write-target for LWJGL's *own* self-extraction from a natives jar
/// on the classpath — not a directory `Library.loadSystem()` scans for
/// already-present files. Since Bananium deliberately keeps natives jars
/// off the classpath (they're not real dependencies, just archives of
/// loose files), that self-extraction path never fires, and
/// `Library.loadSystem()` falls back to `java.library.path` alone. The
/// first version of this fix routed each file into only its "correct"
/// component subdirectory and reproduced the exact same crash
/// (`UnsatisfiedLinkError: Failed to locate library: liblwjgl.so`) with
/// `liblwjgl.so` sitting right there in `.../lwjgl/`, just not on
/// `java.library.path`. Given that, and without a documented guarantee
/// that JNA/Netty's own lookup is any less picky, the only actually
/// reliable fix is: every native file is reachable from every
/// native-loading property Bananium sets, at the cost of a few extra
/// hardlinks (same inode, so effectively free).
///
/// Flattening matters independent of all that: `java.library.path` and its
/// siblings are never searched recursively, only their top level. Some
/// native jars ship their `.so`/`.dll`/`.dylib` at the archive root (older
/// LWJGL), but modern ones (e.g. LWJGL 3.4's package-namespaced layout,
/// `linux/x64/org/lwjgl/liblwjgl.so`) nest it several directories deep —
/// preserving that structure on disk would make the library invisible to
/// the JVM regardless of which directory it's under.
///
/// Excludes are still matched against the *full* archive path (so
/// `META-INF/` continues to exclude nested manifest files), only the
/// destination path is flattened. Directory entries are skipped outright
/// (there's nothing to flatten them to).
fn extract_jar(jar_path: &Path, root: &Path, component: &str, exclude: &[String]) -> Result<()> {
    let file = std::fs::File::open(jar_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if name.ends_with('/') {
            continue;
        }
        if DEFAULT_EXCLUDES.iter().any(|p| name.starts_with(p))
            || exclude.iter().any(|p| name.starts_with(p.as_str()))
        {
            continue;
        }

        let Some(file_name) = Path::new(&name).file_name() else {
            continue;
        };
        let primary_path = root.join(component).join(file_name);
        if primary_path.is_file() {
            tracing::warn!(
                entry = %name, existing = %primary_path.display(),
                "two natives-jar entries flatten to the same filename; keeping the last one extracted"
            );
        }
        let mut out = std::fs::File::create(&primary_path)?;
        std::io::copy(&mut entry, &mut out)?;

        let mut link_targets = vec![root.join(file_name)];
        link_targets.extend(
            NATIVE_COMPONENTS
                .iter()
                .map(|c| root.join(c).join(file_name)),
        );
        for target in link_targets {
            if target == primary_path || target.exists() {
                continue;
            }
            if std::fs::hard_link(&primary_path, &target).is_err() {
                std::fs::copy(&primary_path, &target)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classpath::{native_component, ResolvedArtifact};
    use std::io::Write;

    fn make_test_jar(path: &Path, entries: &[(&str, &[u8])]) -> String {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, contents) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(contents).unwrap();
        }
        zip.finish().unwrap();

        let bytes = std::fs::read(path).unwrap();
        let mut hasher = Sha1::new();
        hasher.update(&bytes);
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// Puts a jar's blob in the store and returns a `NativesEntry` pointing
    /// at it, classified the same way `resolve_libraries` would.
    fn store_jar(
        paths: &Paths,
        dir: &Path,
        name: &str,
        group_artifact: &str,
        jar_entries: &[(&str, &[u8])],
    ) -> NativesEntry {
        let jar_path = dir.join(name);
        let sha1 = make_test_jar(&jar_path, jar_entries);
        let store_path = paths.store_blob(&sha1);
        std::fs::create_dir_all(store_path.parent().unwrap()).unwrap();
        std::fs::copy(&jar_path, &store_path).unwrap();
        NativesEntry {
            artifact: ResolvedArtifact {
                sha1,
                url: String::new(),
                size: 0,
                name: name.to_string(),
            },
            exclude: vec![],
            component: native_component(group_artifact),
        }
    }

    #[test]
    fn extracts_natives_and_skips_meta_inf() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let entry = store_jar(
            &paths,
            dir.path(),
            "natives.jar",
            "org.lwjgl:lwjgl",
            &[
                ("liblwjgl.so", b"pretend native library bytes"),
                ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\n"),
            ],
        );

        let natives_dir = extract_natives(&paths, &[entry]).unwrap();

        // Available at the shared root (older, single-directory profiles),
        // under its own classified component (LWJGL's own extract path),
        // AND under every other component — see `extract_jar`'s doc
        // comment for why "only its own component" isn't enough in
        // practice (that's exactly what caused the real crash this fixes).
        assert!(natives_dir.join("liblwjgl.so").is_file());
        for component in NATIVE_COMPONENTS {
            assert!(
                natives_dir.join(component).join("liblwjgl.so").is_file(),
                "missing under {component}"
            );
        }
        assert!(!natives_dir.join("META-INF").exists());
    }

    /// Regression test for the real crash this fixed: LWJGL 3.4's native
    /// jars nest their `.so` files several directories deep
    /// (`linux/x64/org/lwjgl/liblwjgl.so`), and `java.library.path` is
    /// never searched recursively — so extraction must flatten to the
    /// filename, not preserve the jar's internal layout.
    #[test]
    fn nested_jar_paths_are_flattened_to_their_filename() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let entry = store_jar(
            &paths,
            dir.path(),
            "lwjgl-natives.jar",
            "org.lwjgl:lwjgl",
            &[(
                "linux/x64/org/lwjgl/liblwjgl.so",
                b"pretend liblwjgl.so bytes",
            )],
        );

        let natives_dir = extract_natives(&paths, &[entry]).unwrap();

        assert!(natives_dir.join("liblwjgl.so").is_file());
        assert!(natives_dir.join("lwjgl").join("liblwjgl.so").is_file());
        assert!(!natives_dir.join("linux").exists());
    }

    /// Regression test for the real crash *after* the first fix: routing
    /// `liblwjgl.so` only into `.../lwjgl/` (its "correct" component)
    /// reproduced the identical `UnsatisfiedLinkError` on a real launch,
    /// because `-Djava.library.path=.../java` is the directory
    /// `Library.loadSystem()` actually searches — `SharedLibraryExtractPath`
    /// is not scanned for pre-existing files. So an LWJGL-classified file
    /// must land under `java` too, not only under `lwjgl`.
    #[test]
    fn lwjgl_classified_file_is_still_reachable_via_java_library_path() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let entry = store_jar(
            &paths,
            dir.path(),
            "lwjgl-natives.jar",
            "org.lwjgl:lwjgl",
            &[(
                "linux/x64/org/lwjgl/liblwjgl.so",
                b"pretend liblwjgl.so bytes",
            )],
        );

        let natives_dir = extract_natives(&paths, &[entry]).unwrap();

        assert!(natives_dir.join("java").join("liblwjgl.so").is_file());
    }

    /// Regression test for the second real bug: newer Mojang profiles pass
    /// `java.library.path`, `jna.tmpdir`,
    /// `org.lwjgl.system.SharedLibraryExtractPath`, and
    /// `io.netty.native.workdir` four *different* subdirectories of
    /// `${natives_directory}`, so every one of them must exist even when
    /// nothing routes a file there (JNA self-extracts into its own).
    #[test]
    fn every_native_component_directory_is_created_even_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let natives_dir = extract_natives(&paths, &[]).unwrap();

        for component in NATIVE_COMPONENTS {
            assert!(
                natives_dir.join(component).is_dir(),
                "{component} directory should exist"
            );
        }
    }

    #[test]
    fn re_extraction_is_skipped_once_marker_exists() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let entries: Vec<NativesEntry> = vec![];
        let natives_dir = extract_natives(&paths, &entries).unwrap();
        std::fs::write(natives_dir.join("sentinel"), b"keep me").unwrap();

        // Second call with the same (empty) entry set must hit the marker
        // and not wipe the directory.
        extract_natives(&paths, &entries).unwrap();
        assert!(natives_dir.join("sentinel").is_file());
    }
}
