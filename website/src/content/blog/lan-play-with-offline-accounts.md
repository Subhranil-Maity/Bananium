---
title: "Bananium 0.1.2: LAN play with offline accounts"
description: Friends can now join your LAN worlds with offline accounts, multiplayer works again on 1.16.5, and Open folder buttons do what they say.
pubDate: 2026-10-25
updatedDate: 2026-10-25
tags: [release, multiplayer]
---

> **Update:** this is only a partial fix. Joining from the LAN world list
> works, but joining by typing an address (Direct Connect), for example over
> Tailscale or another VPN, still fails. More on that
> [below](#not-fixed-yet-direct-connect-and-vpns).

Bananium 0.1.2 fixes the most common complaint about playing together: open
your world to LAN, a friend tries to join, and they get kicked with
**"Invalid session"**. Now they get in.

## What was going wrong

Bananium accounts are offline, so the game has no real Mojang login to show
when someone joins your world. On Minecraft 26.x, a LAN world asks Mojang to
check every player who joins. Mojang answers that the offline login is
invalid, and the player gets kicked.

The game has an old fallback for exactly this case: if Mojang's login
servers can't be reached, a LAN world lets players in anyway. Bananium now
uses it. It starts the game pointed at an address that doesn't exist, so the
login check fails as "can't connect" rather than "invalid", and your friends
join.

The same change fixes 1.16.4 and 1.16.5. There, offline accounts were greyed
out of the Multiplayer menu with a "check your Microsoft account settings"
message.

## Who needs to update

- **On 26.x, only the host.** Friends can join from any launcher, with any
  account.
- **On 1.16.4–1.16.5, everyone.** Each player needs the fix to get into the
  Multiplayer menu at all.
- Join from the **LAN world list** in Multiplayer. Typing the address into
  Direct Connect can still fail for offline accounts.

Tested on 1.16.5, 1.20.1, 1.21.11 and 26.3.

## Not fixed yet: Direct Connect and VPNs

If you join by typing your friend's address into **Direct Connect** or a saved
server, for example a Tailscale address like `100.x.x.x`, it still fails. With
the fix on you'll see **"cannot reach authentication servers"**, and with it
off you'll see **"Invalid session"**.

This happens on the joining player's side. Before connecting, your own game
checks your login with Mojang. It only ignores a failed check for worlds that
appeared in the LAN list, so for a typed-in address the join stops before
your friend's game can let you in. Tailscale doesn't carry the broadcasts
that make a world show up in the LAN list, so over Tailscale, Direct Connect
is the only option.

A fix is in progress. Until it lands, either:

- use a VPN that carries LAN broadcasts, such as ZeroTier or Radmin VPN, so
  the world appears in the LAN list, or
- play on a server with `online-mode=false`.

## A switch, if you want it

The fix is on by default. You can turn it off under **Settings → Multiplayer
→ Offline Fix** to launch exactly as before. It takes effect on the next
launch. One trade-off while it's on: skins of real Microsoft accounts won't
load in-game.

## Also fixed: Open folder

"Open folder" on an instance, and the folder buttons in the content manager,
the file browser and Settings, did nothing at all. They now open Explorer,
and show an error if a folder can't be opened.

> Bananium is meant for people who own Minecraft. Please buy the game to
> support Mojang.

## Get it

Download `Bananium_0.1.2_x64-setup.exe` from the
[Releases page](https://github.com/Subhranil-Maity/Bananium/releases). It
upgrades your existing install, and your instances and settings are kept.
Bug reports are welcome on
[GitHub](https://github.com/Subhranil-Maity/Bananium/issues).
