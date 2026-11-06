---
id: 89
title: Lua event visibility for conditions
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Which ledger events does a condition see, and in what table shape? Today `event_to_lua` encodes only zone changes, so a condition that wants a damage, life, tap, untap or mill event reads `nil`.

## Notes

- Origin: backlog item 6 and `docs/lua/not-available-yet.md` ("Only zone changes are visible to a condition", "No becomes tapped condition", "Mill and life loss have no trigger hooks"). This unblocks the most card scripts for the least work.
- `GainLife`, lifelink and damage to a player already write `LifeChanged` (`docs/implementation/31-snapshot-fold-events.md`), so `not-available-yet.md` is stale on that point. Ticket 110 fixes the prose.
- Decide the field names per event kind, how hidden information is kept out (milled cards, CR 701.17), and which `Game.Conditions.*` entries ship with the events.
- Use the `grilling` and `domain-modeling` skills. Code to read first: `crates/engine/src/script/encode.rs`, `script/runtime.rs` near `invoke_condition`, `docs/implementation/10-triggers.md`.
- Ticket 102 (stun counters and tap hooks) waits on this one.

## Resolution

<Filled in at close.>
