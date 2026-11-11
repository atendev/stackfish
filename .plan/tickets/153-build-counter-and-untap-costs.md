---
id: 153
title: Build counter and untap costs
type: task
mode: AFK
status: open
blocked_by: [152]
claimed: false
---

## Question

Build `CostSubject { Source, Chosen { filter, count } }` with `RemoveCounters { subject, kind, amount }`, `PutCounter { kind, amount }` and `UntapSelf`.

## Notes

- Origin 123. The full decision is in `closed/123-cost-shapes-past-the-current-set.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- `RemoveCounters` pays one `CostPayment::RemoveCounter(ObjectId, CounterKind)` per counter. `kind: None` takes a counter of any kind. Validation refuses more counters of a kind than the object has.
- `UntapSelf` needs a tapped source (CR 107.6) and, on a creature, CR 302.6 as `{T}` does.
- `amount` is `CostAmount::Fixed` only here. X comes in "Build X-tied cost counts".
- Lua API change: update `docs/lua/game-constants.md` and `docs/lua/stubs/game.lua`. Add the wire arm additively, with `PROTOCOL_VERSION` left at 1.

## Resolution

<Filled in at close.>
