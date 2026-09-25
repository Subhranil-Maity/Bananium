# Agent guide: Bananium

This is the orientation document for any agent (or human) picking up this
repo cold. Read this first; read `PLAN.md` second for the full milestone-by-
milestone roadmap and design rationale. This file tells you *where things
are and what to watch out for*; `PLAN.md` tells you *why the project is
shaped this way and what's coming next*.

## What this is

Bananium is a low-RAM, offline-first Minecraft launcher: a Rust core with a
TUI as the primary interface and a CLI beside it, talking only to official
Mojang endpoints, with local-only (offline-mode) accounts. See `PLAN.md`'s
top section for the full pitch and RAM budgets.

## Current state (read this before assuming something is done)

**M0 (skeleton + contract) and M1 (vanilla launch, offline) are complete and
verified against real Mojang endpoints and a real JVM** — not just unit
tested. M2/M3/M4 are still mostly not implemented, but several pieces of
them exist ahead of schedule, built on direct request rather than in
milestone order — see the table below for exactly what each covers:

- `bananium-tui` is a real (if intentionally minimal) ratatui app now, not
  the `BOILER PLATE` stub PLAN.md's milestone order would suggest: an
  instance list with running status, launch, quit, refresh, and an edit
  overlay (`e`) over `Command::InstanceSet` for a selected instance's RAM
  cap and extra JVM args. A launched instance's JVM stdout/stderr are
  redirected to a per-launch file under `instance_logs_dir` (never inherited
  from the frontend) so the raw-mode/alternate-screen UI can't be corrupted
  by game log spam — see `Session::launch`'s doc comment in
  `bananium-api/src/session.rs`. The *live* log pane (tailing that file in
  its own pane while the game runs), task tray, and command palette PLAN.md
  describes for the rest of M2 don't exist yet.
- **`desktop/` is the graphical frontend**: a Tauri 2 app (crate
  `bananium-desktop` in `desktop/src-tauri`, React + TypeScript + Vite +
  shadcn/ui in `desktop/src`). It replaced the old egui GUI (deleted, along
  with `bananium --gui`), built on direct request, ahead of any PLAN.md
  milestone. The Rust side is a thin bridge: one Tauri command,
  `dispatch(Command) -> CommandOutput`, and every `Event` re-emitted to the
  webview as `bananium://event`. Screens: Library (grid/list, group/sort),
  instance page (Content — mods/resource packs/shaders — Files, Logs,
  Screenshots, Settings), Browse (Modrinth), Presets, Screenshots (all
  instances), Accounts, Settings. The webview's native right-click menu is
  replaced app-wide by `GlobalContextMenu`; component menus (Radix
  `ContextMenu`) `preventDefault` first and take precedence.
  Rail/tooltip gotcha: never give a Radix `asChild` child (e.g. `NavLink`)
  a function-valued `className` — `Slot` stringifies it.
  TypeScript types in `desktop/src/bindings/` are **generated** from the
  Rust types by ts-rs (see "Building and verifying") — never hand-edit them.
  **Use bun, never npm**, for everything in `desktop/`.
- Also built on direct request, ahead of PLAN.md's M3–M6 order, to back the
  GUI (each is a `Command` any frontend can use):
  - **Fabric** (only loader supported): `bananium-meta/src/fabric.rs` merges
    Fabric's profile into vanilla's; `install --fabric <ver|latest>`.
  - **Content** (`bananium-instance/src/content.rs` + `bananium-api/src/
    session/content.rs`): Modrinth search/project/versions, install with
    required-dependency resolution, toggle/remove/import/identify, update
    checks. The `bananium.lock.toml` lockfile holds metadata; the files in
    `mods/`/`resourcepacks/`/`shaderpacks/` are the source of truth.
  - **Presets** (`bananium-instance/src/presets.rs`): save an instance's
    Modrinth projects, apply to other instances (versions re-picked per
    target).
  - Offline account management (`Profile*`), instance rename/clone/remove/
    kill, log reading, screenshots, `ConfigSet`, Java listing.
- `bananium-rpc` is still a one-line dummy binary that prints
  `BOILER PLATE`. It exists so the workspace and the frontend-dependency
  check have something to check against, nothing more.
- `bananium-modrinth` is a tested Modrinth v2 REST client, used by
  `bananium-api` for search/install/updates. The SQLite/FTS5 offline
  mirror M4 calls for is **not** built — Modrinth browsing needs a network.
- `bananium-instance`: named instances, RAM/JVM-arg/Java overrides,
  library groups, custom icons (`icon-<millis>.<ext>` so each change gets a
  new URL), clone/rename/remove, pid-based running tracking (`running.toml`
  records each run's start time and log; liveness checked on Linux and
  Windows), play stats (`stats.toml`: last played + playtime — exact on exit
  in a long-lived session, estimated from the log's mtime when a dead run is
  swept), the content lockfile, and presets. Still missing: export/import
  formats, tags.
- `File*` commands (`bananium-api/src/session/files.rs`) are a file manager
  over an instance's game directory; `resolve_game_path` is the single gate
  that refuses anything escaping it.
- `bananium-java` only does system-JVM detection (`JAVA_HOME`/`PATH`/
  `/usr/lib/jvm`). Mojang java-runtime auto-provisioning and the aarch64
  Adoptium fallback are M3 work and are **not** implemented — if no system
  JVM is found, `bananium launch` just errors.
- No golden-file dry-run test matrix yet (PLAN.md calls this out as the
  highest-value test in the project — it doesn't exist yet, only ad hoc
  manual verification has been done).
- Verified for real on Linux x86_64 (vanilla) and Windows 11 x86_64
  (vanilla and Fabric 1.21.1 with Sodium + Iris + a shader pack + a
  resource pack, launched to the main menu; the desktop app run and
  inspected). aarch64 is unverified.
- **There is no CI** — it was removed on request. Every check in "Building
  and verifying" is run locally; keep them all green before calling work
  done.

Don't assume a `Command` variant, CLI subcommand, or crate capability exists
just because PLAN.md describes it for a later milestone — check the actual
code.

## Workspace layout

A Cargo workspace under `crates/`. Every crate has a `//!` doc comment at
the top of `src/lib.rs` (or `main.rs`) restating its one-line job — trust
those over this table if they ever disagree.

| Crate | Status | Role |
|---|---|---|
| `bananium-core` | done for current needs | `Paths` (all on-disk locations), layered `Config` (+ `update_file`), shared `Error` taxonomy, `tracing` init |
| `bananium-api` | **The frontend facade** | `Command`, `Event`, `Session::dispatch`/`events`; `session/` has one module per area (content, presets, instances, logs, profiles, versions, system, download). Every frontend depends on *only* this crate. |
| `bananium-net` | done for current needs | `HttpClient`, bounded-concurrency resumable `Downloader` |
| `bananium-meta` | + Fabric | Mojang manifest/profile parsing, rule evaluation, asset index types, `MetaClient` (fetch-with-offline-fallback), Fabric loader metadata + profile merge |
| `bananium-store` | done for current needs | Content-addressed blob store, reflink→hardlink→copy materialization |
| `bananium-instance` | ahead of M3 | Instances (clone/rename/remove, overrides, running pids), content lockfile, presets |
| `bananium-launch` | done for current needs | Classpath dedup, native extraction, argument templating, offline UUIDs + profile store, `LaunchPlan` |
| `bananium-java` | detection only | System JVM search (`find_java`, `find_all_java`). No Mojang runtime provisioning yet. |
| `bananium-modrinth` | tested API client | v2 REST client used by `bananium-api`; no offline mirror |
| `bananium-cli` | broad | clap frontend: install/launch/instance/profile/search/content/preset/java/screenshots |
| `bananium-tui` | **first slice** | ratatui instance list + launch + RAM/JVM-args edit overlay; no live log pane/task tray/command palette yet |
| `bananium-desktop` (`desktop/src-tauri`) | **the GUI** | Tauri 2 bridge for the React app in `desktop/src` |
| `bananium-rpc` | **dummy stub** | prints `BOILER PLATE`, nothing else |

## The frontend contract — do not violate this

`bananium-cli`, `bananium-tui`, `bananium-desktop`, and `bananium-rpc` may
depend on `bananium-api` and their own UI libraries (clap, ratatui, tauri,
...) — **nothing else in the workspace**. All filesystem/network access,
all business logic, lives behind `Session::dispatch(Command) ->
Result<CommandOutput>` and `Session::events() -> Stream<Event>` in
`bananium-api`. The desktop webview only ever sees `Command`/`CommandOutput`/
`Event` JSON; it never touches the filesystem itself (screenshot images are
the one read-only exception, via Tauri's asset protocol scoped at runtime to
the instances directory).

This is mechanically enforced: `scripts/check_frontend_deps.py` parses each
frontend crate's `Cargo.toml` and fails if it names any other `bananium-*`
crate. Run it yourself after touching any frontend crate's dependencies:

```sh
python3 scripts/check_frontend_deps.py
```

If you need a frontend to do something new, the fix is always "add a
`Command`/`Event` variant and implement it in `Session`," never "add a
dependency."

## Building and verifying

There is no CI; run all of these locally. **The desktop app's web bundle
must exist before any workspace-wide cargo command**: `tauri::generate_context!`
in `bananium-desktop` fails to compile without `desktop/dist`.

```sh
(cd desktop && bun install && bun run build)       # once, and after frontend changes
cargo build --workspace
cargo test --workspace
cargo fmt --all                                    # apply
cargo fmt --all -- --check                          # verify
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --workspace --no-deps                     # catches broken/private intra-doc links
python3 scripts/check_frontend_deps.py
(cd desktop && bun run typecheck && bun run lint)
```

**After changing any `Command`/`CommandOutput`/`Event` or DTO, regenerate
the TypeScript bindings** (ts-rs, behind the `ts` feature; output path set
in `.cargo/config.toml`) and commit them:

```sh
cargo test -p bananium-api --features ts
```

Run the GUI with `cd desktop && bun run tauri dev` (set `BANANIUM_HOME` to
sandbox it). Use **bun** for everything in `desktop/` — never npm/npx.

All of the above are clean on the current tree. Keep them clean.

### End-to-end testing against a real install

`bananium` reads `BANANIUM_HOME` to relocate its whole state directory, which
makes sandboxed E2E testing trivial without touching a real `~/.bananium`:

```sh
rm -rf /tmp/bananium-test
BANANIUM_HOME=/tmp/bananium-test cargo run -p bananium-cli -- install 1.21.1
BANANIUM_HOME=/tmp/bananium-test cargo run -p bananium-cli -- launch --dry-run
BANANIUM_HOME=/tmp/bananium-test cargo run -p bananium-cli -- launch   # real launch if you have a JVM + display
```

`install` for a real version pulls the full asset set (thousands of small
files) — expect it to take a minute or two, not be instant. This machine has
a real JVM (`java -version` works) and a display (`$DISPLAY` is set), so a
real (non-dry-run) launch is a legitimate way to verify changes to
`bananium-launch`, not just a theoretical option.

To test the offline gate (PLAN.md's "bring the interface down" manual
check) without touching real networking, route requests to an address that
refuses connections instantly:

```sh
BANANIUM_HOME=/tmp/bananium-test HTTPS_PROXY=http://127.0.0.1:1 HTTP_PROXY=http://127.0.0.1:1 \
  cargo run -p bananium-cli -- launch --dry-run
```

This should return instantly from cache, not hang or fail — `reqwest`
respects the proxy env vars, and `HttpClient` has a 10s connect timeout
specifically so a genuinely offline network fails fast rather than stalling
(see its doc comment in `crates/bananium-net/src/client.rs`).

## Design decisions and non-obvious behavior worth knowing before you touch the code

Everything below has a fuller explanation as a doc comment at its actual
call site — this is a map to where to look, plus the two things that bit us
for real and are worth knowing up front.

- **Content-addressed store** (`bananium-store`, `Paths::store_blob`):
  everything downloaded (client jar, libraries, assets) lives once at
  `store/<sha1[0..2]>/<sha1>`. The classpath just points directly at store
  paths — no per-instance copy of library jars is needed. The assets tree
  (`Paths::assets_dir()`) and native-extraction cache *are* materialized
  separately (via reflink→hardlink→copy) because the JVM needs them at
  specific relative paths, not arbitrary ones.

- **Offline UUID** (`bananium_launch::offline_uuid`): reproduces Java's
  `UUID.nameUUIDFromBytes("OfflinePlayer:<name>")` by hand — this is *not*
  RFC 4122 UUIDv3 over a namespace (no namespace bytes are prepended), so
  the `uuid` crate's `new_v3` would give a different, wrong answer. Verified
  against an independent Python re-implementation, not just self-consistent
  Rust code — see the test in `crates/bananium-launch/src/offline.rs`.

- **Mojang `rules` evaluation** (`bananium_meta::evaluate_rules`): starts at
  `false`; each matching rule overwrites the verdict with its own action;
  empty rules array always applies. This is the actual official-launcher
  semantics, not a guess — get this wrong and libraries silently go missing
  or extra ones get pulled in on the wrong platform.

- **Native library extraction is the sharpest edge in this codebase** —
  two real, empirically-discovered bugs already happened here, both from
  Mojang profile packaging that changed between Minecraft versions:

  1. Newer LWJGL (3.4+) native jars nest their `.so` several directories
     deep (`linux/x64/org/lwjgl/liblwjgl.so`); `java.library.path` is never
     searched recursively, so extraction must flatten every entry to its
     basename.
  2. Newer Mojang profiles pass **four different subdirectories** of
     `${natives_directory}` to `java.library.path`, `jna.tmpdir`,
     `org.lwjgl.system.SharedLibraryExtractPath`, and
     `io.netty.native.workdir`. The tempting fix — route each library's
     native jar into "its" subdirectory by maven group — reproduces the
     exact same crash, because `Library.loadSystem()` only actually scans
     `java.library.path`; `SharedLibraryExtractPath` etc. are
     self-extraction *targets* for a natives jar on the classpath, and
     Bananium deliberately keeps natives jars off the classpath. The actual
     fix, in `crates/bananium-launch/src/natives.rs::extract_jar`, hardlinks
     every extracted file into *every* component subdirectory plus the flat
     root, so it's found regardless of which property/mechanism a given
     library actually checks. Read that function's doc comment before
     changing this again — it explains why the "obviously correct" targeted
     version doesn't work, backed by a real crash report and a real
     re-launch, not just reasoning.

  If you're touching native extraction: there is a real Minecraft process
  you can launch to check your work (see "End-to-end testing" above), and
  you should, because this is exactly the kind of bug unit tests alone
  didn't catch the first time.

- **`MetaClient` fetch order** (`bananium-meta/src/client.rs`): tries the
  network *first*, falls back to the on-disk cache only on a network
  *error*. This ordering is deliberate — cache-first would silently serve
  stale metadata to a user who's actually online. Network-first-with-
  fallback is what makes the offline gate work without ever risking staleness
  while connected.

- **Download engine** (`bananium-net/src/download.rs`): bounded by a
  `Semaphore`, resumes via `Range` requests against a sibling `.part` file,
  retries with exponential backoff, verifies the checksum *after* a
  download completes and only then atomically renames into place. A
  checksum mismatch deletes the `.part` file rather than retrying a resume
  from possibly-corrupt data. Every `Progress` update carries a per-attempt
  `bytes_per_sec`; `Session::install` (in `bananium-api`) sums the whole
  job's per-file progress into a separate `Event::OverallProgress` (total
  bytes done/total, average speed, files done/total) since `bananium-net`
  only ever sees one file at a time and can't produce that itself.
  `bananium-cli` renders that aggregate as a throttled stderr ticker — see
  its `ProgressPrinter`.

- **The launch classpath uses `.jar`-named links, not raw store paths**
  (`Paths::jar_link`, materialized in `Session::launch`). Store blobs are
  extensionless, and Fabric's remapper silently skips classpath inputs not
  ending in `.jar` — it then dies with "Generated deobfuscated JARs contain
  no classes". Found by a real Fabric launch; vanilla never noticed.

- **Content compatibility comes from the Modrinth API only** (no jar
  inspection — an explicit decision). Rules, in `session/content.rs`:
  - Automatic picks (install, dependencies, updates) are **stable releases
    only**; betas/alphas only when the user picks that exact version (or an
    author pins one as a dependency). Only-prerelease projects error with
    `OnlyPrereleases` telling the user to pick one explicitly.
  - Versions must list the instance's Minecraft version and a usable
    loader; older stable versions are the fallback.
  - Dependencies: an author's pinned `version_id` is used; an **unpinned**
    one gets the newest stable version published **no later than** the
    dependent (`Pin::NotAfter`). Modrinth has no dependency ranges, and this
    is what fixed a real crash: Iris 1.8.8 lists Sodium unpinned but needs
    0.6.x; "newest Sodium" (0.8.x) breaks it.
  - "incompatible" declarations are checked both ways against installed
    content; a conflicting candidate falls back to older versions, else the
    install is refused naming the pair.
  - Updates obey the same caps from installed dependents (`find_updates`),
    and `ContentUpdate` installs exactly what the check offered.

- **Progress events are throttled at the source** (~10/s per task,
  `session/download.rs`), with an exact final update always sent. Unthrottled
  per-file events (thousands/s during an install) froze the desktop UI and
  overflowed the event channel, dropping `TaskCompleted` so finished tasks
  stayed "running" in the tray. Don't reintroduce per-file event emission.

- **Shader packs pull in Iris** automatically (they can't declare it as a
  Modrinth dependency themselves); Iris then brings Sodium. Mods and
  shaders are refused on vanilla instances.

- **`bananium-api::Error`** boxes every sub-crate error variant
  (`Box<bananium_core::Error>` etc.) to satisfy clippy's
  `result_large_err`, with hand-written `From` impls (a macro generates
  them) instead of `#[from]`, because `#[from]` can't insert the `Box::new`
  itself. If you add a new sub-crate error source, follow the existing
  `impl_boxed_from!` pattern in `crates/bananium-api/src/error.rs`.

## Conventions

- **Every public item gets a doc comment** — struct, enum, function, field
  where the name doesn't already say it all. This was an explicit ask, not
  just a style nicety; keep it up when you add code. Prefer explaining *why*
  over restating the signature. `cargo doc --workspace --no-deps` should
  stay warning-free (broken/private intra-doc links included).
- Each crate defines its own `Error` enum via `thiserror` and its own
  `Result<T>` alias; `bananium-core::Error` is the shared taxonomy other
  crates convert into or wrap, not a dumping ground every crate is forced to
  extend.
- No comments that just restate the code. Comments explain non-obvious
  *why* (a workaround, an invariant, a subtlety like the two above) — see
  the general house style already applied throughout `crates/`.
- Don't add functionality beyond what's asked or what the current milestone
  needs — e.g. don't start on M3's Java provisioning or M2's TUI screens
  unless that's the actual task. Check `PLAN.md`'s milestone list before
  assuming something should exist yet.
- `rustfmt` defaults (no custom `rustfmt.toml`). Run `cargo fmt --all`
  before considering anything done.
