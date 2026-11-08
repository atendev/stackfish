---
id: 99
title: Riders beyond leaving the stack
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

How are the riders 085 left out carried? Those are "if it would die or leave exile this way", battlefield-side exile such as unearth, and a rider on a permanent spell's battlefield entry.

## Notes

- Origin: 085 and 088, and the three TODOs in `legality.rs` and `types/stack.rs`. Escape counters (CR 702.138c) were also deferred.
- 085 put the rider on the `StackObject` because a spell-leaving rider is a per-cast fact. A rider that outlives the stack needs a different carrier, probably a permanent-side or replacement-effect record. Decide whether the two share a type.
- Read `docs/implementation/38-zone-permission-riders.md` and `closed/085-zone-permission-riders.md`.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

A Battlefield rider replaces one named permanent's move off the battlefield with exile. It lives in a new `GameState` list, apart from `state.replacement_effects` and from `LeaveStackRider`. The two rider kinds do not share a type, because they have different readers: `StackObject::leave_destination` for the stack and the replacement pass for the battlefield. Decided in auto mode, then approved by the user. Builds are "Build die and leave riders" and "Build permission entry rider".

- Row: `{ object, shape: Die | Leave, duration, granted_at: Option<Timestamp> }`, with no source. Nothing reads a source: a row ends with its object, and CR 616.1 asks the affected object's controller. Only `UntilEndOfTurn` and `Permanent` are accepted.
- Why not a `ReplacementEffect` row: rows have no duration, die with their source (`retire_object`), cannot name another object, and are found by `source` (`apply_one_replacement`). Fixing that changes the `Registered` wire shape and every row literal. Why not a field on `GameObject` or a wider `LeaveStackRider`: both skip the CR 616.1 choice against regeneration and other replacements.
- Matching: Die matches a `ZoneChange` from Battlefield to Graveyard for the object from any cause (CR 700.4). Leave matches any `ZoneChange` from Battlefield for the object whose destination is not Exile.
- CR 616.1: rows join `matching` in `apply_replacements_from` through a new `ReplacementCandidate` variant keyed by the object, as `Protection` joins. One candidate per object, however many rows match, because every printed rider sends the object to exile. A rider with another destination would need a per-row id, and the build keeps a TODO for it. Candidates are re-evaluated after each choice (CR 616.1f), so a Die row no longer matches an event another effect redirected off the graveyard.
- Wire: one additive `WireCandidate` arm in `crates/host/src/wire/mod.rs`, with the matching arms in `decision/answer.rs`, `decision/open.rs` and `apply_one_replacement_or_protection`. `GameState` has no serde, so nothing else serializes. The list joins the `loop_watch.rs` fingerprint (CR 104.4b).
- Lifetime: `retire_object` drops rows whose object departs (CR 400.7). A source leaving drops nothing (CR 611.2a, 611.3d). `UntilEndOfTurn` rows drain in `begin_cleanup_step` (CR 514.2).
- Lua: `Game.Ops.ExileIfWouldDieThisTurn(opts)` works inside `Chain` on the operation's targets, and creates a row only for an object still on the battlefield. With `{ dealt_damage = true }` it covers each object the resolving source dealt more than 0 damage to during this resolution, read from the ledger, so prevented damage gives no rider. `exile_if_leaves = true` on `ReturnToBattlefield` and `ReturnAllToBattlefield` creates a `Permanent` Leave row with the new id. No generic `ExileIfWouldLeave` until a printed card needs one, since Magar of the Magic Strings needs face-down play.
- The Eighth Doctor: `ZonePermission.exile_if_leaves: bool` joins option identity. A cast copies it to a new `StackObject` field read at `permanent_entry`, and a land play reads it directly. The field only carries the choice from the cast to `permanent_entry`, where the row is built, so it is not a rider reader and does not skip CR 616.1. The row names the battlefield `ObjectId` (CR 611.3d, 400.7b). A countered spell and a copy of a spell get no row.
- Layer 6: `granted_at` is `None` for a rider from a resolved effect (Suplex, Whip of Erebos), which ability removal does not touch. It is `Some(t)` for an ability granted by a static permission. A `Some(t)` row matches only if no `RemoveAllAbilities` effect applying to the object has a later timestamp than `t` (CR 613.1f, 613.7). `t` is the later of the granting permanent's timestamp and the moment the granted permanent enters (CR 613.7a). The entry is a chosen reading of "the effect that created the ability", so the build states it and pins the test to it. The check scans the layer 6 effects applying to the object, as `applicable_effects_in_layer` selects them, and never runs the full layer computation from the replacement pass or calls back into it.
- TODOs for the builds: `// TODO: effects that say an object can't have an ability do not stop a granted rider (CR 613.1f)`, since the engine cannot express that effect, and the per-row id above.
- Not this ticket: unearth's and Whip of Erebos's end-step exile, which need delayed triggers (CR 603.7a), a new grilling ticket. A "leave exile" rider has no printed card, so that phrase in the Question was a wording slip. Escape counters are built (`EntersWithCountersEscape`), and only the linked trigger stays open.
- Tests: a Suplex-style chain, a damage rider with prevented damage giving no rider, a Whip-style return proving only the leave rider, a token exiled with no dies trigger, a CR 616.1 choice against regeneration and against a redirect to hand, a Die row plus a Leave row giving one candidate, end-of-turn expiry, a rider surviving its source, and an ability-removal effect after entry stopping a granted rider while one before entry does not. Scenario fixtures, no proof deck change. `BEHAVIOUR_REV` moves only if the replay pin moves.
- Printed card counts from `stackfish.db` oracle text, approximate: 58 "would die this turn, exile it instead", 15 effect-made "would leave the battlefield, exile it" plus 64 Unearth, and one permission rider, The Eighth Doctor.
- Terms: `CONTEXT.md` adds "Battlefield rider".
