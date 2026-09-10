# ADR 0010: Discord Rich Presence via a hand-rolled IPC client, no crate

## Status
Accepted

## Context
The feature request originated from a misunderstanding worth recording:
Discord's own "Compartilhar minha atividade" (share my activity) privacy
toggle only controls whether Discord *shows* activity to friends — it
does nothing on its own. An app has to actively speak Discord's local
Rich Presence IPC protocol for anything to appear at all.

That protocol is a Unix domain socket (`$XDG_RUNTIME_DIR/discord-ipc-0`,
falling back to `/tmp`) carrying an 8-byte header (opcode + little-endian
`u32` length) followed by a JSON payload — a handshake frame once per
connection, then `SET_ACTIVITY` frames to update or clear the status.
Ready-made crates exist for this (e.g. `discord-rich-presence`).

## Decision
Implement the protocol directly in `src-tauri/src/platform/discord.rs`,
using only dependencies Echora already has (`tokio` with its `net`/
`io-util`/`time` features, already enabled; `serde_json`). No new Cargo
dependency was added.

This follows the same reasoning as ADR 0005 (prefer the option with
fewer/no new dependencies where the protocol involved is small and
stable) and CLAUDE.md's "don't add a dependency just in case" rule:
Discord's Rich Presence framing has been stable for years and amounts to
roughly 150-250 lines of code, well within what hand-rolling costs
against pulling in a new crate, a licensing review, and extra surface in
the dependency tree.

## Consequences
- No new entry needed in `THIRD_PARTY_NOTICES.md`, no
  `licensing-compliance-reviewer` pass required for this feature.
- Echora owns protocol correctness. If Discord ever changes the IPC
  framing (no indication it plans to — this has been stable for years),
  `platform/discord.rs` needs a matching update; a crate would have
  absorbed that instead.
- The feature is opt-in (`Settings.discord_presence_enabled`, default
  `false`), consistent with "no telemetry/cloud by default" — sending
  now-playing data to a third-party client like Discord only happens
  when the user turns it on.
