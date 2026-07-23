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
  since no attach call site can produce multiple simultaneous candidates. CR 303.4f/g/h are real now
  too, for Auras (`phase5/aura-enters-attached`, see that slice's own note below). Still open: CR
  303.4i (permanent-attachment-attempt-to-an-illegal-target, distinct from entry - no such effect
  exists), CR 303.4k (morph - no face-down mechanic exists), and CR 303.4m ("the enchanted X" as a
  script reference - no such reference plumbing exists yet, Aura-specific or generic).

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

## Slice: aura-enters-attached (merged)

- Branch `phase5/aura-enters-attached`.
- Scope: CR 303.4f/g/h — a permanent entering the battlefield already attached, by an effect other
  than resolving as an Aura spell or activating Equip. Closes the last open piece of CR 303.4 named
  by `phase5_aura_attach.rs`'s own doc comment ("no effect in this engine puts an Aura onto the
  battlefield except by resolving as a spell"), for the one real non-casting battlefield-entry
  producer that exists in this engine: `SearchDestination::Battlefield`. Scoped to Auras only —
  Equipment/Fortification pre-attached entry is a separate, still-open follow-up (Equipment's attach
  legality isn't governed by a `target_requirement` the way an Aura's enchant ability is, so it needs
  its own legality shape).
- Verify: 5 new tests (`phase5_aura_enters_attached.rs`), full workspace suite green.
- Key finding that reshaped the plan: CR 303.4f's "the player chooses what it will enchant as it
  enters" needed a real, *live* continuation of the search-resolution pipeline once answered — the
  pre-existing `ProcessState::AwaitingAttachChoice`/`submit_attach_choice` (built by
  `aura-attach-choice`) was dormant specifically because it had no such continuation ("there's no
  real pipeline stage to hand back to since nothing produces this pause yet" — its own doc comment).
  Rather than retrofitting that continuation onto a pause type named for a different CR clause (CR
  303.4d, "more than one candidate for an *already-targeted* attach"), this slice adds its own
  sibling pause, `ProcessState::AwaitingAuraEntryAttachChoice`, carrying everything
  `submit_search_choice`'s battlefield-entry loop needs to pick back up.
- Key changes:
  - `SearchDestination::Battlefield { tapped, enters_attached: bool }` — new field, `false` (the
    default/common case) leaves every plain fetch-land-style search unaffected. Meaningful only for
    a found Aura card (`commands::is_aura`); a no-op for anything else (CR 303.4h).
  - `commands::legal_aura_entry_candidates(state, script_runtime, aura_definition, controller,
    aura_source) -> Vec<ObjectId>` — scans every object in `state` for `target_is_legal` matches
    against the Aura's own `target_requirement`, evaluated fresh (unlike the cast-time path, no
    target was ever chosen to recheck). Reuses `target_is_legal` directly — no new legality logic.
    Player candidates (CR 303.4b) are out of scope, same posture `aura-attach-choice` already took.
  - `Engine::continue_search_battlefield_entry` — `submit_search_choice`'s old inline per-card loop,
    factored into its own method taking a `VecDeque<ObjectId>` of still-unprocessed chosen cards so
    it can be re-entered after a pause. For a `Battlefield { enters_attached: true }` Aura: 0
    candidates (CR 303.4g, current zone isn't the stack) skips the card entirely — it stays in the
    library, nothing moves; 1 candidate attaches immediately, no pause; 2+ candidates commits the
    zone-move (the Aura is already a real, unattached battlefield permanent — same order the
    cast-time Aura-ETB-attach path uses) then pauses via `AwaitingAuraEntryAttachChoice`, saving
    `remaining`/`destination`/`controller`/`stack_id`/`shuffle_after`/`events_so_far`.
  - `Engine::submit_aura_entry_attach_choice(chosen) -> Result<(), AttachChoiceError>` — validates,
    attaches, logs `Attached`, then calls `continue_search_battlefield_entry` again with the saved
    `remaining` queue, so a search with several chosen cards can pause and resume once per card that
    needs it (each pause is independent and sequential — never more than one active at a time, so no
    "two simultaneous choosers" problem the way `AssignCombatDamage`'s own banding note has).
  - `Game.Ops.Search`'s Lua builder gained `opts.enters_attached` alongside `opts.tapped`.
- Unresolved:
  - Equipment/Fortification pre-attached entry — named follow-up, closed by
    `phase5/attach-entry-equipment-fortification` below.
  - CR 303.4i (an effect specifies an *explicit* illegal enchant target, as opposed to CR 303.4f's
    "nothing specified") — genuinely different from what this slice built (nothing in this engine's
    `Search` shape can specify an attach target at all, only "let the player choose" or "none"), so
    still open, not silently narrowed into 303.4g's coverage.
  - CR 303.4k (morph), CR 303.4m ("the enchanted X" script reference) — unchanged, still open.

## Slice: attach-entry-equipment-fortification (merged)

- Branch `phase5/attach-entry-equipment-fortification`.
- Scope: CR 301.5e/301.6 — extends `aura-enters-attached`'s `SearchDestination::Battlefield {
  enters_attached }` machinery from Auras to Equipment and Fortification. Equipment/Fortification
  attach legality isn't `target_requirement`-shaped the way an Aura's enchant ability is — CR 702.6c
  already establishes that an Equip ability's own quality restrictions ("Equip [quality]") don't
  restrict what the Equipment may be attached to at all, so legality is just "every creature/land
  currently on the battlefield," no filter to evaluate.
- Verify: 4 new tests (`phase5_attach_entry_equipment_fortification.rs`), 9 existing Aura tests
  (`phase5_aura_enters_attached.rs`) updated for the rename below and re-verified green, full
  workspace suite green.
- Key finding that reshaped the plan: the 0-candidate case is a genuine behavior divergence, not
  just a renamed code path. CR 303.4g says a 0-candidate Aura remains in its current zone entirely
  (never enters the battlefield); CR 301.5e/301.6 say a 0-candidate Equipment/Fortification *still
  enters the battlefield*, just unattached. The old code's `0 => unreachable!(...)` arm (safe only
  because the Aura-only 0-candidate case was filtered out earlier via an early `continue`) had to
  become a real, reachable branch that does nothing (the object already entered normally by the time
  that match runs).
- Key changes:
  - `commands::is_fortification` — mirrors `is_equipment`'s ad hoc subtype-string-comparison shape.
  - `commands::legal_entry_attach_candidates(state, script_runtime, definition, controller, source)
    -> Option<Vec<ObjectId>>` — the new dispatch point, replacing the old direct call to
    `legal_aura_entry_candidates` at the one call site. `Some(...)` for all three attach-capable
    kinds (Aura delegates to the pre-existing `legal_aura_entry_candidates`; Equipment/Fortification
    scan `state.objects` for battlefield objects whose *effective* `card_types` — not the raw printed
    ones — intersect `CREATURE`/`LAND`), `None` for anything else, so the caller can tell
    "not attach-capable, always unattached, no scan" (CR 303.4h) apart from "attach-capable, zero
    live candidates right now" (`Some(vec![])`).
  - `Engine::continue_search_battlefield_entry` — the early "0 candidates → skip this card" branch
    is now gated on `commands::is_aura` specifically (CR 303.4g), not on "candidates is empty";
    Equipment/Fortification fall through to the normal move-onto-battlefield path, and the
    end-of-loop match's `0` arm is now a real no-op (enters unattached) instead of `unreachable!()`.
  - Renamed `ProcessState`/`StepStatus::AwaitingAuraEntryAttachChoice` →
    `AwaitingEntryAttachChoice` (field `aura` → `attaching`) and
    `Engine::submit_aura_entry_attach_choice` → `submit_entry_attach_choice`, since the pause is no
    longer Aura-exclusive — keeping the old name once it also serves Equipment/Fortification would
    have misled the next reader. Mechanical ripple: `crates/scenario/src/lib.rs`'s `drain` match arm.
- Unresolved: CR 801.9's range-of-influence carve-out (out of scope — this engine is 2-player only
  and models no ROI anywhere else either, not a new gap); CR 303.4i/k/m (Aura-specific, unchanged).

## Equipment / Equip (CR 702.6)

- `AbilityEffect::Attach` primitive; `ActivateAbility` enforces "only as sorcery" for `Attach`.
- `commands::attach_permanent` handles reattach safely for Equipment.
- `sba::collect_illegally_attached_equipment`/`commit_unattach` keeps Equipment on battlefield, unattached in place.
- Pre-attached battlefield entry (CR 301.5e/301.6, Fortification too) real now — see
  `phase5/attach-entry-equipment-fortification` above.
- `Game.Tokens.Treasure` deferred — needs coroutine color choice.

## Attach-related keywords

- `Flying`, `Reach`, `Menace` were first combat keyword checks.
- `Fear`, `Intimidate`, `Shadow`, `Horsemanship` added as evasion keywords in `DeclareBlockers`.
- `KeywordSet` widened to `u32` to fit new bits.
- `Landwalk` and `Banding` still open; Flanking/Rampage are triggered abilities, not blocking restrictions.
