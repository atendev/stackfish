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

## Slice: landwalk (merged)

- Branch `phase5/landwalk`.
- Scope: CR 702.14 Landwalk, narrowed to single-subtype matching (the 5 basic landwalks).
- Verify: 4 new tests, full workspace suite green.
- Key changes:
  - Not a `KeywordSet` bit (unlike the other evasion keywords) - parametrized by land subtype, same shape as protection: `EffectPrimitive::AddLandwalkSubtype(Subtype)` (layer 6), `EffectiveCharacteristics::landwalk_subtypes: Vec<Subtype>`, `Game.Effects.AddLandwalkSubtype(name)` Lua binding.
  - No printed field on `CardDefinition` - always effect-granted, same "no printed-protection field" precedent; a card with printed Islandwalk self-registers an always-on static ability via its script.
  - `DeclareBlockers` per-pair check (alongside Fear/Intimidate/Shadow/Horsemanship): if the attacker's `landwalk_subtypes` is non-empty and the *defending player* (`command.actor`) controls any battlefield permanent whose effective subtypes intersect it, the block is rejected outright - unlike the other evasion keywords, this checks the defending player's battlefield, not the blocker's own characteristics.
- Unresolved:
  - CR 702.14a's card-type/supertype-combination forms ("artifact landwalk," "nonbasic landwalk," "snow swampwalk").

## Slice: banding (merged)

- Branch `phase5/banding`.
- Scope: CR 702.22 Banding, narrowed to the plain form (not "bands with other [quality]").
- Verify: 5 new tests, full workspace suite green (including a rewrite of the pre-existing `AssignCombatDamage` handler).
- Key changes:
  - `KeywordSet::BANDING` (plain bit - no parameter needed for the base form, unlike Landwalk).
  - `CombatState::bands: Vec<Vec<ObjectId>>`, new `PlayerActionKind::DeclareBands` action (its own action rather than a field on `DeclareAttackers`, to avoid touching every existing `DeclareAttackers` call site - only legal during `Step::DeclareAttackers`, active player only, replaces `bands` wholesale). Validates CR 702.22c (>=1 banding member, <=1 non-banding member) and CR 702.22d/802.3b (every member shares one attack target).
  - `DeclareBlockers` (CR 702.22h): after the normal per-pair validation, a propagation pass forces every other member of a directly-blocked attacker's band onto the same blocker - bypassing every evasion/protection check already run (the CR 702.22h worked example blocks a swampwalker this way even though the defender controls a Swamp). Scoped to direct blocks in the same submission only (CR 702.22d already forces one shared defending player per band).
  - `AssignCombatDamage` generalized rather than left attacker-only: a blocker forced onto 2+ attackers by band propagation (CR 702.22k) now needs the same kind of chosen split `requires_assignment` already modeled attacker-side, reusing the same `assignments: Vec<(ObjectId, Vec<(ObjectId,u32)>)>` shape (disambiguated by which of `blockers_by_attacker`/`attackers_by_blocker` contains the id). No trample-style carve-out on the blocker side - a blocker has no "spill to the player" outlet, so its split must sum to exactly its power.
- Unresolved:
  - CR 702.22b/c's "bands with other [quality]" variant (needs a parameter - same shape gap Landwalk's own removal from `KeywordSet` documents).
  - CR 702.22i (a band member blocked "due to an effect" forces the whole band blocked) - no such effect exists in this engine yet.
  - CR 702.22j (the defending player, not the active player, chooses an attacker's split when it's blocked by a creature with banding) is real now too, see `banding-split-actor` below.

## Slice: banding-split-actor (merged)

- Branch `phase5/banding-split-actor`.
- Scope: CR 702.22j/k - who chooses a combat damage split is no longer hardcoded to the active player.
- Verify: 3 new tests, full workspace suite green.
- Key changes:
  - `AssignCombatDamage`'s actor check moved from a hardcoded `command.actor == state.active_player` at the top of the handler to a per-split-id computation once `requires_assignment` is known: each id's required chooser defaults to the object's own controller (CR 510.1c/d - attacker splits by the attacker's controller, blocker splits by the blocker's controller), flipped to the *other* side's player when a `BANDING` creature is on the other end of that particular block (702.22j: attacker blocked by a banding creature -> defending player; 702.22k: blocker blocking a banding creature -> active player).
  - 702.22k's pre-existing band-forced-blocker case (`phase5/banding`) was already correct before this slice, but only by accident: every band needs >=1 banding member, so the old blanket-active-player hardcode always happened to match. Now it resolves for the actual documented reason.
  - No-split-needed damage steps keep the old convention (active player submits an empty batch) - every pre-existing zero-split `AssignCombatDamage` call site across the test suite is unaffected.
- Unresolved:
  - A damage step needing splits chosen by two different players *at once* (e.g. a plain multi-block plus a separate banding-blocked attacker, simultaneously) - CR 510.1e implies both players submit their own creatures' splits independently, which would need a real two-phase pause-and-resume protocol (a new `ProcessState`, similar to `AwaitingAttachChoice`). Deliberately narrowed instead: rejected outright with a clear `IllegalAction` rather than silently letting one player choose for both.

## Slice: bands-with-other (merged)

- Branch `phase5/bands-with-other`.
- Scope: CR 702.22b/c's "bands with other [quality]" - the second, quality-parametrized way to form an attacking band, left unmodeled by `banding` above ("needs a parameter, same shape gap Landwalk's own removal from `KeywordSet` documents").
- Verify: 5 new tests (`phase5_bands_with_other.rs`), full workspace suite green.
- Key changes:
  - `EffectPrimitive::AddBandsWithOther(Subtype)` (layer 6) / `EffectiveCharacteristics::bands_with_other_subtypes: Vec<Subtype>` / `Game.Effects.AddBandsWithOther(name)` - exact mirror of `AddLandwalkSubtype`'s existing shape (no printed `CardDefinition` field, always effect-granted; `RemoveAllAbilities` clears it alongside `landwalk_subtypes` and the other layer-6 accumulators).
  - `DeclareBands` now accepts a band matching *either* of CR 702.22c's two sentences: the pre-existing plain form (>=1 `BANDING` member, <=1 non-banding member), or the new quality form (>=1 member with `bands_with_other_subtypes` containing quality Q, every *other* member's own `subtypes` include Q - no banding-keyword requirement, no cap on member count). Tries every quality named by any member in turn; legal if any one of them makes every other member match.
  - Quality = creature subtype only, same narrowing precedent Landwalk set (single-subtype matching only).
- Unresolved:
  - CR 702.22c/m's supertype-quality form ("bands with other Legends," i.e. Legendary) - TODO, unmodeled, same class of gap Landwalk's own card-type/supertype-combination forms document.
  - CR 702.22b (losing the keyword loses bands-with-other too) - no effect in this engine removes a keyword yet, untestable, not built.
  - CR 702.22j/k's *other* trigger condition - a blocking creature with bands-with-other[quality] alongside another matching-quality blocker flips the combat-damage split chooser **independent of any declared band at all** (a pure blocker-side check at the damage step, not band-formation). Materially separate mechanic from this slice's scope; deferred to its own follow-up, matching the project's own precedent of splitting `banding` and `banding-split-actor` into two slices.
