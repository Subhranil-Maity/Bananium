use std::collections::HashMap;
use std::path::PathBuf;

use bananium_meta::{FeatureFlags, Platform, VersionProfile};
use uuid::Uuid;

/// Everything about *this* launch that isn't baked into the version
/// profile: who's playing, and where things landed on disk.
#[derive(Debug, Clone)]
pub struct LaunchContext {
    pub player_name: String,
    pub player_uuid: Uuid,
    /// `"0"` for every offline launch — Bananium never talks to an auth server.
    pub access_token: String,
    /// `"legacy"` for every offline launch, per the offline-UUID convention
    /// singleplayer/LAN/offline-mode servers all accept.
    pub user_type: String,
    /// Passed as `--gameDir`/`${game_directory}`; this is the actual
    /// `minecraft/` instance directory, not the instance root.
    pub game_directory: PathBuf,
    /// Passed as `--assetsDir`/`${assets_root}`; the shared, Mojang-shaped
    /// assets tree at `Paths::assets_dir()`.
    pub assets_root: PathBuf,
    /// Passed as `${natives_directory}` (and the several other
    /// natives-path JVM properties); the per-jar-set directory
    /// `extract_natives` extracted into.
    pub natives_directory: PathBuf,
    /// Library jars plus the client jar, in final classpath order.
    pub classpath: Vec<PathBuf>,
    /// Substituted into `${launcher_name}`/`-Dminecraft.launcher.brand`.
    pub launcher_name: String,
    /// Substituted into `${launcher_version}`/`-Dminecraft.launcher.version`.
    pub launcher_version: String,
    /// `-Xmx<ram_mb>M` heap cap; `None` leaves the JVM's own default in
    /// place. From `InstanceConfig::ram_mb`.
    pub ram_mb: Option<u32>,
    /// The instance's own extra JVM arguments (`InstanceConfig::jvm_args`),
    /// appended after every other JVM argument this module generates —
    /// see [`build_launch_plan`]'s doc comment for why append-only is the
    /// only placement that keeps these interpreted as JVM flags at all.
    pub extra_jvm_args: Vec<String>,
}

/// A fully-resolved, ready-to-run launch: the exact JVM binary, arguments,
/// and working directory, with every `${...}` placeholder already
/// substituted. What `--dry-run` prints and what a real launch executes are
/// the exact same value — that's the whole point of this type.
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub java_bin: PathBuf,
    pub jvm_args: Vec<String>,
    pub main_class: String,
    pub game_args: Vec<String>,
    pub working_dir: PathBuf,
}

impl LaunchPlan {
    /// A `std::process::Command` ready to spawn. No shell is involved, so
    /// arguments never need quoting here — only `command_line()` (for
    /// `--dry-run` display) does.
    pub fn to_command(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(&self.java_bin);
        cmd.args(&self.jvm_args)
            .arg(&self.main_class)
            .args(&self.game_args)
            .current_dir(&self.working_dir);
        cmd
    }

    /// A human-readable, shell-quoted rendering of the exact command line —
    /// the primary debugging tool for the rest of the project, per
    /// `bananium launch --dry-run`.
    pub fn command_line(&self) -> String {
        let mut parts = vec![shell_quote(&self.java_bin.to_string_lossy())];
        parts.extend(self.jvm_args.iter().map(|a| shell_quote(a)));
        parts.push(shell_quote(&self.main_class));
        parts.extend(self.game_args.iter().map(|a| shell_quote(a)));
        parts.join(" ")
    }
}

/// POSIX-shell-single-quote `s` if it contains anything that would need
/// escaping, otherwise return it bare. This is purely cosmetic, for
/// [`LaunchPlan::command_line`]'s human-readable `--dry-run` output — the
/// real spawn path ([`LaunchPlan::to_command`]) never goes through a shell,
/// so it never needs this.
fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=$,{}".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// Build the full JVM + game argument list, handling both the modern
/// (1.13+) `arguments.game`/`arguments.jvm` rule arrays and the legacy
/// `minecraftArguments` string — substituting every `${...}` placeholder
/// Mojang profiles reference.
///
/// `ctx.ram_mb` (as `-Xmx<n>M`) and then `ctx.extra_jvm_args` verbatim are
/// appended *last*, after every JVM argument generated above. That ordering
/// is deliberate, not incidental: everything on `jvm_args` up to that point
/// must come before the main class or the JVM parses it as a game argument
/// instead — appending is the only placement where a user's custom flag
/// still reaches the JVM at all, and appending last (rather than first)
/// means a custom flag can override an auto-generated one (e.g. a
/// different GC) since the JVM honors the last repeated flag.
pub fn build_launch_plan(
    profile: &VersionProfile,
    platform: &Platform,
    features: &FeatureFlags,
    ctx: &LaunchContext,
    java_bin: PathBuf,
) -> LaunchPlan {
    let vars = template_vars(profile, ctx);
    let mut jvm_args = Vec::new();
    let mut game_args = Vec::new();

    if let Some(args) = &profile.arguments {
        for arg in &args.jvm {
            if let Some(tokens) = arg.resolve(platform, features) {
                jvm_args.extend(tokens.iter().map(|t| substitute(t, &vars)));
            }
        }
        for arg in &args.game {
            if let Some(tokens) = arg.resolve(platform, features) {
                game_args.extend(tokens.iter().map(|t| substitute(t, &vars)));
            }
        }
    } else {
        // Pre-1.13 profiles carry no `arguments.jvm`; synthesize the
        // handful of flags every legacy launcher passed by hand.
        jvm_args.push(substitute(
            "-Djava.library.path=${natives_directory}",
            &vars,
        ));
        jvm_args.push("-cp".to_string());
        jvm_args.push(substitute("${classpath}", &vars));

        if let Some(legacy) = &profile.minecraft_arguments {
            let substituted = substitute(legacy, &vars);
            game_args.extend(substituted.split_whitespace().map(str::to_string));
        }
    }

    if let Some(ram_mb) = ctx.ram_mb {
        jvm_args.push(format!("-Xmx{ram_mb}M"));
    }
    jvm_args.extend(ctx.extra_jvm_args.iter().cloned());

    LaunchPlan {
        java_bin,
        jvm_args,
        main_class: profile.main_class.clone(),
        game_args,
        working_dir: ctx.game_directory.clone(),
    }
}

/// Build the `${key}` -> value map every argument token gets substituted
/// against. Covers both the placeholders PLAN.md enumerates and a few more
/// that real Mojang profiles reference (`launcher_name`, `clientid`,
/// `resolution_width`/`height`, ...) so `--dry-run` output never shows an
/// unresolved `${...}` for a real profile.
fn template_vars(profile: &VersionProfile, ctx: &LaunchContext) -> HashMap<String, String> {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let classpath = ctx
        .classpath
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(sep);

    let mut vars = HashMap::new();
    vars.insert("auth_player_name".to_string(), ctx.player_name.clone());
    vars.insert("auth_uuid".to_string(), ctx.player_uuid.to_string());
    vars.insert("auth_access_token".to_string(), ctx.access_token.clone());
    vars.insert("user_type".to_string(), ctx.user_type.clone());
    vars.insert("version_name".to_string(), profile.id.clone());
    vars.insert("version_type".to_string(), profile.version_type.clone());
    vars.insert(
        "game_directory".to_string(),
        ctx.game_directory.to_string_lossy().to_string(),
    );
    vars.insert(
        "assets_root".to_string(),
        ctx.assets_root.to_string_lossy().to_string(),
    );
    vars.insert("assets_index_name".to_string(), profile.assets.clone());
    vars.insert(
        "natives_directory".to_string(),
        ctx.natives_directory.to_string_lossy().to_string(),
    );
    vars.insert("classpath".to_string(), classpath);
    vars.insert("launcher_name".to_string(), ctx.launcher_name.clone());
    vars.insert("launcher_version".to_string(), ctx.launcher_version.clone());
    // Real placeholders on some profiles/flags that Bananium doesn't drive
    // yet (auth is always offline; window size isn't configurable in M1).
    vars.insert("clientid".to_string(), String::new());
    vars.insert("auth_xuid".to_string(), String::new());
    vars.insert("resolution_width".to_string(), "925".to_string());
    vars.insert("resolution_height".to_string(), "530".to_string());
    vars
}

/// Replace every `${key}` occurrence in `token` using `vars`, leaving
/// unrecognized placeholders untouched rather than panicking — an unknown
/// key is far more useful to see in `--dry-run` output than to swallow.
fn substitute(token: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = token;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match vars.get(key) {
                    Some(value) => out.push_str(value),
                    None => {
                        out.push_str("${");
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str("${");
                rest = after;
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> LaunchContext {
        LaunchContext {
            player_name: "Player".into(),
            player_uuid: Uuid::nil(),
            access_token: "0".into(),
            user_type: "legacy".into(),
            game_directory: PathBuf::from("/home/test/.bananium/instances/1.21.1/minecraft"),
            assets_root: PathBuf::from("/home/test/.bananium/assets"),
            natives_directory: PathBuf::from("/home/test/.bananium/cache/natives/abc123"),
            classpath: vec![
                PathBuf::from("/store/aa/aaaa"),
                PathBuf::from("/store/bb/bbbb"),
            ],
            launcher_name: "bananium".into(),
            launcher_version: "0.1.0".into(),
            ram_mb: None,
            extra_jvm_args: Vec::new(),
        }
    }

    fn platform() -> Platform {
        Platform {
            os_name: "linux".into(),
            arch: "x86_64".into(),
            os_version: String::new(),
        }
    }

    #[test]
    fn ram_and_extra_jvm_args_are_appended_after_every_generated_jvm_arg() {
        let profile: VersionProfile = serde_json::from_str(
            r#"{
                "id": "1.21.1",
                "type": "release",
                "mainClass": "net.minecraft.client.main.Main",
                "assetIndex": {"id":"17","sha1":"a","size":1,"url":"http://x/17.json"},
                "assets": "17",
                "downloads": {"client": {"sha1":"b","size":1,"url":"http://x/client.jar"}},
                "libraries": [],
                "arguments": {
                    "game": ["--username", "${auth_player_name}"],
                    "jvm": ["-Djava.library.path=${natives_directory}"]
                }
            }"#,
        )
        .unwrap();

        let mut context = ctx();
        context.ram_mb = Some(3072);
        context.extra_jvm_args = vec!["-XX:+UseG1GC".to_string()];

        let plan = build_launch_plan(
            &profile,
            &platform(),
            &FeatureFlags::default(),
            &context,
            PathBuf::from("java"),
        );
        assert_eq!(
            plan.jvm_args,
            vec![
                "-Djava.library.path=/home/test/.bananium/cache/natives/abc123".to_string(),
                "-Xmx3072M".to_string(),
                "-XX:+UseG1GC".to_string(),
            ]
        );
    }

    #[test]
    fn modern_arguments_are_substituted_and_filtered_by_rules() {
        let profile: VersionProfile = serde_json::from_str(
            r#"{
                "id": "1.21.1",
                "type": "release",
                "mainClass": "net.minecraft.client.main.Main",
                "assetIndex": {"id":"17","sha1":"a","size":1,"url":"http://x/17.json"},
                "assets": "17",
                "downloads": {"client": {"sha1":"b","size":1,"url":"http://x/client.jar"}},
                "libraries": [],
                "arguments": {
                    "game": ["--username", "${auth_player_name}", {"rules":[{"action":"allow","features":{"is_demo_user":true}}],"value":"--demo"}],
                    "jvm": ["-Djava.library.path=${natives_directory}", "-cp", "${classpath}"]
                }
            }"#,
        )
        .unwrap();

        let plan = build_launch_plan(
            &profile,
            &platform(),
            &FeatureFlags::default(),
            &ctx(),
            PathBuf::from("java"),
        );
        assert_eq!(plan.game_args, vec!["--username", "Player"]);
        assert_eq!(
            plan.jvm_args[0],
            "-Djava.library.path=/home/test/.bananium/cache/natives/abc123"
        );
        assert_eq!(plan.jvm_args[2], "/store/aa/aaaa:/store/bb/bbbb");
        assert_eq!(plan.main_class, "net.minecraft.client.main.Main");
    }

    #[test]
    fn legacy_minecraft_arguments_string_is_split_after_substitution() {
        let profile: VersionProfile = serde_json::from_str(
            r#"{
                "id": "1.8.9",
                "type": "release",
                "mainClass": "net.minecraft.client.main.Main",
                "assetIndex": {"id":"1.8","sha1":"a","size":1,"url":"http://x/1.8.json"},
                "assets": "1.8",
                "downloads": {"client": {"sha1":"b","size":1,"url":"http://x/client.jar"}},
                "libraries": [],
                "minecraftArguments": "--username ${auth_player_name} --version ${version_name} --gameDir ${game_directory} --assetsDir ${assets_root} --assetIndex ${assets_index_name} --uuid ${auth_uuid} --accessToken ${auth_access_token} --userType ${user_type} --versionType ${version_type}"
            }"#,
        )
        .unwrap();

        let plan = build_launch_plan(
            &profile,
            &platform(),
            &FeatureFlags::default(),
            &ctx(),
            PathBuf::from("java"),
        );
        assert_eq!(
            plan.jvm_args[0],
            "-Djava.library.path=/home/test/.bananium/cache/natives/abc123"
        );
        assert_eq!(plan.jvm_args[1], "-cp");
        assert_eq!(
            plan.game_args,
            vec![
                "--username",
                "Player",
                "--version",
                "1.8.9",
                "--gameDir",
                "/home/test/.bananium/instances/1.21.1/minecraft",
                "--assetsDir",
                "/home/test/.bananium/assets",
                "--assetIndex",
                "1.8",
                "--uuid",
                "00000000-0000-0000-0000-000000000000",
                "--accessToken",
                "0",
                "--userType",
                "legacy",
                "--versionType",
                "release",
            ]
        );
    }

    #[test]
    fn command_line_quotes_arguments_with_spaces() {
        let plan = LaunchPlan {
            java_bin: PathBuf::from("/usr/bin/java"),
            jvm_args: vec!["-Xmx2G".to_string()],
            main_class: "net.minecraft.client.main.Main".to_string(),
            game_args: vec!["--username".to_string(), "Player One".to_string()],
            working_dir: PathBuf::from("/home/test"),
        };
        assert_eq!(
            plan.command_line(),
            "/usr/bin/java -Xmx2G net.minecraft.client.main.Main --username 'Player One'"
        );
    }
}
