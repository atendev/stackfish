---
id: 137
title: Build PutOntoBattlefield from a custom function
type: task
mode: HITL
status: open
blocked_by: [133, 135]
claimed: false
---

## Question

Build `ScriptZoneMoveKind::Enter` and `Game.Ops.PutOntoBattlefield(pick, opts)`, as "Reanimation options" decided.

## Notes

- Read `closed/100-reanimation-options.md`.
- The op accepts hand, graveyard and exile picks and runs through the Reanimate mover. `PendingZoneMoves` keeps its shape, and `resolve/misc_effects.rs` and `resolve/each_player.rs` route `Enter`.
- "May" returns use `Game.MayChoose` in a custom function, so no `may` option is added.
- Check the hidden-event rule in `crates/host/src/view.rs` for a hand card's `ZoneChange`. The entering card's identity is public.
- Update the game stub in `docs/lua/stubs/game.lua`.

## Resolution

<Filled in at close.>
