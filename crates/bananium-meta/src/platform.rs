/// The current OS/arch, and a best-effort OS version string, as used to
/// evaluate Mojang's library and argument `rules`. Fully injectable so rule
/// evaluation can be unit-tested without depending on the machine running
/// the tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    /// Mojang's spelling: "windows", "osx", or "linux".
    pub os_name: String,
    /// Mojang's spelling: "x86" (32-bit) or "x86_64"/"arm64".
    pub arch: String,
    /// Best-effort kernel/OS version string, matched against `os.version`
    /// regexes. Only a handful of rules (mostly Windows 10 detection) ever
    /// check this.
    pub os_version: String,
}

impl Platform {
    /// Detect the real platform this process is running on, mapping Rust's
    /// `std::env::consts` names to the spellings Mojang's profiles use.
    pub fn current() -> Self {
        let os_name = match std::env::consts::OS {
            "macos" => "osx",
            "windows" => "windows",
            other => other, // "linux", and BSDs fall back to linux-shaped libraries
        }
        .to_string();

        let arch = match std::env::consts::ARCH {
            "x86" => "x86",
            "aarch64" => "arm64",
            other => other, // "x86_64" as-is
        }
        .to_string();

        Self {
            os_name,
            arch,
            os_version: detect_os_version(),
        }
    }

    /// 32 vs 64-bit, for substituting `${arch}` into legacy natives classifier keys.
    pub fn arch_bits(&self) -> &'static str {
        if self.arch == "x86" {
            "32"
        } else {
            "64"
        }
    }
}

/// Best-effort kernel/OS version string via `uname -r` on Unix; empty (and
/// therefore never matching an `os.version` regex) on any other platform or
/// if the command fails. Good enough because the only rules that check this
/// target Windows specifically, where this always returns empty and those
/// rules correctly never match on Unix.
fn detect_os_version() -> String {
    #[cfg(unix)]
    {
        if let Ok(output) = std::process::Command::new("uname").arg("-r").output() {
            if output.status.success() {
                return String::from_utf8_lossy(&output.stdout).trim().to_string();
            }
        }
    }
    String::new()
}
