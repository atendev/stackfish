# 14-protection-attach

## Protection (CR 702.16)

Enforced at damage, targeting, and blocking as an OR across six quality fields on `EffectiveCharacteristics`:

- `protection_colors: ColorSet` — `AddProtectionColor`
- `protection_card_types: CardTypeSet` — `AddProtectionCardType`
- `protection_subtypes: Vec<Subtype>` — `AddProtectionSubtype`
- `protection_names: Vec<String>` — `AddProtectionName`
- `protection_everything: bool` — `AddProtectionEverything`
- `protection_players: Vec<FilterController>` — `AddProtectionPlayer` (`You`/`Opponent`, rejects `Any`)

All applied in layer 6; `RemoveAllAbilities` clears all six fields. `apply_protection` runs inside `apply_replacements_from` so mid-resolution redirects are rechecked. `target_is_legal`/`DeclareBlockers` consult `target_has_protection_from`.

`target_is_legal` gained a `source: ObjectId` parameter to know the source's color/type/name.

Protection from player uses the protected object's own controller as the reference point; no true arbitrary `PlayerId` binding yet.

Only CR 702.16b (Enchant/Equip) remains open — needs attach machinery, not a protection-specific gap.

## Aura attach (CR 303.4)

- `GameObject.attached_to`/`attachments` bidirectional; `commands::attach_permanent` is the sole writer.
- `ResolveStackTop` attaches an Aura on battlefield entry to its first (only) legal target.
- `sba::collect_illegally_attached_auras` feeds `resolve_deaths` (non-destroy) when target is gone/illegal/protected.
- `retire_object` purges the departed object's id from its parent's `attachments`.
- `GameEventKind::Attached` logs in-place attach changes.
- CR 303.4d (`phase5/aura-self-enchant`): two further SBA checks alongside the above -
  `sba::collect_self_enchanted_auras` (an Aura attached to itself dies; only reachable via a
  directly-constructed fixture today, no attach call site can target an Aura at itself) and
  `sba::collect_creature_auras` (an Aura whose *effective* card types include Creature - layer 4
  `AddCardType` - unattaches via `commit_unattach`, shared with Equipment's own 704.5n handling,
  then dies in the same pass). CR 303.4e (only the Aura's own controller activates its own
  ability) was already correct by construction - `ActivateAbility`'s actor check reads the
  ability's registered source's controller, which is the Aura itself for the Aura's own ability -
  confirmed by a regression test, no code change needed. CR 303.4d's third clause ("can't enchant
  more than one object" - if a spell/ability would cause that, the Aura's controller picks which
  one) is real now too (`phase5/aura-attach-choice`, see that slice's own note below) - **dormant**,
  since no attach call site can produce multiple simultaneous candidates. Still open: CR 303.4f/g/h/i
  (a permanent entering the battlefield pre-attached by a non-casting, non-equip-activation effect -
  no such effect exists in this engine yet), CR 303.4k (morph - no face-down mechanic exists), and
  CR 303.4m ("the enchanted X" as a script reference - no such reference plumbing exists yet,
  Aura-specific or generic).

## Slice: aura-attach-choice (merged)

- Branch `phase5/aura-attach-choice`.
- Scope: CR 303.4d's third clause - "An Aura can't enchant more than one object or player. If a
  spell or ability would cause an Aura to become attached to more than one object or player, the
  Aura's controller chooses which object or player it becomes attached to." The last open piece of
  CR 303.4d, deferred by `phase5/aura-self-enchant` since it needed a "player picks one of several
  candidates" mechanism rather than another SBA scan.
- Verify: 3 new tests (`phase5_aura_attach_choice.rs`, external - `propose_attach` is `pub`) + 2 new
  tests inside `engine.rs`'s own `#[cfg(test)] mod tests` (the pause/resume half, since
  `process_state` is a private field), full workspace suite green.
- **Explicitly dormant**: no effect, spell, or ability anywhere in this engine ever computes more
  than one simultaneous attach candidate for one Aura - both real attach call sites
  (`ResolveStackTop`'s Aura-ETB-attach, `Equip`'s own resolution) already narrow to a single legal
  target via `TargetRequirement { count: 1 }` before this logic would ever run. This was a deliberate
  choice, confirmed with the user before building: the rule is genuinely possible in real Magic (some
  future redirect/duplicate-attachment effect could trigger it) even though nothing in this codebase
  produces the precondition today, so the machinery is built for real but only exercised by direct
  construction, not through any real card/spell pipeline.
- Key changes:
  - `commands::propose_attach(candidates: &[ObjectId]) -> AttachProposal` - pure decision function:
    0 candidates -> `None`, 1 -> `Attach(id)` (no choice needed), 2+ -> `NeedsChoice(candidates)`.
    `attach_permanent` itself stays single-target, unchanged - `GameObject.attached_to` is a plain
    `Option<ObjectId>`, not a set, so "attached to two things" was never representable even
    transiently.
  - `ProcessState::AwaitingAttachChoice { aura, controller, candidates }` /
    `StepStatus::AwaitingAttachChoice { player, aura, candidates }` - same "put the exact state back
    until answered" idiom every other choice-pause (`AwaitingLegendRuleChoice` etc.) already uses.
  - `Engine::submit_attach_choice(chosen) -> Result<(), AttachChoiceError>` - validates `chosen` is
    one of the offered candidates, then calls `attach_permanent` and resumes to the generic
    `PriorityWindow` (no `SbaResumeThen`-style continuation, unlike legend-rule/commander-return -
    there's no real pipeline stage to hand back to since nothing produces this pause yet).
  - `crates/scenario/src/lib.rs`'s `drain` match gained the new `StepStatus` arm (mechanical,
    compiler-forced by the new non-exhaustive variant).
- Scoping notes: player candidates (CR 303.4b: an Aura can enchant a player too) are out of scope -
  `attached_to`/`attach_permanent` are `ObjectId`-only, since this engine has no support at all yet
  for an Aura enchanting a player (a separate, larger, pre-existing gap, not touched here).
- Unresolved: CR 303.4f/g/h/i, CR 303.4k, CR 303.4m (see the note above, unchanged by this slice);
  player-candidate attach choice (blocked on the pre-existing Aura-enchants-a-player gap).

## Equipment / Equip (CR 702.6)

- `AbilityEffect::Attach` primitive; `ActivateAbility` enforces "only as sorcery" for `Attach`.
- `commands::attach_permanent` handles reattach safely for Equipment.
- `sba::collect_illegally_attached_equipment`/`commit_unattach` keeps Equipment on battlefield, unattached in place.
- `Game.Tokens.Treasure` deferred — needs coroutine color choice.

## Attach-related keywords

- `Flying`, `Reach`, `Menace` were first combat keyword checks.
- `Fear`, `Intimidate`, `Shadow`, `Horsemanship` added as evasion keywords in `DeclareBlockers`.
- `KeywordSet` widened to `u32` to fit new bits.
- `Landwalk` and `Banding` still open; Flanking/Rampage are triggered abilities, not blocking restrictions.
