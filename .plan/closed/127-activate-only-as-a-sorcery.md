---
id: 127
title: Activate only as a sorcery on any ability
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

How does an activated ability other than Equip declare "Activate only as a sorcery" (CR 602.5d), so that `activate.rs` and `entries.rs` read one rule and Lua can set it?

## Notes

- Origin: "Timing permission follow-ups". 521 printed cards use the phrase, by a query of `../stackfish-db/stackfish.db`. No engine field or Lua setter exists.
- `types/ability.rs:480` says the check matches `AbilityEffect::Attach` to avoid a field on about 94 `RegisteredAbility` literals, and to make it a field once a second ability needs the timing. This is that second ability.
- Decide the field's name and type, whether Equip moves onto it, and how it reads the `TimingSubject` from the instant-speed equip build.
- Also weigh "Activate only during your turn" (82 hits by a broader pattern) and whether one timing field covers both.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

Activation restrictions become a field on the ability. Equip moves onto that field automatically, and the work splits into two builds. Decided in auto mode.

The card counts come from oracle text in `../stackfish-db/stackfish.db`. 529 cards say "Activate only as a sorcery", and 14 of those are Equip, Fortify or Reconfigure. 78 say "during your turn", 53 "during your upkeep" and 108 "once each turn".

- Field: `RegisteredAbility` gains `activation: ActivationRestriction { sorcery_speed: bool, condition: Option<InterveningIfSpec> }`, deriving `Default`. All 176 literal sites get `ActivationRestriction::default()`. It is not a side map, because a field stays with the ability through clones, grants and remaps (CR 602.5b, 602.5c), and a side map can drift out of sync on remap, as in ticket 107. It reuses neither `condition`, the trigger-event filter, nor `intervening_if`, the CR 603.4 recheck. `InterveningIfSpec` matches the Timing permission condition from 098, and its doc gains one line saying it also serves as the activation condition.
- Sorcery timing stays a flag because it reads priority and the stack (CR 602.5d). "During your turn" is `condition = DuringYourTurn` with `sorcery_speed = false`. Upkeep, combat, declare blockers and "before attackers are declared" are step conditions over the snapshot (CR 602.5). "As an instant" needs no field (CR 602.5e).
- Equip: `Register` sets `sorcery_speed` when `effect == Some(AbilityEffect::Attach)`, which `load_operation` sets before `Register` runs. Equip scripts and the proof card stay unchanged. Fortify (CR 702.67a) gets the same default. The two `matches!(Attach)` checks at `activate.rs:129` and `entries.rs:304` go, and the `AbilityEffect::Attach` doc paragraph is rewritten. `docs/lua/activated-abilities.md` documents the default.
- Helper: `can_activate_now(state, runtime, player, ability)` in `commands/legality.rs`. The player needs priority. With `sorcery_speed` set, they also need `is_sorcery_speed`, unless the ability's effect is `Attach` and a `TimingSubject::EquipAbilities` permission covers them. Then the condition must hold. The `Attach` match is read only for that exemption. A TODO notes that an `EquipAbilities` permission also covers Fortify (CR 702.67a).
- Callers: the `ActivateAbility` arm and the `activate_entries` scan, after the zone check so error strings stay stable. `mana_ability_entries` reuses the scan, and payment validates against that list, so `check_mana_ability` needs no change. Mana abilities may carry a restriction (CR 605.3a), with no bypass. The check applies whatever zone the source is in, so it also covers abilities that use `:SetFunctionsFrom`.
- Check point: only when activation begins, never at resolution (CR 602.5). CR 608.2a covers triggers only, and casts are not rechecked either (098).
- Lua: `:SetSorcerySpeed()` and `:SetActivationCondition(condition)` on `ActivatedAbilityBuilder`. Update `docs/lua/index.md`, `docs/LUA_API.md`, `docs/lua/activated-abilities.md` and `docs/lua/stubs/game.lua`.
- Wire and replay: no protocol change. `host/fingerprint.rs` encodes definitions, not `RegisteredAbility`. `loop_watch.rs` hashes Debug text, so it includes the field automatically. `BEHAVIOUR_REV` does not move for the sorcery build.
- Condition scope: `state_to_lua` (`script/encode.rs:72`) exposes life, hand and library counts, graveyard, command, battlefield and exile cards with id, name, base power and toughness, a creature flag, controller and attachments. Build 125 adds the turn fields and `tapped`. So "during" shapes, "N or more cards in your graveyard" and "N or more creatures you control" are buildable. Types beyond creature, subtypes, colors, current power, spells cast and deaths this turn are not.
- Once each turn (108 cards, plus the combined sorcery and upkeep shapes) needs per-turn spent-use state keyed by ability, like `zone_permission_uses` in 097. It gets its own grilling ticket.

New tickets: 160 Build activation sorcery speed (blocked by 126), 161 Build activation conditions (blocked by 160), and 162 Activation limits per turn.
