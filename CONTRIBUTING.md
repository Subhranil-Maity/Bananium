# Contributing to Bananium

Thanks for your interest in Bananium! Bug reports, ideas, and pull requests
are all welcome.

- **Found a bug?** [Open an issue](https://github.com/Subhranil-Maity/Bananium/issues)
  with your OS, the Bananium version (shown on the About page, which you open
  by clicking the banana logo), the Minecraft version and loader, and the
  steps to reproduce it. If the game crashed, attach the launch log from the
  instance's **Logs** tab.
- **Want to add something big?** Open an issue first so we can agree on the
  approach before you spend time on it.

## Project layout

Bananium is a Cargo workspace plus a Tauri desktop app:

| Path | What it is |
|---|---|
| `crates/bananium-api` | The frontend facade: `Command`, `Event`, and `Session`. Every frontend talks only to this crate |
| `crates/bananium-core` | Data paths, layered config, shared error types, logging |
| `crates/bananium-net` | HTTP client and the bounded, resumable downloader |
| `crates/bananium-meta` | Mojang and Fabric metadata, library rules, assets |
| `crates/bananium-store` | Content-addressed file store (one copy of every file, linked into instances) |
| `crates/bananium-instance` | Instances, the content lockfile, presets |
| `crates/bananium-launch` | Classpath, natives, argument templating, offline accounts |
| `crates/bananium-java` | Mojang Java runtimes and system JVM detection |
| `crates/bananium-modrinth` | Modrinth v2 API client |
| `crates/bananium-cli`, `crates/bananium-tui` | Terminal frontends |
| `crates/bananium-rpc` | Placeholder for a future JSON-over-stdio frontend |
| `desktop/` | The desktop app: Tauri 2 (`src-tauri/`) + React, TypeScript, and shadcn/ui (`src/`) |
| `website/` | The project website and blog: Astro, static, deployed to GitHub Pages. Posts are Markdown files in `website/src/content/blog/` |

Each crate's `lib.rs` or `main.rs` starts with a `//!` comment describing its
job.

## The frontend contract

This is the one rule that shapes the whole codebase:

1. **Frontend crates (`bananium-cli`, `bananium-tui`, `bananium-rpc`,
   `bananium-desktop`) depend only on `bananium-api`** plus their own UI
   libraries (clap, ratatui, tauri, ...). `scripts/check_frontend_deps.py`
   fails the build if a frontend names any other workspace crate.
2. **`Command` and `Event` are `serde` types**, so any frontend, in-process
   or not, speaks the same vocabulary.
3. **Frontends never touch the filesystem or the network themselves.** If a
   frontend needs something new, add a `Command` (and an `Event` if needed)
   and implement it in `Session`. Don't add a dependency.
4. **Long-running work reports `Event::Progress`**, so every frontend shows
   progress and failure the same way.

The desktop webview only ever sees `Command`, `CommandOutput`, and `Event`
JSON. The one exception is screenshots, which it reads through Tauri's asset
protocol, limited to the instances folder.

## Setting up

You need:

- [Rust](https://rustup.rs/) (stable)
- [Bun](https://bun.sh/). Use bun, not npm, for everything in `desktop/`
- The [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for
  your OS
- Python 3 (for the frontend-dependency check)

The desktop web bundle must be built **before** any workspace-wide `cargo`
command, because the desktop crate embeds `desktop/dist` at compile time:

```sh
cd desktop && bun install && bun run build && cd ..
cargo build --workspace
```

Run the desktop app with:

```sh
cd desktop
bun run tauri dev
```

### Keeping your real data safe

Set `BANANIUM_HOME` to point Bananium at a throwaway folder while you test,
so your real `~/.bananium` instances aren't touched:

```sh
BANANIUM_HOME=/tmp/bananium-test cargo run -p bananium-cli -- install 1.21.1
BANANIUM_HOME=/tmp/bananium-test cargo run -p bananium-cli -- launch --dry-run
```

To check that an installed instance really launches offline, send all
network traffic to a port that refuses connections. The launch should come
back instantly from the local cache:

```sh
BANANIUM_HOME=/tmp/bananium-test HTTPS_PROXY=http://127.0.0.1:1 HTTP_PROXY=http://127.0.0.1:1 \
  cargo run -p bananium-cli -- launch --dry-run
```

## Before you open a pull request

Run all of these and make sure they pass:

```sh
(cd desktop && bun run build)
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --workspace --no-deps
python3 scripts/check_frontend_deps.py
(cd desktop && bun run typecheck && bun run lint)
```

If you touched `website/`, also run
`(cd website && bun install && bun run check && bun run build)`.

If you changed `Command`, `CommandOutput`, `Event`, or any type they carry,
regenerate the TypeScript bindings and commit them:

```sh
cargo test -p bananium-api --features ts
```

The files in `desktop/src/bindings/` are generated. Never edit them by hand.

## Code style

- **Rust:** default `rustfmt` settings. Every public item gets a doc comment
  that explains *why*, not just what. Each crate has its own `thiserror`
  `Error` enum and `Result` alias.
- **Comments** explain non-obvious reasons (a workaround, an invariant, an
  edge case). Don't add comments that just repeat the code.
- **TypeScript:** follow the existing patterns in `desktop/src`. Use the
  shadcn/ui components in `desktop/src/components/ui`, and fetch data through
  the TanStack Query hooks in `desktop/src/hooks`.
- **Mods:** compatibility is decided from Modrinth's API metadata only, and
  automatic version picks are stable releases only. Betas and alphas are
  installed only when a user picks that exact version.
- **Keep pull requests focused.** One feature or fix per PR is much easier to
  review.

## License

By contributing, you agree that your contributions are licensed under the
[MIT License](LICENSE).
