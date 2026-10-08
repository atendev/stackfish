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

## Hexproof and Shroud vs attachment (`phase5/hexproof-shroud`)

`target_is_legal` now also rejects hexproof/shroud (`target_blocked_by_hexproof_or_shroud`). The
attachment-legality callers use `attachment_target_is_legal` instead, a sibling that skips that
check but keeps protection: both `sba.rs` Aura rechecks (`collect_illegally_attached_auras`), the
Equipment recheck (`collect_illegally_attached_equipment`), and `attach::legal_aura_entry_candidates`
(CR 303.4f entry). An Aura permanent doesn't target (CR 115.1b), and SBA or entry attachment isn't
targeting, so a hexproof or shroud creature keeps its Auras and Equipment. Protection still
unattaches them (CR 702.16d). Casting an Aura spell and activating Equip do target, so they go
through the full `target_is_legal`.

## Aura attach (CR 303.4)

- `GameObject.attached_to`/`attachments` bidirectional; `commands::attach_permanent` is the sole writer.
  CR 303.4b's "or player" half is real now too (`phase5/aura-enchants-a-player`, see that slice's own
  note below) — `GameObject.attached_to_player: Option<PlayerId>`, a sibling field, mutually
  exclusive with `attached_to`.
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
- Scoping notes: player candidates (CR 303.4b: an Aura can enchant a player too) were out of scope
  at the time - `attached_to`/`attach_permanent` were `ObjectId`-only. Enchanting a player at all is
  real now (`phase5/aura-enchants-a-player`, below); this specific multi-candidate *choice* function
  is still object-only, since no producer generates a mixed object/player candidate set either.
- Unresolved: CR 303.4f/g/h/i, CR 303.4k, CR 303.4m (see the note above, unchanged by this slice);
  player-candidate attach choice (still dormant, same reasoning as the object case).

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

## Slice: enchanted-reference (merged)

- Branch `phase5/enchanted-reference`.
- Scope: CR 303.4m — "an ability of a permanent that refers to the 'enchanted [object or player]'
  refers to whatever object or player that permanent is attached to." `GameObject.attached_to`
  already existed and was correctly maintained by the attach machinery, but nothing read it for
  continuous-effect matching — only SBA (CR 704.5m/n) ever consulted it, so "Enchanted creature gets
  +1/+1" had no real implementation path at all before this slice.
- Verify: 8 new tests (`phase5_enchanted_reference.rs`), full workspace suite green.
- Key changes:
  - `AffectedFilter::EnchantedOnly` — a new fieldless variant (sibling to `Primitive`/`Script`), not a
    field on `EffectFilter` (129 struct-literal construction sites with no `Default` impl — the same
    ripple class the modal-spell slices already hit and avoided each time by adding new enum variants
    instead). `ContinuousEffect::applies_to` resolves it directly as `source.attached_to ==
    Some(object.object_id)` — both are already full `&GameObject`s at that call site, no `GameState`
    lookup needed, unlike `layers::applicable_effects_in_layer`'s own `Script` arm.
    `applicable_effects_in_layer`'s outer match grew one arm (`Primitive | EnchantedOnly` both call
    `applies_to`).
  - `ScriptRuntime::invoke_continuous_affects`/`invoke_continuous_effect` both gained a trailing
    `enchanted: Option<ObjectId>` argument (nil in Lua when `None`), computed by the caller from the
    effect's own `source.attached_to` — appended *after* the existing `state` snapshot argument so no
    existing Lua closure signature breaks (Lua ignores a trailing arg a function doesn't declare).
    Lets a scripted Aura reference the enchanted permanent's id for logic beyond a plain identity
    match (e.g. a dynamic value keyed off it).
  - `script::decode_affected_value` recognizes `{ enchanted_only = true }` from `:SetAffected`,
    short-circuiting to `AffectedFilter::EnchantedOnly` before `controller`/`card_types`/`self_only`
    are even read — same "no real card needs both" narrowing `self_only` itself already accepts.
  - `card_summary_to_lua` now sets `attached_to`/`attachments` on every card summary in a `state_to_lua`
    snapshot, for general board-state visibility from any script (not just continuous effects).
- Scoping notes: fieldless by design — `EnchantedOnly` can't combine with `controller`/`card_types`
  the way `Primitive` can, since no real "enchanted creature that's also a [type]"-filtered *affects*
  clause exists to justify it. CR 303.4m for a permanent enchanting a *player* stays unmodeled even
  after `phase5/aura-enchants-a-player` made enchanting a player possible at all — this engine's
  continuous-effect machinery (`AffectedFilter`/`ContinuousEffect::applies_to`) only ever affects
  objects, not player-level static abilities, a separate and larger gap that slice deliberately
  deferred rather than touched. A *replacement* effect's own "enchanted permanent" reference (e.g.
  Umbra armor, CR 702.89a: "if enchanted permanent would be destroyed...") stays unmodeled too —
  `ReplacementEffect.affected: EffectFilter` is a different, non-`AffectedFilter` type; out of scope
  for this slice (continuous effects only).
- Unresolved: CR 303.4i/k (unchanged, see above); player-side CR 303.4m; replacement-effect "enchanted
  permanent" references.

## Slice: aura-enchants-a-player (merged)

- Branch `phase5/aura-enchants-a-player`.
- Scope: CR 303.4b's "or player" half of Aura attachment — the pre-existing gap named by
  `aura-attach-choice`, `aura-enters-attached`, and `enchanted-reference` above, all of which
  deferred it as "a separate, larger, pre-existing gap."
- Real finding that scoped this slice: casting an Aura *at* a player already technically passed
  cast-time legality (`TargetFilter::Player` was already fully generic, checked by
  `commands::target_is_legal` regardless of caller), but `ResolveStackTop`'s attach branch only ever
  matched `Target::Object` — a chosen `Target::Player` silently fell through, the Aura entered the
  battlefield completely unattached, and the pre-existing CR 704.5m SBA check (`attached_to: None`
  is illegal) killed it the very next pass. So "Enchant player" was a card that resolved and
  instantly died for no visible reason, not a card that couldn't be cast at all.
- Design confirmed with the user before implementing (a bigger surface than a narrow sibling
  addition, so scoped explicitly rather than assumed): `GameObject.attached_to_player:
  Option<PlayerId>` as a new sibling field, not a widened `attached_to: Option<Target>` — the latter
  would force every existing object-attach call site (`attach_permanent`, every SBA/layer/
  replacement check already reading `attached_to` as a plain `ObjectId`) to handle a case that can
  never apply to them, for zero benefit. Same "additive sibling, not a widened payload" precedent
  this backlog's own `ManaUnit`/`EffectPrimitive` variants already established.
- Verify: 6 new tests (`phase5_aura_enchants_a_player.rs`: casting at a player attaches it and logs
  `AttachedToPlayer`; a healthy player-attached Aura is unaffected by SBA; one dies via SBA if its
  player becomes eliminated; the pre-existing "attached to nothing" SBA check still fires with the
  new field present; a scripted player-filter re-check makes it fall off/stay attached correctly),
  full workspace suite green, zero new warnings.
- Key changes:
  - `GameObject.attached_to_player: Option<PlayerId>` — new field, ~8 non-test construction sites
    updated (mechanical, compiler-forced) plus 2 test files.
  - `commands::attach_permanent_to_player(state, attaching_id, player)` — CR 303.4b's sibling to
    `attach_permanent`, no bidirectional `PlayerState`-side list (nothing needs to enumerate "every
    Aura attached to this player" yet — see `enchanted-reference`'s own note on why CR 303.4m for
    players is deferred separately).
  - `GameEventKind::AttachedToPlayer { object, player }` — new variant, sibling to `Attached`; both
    are pure log entries (not `CommitEvents`-dispatched), so this was low-ripple.
  - `ResolveStackTop`'s Aura-attach branch (`resolve/permanent_entry.rs`) now matches `Target::Player`
    alongside `Target::Object`, dispatching to the new attach function/event.
  - `sba::collect_illegally_attached_auras` extended: illegal if *neither* `attached_to` nor
    `attached_to_player` is set (previously just `attached_to.is_none()`), or if the attached player
    has left the game (`PlayerState.eliminated` — `target_is_legal`'s own `Player` arm only checks
    the player still *exists* in `state.players`, which stays true after elimination, so this needed
    an explicit check `target_is_legal` alone couldn't provide), or no longer matches the Aura's own
    `target_requirement` filter (reusing `target_is_legal` with `Target::Player`, same re-check shape
    the object case already had).
  - `card_summary_to_lua` gained `attached_to_player` alongside the existing `attached_to`.
- Deliberately deferred, named not silently dropped: CR 303.4f/g (entering already-attached via
  Search/token, not casting) for a player target — `legal_aura_entry_candidates` still only scans
  object candidates; CR 303.4d's multi-candidate choice mixing object and player candidates — still
  dormant, object-only; CR 303.4m's "enchanted player" reference for continuous/replacement effects
  — this engine's continuous-effect machinery only ever affects objects, a separate and larger gap
  (see `enchanted-reference`'s own updated note above).
- Real bug hit and fixed during this slice's own test-writing (not a repeat find, a second real
  instance of the same class `phase5/script-target-requirement-predicate` already logged): building
  a `CardDefinition`'s scripted `TargetFilter::PlayerScript` handle from one `ScriptRuntime` and then
  calling `check_state_based_actions` with a *different* one made the predicate always silently
  evaluate to "illegal" (`.unwrap_or(false)` swallowing the resulting "unknown script handle" error).
  Fixed by threading one shared `ScriptRuntime` through both the definition builder and the SBA call
  in the test. `99-backlog.md`'s "Mistakes caught" note was generalized to flag this as a recurring
  trap, not a one-off.

## Slice: aura-enters-attached-player (merged)

- Branch `phase5/aura-enters-attached-player`.
- Scope: CR 303.4f/g's "or player" half for the search-entry path — the exact gap
  `aura-enchants-a-player` named as deliberately deferred above (that slice only closed the
  cast-and-resolve path, not `SearchDestination::Battlefield::enters_attached`).
- Real finding: `commands::target_is_legal` was already fully generic over `Target::Object`/
  `Target::Player` for every relevant `TargetFilter` variant (`Player`, `PlayerScript`, `AnyTarget`)
  before this slice — the gap was entirely upstream, in three places that were object-shaped by
  construction: `legal_aura_entry_candidates`'s own scan (only walked `state.objects`, never
  `state.players`) and return type (`Vec<ObjectId>`), `ProcessState`/`StepStatus::
  AwaitingEntryAttachChoice`'s `candidates` field, and `Engine::submit_entry_attach_choice`'s
  `chosen` parameter.
- Verify: 4 new tests (`phase5_aura_enters_attached_player.rs`), full workspace suite green (all
  pre-existing `phase5_aura_enters_attached.rs`/`phase5_attach_entry_equipment_fortification.rs`
  tests updated for the new `Target`-shaped candidate type and still pass unchanged in behavior).
- Key changes:
  - `legal_aura_entry_candidates` (`commands/attach.rs`) now chains a `state.players` scan
    (`commands::non_eliminated_players`, wrapped as `Target::Player`) after the existing
    `state.objects` scan (`Target::Object`), both filtered through the same `target_is_legal` call —
    no new legality logic, just a second source of candidates. Return type widened to `Vec<Target>`.
  - `legal_entry_attach_candidates`'s `Option<Vec<ObjectId>>` widened to `Option<Vec<Target>>` to
    match — the Equipment/Fortification arm still only ever produces `Target::Object` (CR 301.5a/
    301.6 forbid attaching either to a player), unaffected in practice.
  - `ProcessState`/`StepStatus::AwaitingEntryAttachChoice`'s `candidates: Vec<ObjectId>` widened to
    `Vec<Target>` in both the internal (continuation-carrying) and public (caller-facing) variants.
  - `Engine::submit_entry_attach_choice`'s `chosen: ObjectId` parameter widened to `chosen: Target`;
    its body now matches `Target::Object`/`Target::Player` and dispatches to `attach_permanent`/
    `attach_permanent_to_player` (+ `Attached`/`AttachedToPlayer` event) respectively — the same
    two-armed shape `ResolveStackTop`'s cast-time attach branch already established.
    `continue_search_battlefield_entry`'s own 1-candidate immediate-attach arm got the identical
    two-armed match.
  - `commands::attach_permanent_to_player` and `commands::target_is_legal` needed **no changes** —
    both were already complete/correct, just never reached from this call site before.
- A `TargetFilter::Player` has no controller-restriction field (no engine-side "you"/"opponent"
  model for it), so it unconditionally offers *every* player including the searching player's own
  controller — never exactly one candidate in a 2-player game. The "1 candidate, auto-attach"
  test needed a scripted `PlayerScript` opponent-only predicate to force that shape; a plain
  `TargetFilter::Player` in a 2-player game is actually the "2 candidates, pauses" case.
- Deliberately deferred, restated as non-goals rather than silently dropped: mixed object-and-player
  candidates in one Aura's choice (this engine's `TargetFilter` has no "object or player" variant,
  so no producer can create a mixed set); token creation's own CR 303.4f/g support (`AbilityEffect::
  CreateToken`/`resolve/tokens.rs` never touches `SearchDestination`/`legal_entry_attach_candidates`
  at all, for object or player targets alike — a pre-existing, separate gap, unaffected either way);
  CR 303.4m's "enchanted player" reference for continuous effects (unchanged, see
  `enchanted-reference`'s own note above).

## Slice: enchanted-player-replacement (merged)

- Branch `phase5/enchanted-player-replacement`.
- Scope: CR 303.4m's "enchanted player" reference for a *replacement* effect — the one real,
  CR-doc-cited "enchanted player" card left unbuilt after the `enchanted-player-hand-size`/
  `hand-reveal-modifier`/`draw-limit-modifier` cluster (none of which have a real card motivating
  their own `EnchantedPlayer` variant, confirmed while scoping this slice). CR 616's own worked
  example under replacement-effect ordering names it directly: **Wheel of Sun and Moon** — "If a
  card would be put into enchanted player's graveyard from anywhere, instead that card is revealed
  and put on the bottom of its owner's library." The player-attached sibling of `phase5/umbra-armor`'s
  object-attached `EventFilter::WouldDieEnchanted`.
- Verify: 6 new tests (`phase5_enchanted_player_replacement.rs`) — direct `matches()` unit checks
  (matches the enchanted player's own card regardless of source zone or `is_destroy`, rejects a
  different player's card, rejects a non-Graveyard destination, rejects an unattached source), an
  end-to-end SBA-driven death redirected to the bottom of the owner's library instead of the
  graveyard, a no-Aura control case, and a `Game.CreateReplacementEffect` self-registration test.
  Full workspace suite green.
- Key changes:
  - `EventFilter::WouldEnterEnchantedPlayerGraveyard` — fieldless, sibling to `WouldMoveZone` rather
    than a new field on it (same ripple-avoidance reasoning `WouldDieEnchanted` already gives).
    Resolves directly in `ReplacementEffect::matches`: `self.source`'s own `attached_to_player`
    against the moving card's own *owner* (CR 400.7 — graveyards are owner-keyed, not
    controller-keyed, unlike `WouldDieEnchanted`'s object-identity check). Not gated on `is_destroy`
    — CR 616's own wording ("from anywhere") covers a discard/mill the same as a death, same ungated
    stance the plain `WouldMoveZone` variant already takes.
  - **No new `ReplacementPayload`** — `ReplaceZoneDestination(Library)` is reused as-is. The real
    finding while scoping: `sba::commit_death`'s existing `to`-driven zone lookup
    (`ZoneKind::Battlefield | ZoneKind::Exile => None, _ => Some(owner)`) already resolves a
    `Library` destination against the moving card's own *owner* and `push_bottom`s it there —
    already-tested generic machinery (originally proven for the Exile case), not new plumbing. The
    end-to-end test passed on the first run with zero `sba.rs` changes.
  - "Revealed" isn't modeled — this engine has no per-viewer hidden-information boundary to reveal
    from in the first place (`VisibilityPolicy::is_visible_to` is itself still `todo!()`, no
    callers, same standing gap `HandRevealModifier`'s own doc comment already names) — there's
    nothing for "revealed" to mean beyond the zone move that already happens either way.
  - `Game.EventFilters.WouldEnterEnchantedPlayerGraveyard` / pairs only with
    `Game.ReplacementPayloads.ReplaceZoneDestination` — wired through
    `validate_replacement_filter_payload_pairing` the same way `WouldDieEnchanted`/`UmbraArmor`
    already are; `:SetAffected` is a no-op for this shape too (same reason).
- Unresolved: `HandRevealAffected`/`DrawLimitAffected::EnchantedPlayer` stay unbuilt — no real card
  motivates them, unchanged from before this slice. CR 303.4m's "enchanted player" reference for
  general *continuous* effects (as opposed to this slice's replacement-effect shape) remains a
  separate, larger, unaddressed gap — no shared mechanism links the two effect classes.

## Slice: enchanted-player-controls (merged)

- Branch `phase5/enchanted-player-controls`.
- Scope: CR 303.4m's "enchanted player" reference for a general *continuous* effect whose affected
  set is other objects, not the enchanted player's own characteristics — the gap named unclosed by
  every prior slice in this cluster. The real card: **Curse of Vitality** — "Creatures enchanted
  player controls have lifelink."
- Verify: 7 new tests (`phase5_enchanted_player_controls.rs`) — direct `applies_to()` unit checks
  (matches only the enchanted player's own objects, not the Aura's controller's; no match when the
  source has no `attached_to_player`; no accidental match against an unrelated object), a real
  `effective_characteristics` consumer test proving Curse of Vitality's own shape end to end, a
  change-of-control re-evaluation test (no caching — losing/gaining qualification tracks control
  changing hands live), and two `Game.CreateStaticAbility`/`:SetAffected({ enchanted_player_controls
  = true })` self-registration tests. Full workspace suite green.
- Key changes:
  - `AffectedFilter::EnchantedPlayerControls` — fieldless, sibling to `EnchantedOnly` rather than a
    field on `EffectFilter` (same ripple-avoidance precedent `EnchantedOnly` itself already set).
    Resolved directly in `ContinuousEffect::applies_to`: `source.attached_to_player.is_some_and(|p|
    object.controller == p)` — no `GameState` lookup needed, since `source: &GameObject` is already
    borrowed there, same "free" resolution shape `EnchantedOnly` gets (cheaper than the `state.
    object(m.source)` lookup `HandSizeModifier`/`HandRevealModifier`'s own `EnchantedPlayer` variants
    need, since those are evaluated somewhere that only has a bare `source: ObjectId`).
  - `layers::applicable_effects_in_layer`'s `AffectedFilter` match widened to include the new variant
    alongside `Primitive`/`EnchantedOnly` (all three route through `e.applies_to`).
  - `decode_affected_value`: `{ enchanted_player_controls = true }` short-circuits the same way
    `{ enchanted_only = true }` already does, checked second (so a table setting both keys
    deterministically prefers `enchanted_only` rather than erroring).
  - Fieldless and exclusive with `controller`/`card_types`, same narrowing `EnchantedOnly` already
    took — no real "creatures enchanted player controls that are also a [type]" card exists to
    justify combining this with `EffectFilter`'s other fields.
  - `docs/lua/static-abilities.md` updated with the new filter-table key alongside `enchanted_only`.
- Unresolved: combining with `EffectFilter`'s `card_types`/`subtypes`/`controller` (named above, not
  narrowed by mistake). `HandRevealAffected`/`DrawLimitAffected::EnchantedPlayer` remain a separate,
  unrelated, still-unbuilt gap (player-level, not object-level — no shared mechanism). This slice
  closes CR 303.4m's last named-unaddressed shape (player-level, replacement-effect, and now
  general-continuous-effect are all real) — CR 303.4m itself has no further known gap.

## Equipment / Equip (CR 702.6)

- `AbilityEffect::Attach` primitive; `ActivateAbility` enforces "only as sorcery" for `Attach`.
- `commands::attach_permanent` handles reattach safely for Equipment.
- `sba::collect_illegally_attached_equipment`/`commit_unattach` keeps Equipment on battlefield, unattached in place.
- Pre-attached battlefield entry (CR 301.5e/301.6, Fortification too) real now — see
  `phase5/attach-entry-equipment-fortification` above.
- `Game.Tokens.Treasure` deferred — needs coroutine color choice (later fixed by
  `phase5/treasure-token`; the coroutine premise was stale, `AddManaChoice` already resolves
  synchronously).

## Attach-related keywords

- `Flying`, `Reach`, `Menace` were first combat keyword checks.
- `Fear`, `Intimidate`, `Shadow`, `Horsemanship` added as evasion keywords in `DeclareBlockers`.
- `KeywordSet` widened to `u32` to fit new bits.
- `Landwalk` and `Banding` still open; Flanking/Rampage are triggered abilities, not blocking restrictions.

## Aura entry by reanimation (`phase5/reanimation`)

- `commit_reanimate` uses `legal_aura_entry_candidates` (the hexproof-free `attachment_target_is_legal`) before the
  Aura moves. Exactly one legal host attaches it (`Attached` or `AttachedToPlayer`); none leaves it in the
  graveyard (CR 303.4g). Two or more also leave it there for now: the controller's host choice (CR 303.4f) needs
  an entry-attach pause that doesn't depend on a stack object or a search. Equipment and other permanents enter
  unattached through this path.
