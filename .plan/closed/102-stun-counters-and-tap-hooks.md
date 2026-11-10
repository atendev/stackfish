---
id: 102
title: Stun counters and tap or untap replacement hooks
type: grilling
mode: HITL
status: closed
blocked_by: [89]
claimed: false
---

## Question

How do stun counters (CR 122.1d) and "would become tapped or untapped" replacements work, so an untap no longer always succeeds?

## Notes

- Origin: backlog item 10 and `docs/lua/not-available-yet.md` (No becomes tapped condition). `Game.Ops.Tap` and `Untap` already write `Tapped` and `Untapped` events on a real flip (CR 603.2e).
- Blocked by the event visibility decision, which settles how a condition sees a tap event. That decision owns the "becomes tapped" condition (`Game.Conditions.BecomesTapped`), so it is not part of this ticket.
- Decide whether stun is a counter kind with a built-in replacement or a general replacement hook, and where the untap step checks it.
- Read `docs/implementation/08-replacements.md`.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

Stun is a built-in rule check in one engine helper that every untap goes through. It is not a general "would untap" replacement filter. "Would become tapped" is dropped. The build is "Build stun counters". Decided in auto mode.

- Card counts from `stackfish.db` oracle text, approximate: 88 cards mention stun counters. All 69 "would become untapped" texts are stun reminder text. No card says "would become tapped". The only non-stun untap replacements are Bewitching Leechcraft and Edge of Malacol. 374 cards say "doesn't untap during ... untap step", and 10 say "can't untap more than N".
- Carrier: one `Engine` method is the single seam for an untap attempt. `untap_step` and the untap side of `flip_tapped` call it. It owns both outcomes. Either it writes `tapped = false` and pushes `Untapped`, or it leaves the flag alone and pushes a proposed `CounterAdded { kind: Stun, amount: -1 }` (CR 122.1d, 614.6). No `Untapped` event means `BecomesUntapped` cannot fire on a replaced untap. A general `EventFilter::WouldUntap` would first need untap to become a proposed event. A later general would-untap replacement slots into this helper.
- Untap attempt: only a tapped battlefield permanent makes one (CR 701.26b). `flip_tapped` keeps its early return ahead of the helper, so a stun counter on an untapped permanent stays. A phased-out permanent makes no attempt (CR 702.26b). The helper skips it entirely, with no flag write, no event and no counter removal. This changes `untap_step`, which untaps phased-out permanents today.
- Stun count: the helper reads the committed stun count plus any pending stun deltas for that object in the same event list, and proposes a removal only when the sum is above 0. One attempt removes at most one counter (CR 614.5). With one stun counter, two untaps in one resolution remove the counter, and then the permanent untaps. `CounterSet` gets no clamp.
- Commit: the removal goes through `ReplacementCheck` and is applied in `CommitEvents`, as every `CounterAdded` is. In the untap step it takes its object's slot in the id-sorted batch. A replaced untap still counts as one the player chose to untap (CR 502.3), which matters only once untap limits exist.
- Counter kind: `CounterKind::Stun`, with Lua name `"Stun"` in `parse_counter_kind_name`, `Game.Counter.Stun`, scenario name `"stun"` and an additive `WireCounterKind::Stun` arm. `PROTOCOL_VERSION` is unchanged. Every exhaustive `match` on `CounterKind` gets the arm. "Enters tapped with N stun counters" uses the existing `EntersTapped` and `EntersWithCounters`.
- Lua visibility: the removal is a ledger `CounterAdded` with a negative amount. The event-visibility decision keeps Rust field names, and `amount` is an `i32`, so a condition can test `amount < 0`. The "Bridge condition events into Lua" build should pin the sign in a test. No "counter removed" tag ships.
- Counter-placement replacements: a future "would put counters" replacement (CR 614.16) matches only a positive amount, so it never touches a stun removal. None exist today, because `EventFilter` has no counter shape.

TODOs for the build:

- `// TODO: other would-untap replacements such as Bewitching Leechcraft and Edge of Malacol, ordered with stun (CR 616.1)` at the helper.
- `// TODO: no "would put counters" replacement, which must match only a positive amount (CR 614.16)` at the helper.
- `// TODO: no condition tag for a counter being removed`.
- The `Tap` TODO at `types/ability.rs` ("no would become tapped replacement event") stays as a known gap. The `Untap` TODO is narrowed to the non-stun would-untap replacements, not deleted. The `untap_step` TODO for phasing and "doesn't untap" stays, and gains: a permanent that doesn't untap keeps its stun counter, because it makes no untap attempt.

Tests the build must add: the untap step with one and with two stun counters (2, then 1, then 0, then untapped). `Game.Ops.Untap` on a stunned creature. `UntapAll` across stunned and plain creatures. An untapped permanent keeps its stun counter through the untap step. A replaced untap writes no `Untapped` event and fires no `BecomesUntapped`. One stun counter and two untaps in one resolution: one counter is removed and the permanent ends untapped. A phased-out stunned permanent keeps its tapped flag and its counter. Enters tapped with stun counters, then the first untap step removes one. The Lua, scenario and wire `Stun` names.

Docs: `docs/lua/stubs/game.lua`, `game-constants.md`, `docs/lua/not-available-yet.md` lines 44-48 and the `99-backlog.md` tap/untap entry. They say stun counters exist, and that a would-tap hook and the non-stun would-untap replacements are missing. If "Bridge condition events into Lua" has not landed, the becomes-tapped and becomes-untapped condition gap stays in `not-available-yet.md`.

Not this ticket: "doesn't untap" effects and "can't untap more than N" limits (CR 502.3), non-stun would-untap replacements and "would become tapped" (no printed card). Phasing itself (CR 502.1) stays a TODO.

Terms: `CONTEXT.md` adds "Untap attempt".
