---
id: 130
title: Build die and leave riders
type: task
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Build Battlefield riders for "would die this turn" and "would leave the battlefield", as "Riders beyond leaving the stack" decided.

## Notes

- Origin: the Resolution of [closed/099-riders-beyond-leaving-the-stack.md](../closed/099-riders-beyond-leaving-the-stack.md), which holds the rules for every part below.
- `types/game.rs`: the new rider list, its row type and `granted_at`. Add the list to the `loop_watch.rs` destructure and format string, and to the `testutil` atomicity destructure.
- `replacement.rs`: fold rows into `matching` as a new `ReplacementCandidate` variant, one per object, and apply it in `apply_one_replacement_or_protection`. Keep the per-row id TODO.
- `decision/answer.rs`, `decision/open.rs`, `crates/host/src/wire/mod.rs`: the additive candidate arm.
- `commands/attach.rs`: `retire_object` drops rows by affected object. `engine/turn.rs`: `begin_cleanup_step` drains `UntilEndOfTurn` rows and answers the TODO at its head for this list only.
- Lua: `Game.Ops.ExileIfWouldDieThisTurn(opts)` and `exile_if_leaves` on `ReturnToBattlefield` and `ReturnAllToBattlefield`. Update `docs/LUA_API.md`, `docs/lua/game-constants.md` and the stub in `docs/lua/stubs/game.lua`.
- Do not call this unearth or Whip of Erebos support. Their end-step exile waits on "Delayed triggered abilities".
- Update the TODOs at `commands/legality.rs:81` and `types/stack.rs:70` so each names only what stays missing. Update `docs/DESIGN.md` and `38-zone-permission-riders.md`.
- Tests are listed in the Resolution, apart from the granted rider cases, which belong to "Build permission entry rider".
- Follow the CLAUDE.md implementation loop on a `phase5/` branch.

## Resolution

<Filled in at close.>
