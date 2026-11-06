---
id: 91
title: Divide damage or counters among targets
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

How does a spell or ability divide an amount among its targets as it is cast (CR 601.2d)? Today each legal target takes the full stated amount, so "deals 3 damage divided as you choose among any number of targets" cannot be played correctly.

## Notes

- Origin: backlog item 8. TODOs at `commands/cast.rs:338`, `commands/legality.rs:825`, `engine/resolve/mod.rs:289` and `tests/multi_target_resolution.rs:2`.
- Decide where the division is announced, what the Answer carries, how the entries and `legal_actions` describe the amount, what happens when a target becomes illegal (CR 608.2b keeps each legal share), and the Lua spelling.
- Combat damage assignment already has `AssignDamage` Decisions (`docs/implementation/17-legal-actions.md`). Decide whether to reuse that reply shape.
- Also decide whether counters ("distribute three +1/+1 counters") share the mechanism.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

Decided in an auto-mode grilling run. A subagent stood in for the user and took every recommendation, adding a few constraints of its own. The build is "Build divided damage and counters".

**Announced in the action.** `CastSpell`, `ActivateAbility` and the trigger-target answer (`submit_trigger_targets`) gain `division: Vec<u32>`, parallel to `targets` (CR 601.2d, with 602.2b and 603.3d applying it to abilities). There is no separate Decision. `StackObject` stores it index for index with `targets`, an empty list meaning undivided or a single target, and a copy that keeps its targets keeps the division (CR 707.10). It must round-trip through save and replay serialization. `ReplyShape::AssignDamage` is not reused, because it is a resolution-time split keyed attacker to blockers. One pure helper, `validate_division(total, n_targets, division)`, serves all three entry points.

**The effect declares it.** New `AbilityEffect::DividedDamage(total)` and `DividedCounters { kind, total }`, with `TargetRequirement` unchanged. The total is a fixed `u32`. TODO: an X-dependent total, "X damage divided" (CR 601.2d, 107.3). The validator must not read `x_value` for it.

**Valid division.** The length equals the target count, every share is at least 1 and the sum equals the total, so the target maximum is the smaller of the requirement's maximum and the total. With exactly one target the field must be empty and the share is the full total. `[total]` is rejected as a wrong length, so there is one wire form. With no targets the field must be empty. A division on an undivided effect, or one of the wrong length, is rejected. A divided effect with a total of 0, or a requirement whose minimum is above the total, is a load error. Duplicate targets stay rejected by the existing dedup.

**Entries.** `TargetSlot` gains `divide_total: Option<u32>`, and its `max` is capped at the total. `legal_actions` never lists compositions. The Seat or bot builds the split. There is no affordability check.

**At resolution.** Each still-legal target keeps its announced share, an illegal target's share is lost with no redistribution, and the spell fizzles only when every target is illegal (CR 608.2b). One `DamageDealt` or counter event per share, in target order, so prevention and replacement apply per target. The events are all pushed before state-based actions run, as the existing multi-target `Damage` arm does, so the shares land together. The build cites a rule for this only after checking its text.

**Counters share the mechanism.** One `division` field and one validation, two effect variants. `DividedCounters` is object-only because `CounterAdded` carries an `ObjectId`, and no printed card distributes counters among players. A requirement that is not `TargetFilter::Object` (`Player` or `AnyTarget`) is a load error.

**Scope.** A divided effect uses exactly one `Primitive` target requirement, and a multi-entry `TargetSpec` with one is refused at load. In a modal spell the division runs along the flat target list and is checked per chosen mode chunk, with two divided modes validated separately. TODO: divided amounts in function and chain operations (CR 601.2d). No redirect effect exists in the engine today. Ticket 103 must keep the announced division when it adds one. TODO: a copy that chooses new targets re-announces its division (CR 707.10c). No spell-copy effect exists to retarget yet, so the build adds the TODO beside the copy code and nothing more. TODO: untargeted division chosen at resolution (CR 608.2d), which is a separate rule from 601.2d.

**Lua.** `Game.Ops.DividedDamage(total)` and `Game.Ops.DividedCounters(kind, total)`, used like `Damage` with `s.target_requirement()` or `:SetTarget`, with no per-share values in the script. `DealDamageTo` and `AddCountersTo` stay independent per call, and the docs say they do not divide.

**Wire and bot.** `division` is an additive skip-if-empty field, `PROTOCOL_VERSION` stays 1, and a test checks that an action with no `division` still deserializes. The random bot picks a random target count up to the smaller of the maximum and the total, then a random composition with each share at least 1. A "divided as you choose" damage card joins a proof deck. The replay pin and `BEHAVIOUR_REV` are re-checked.

**Tests.** Sum mismatch, a zero share, wrong length, a single target with an empty division, a partial-illegal resolution that keeps the other shares, an all-illegal fizzle, a modal spell with a divided chunk, a divided activated ability, a divided trigger, counters with a player target rejected, a total of 0, a minimum above the total, `[total]` rejected for one target, a division on an undivided effect, and a division round-trip through save and replay.

**Cleanup and docs.** The four TODOs named in Notes (`cast.rs:338`, `legality.rs:825`, `resolve/mod.rs:289`, `multi_target_resolution.rs:2`) are each resolved by this build, so they are removed, and the plain `Damage` arm comment says each legal target takes the full amount (CR 608.2b). Every new TODO above stays. Update `docs/lua/stubs/game.lua`, `game-constants.md`, `targeting-and-choices.md` (the "still isn't modeled" notes), `scenario_format.md` if a scenario field is added, `docs/implementation/07-targeting.md` and `DESIGN.md`, and keep `lua_api_doc_sync` passing. **Split.** One build covers spells, activated abilities and triggers, because all three go through `validate_targets`. If the diff grows too large, the fallback is spells and abilities first and the trigger answer second.
