//! Mojang java-runtime provisioning and system JVM detection.
//!
//! [`runtime`] models Mojang's official runtimes (`javaVersion.component`
//! -> the java-runtime manifest -> a full file tree with `link`/`executable`
//! handling) and lays them out on disk; downloading them is `bananium-api`'s
//! job. The rest of this crate detects JVMs already on the machine, for
//! users who pick their own. There is no Adoptium fallback for platforms
//! Mojang doesn't publish runtimes for (such as aarch64 Linux).

pub mod runtime;

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "no usable java runtime was found (checked JAVA_HOME, PATH, and common install locations)"
    )]
    NotFound,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("java runtime manifest has an unsafe path {0:?}")]
    BadRuntimePath(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A JVM found on this machine: where it lives, and which major version it
/// reports (via `java -version`).
#[derive(Debug, Clone)]
pub struct JavaCandidate {
    pub path: PathBuf,
    pub major_version: u32,
}

/// The platform-appropriate executable name to look for.
fn java_exe_name() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

/// Find a usable JVM, preferring (in order): an explicit override, `JAVA_HOME`,
/// `PATH`, and a handful of well-known install locations.
pub fn find_java(override_path: Option<&Path>) -> Result<JavaCandidate> {
    if let Some(path) = override_path {
        if let Some(candidate) = probe(path) {
            return Ok(candidate);
        }
    }

    if let Ok(home) = std::env::var("JAVA_HOME") {
        let candidate_path = PathBuf::from(home).join("bin").join(java_exe_name());
        if let Some(candidate) = probe(&candidate_path) {
            return Ok(candidate);
        }
    }

    if let Some(resolved) = resolve_from_path(java_exe_name()) {
        if let Some(candidate) = probe(&resolved) {
            return Ok(candidate);
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/usr/lib/jvm") {
            for entry in entries.flatten() {
                let candidate_path = entry.path().join("bin").join("java");
                if let Some(candidate) = probe(&candidate_path) {
                    return Ok(candidate);
                }
            }
        }
    }

    Err(Error::NotFound)
}

/// Every JVM that can be found on this machine, deduplicated, for a
/// frontend's Java picker: `JAVA_HOME`, every `PATH` hit (not just the
/// first), and the usual per-OS install folders (including IntelliJ's
/// `~/.jdks`). Each is probed with `java -version`, so this takes a moment
/// per candidate — call it on demand, not on every launch.
pub fn find_all_java() -> Vec<JavaCandidate> {
    let exe = java_exe_name();
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Ok(home) = std::env::var("JAVA_HOME") {
        paths.push(PathBuf::from(home).join("bin").join(exe));
    }
    if let Some(path_var) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path_var).map(|dir| dir.join(exe)));
    }

    // Parent folders whose children are JDK/JRE installs.
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        roots.push(PathBuf::from(home).join(".jdks"));
    }
    if cfg!(windows) {
        for base in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(pf) = std::env::var_os(base) {
                let pf = PathBuf::from(pf);
                for vendor in [
                    "Java",
                    "Eclipse Adoptium",
                    "Microsoft",
                    "Zulu",
                    "Amazon Corretto",
                    "BellSoft",
                ] {
                    roots.push(pf.join(vendor));
                }
            }
        }
    } else if cfg!(target_os = "macos") {
        roots.push(PathBuf::from("/Library/Java/JavaVirtualMachines"));
    } else {
        roots.push(PathBuf::from("/usr/lib/jvm"));
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            // macOS bundles keep the JDK under Contents/Home.
            paths.push(dir.join("bin").join(exe));
            paths.push(dir.join("Contents").join("Home").join("bin").join(exe));
        }
    }

    let mut seen = std::collections::HashSet::new();
    let mut found: Vec<JavaCandidate> = paths
        .into_iter()
        .filter(|p| p.is_file())
        .filter(|p| seen.insert(std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())))
        .filter_map(|p| probe(&p))
        .collect();
    found.sort_by_key(|c| std::cmp::Reverse(c.major_version));
    found
}

/// Probe one user-chosen path (e.g. from a file picker), returning its
/// version if it's a runnable `java`.
pub fn probe_java(path: &Path) -> Option<JavaCandidate> {
    probe(path)
}

/// Search every directory on `PATH` for `exe_name`, returning the first hit
/// — a manual re-implementation of shell `which` so this crate doesn't need
/// an extra dependency just for that.
fn resolve_from_path(exe_name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(exe_name))
        .find(|candidate| candidate.is_file())
}

/// Run `<path> -version` and, if it succeeds and its banner parses, return
/// the candidate. Doubles as the existence check — a `path` that isn't a
/// real, runnable `java` binary simply yields `None` here.
fn probe(path: &Path) -> Option<JavaCandidate> {
    let output = std::process::Command::new(path)
        .arg("-version")
        .output()
        .ok()?;
    // `java -version` writes its banner to stderr, e.g. `openjdk version "21.0.7" 2025-04-15`.
    let text = String::from_utf8_lossy(&output.stderr);
    let major_version = parse_major_version(&text)?;
    Some(JavaCandidate {
        path: path.to_path_buf(),
        major_version,
    })
}

/// Extract the major version from a `java -version` banner. Handles both
/// the modern scheme (`"21.0.7"` -> 21) and the pre-Java-9 scheme
/// (`"1.8.0_392"` -> 8, since the real major version was always the *second*
/// component back when every release started with `1.`).
fn parse_major_version(version_output: &str) -> Option<u32> {
    let start = version_output.find('"')? + 1;
    let rest = &version_output[start..];
    let end = rest.find('"')?;
    let version_str = &rest[..end];

    let mut parts = version_str.split(['.', '-']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        // Pre-Java-9 scheme: "1.8.0_392" means major version 8.
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modern_version_scheme() {
        assert_eq!(
            parse_major_version("openjdk version \"21.0.7\" 2025-04-15\n"),
            Some(21)
        );
        assert_eq!(
            parse_major_version("openjdk version \"26.0.2.1\" 2026-08-18\n"),
            Some(26)
        );
    }

    #[test]
    fn parses_legacy_1_dot_x_version_scheme() {
        assert_eq!(parse_major_version("java version \"1.8.0_392\"\n"), Some(8));
    }

    #[test]
    fn finds_the_real_java_on_this_machine() {
        // This test only makes sense on a dev machine with a JDK installed,
        // which every CI runner and this workspace's dev environment has.
        let found = find_java(None);
        assert!(
            found.is_ok(),
            "expected to find a JVM via JAVA_HOME/PATH: {found:?}"
        );
        assert!(found.unwrap().major_version >= 8);
    }

    #[test]
    fn find_all_includes_what_find_java_finds() {
        let first = find_java(None).unwrap();
        let first = std::fs::canonicalize(&first.path).unwrap();
        let all: Vec<PathBuf> = find_all_java()
            .iter()
            .map(|c| std::fs::canonicalize(&c.path).unwrap())
            .collect();
        assert!(all.contains(&first), "{first:?} missing from {all:?}");
    }
}
