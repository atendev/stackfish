---
id: 85
title: Zone permission riders
type: grilling
mode: HITL
status: closed
blocked_by: [82]
claimed: false
---

## Question

How does a permission carry "if it would leave the stack, exile it instead" or a similar after-cast rule (CR 608.2n, 702.34a)?

## Notes

- Origin 080. A spell cast through a zone permission follows CR 608.2n with no rider.
- Compare how the flashback exile clause works today, and decide whether the rider belongs on the stack object.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

A zone permission can carry a rider that sends the spell to exile instead of the graveyard. The rider is a per-cast fact, so it lives on the `StackObject`, not in a replacement effect. The permission only supplies the value at cast time. Today's `exile_on_leaving_stack: bool` becomes one field, `leave_stack_rider: Option<LeaveStackRider>`, and flashback and permissions both write it. `can_cast_from_zone` stops returning a bool and returns the matching permission's rider. A single helper replaces the three duplicated `if` blocks. Permanents are untouched. The proof card is Kess, Dissident Mage, tested by scenario fixture. Build is 088.

The full answer set is below.

- Carrier (Q1, Q6): `StackObject` in `types/stack.rs` holds the rider (CR 702.34a is a stack-object fact). Not a `ReplaceZoneDestination` entry, because that machinery would need a per-object condition and a cleanup when the spell leaves. The value is copied at the three push sites in `cast.rs` (:467, :490, :574) and survives the source leaving (CR 611.3b only retires future casts). A copy of a spell does not carry it (CR 707.10 copies ceasing to exist never reach a zone anyway); copy constructors set `None`.
- Type (Q3): `enum LeaveStackRider { ExileInstead, ExileInsteadOfGraveyard }`. `ExileInstead` is flashback's text, any leave (CR 702.34a). `ExileInsteadOfGraveyard` is the Kess wording, graveyard destinations only. `ZonePermission` gains `rider: Option<LeaveStackRider>`. The `AlternativeCost::Replacement` bool `exile_on_leaving_stack` stays as the Lua-facing flag and maps to `ExileInstead` at the cast.rs:367 extraction; decode.rs is unchanged. Not supported now: "if it would die or leave exile this way", battlefield-side exile (unearth), and escape. Each gets a TODO.
- Which permission applied (Q2): add `cast_zone_source(state, player, card) -> Option<ZoneCastSource>` in `commands/legality.rs` (`Hand`, `Command`, `Permission(Option<LeaveStackRider>)`). `can_cast_from_zone` becomes `.is_some()` so `legal_actions` is unchanged. When several permissions match, the rider-less one wins, else the first by id. This is deterministic and never worse for the player. TODO: no player choice of permission (CR 601.2).
- Composition (Q4): a flashback alternative cost and a permission can both apply to a graveyard card. The chosen alternative cost wins, since its `from_zone` is what the cast checks. `ExileInstead` beats `ExileInsteadOfGraveyard` when both could set it. Both exile, so the result is identical. Buyback stays as coded at CR 702.27a: it returns the spell to hand before any rider is read, so a rider never overrides buyback.
- Permanents (Q5): `permanent_entry.rs` never reads the rider on resolve, since a permanent spell goes to the battlefield (CR 608.3). `counter.rs` reads it for all spells: a countered permanent spell cast under `ExileInsteadOfGraveyard` goes to exile, which matches the card wording. Escape (CR 702.138a/b) is not a rider. "Escaped" is a separate permanent-side flag and stays as it is. TODO: escape counters (CR 702.138c).
- Exits (Q7): resolve (`permanent_entry.rs:34`), fizzle (`resolve/mod.rs:199`), counter (`counter.rs:50`). New `StackObject::leave_destination(&self, default: ZoneKind) -> ZoneKind` in `types/stack.rs` is the only reader. Copies cease to exist (CR 707.10) and ignore it.
- Lua (Q8): `ZonePermissionBuilder:SetRider("ExileInstead" | "ExileInsteadOfGraveyard")`, optional, default none, accepted for `Cast` kind only, rejected on `PlayLand`. Changes are in `script/builders.rs`, `script/types.rs` and the validation; update `docs/lua/stubs/game.lua`, `docs/lua/player-modifiers.md`, `docs/LUA_API.md`, `docs/DESIGN.md` and `docs/lua/game-constants.md` (new constant names).
- Tests (Q9): in `tests/zone_permissions.rs`: rider cast then resolve, counter and fizzle each go to exile; no rider goes to the graveyard; permanent spell resolves to the battlefield and is exiled when countered; flashback plus permission; source gone after cast; two permissions, rider-less wins. Plus a Lua round trip in `tests/script_zone_permission.rs`, and a hash-walk lint pass for the new field. Encoding `None` as the old `false` keeps the replay pin; bump `BEHAVIOUR_REV` only if it moves.
- Proof (Q10): Kess, Dissident Mage via scenario fixture only. The proof deck stays unchanged. TODO: Kess's once-per-turn limit is not represented, and no ticket owns it yet.
- TODOs: `// TODO: no player choice between matching permissions (CR 601.2)`, `// TODO: once-per-turn permission limit`, `// TODO: rider on a permanent spell's battlefield entry`.
- Terms: `CONTEXT.md` adds "Leave-stack rider" and edits "Zone permission" to say it may carry one.
