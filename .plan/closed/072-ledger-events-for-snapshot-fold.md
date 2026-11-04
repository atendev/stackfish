---
id: 072
title: Ledger events the snapshot fold needs
type: task
mode: HITL
status: closed
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

Built on `phase5/ledger-events-snapshot-fold` (`52b1a1c`). Notes in `docs/implementation/31-snapshot-fold-events.md`.

- Every life change writes `LifeChanged`, including damage to a player (CR 120.3a). A Seat's life is the sum of `life_changed` deltas, so neither the bot nor the fold reads `damage_dealt`. The life and poison changes that damage makes come back from `apply_damage_event` and follow the damage as its children.
- Infect damage to a player writes a new `PlayerCounterAdded` event. A script's `GainLife` writes its event straight to the ledger, so `OperationOutcome` stays unchanged.
- `Engine::set_priority` writes `PrioritySet` on every change of `priority_player`. CR 800.4a writes one `LeftGame` per object removed, and passes priority on when the leaving player held it. The host keeps a `left_game` id from Seats that could not see the object.
- The snapshot does not keep a revealed card's identity once the card is back in a hidden zone, because a snapshot shows the state after the reveal's effect (CR 701.20a).
- The fold tests compare priority, life and poison, and play five proof games to the end. `BEHAVIOUR_REV` is 3.
- Left open: the wire `v` bump, CR 119.9 gain-life triggers, and the rest of CR 800.4a (ending control effects and exiling objects the player still controls).
