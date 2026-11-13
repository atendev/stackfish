# Stun counters

A stun counter replaces an untap attempt with removing one stun counter (CR 122.1d). The decision is in `.plan/closed/102-stun-counters-and-tap-hooks.md`.

## Shape

- `Engine::attempt_untap` in `engine/turn.rs` is the one seam for an untap attempt. `untap_step` calls it for each tapped permanent the active player controls, in id order. `flip_tapped` calls it for `Untap`, `UntapSelf` and `UntapAll`, after its early return for an untapped or off-battlefield object, so an untapped permanent keeps its stun counter.
- The helper adds the committed stun count to the stun deltas already in the event list. Above 0, it pushes a proposed `CounterAdded { kind: Stun, amount: -1 }`, leaves `tapped` alone and pushes no `Untapped`, so `BecomesUntapped` cannot fire. One attempt removes at most one counter (CR 614.5). With one counter, two untaps in one resolution remove the counter and then untap.
- A phased-out permanent makes no attempt (CR 702.26b), so `untap_step` no longer untaps one.
- The removal is applied in `CommitEvents` like every `CounterAdded`. `CounterSet` has no clamp.
- `CounterKind::Stun` is `"Stun"` in `parse_counter_kind_name` and `Game.Counter.Stun`, `"stun"` in scenarios and an additive `WireCounterKind::Stun`. `PROTOCOL_VERSION` stays 1. "Enters tapped with N stun counters" uses `EntersTapped` and `EntersWithCounters("Stun", n)`.
- A Lua condition sees a removal as `counter_added` with a negative `amount`.
- The replay pin did not move, so `BEHAVIOUR_REV` stays 10.

## Tests

- `stun_counters.rs` (10): the untap step with one and with two stun counters, an untapped permanent keeping its counter, a phased-out permanent, `Untap` and `UntapAll`, no `BecomesUntapped` on a replaced untap, two untaps in one resolution, enters tapped with a stun counter, and the Lua names.
- `scenario::build` and `host::wire::event` unit tests for the `"stun"` names.

## Open

- Other would-untap replacements such as Bewitching Leechcraft and Edge of Malacol, and their CR 616.1 order with stun.
- "Doesn't untap" effects and "can't untap more than N" limits (CR 502.3). A permanent that doesn't untap makes no attempt and keeps its stun counter.
- A "would put counters" replacement (CR 614.16) must match only a positive amount.
- No condition tag for a counter being removed.
