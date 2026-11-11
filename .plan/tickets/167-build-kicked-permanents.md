---
id: 167
title: Build kicked permanents
type: task
mode: AFK
status: open
blocked_by: [166]
claimed: false
---

## Question

Carry a spell's `optional_paid` onto the permanent it becomes, so "when this enters, if it was kicked" and "enters with a counter for each time it was kicked" work (CR 603.4, 614.1c, 702.33d).

## Notes

- Origin 159. The full decision is in `closed/159-optional-additional-costs-and-kicker.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- `GameObject.optional_paid` is set on battlefield entry, as `entered_via_escape` is, and resets on a zone change (CR 400.7). A copy of a permanent never gets it (CR 707.2). Test that a token copy of a kicked creature is unkicked.
- A triggered or activated ability copies its source's `optional_paid` onto its `StackObject` when it is put on the stack, or takes it from `triggers::find_departed_lki` if the source left. A delayed trigger gets none. The `cast` argument then works for abilities too.
- `Game.Conditions.Kicked(min?, which?)` is an intervening-if. `ReplacementPayload::EntersWithCountersKicked { counter, per_kick, which }` joins the no-ordering-pause lists in `replacement.rs` and `script/builders.rs`.
- Test one ETB trigger per amount op (`GainLife`, `DrawCards`, `DealDamageTo`, `CreateToken`, `ChooseN` with `DiscardChosen`), each with a count of 0 doing nothing.
- TODO: an "as enters" action other than counters or tapped (Urborg Lhurgoyf, CR 614.12).
- Lua API change: update `game-constants.md` and `docs/lua/stubs/game.lua`.

## Resolution

<Filled in at close.>
