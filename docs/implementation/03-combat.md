# 03-combat

## Slice: declare-attackers-blockers (merged)

- Branch `phase5/declare-attackers-blockers`.
- Scope: CR 508.1/509.1 declarations, no damage; CR 302.6/602.5a summoning sickness.
- Verify: 22 new tests + 1 regression test, 71 total, 0 failed.
- Key changes:
  - `CombatState { attackers, blocks, declared_defenders }` later evolved from `blockers_declared: bool`.
  - Summoning sickness: `GameObject.controlled_since_turn: u32`, `GameState.last_active_turn_number: HashMap<PlayerId, u32>`, `commands::has_summoning_sickness(state, object)`.
  - `DeclareAttackers`: atomic batch validation, taps attackers, emits `AttackerDeclared` + `Tapped`.
  - `DeclareBlockers`: atomic validation, defending player(s), emits `BlockerDeclared`; blocking doesn't tap.
  - `DeclareAttackers`/`DeclareBlockers` skip priority check (turn-based actions).
- Deviations:
  - `submit_action` special-cased these to skip priority check.
- Unresolved:
  - Combat damage, first/double strike, costs to attack/block, banding, restrictions/requirements, multiplayer, haste, step-skip on zero attackers.

## Slice: combat-damage (merged)

- Branch `phase5/combat-damage`.
- Scope: CR 510 assign+deal as one action.
- Verify: 10 new tests, 91 total, 0 failed.
- Key changes:
  - `PlayerActionKind::AssignCombatDamage { assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)> }`.
  - `PlayerState::lose_life` real body.
  - `AssignCombatDamage`: validates active player, correct step, atomic batch; only multi-blocked attackers (2+ blockers) need submitted splits.
  - Damage to player calls `lose_life`; to creature marks `damage_marked`.
  - `damage_marked` cleared entering Cleanup (CR 514.2).
- Unresolved:
  - First/double strike, deathtouch, CDA power, continuous P/T pump, damage prevention/replacement, planeswalker/battle damage, multiplayer.

## Slice: first-strike (merged)

- Branch `phase5/first-strike`.
- Scope: CR 510.1a/702.4/702.7 first/double strike.
- Verify: 6 new tests, 482 total, 0 failed.
- Key changes:
  - `Step::FirstStrikeDamage` added between `DeclareBlockers` and `CombatDamage`.
  - `KeywordSet::FIRST_STRIKE`/`DOUBLE_STRIKE` (bits 12/13).
  - `advance_turn_step` inserts `FirstStrikeDamage` if any attacker/blocker has keyword, else goes straight to `CombatDamage`.
  - `participates_in_damage_step(keywords, step)`; attacker keywords decide which step its damage lands, blocker keywords only decide if blocker strikes back that step.
  - `alive_blocks` separate from `blocks` so first-strike attacker can still damage a non-first-strike blocker.
  - Fixed: missing ids filtered as ineligible rather than erroring.
- Unresolved:
  - Attacker removed from combat bookkeeping beyond damage consequence; continuous-effect-granted keywords already work via `effective_keywords`.

## Slice: trample (merged)

- Branch `phase5/trample`.
- Scope: CR 702.19a/b/d trample.
- Verify: 9 new tests, 647 total, 0 failed.
- Key changes:
  - `KeywordSet::TRAMPLE` (bit 14).
  - Fixed pre-existing bug: `declared_blocked_attackers` tracked separately from alive blockers; blocked attacker with dead blockers deals no damage unless trample.
  - `effective_toughness`, `lethal_damage_needed` helpers (factoring deathtouch).
  - Single trampling blocker now requires lethal-vs-excess split; remainder spills to original target.

## Slice: combat-evasion-keywords (merged)

- Branch `phase5/combat-evasion-keywords`.
- Scope: Fear/Intimidate/Shadow/Horsemanship (CR 702.36/702.13/702.28/702.31).
- Verify: 12 new tests, 0 failed.
- Key changes:
  - `KeywordSet` widened from `u16` to `u32`; added `FEAR`, `INTIMIDATE`, `SHADOW`, `HORSEMANSHIP`.
  - `DeclareBlockers` per-pair checks for each keyword using `effective_keywords`/`effective_characteristics`.
- Unresolved:
  - Landwalk, Banding, Flanking/Rampage.

## Slice: landwalk (implemented, not yet merged)

- Branch `phase5/landwalk`.
- Scope: CR 702.14 Landwalk, narrowed to single-subtype matching (the 5 basic landwalks).
- Verify: 4 new tests, full workspace suite green.
- Key changes:
  - Not a `KeywordSet` bit (unlike the other evasion keywords) - parametrized by land subtype, same shape as protection: `EffectPrimitive::AddLandwalkSubtype(Subtype)` (layer 6), `EffectiveCharacteristics::landwalk_subtypes: Vec<Subtype>`, `Game.Effects.AddLandwalkSubtype(name)` Lua binding.
  - No printed field on `CardDefinition` - always effect-granted, same "no printed-protection field" precedent; a card with printed Islandwalk self-registers an always-on static ability via its script.
  - `DeclareBlockers` per-pair check (alongside Fear/Intimidate/Shadow/Horsemanship): if the attacker's `landwalk_subtypes` is non-empty and the *defending player* (`command.actor`) controls any battlefield permanent whose effective subtypes intersect it, the block is rejected outright - unlike the other evasion keywords, this checks the defending player's battlefield, not the blocker's own characteristics.
- Unresolved:
  - CR 702.14a's card-type/supertype-combination forms ("artifact landwalk," "nonbasic landwalk," "snow swampwalk").
