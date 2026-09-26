---
title: Introducing Bananium
description: A fast, lightweight Minecraft launcher with a Rust core. What it does today, and why it exists.
pubDate: 2026-09-26
tags: [release, launcher]
---

Bananium is a Minecraft launcher built around three ideas: it should be light
on memory, it should work offline, and it should only talk to official
sources. Version 0.1.0 is early, but everything described here is built and
working on Windows 11 and Linux.

## Why another launcher?

Most launchers are heavier than they need to be. Bananium is a Rust core with
a small desktop shell, so the launcher isn't what's eating your RAM while the
game runs.

- **Plays without internet.** Once an instance is installed, it launches from the local
  cache with no network.
- **Official sources only.** Game files and Java runtimes come straight from
  Mojang's servers. No third-party mirrors.
- **One copy of everything.** Libraries, assets, and mod jars are stored once
  and shared between instances.
- **No Java setup.** Bananium downloads the exact runtime Mojang publishes for
  each Minecraft version.

## What's in 0.1.0

The desktop app has a library of instances, a Modrinth browser for mods,
resource packs, shaders, and modpacks, presets you can apply across
instances, a screenshot gallery, offline accounts, and Discord Rich Presence.
Fabric is the only mod loader supported so far.

Everything the app does is also available from the `bananium` CLI:

```sh
bananium install 1.21.1 --name modded --fabric latest
bananium content add modded sodium
bananium launch modded
```

## Picking mod versions that don't break

Automatic picks are stable releases only, and a dependency without a pinned
version gets the newest stable release **no later than** the mod that needs
it. That one rule prevents a whole class of crashes, like an older Iris
breaking on a much newer Sodium.

> Bananium is meant for people who own Minecraft. Please buy the game to
> support Mojang.

## Try it

Get the Windows installer from the
[Releases page](https://github.com/Subhranil-Maity/Bananium/releases), or
[build it from source](https://subhranil-maity.github.io/Bananium/download/#build).
Bug reports and ideas are welcome on
[GitHub](https://github.com/Subhranil-Maity/Bananium/issues).
