---
id: 004
title: What each Seat may see
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

For a Seat tied to a player, a Seat tied to no player (spectator), and a replay viewer, which parts of game state and which ledger events are visible, redacted or rewritten? Cover hands, library contents and order, face-down cards, revealed cards, searches, shuffles, and other players' Answers to secret choices.

## Notes

- `Engine::state()` returns the full `GameState` today. DESIGN.md lists hidden-information leakage into Lua as an open question.
- Decide whether redaction happens on the ledger events, on a state snapshot, or both.

## Resolution

Visibility is one rule, applied as `(Seat, event)` and `(Seat, state)`. A snapshot for a Seat always equals the fold of the events that Seat may see.

- **Player Seat.** Sees public zones in full (battlefield, graveyard, exile, stack) and its own hand. Opponents' hands and every library show counts only. Library order is hidden from all Seats, including the owner.
- **Spectator Seat.** Same view as a player Seat without a hand and without owner-only events. The host may set an `omniscient` flag when it creates the Seat. The engine enforces the flag and does not grant it. A player Seat is never omniscient.
- **Replay viewer.** Omniscient for finished games. It reads the unredacted event stream. A live spectator never receives that stream.
- **Secret choices.** Other Seats learn that a Decision was answered and by whom. They learn nothing about its content. Results appear only through the public events they cause.
- **Owner-only events.** `CardsScried` and `CardsSurveiled` go to the controller's Seat only. `RngDrawn` is replay-only. `LibraryShuffled` is public and reveals no order.
- **Moves between hidden zones.** The identity is visible to the controller and owner only. Other Seats see that a card moved between hidden zones. `last_known_info` is dropped from the event for Seats that cannot see the identity.
- **Moves into public zones.** The identity is visible to all Seats.
- **Reveal effects.** A new `Revealed` event carries the identities to all Seats. The cards stay in their zone. TODO: add the `Revealed` variant to `GameEventKind`.
- **Face-down cards.** Only the controller sees the identity. Other Seats see that the object is face-down and nothing else. Turning it face up is a public event.
- **Decision candidate lists.** The chooser sees the full list. Non-choosers see the Decision kind and the candidate count. Candidates from public zones are listed for everyone.
- **Rejected Answers.** The rejection goes to the sending Seat only and may name hidden cards and zones.
- **Lua scripts.** Scripts run engine-side with full state. Visibility applies only to what reaches a Seat. Script visibility is not a security boundary, so the Lua side needs no protection against a modified engine.
- **Free-text event fields** (`ScriptError`, `StateBasedActionApplied`, `RngDrawn`). The engine never writes a hidden identity into these strings, because every Seat may receive them. TODO: write this rule into the engine's event-writing guidelines.

Dependency on 005: ids for hidden objects must come from a counter unrelated to deck order or shuffle results. If 005 cannot guarantee this, fall back to a per-Seat opaque handle for hidden objects.

