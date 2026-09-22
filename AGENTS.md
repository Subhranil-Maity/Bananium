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
tested. M2/M3/M4 are still mostly not implemented, but three pieces of them
exist ahead of schedule, built on direct request rather than in milestone
order — see the table below for exactly what each covers:

- `bananium-tui` is a real (if intentionally minimal) ratatui app now, not
  the `BOILER PLATE` stub PLAN.md's milestone order would suggest: an
  instance list with running status, launch, and quit. The log pane, task
  tray, and command palette PLAN.md describes for the rest of M2 don't exist
  yet.
- `bananium-rpc` is still a one-line dummy binary that prints
  `BOILER PLATE`. It exists so the workspace and the frontend-dependency
  check have something to check against, nothing more.
- `bananium-modrinth` has a tested Modrinth v2 REST client (search, project/
  version lookups, `version_files`, rate limiting) — built in an isolated
  worktree/branch and not yet merged to `main`; check `git branch -a` before
  assuming it's present in your checkout. The SQLite/FTS5 offline mirror,
  lockfile, and dependency resolution M4 also calls for are **not** built,
  and nothing wires this crate into `bananium-api`/any frontend yet.
- `bananium-instance` covers more than the M1 slice now: named instances
  (several can share one Minecraft version), a per-instance RAM cap and
  append-only extra JVM args, and pid-based running-instance tracking
  (`running.toml`, Linux-only liveness check, one live process per instance
  at a time). Still missing: `clone|rm|rename|export`, groups, tags, and the
  mod lockfile (M3/M4).
- `bananium-java` only does system-JVM detection (`JAVA_HOME`/`PATH`/
  `/usr/lib/jvm`). Mojang java-runtime auto-provisioning and the aarch64
  Adoptium fallback are M3 work and are **not** implemented — if no system
  JVM is found, `bananium launch` just errors.
- No golden-file dry-run test matrix yet (PLAN.md calls this out as the
  highest-value test in the project — it doesn't exist yet, only ad hoc
  manual verification has been done).
- Only tested on Linux x86_64. Windows and aarch64 are unverified (the code
  is written to be portable — `PathBuf`, no hardcoded separators, `dirs`
  crate for home resolution — but nothing has actually run there).
- No git history predates this file; the repo was freshly initialized
  alongside it.

Don't assume a `Command` variant, CLI subcommand, or crate capability exists
just because PLAN.md describes it for a later milestone — check the actual
code.

## Workspace layout

A Cargo workspace under `crates/`. Every crate has a `//!` doc comment at
the top of `src/lib.rs` (or `main.rs`) restating its one-line job — trust
those over this table if they ever disagree.

| Crate | Status | Role |
|---|---|---|
| `bananium-core` | done for M1 needs | `Paths` (all on-disk locations), layered `Config`, shared `Error` taxonomy, `tracing` init |
| `bananium-api` | done for M1 needs | **The frontend facade**: `Command`, `Event`, `Session::dispatch`/`events`. Every frontend depends on *only* this crate. |
| `bananium-net` | done for M1 needs | `HttpClient`, bounded-concurrency resumable `Downloader` |
| `bananium-meta` | done for M1 needs | Mojang manifest/profile parsing, rule evaluation, asset index types, `MetaClient` (fetch-with-offline-fallback) |
| `bananium-store` | done for M1 needs | Content-addressed blob store, reflink→hardlink→copy materialization |
| `bananium-instance` | ahead of M1, short of M3 | `InstanceConfig`/`InstanceStore` — named instances, RAM/JVM-arg overrides, running-pid tracking; no clone/rm/rename/export/groups/lockfile yet |
| `bananium-launch` | done for M1 needs, plus RAM/extra-JVM-arg support | Classpath dedup, native extraction, argument templating, offline UUIDs, `LaunchPlan` |
| `bananium-java` | detection only | System JVM search. No Mojang runtime provisioning yet. |
| `bananium-cli` | `config show`/`install`/`launch`/`instance ls`/`instance set` | clap frontend |
| `bananium-tui` | **first slice** | ratatui instance list + launch + quit; no log pane/task tray/command palette yet |
| `bananium-rpc` | **dummy stub** | prints `BOILER PLATE`, nothing else |
| `bananium-modrinth` | **tested API client, unmerged** | v2 REST client on a separate branch/worktree; no offline mirror, no frontend wiring |

## The frontend contract — do not violate this

`bananium-cli`, `bananium-tui`, and `bananium-rpc` may depend on
`bananium-api` and their own UI libraries (clap, ratatui, ...) — **nothing
else in the workspace**. All filesystem/network access, all business logic,
lives behind `Session::dispatch(Command) -> Result<CommandOutput>` and
`Session::events() -> Stream<Event>` in `bananium-api`.

This is mechanically enforced: `scripts/check_frontend_deps.py` parses each
frontend crate's `Cargo.toml` and fails if it names any other `bananium-*`
crate. It runs as its own CI job in `.github/workflows/ci.yml`. Run it
yourself after touching any frontend crate's dependencies:

```sh
python3 scripts/check_frontend_deps.py
```

If you need a frontend to do something new, the fix is always "add a
`Command`/`Event` variant and implement it in `Session`," never "add a
dependency."

## Building and verifying

Standard Rust workspace commands, nothing project-specific:

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all                                    # apply
cargo fmt --all -- --check                          # verify (CI does this)
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --workspace --no-deps                     # catches broken/private intra-doc links
python3 scripts/check_frontend_deps.py
```

All of the above are clean on the current tree. Keep them clean — `clippy
-D warnings` and the dependency check both gate CI. `cargo doc` isn't gated
in CI yet but treat broken intra-doc links as bugs; they're checked
manually each session.

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
