---
id: 072
title: Ledger events the snapshot fold needs
type: task
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Make every change a Seat's snapshot shows reach the ledger as an event, so a Seat's events always add up to its snapshot (020, decision 1).

## Notes

- Origin 021. Its fold tests in `crates/host/tests/seat_visibility.rs` found these gaps. Each has a TODO at the site.
- Life gain writes no `LifeChanged`: `resolve_gain_life_effect` in `engine/resolve/direct_effects.rs`, the script `GainLife` result in `script/runtime.rs` (an operation returns only one pending kind, so this needs an `OperationOutcome` change), and lifelink in `commands::apply_damage_event`, which runs during commit.
- Poison counters from infect damage to a player write no event (`apply_damage_event`).
- Nothing writes `PrioritySet`, so a Seat cannot follow `priority_player` (CR 117.3).
- CR 800.4a: `sba::remove_players_objects` removes a leaving player's objects with no events.
- A new event may fire triggers that did not fire before, such as "whenever you gain life". Check the engine tests.
- The snapshot drops the identity a `revealed` event showed for a card still in a hand or library (TODO in `host/src/view.rs`). Decide whether the snapshot keeps it.
- Once these land, add `life` and `priority` back to the proof-game fold, and let it run to an elimination.

## Resolution

<Filled in at close: the decision and the reason for it.>
