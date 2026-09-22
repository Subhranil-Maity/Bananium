# Bananium: The Banana Launcher

A low-RAM, offline-first Minecraft launcher — TUI-first, built to grow more frontends.

## Context

There is no code yet: `/home/immortal/code/test/mc` is empty and not a git repo. The goal is a Minecraft **launcher/instance manager** that does what Prism Launcher and the Modrinth App do — Modrinth integration, modpacks, loaders, mod updates, world and server management — without their memory cost. The Modrinth App is Tauri-based and idles in the hundreds of MB; Prism is lighter but has no Modrinth-quality browsing and no offline catalog. Both also assume a live network: cold-start them on a plane and search, browsing, and often launching break.

Bananium is a Rust core with a **TUI as the primary interface**, a CLI beside it, and an explicit contract that makes a third or fourth frontend cheap. Everything it needs lives on disk, so a machine with no network still launches every installed instance and still browses every mod it has ever seen.

Game files come from **Mojang's official endpoints only**, including the Java runtimes Mojang itself publishes. Accounts are local-only: no auth server is contacted, and instances launch with a locally chosen username using the standard offline-UUID derivation that singleplayer, LAN, and offline-mode servers accept. Nothing bypasses ownership or entitlement checks.

Targets for v1: **Linux x86_64 + aarch64, Windows x86_64**. macOS and mobile are post-v1, but nothing in the core may assume a platform.

Budget to hold the design honest: **< 20 MB RSS for the CLI, < 40 MB for the TUI with a 500-mod instance open, < 8 MB for the detached launch supervisor.**

---

## The frontend contract

This is the load-bearing decision, so it comes first. Every frontend — the TUI, the CLI, a future GUI, a script — talks to exactly one crate, `bananium-api`, and talks to it in exactly one vocabulary: send a `Command`, receive a result and a stream of `Event`s.

```
                    bananium-api
        Session::dispatch(Command) -> Result
        Session::events() -> Stream<Event>
           |            |             |
     bananium-tui  bananium-cli  bananium-rpc  ← in-process / in-process / JSON over stdio
                                      |
                        future GUI, web UI, editor plugin  ← no Rust required
```

Four rules, enforced rather than hoped for:

1. **Frontend crates depend only on `bananium-api` plus their own UI libraries.** A CI check reads each frontend's `Cargo.toml` and fails if it names any other workspace crate. That single check is what actually keeps logic out of the UI.
2. **`Command` and `Event` are `serde` types.** They serialize for free, which is why an out-of-process frontend costs almost nothing.
3. **No frontend touches the filesystem or the network.** If a frontend needs something, it is a missing `Command`.
4. **All long-running work reports `Event::Progress { task_id, .. }`**, so every frontend renders progress, cancellation, and failure identically without inventing its own scheme.

`bananium-rpc` ships early, in M2, precisely to prove the seam works while it is still cheap to fix. Two frontends can share an accident; three cannot.

---

## Architecture

A Cargo workspace. All logic lives in UI-free libraries.

| Crate | Responsibility |
|---|---|
| `bananium-core` | Domain types, layered config, paths, error taxonomy |
| `bananium-api` | **The frontend facade**: `Session`, `Command`, `Event`, task registry |
| `bananium-net` | HTTP client, concurrent resumable download engine, checksum verification, rate limiting |
| `bananium-store` | Content-addressed blob store, hardlink/reflink materialization, GC |
| `bananium-meta` | Mojang piston-meta, asset indexes, library rules/natives, loader metadata |
| `bananium-java` | Mojang java-runtime provisioning, system JVM detection |
| `bananium-modrinth` | Modrinth v2 client + SQLite/FTS5 offline mirror |
| `bananium-instance` | Instance model, lockfile, mod graph, import/export formats |
| `bananium-launch` | Classpath/arg construction, JVM profiles, process supervision, crash analysis |
| `bananium-tui` | **ratatui frontend — the primary interface** |
| `bananium-cli` | clap frontend, `--format json` |
| `bananium-rpc` | JSON-RPC frontend over stdio / unix socket |

**Key dependencies:** `tokio`, `reqwest` (rustls), `serde`, `rusqlite` (bundled, FTS5), `ratatui` + `crossterm`, `clap` v4, `sha1`/`sha2`, `zip`, `zstd`, `xz2` (Mojang ships LZMA-compressed runtime files), `fastnbt`, `uuid`, `tracing`.

### How the RAM budget is actually met

Constraints, not aspirations — none of these may be traded away later:

1. **SQLite is the working set, not the heap.** The Modrinth mirror, instance index, and stats live in SQLite with FTS5. Search queries the database and pages results; the catalog is never deserialized wholesale.
2. **Streaming everywhere.** Downloads stream to disk through a hasher — a 400 MB modpack never materializes in RAM. Metadata is parsed with `from_reader` over a buffered file, never `from_str` over a `String`.
3. **Content-addressed store with hardlinks.** One copy of every library, asset, and mod jar at `store/<sha1[0..2]>/<sha1>`, hardlinked into each instance. Ten instances on one MC version cost one copy of the libraries. Try `FICLONE` reflink on btrfs/XFS; `CreateHardLinkW` on NTFS; copy across volumes.
4. **The launcher gets out of the way.** `--detach` spawns the JVM detached (double-fork on Unix, `DETACHED_PROCESS` on Windows) and exits. Attached mode keeps an ~8 MB supervisor with a bounded log ring buffer flushing to a rotating file.
5. **Bounded concurrency.** One shared `reqwest::Client` and a `Semaphore` (default 8), so peak memory scales with buffer size × 8, not with queue length.
6. **The TUI renders what is on screen.** Lists are virtualized and backed by SQL `LIMIT`/`OFFSET`; a 10,000-mod search result holds one screenful of rows.
7. **Release profile:** `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`.

---

## Data layout

One self-contained dot-folder, in the spirit of `.minecraft` — portable, easy to back up, easy to delete. `BANANIUM_HOME` overrides it; a `bananium/` directory beside the binary makes the whole install portable.

```
~/.bananium/
  config.toml
  index.db                       instances, playtime, Modrinth mirror, FTS5
  store/<ab>/<sha1>              content-addressed blobs (libs, assets, mod jars)
  java/<component>/              Mojang runtimes: java-runtime-delta/, jre-legacy/, ...
  meta/                          cached Mojang + loader metadata
  cache/http/                    ETag / Last-Modified cache
  instances/<slug>/
    instance.toml                name, mc version, loader, java override, JVM args, hooks
    bananium.lock.toml           resolved mods: project id, version id, sha512, side, enabled
    minecraft/                   the game dir (mods/, saves/, config/, ...)
    logs/
```

Config is layered: defaults → `config.toml` → per-instance `instance.toml` → environment → CLI flags.

---

## Official Mojang APIs

Only these, no third-party mirrors:

| Purpose | Endpoint |
|---|---|
| Release/snapshot list | `piston-meta.mojang.com/mc/game/version_manifest_v2.json` |
| Per-version profile | `piston-meta.mojang.com/v1/packages/<sha1>/<id>.json` |
| Client jar, libraries | `piston-data.mojang.com/...`, `libraries.minecraft.net/...` |
| Asset objects | `resources.download.minecraft.net/<hash[0..2]>/<hash>` |
| **Java runtimes** | `piston-meta.mojang.com/v1/products/java-runtime/<hash>/all.json` |

**Java the Mojang way.** Each version profile carries a `javaVersion: { component, majorVersion }` field — so the correct runtime is *read from the official metadata*, not guessed from a hardcoded version table. That component name (`jre-legacy`, `java-runtime-alpha`, `-beta`, `-gamma`, `-delta`) is looked up in the java-runtime manifest under the current platform key (`linux`, `linux-i386`, `windows-x64`, `windows-x86`, `windows-arm64`, …), which yields a per-component manifest describing a full file tree.

Implementing that tree correctly is the part people get wrong, so it is called out here: entries are typed `file`, `directory`, or `link`; files offer both `raw` and `lzma` downloads (prefer `lzma`, hence `xz2`); the `executable` flag must become `chmod +x` or the JVM will not start; `link` entries must be recreated as symlinks. Runtimes land in `~/.bananium/java/<component>/` and are shared across every instance that needs them.

Two documented gaps, both handled: Mojang publishes **no `linux-arm64` runtime**, so on aarch64 Linux fall back to the Adoptium v3 API; and an already-installed system JVM of the right major is detected (`JAVA_HOME`, `/usr/lib/jvm`, `PATH`, Windows registry) and offered before downloading anything.

---

## The TUI

The primary interface, not a viewer bolted onto a CLI. From M2 onward the standing rule is: **no feature is done until it has both a CLI verb and a TUI surface.**

**Screens**

- **Instances** — virtualized list/grid with groups, last played, playtime, and incremental filter. Enter launches; the log pane opens beside it.
- **Instance detail** — tabbed: Mods · Worlds · Servers · Screenshots · Settings · Logs.
- **Mod browser** — FTS5 search over the offline mirror with facet filters (category, MC version, loader), a detail pane rendering description and changelog, and single-key install/update/remove.
- **Task tray** — a persistent footer showing concurrent downloads with per-task progress, speed, and cancel.
- **Log pane** — live tail with level filtering and search, and the crash analyzer's verdict pinned as a banner at the top when the game dies.
- **Command palette** — `:` or `Ctrl-P`, fuzzy over every available `Command`. This is how the TUI stays complete without growing a menu for each new feature.

**Rules**

Vim-style keys with arrows as aliases, `?` for context help, mouse optional and never required. Must be usable at 80×24 and degrade cleanly without truecolor or unicode. All work is async — the UI never blocks on a download, and every screen is redrawn from `Event`s rather than polling. A banana-yellow default theme, themes as TOML.

---

## Milestones

Each ends in something runnable. M1 carries essentially all the risk; everything after it is additive.

### M0 — Skeleton and the contract
Workspace, `bananium-core` config/paths/errors, `tracing`, and the `Command`/`Event`/`Session` types in `bananium-api`. CI: fmt, clippy `-D warnings`, test, and the frontend-dependency check.
**Done when:** `bananium config show` prints resolved paths on Linux and Windows, and the dependency check fails on a deliberately bad import.

### M1 — Vanilla launch, offline *(the critical milestone)*
- **Download engine:** bounded concurrency, `Range` resume, SHA-1 verification, exponential backoff, atomic rename into the store, progress events.
- **Metadata:** version manifest → version profile. Evaluate library `rules` (os name/arch/version regex, feature flags), handle modern per-OS artifacts *and* legacy `natives` classifiers, resolve the asset index, fetch objects, support the `virtual` / `map_to_resources` legacy asset layouts. **1.13+ first-class, 1.6–1.12 supported, pre-1.6 out of scope.**
- **Launch:** classpath assembly (dedup by maven `group:artifact`, highest version wins), native extraction into a hash-keyed cache, and argument templating covering both the modern `arguments.game`/`arguments.jvm` rule arrays and the legacy `minecraftArguments` string — `${auth_player_name}`, `${auth_uuid}`, `${auth_access_token}`, `${version_name}`, `${game_directory}`, `${assets_root}`, `${assets_index_name}`, `${user_type}`, `${version_type}`, `${classpath}`, `${natives_directory}`.
- **Local profiles:** UUID v3 over `OfflinePlayer:<name>`, token `0`, `user_type=legacy`. Several named profiles, selectable per launch.
- `bananium launch --dry-run` prints the exact command line. This is the primary debugging tool for the rest of the project.

**Done when:** `bananium install 1.21.1 && bananium launch` reaches the main menu — and the same commands succeed with the network interface down.

### M2 — TUI v1, and a third frontend
The TUI shell: instance list, launch, live log pane, task tray, command palette, help, theming. Everything M1 can do is reachable without typing a command.

Alongside it, `bananium-rpc`: the same `Command`/`Event` pair over JSON-RPC on stdio. It is a few hundred lines once the types are `serde`, and it proves the contract holds before M3–M6 pile features onto it.
**Done when:** the TUI launches an instance end to end, and `echo '{"method":"launch",...}' | bananium rpc` does the same thing.

### M3 — Instances, Java, diagnostics
- `instance new|ls|clone|rm|rename|export`, groups and tags, last-played and playtime. Cloning is hardlink-based and effectively instant.
- Java provisioning per the section above: read `javaVersion.component` from the profile, resolve through Mojang's manifest, materialize the file tree into `~/.bananium/java/`, with system-JVM detection and the aarch64 Adoptium fallback.
- **JVM profiles:** a tuned G1 default, optional Generational ZGC on 21+, and a RAM auto-sizer defaulting `Xmx` to `min(4G, 40% of system RAM)` adjusted by mod count. On this 7.5 GB machine that lands near 3 GB rather than the 8 GB people paste from forum posts.
- **Crash analyzer:** match crash reports and `latest.log` against known signatures — missing dependency, `MixinApplyError`, wrong Java major, `OutOfMemoryError`, missing native, incompatible loader — and surface the actionable cause instead of a stack trace. The single biggest quality-of-life win over other launchers, and the reason the TUI log pane has a banner slot.
- Pre/post-launch hooks, per-instance env vars, custom Java path, wrapper command (`gamemoderun`, `prime-run`, MangoHud).

### M4 — Modrinth, online and off
- v2 client at `api.modrinth.com/v2` with a **compliant `User-Agent`** (`<repo>/bananium/<version> (<contact>)`) — Modrinth rate-limits generic agents — and a token bucket honoring `X-Ratelimit-*` under the 300 req/min ceiling.
- `/search` with facets, `/project/{id}`, `/project/{id}/version`, `/version_files` (hash → version) and `/version_files/update` (bulk update check).
- **Offline mirror:** every project and version seen is persisted to SQLite with FTS5 over title/description/categories. `bananium sync` pre-caches chosen categories and MC versions. Search, browse, and changelogs work with no network; actions taken offline queue and replay.
- `mod add|rm|update|ls|disable` writing `bananium.lock.toml` — reproducible, git-friendly, diffable.
- Dependency resolution over `required`/`optional`/`incompatible`/`embedded`, with conflict detection *before* anything is written, and side filtering so server-only mods never land in a client instance.
- `mod adopt`: hash unknown jars in `mods/` and identify them via `/version_files`, pulling hand-dropped jars into the lockfile.
- `mod update --preview` shows changelogs first; the store retains N prior versions so `mod rollback` is instant.
- `instance bump 1.21.1`: report which mods have a compatible version and which block the upgrade, before touching anything.
- TUI: the mod browser screen.

### M5 — Modpacks and the easy loaders
- `.mrpack` import and export: `modrinth.index.json` (formatVersion 1, `files[]` with `downloads[]`/`hashes`/`env`), `overrides/`, `client-overrides/`. Verify every hash; honor `env.client = unsupported`.
- Import Prism/MultiMC instance zips and CurseForge zips.
- **Fabric** (`meta.fabricmc.net/v2`) and **Quilt** (`meta.quiltmc.org/v3`) — both return ready-made launcher JSON that merges into the vanilla profile.

### M6 — The hard loaders, and content managers
- **NeoForge** (`maven.neoforged.net`) and **Forge** ship an installer jar whose `install_profile.json` declares *processors* that must actually run on a JVM to binary-patch the client. This is the hardest part of the project, which is why it is last: processor execution, artifact resolution, and the legacy pre-1.13 installer shapes are each separate work.
- Worlds, resource packs, shader packs, screenshots. Server list read/write via NBT (`servers.dat`).
- World backups: zstd snapshots with retention, and restore.

### M7 — Packaging
Linux tarball, AppImage, `.deb`/`.rpm`, AUR `PKGBUILD`, aarch64 via `cross`. Windows portable `.exe` plus a per-user MSI/NSIS installer. Signed self-update, GitHub Actions release matrix, shell completions, man page.

---

## Verification

**Automated**
- Unit tests over recorded Mojang fixtures: library rule evaluation per OS/arch, native classifier selection, classpath dedup ordering, argument templating for both modern and legacy shapes, and java-runtime tree materialization including `link` entries and the `executable` flag.
- **Golden-file tests on `--dry-run`.** Freeze the full command line for a matrix of (MC version × loader × OS) and diff. Highest-value test in the project: it catches launch regressions without starting a JVM.
- `wiremock` for all HTTP — piston-meta, java-runtime, Modrinth, Adoptium — including rate-limit headers, 5xx retry, and truncated-response resume.
- Store: hardlink dedup, reflink path, cross-volume copy fallback, GC with concurrent readers.
- Crash analyzer: a corpus of real crash reports asserted against expected diagnoses.
- TUI: snapshot tests via `ratatui`'s `TestBackend` for each screen at 80×24 and 200×50.
- The frontend-dependency check, as its own CI job.
- CI matrix: Linux x86_64 + aarch64, Windows x86_64.

**Manual, per milestone**
1. `bananium install 1.21.1 && bananium launch --dry-run` → read the command line by eye.
2. `bananium launch` → main menu; create and save a world.
3. **Offline gate:** bring the interface down, relaunch the instance, run a search, open a synced project. Everything must work. Any command that silently needs the network is a bug.
4. **RAM gate:** `ps -o rss=` for the CLI idle, the TUI with a 500-mod instance open, a 500-mod install, and after `--detach`. Check against 20/40/8 MB **every milestone** — a budget checked only at the end is a budget already blown.
5. **Java gate:** delete `~/.bananium/java/`, launch a 1.21 instance and a 1.8 instance, and confirm each pulls the component its profile names (`java-runtime-delta`, `jre-legacy`) and that the extracted `java` binary is executable.
6. Import a real `.mrpack` (e.g. Fabulously Optimized), launch it, export it, diff the round trip.
7. Dedup gate: clone an instance three times; `du -sh` barely moves and `stat` shows shared inodes.
8. Windows: the same sequence in PowerShell — path handling, NTFS hardlinks, registry JVM detection, and the TUI under Windows Terminal.
