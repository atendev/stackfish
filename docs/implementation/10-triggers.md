# 10-triggers

## Slice: damage-dealt-triggers (merged)

- Branch `phase5/damage-dealt-triggers`.
- Scope: CR 603 "whenever this deals/is dealt damage" conditions.
- Verify: 6 unit + 3 integration tests, 0 failed.
- Key changes:
  - `ConditionPrimitive::{DealsDamage, IsDealtDamage}`.
  - `Game.Conditions.{DealsDamage, IsDealtDamage}`.
- Unresolved:
  - Phase/step-boundary triggers (blocked by `AdvanceTurnStep` not running trigger collection).

## Slice: upkeep-triggers (merged)

- Branch `phase5/upkeep-triggers`.
- Scope: route normal step/phase transitions through `ReplacementCheck`→`CommitEvents`→`SbaLoop`→`CollectTriggers` so phase-boundary triggers fire.
- Verify: 8 unit + 3 integration tests, 0 failed.
- Key changes:
  - `AdvanceTurnStep` normal transitions set `pipeline_context`/`pipeline_exit` and route through `ReplacementCheck`.
  - `ConditionPrimitive::{BeginningOfYourUpkeep, BeginningOfEachUpkeep}`.
  - `ConditionPrimitive::matches` gained `controller`/`active_player` params.
  - `Untap` and skipped Draw step left on direct `log_events` path.
- Unresolved:
  - Cleanup trigger collection.

## Fix: Cleanup trigger collection

- `ProcessState::CleanupSbaCheck` routes through `CollectTriggers`/`PutTriggersApnap` via `PipelineExit::CleanupCheck { since_ledger }`.
- `finish_cleanup_sba_check` stashes departed abilities and enters trigger collection.
- Fixed `CleanupCheck` priority-vs-advance to also check `!stack.is_empty()`.

## Slice: dies-triggers (merged)

- Branch `phase5/dies-triggers`.
- Scope: CR 603.10 LKI fallback for "when this dies"/"leaves the battlefield".
- Verify: 6 unit + 5 integration tests, 333 total, 0 failed.
- Key changes:
  - `retire_object` returns `Vec<RegisteredAbility>` of the departed object's triggered abilities.
  - `commit_death` returns `(GameEvent, Vec<RegisteredAbility>)`.
  - `Engine.departed_triggered_abilities` transient stash; populated by SBA loop, consumed by `collect_triggers`/`PutTriggersApnap`, cleared at pipeline end.
  - `ConditionPrimitive::{Dies, LeavesBattlefield}` match by `last_known_info.object_id`.
  - `triggers::collect_triggers` second loop over departed abilities.
- Unresolved:
  - Sacrifice/discard-as-cost departure paths discarded at that time (fixed by draw-combat-end-triggers).
  - Filter-scoped "a creature you control dies" not supported.

## Slice: draw-combat-end-triggers (merged)

- Branch `phase5/draw-combat-end-triggers`.
- Scope: extend LKI stash to Sacrifice cost payment; add draw/combat/end step conditions.
- Verify: 18 unit + 9 draw/combat/end + 3 sacrifice-dies tests, 0 failed.
- Key changes:
  - `move_to_graveyard_for_cost`/`commit_payment` return departed abilities; `submit_payment` stashes them.
  - `ConditionPrimitive::{DrawsCard, EachDrawsCard, BeginningOfYourCombat, BeginningOfEachCombat, BeginningOfYourEndStep, BeginningOfEachEndStep}`.
  - `draw_step`'s library-side retirement also stashes departed abilities (unreachable in practice).
- Unresolved:
  - Cleanup discard/draw library retirement still discard departed abilities (cleanup fixed later, draw left as inert).

## Fix: once-per-turn trigger cap (CR 603.2h)

- `ConditionSpec::PrimitiveOncePerTurn(ConditionPrimitive)`.
- `GameState.once_per_turn_triggered: HashSet<AbilityId>` (was `HashSet<(ObjectId, u32)>` — rekeyed by
  `phase5/ability-stable-id`, see that slice's own note below), reset each turn.
- `PendingTrigger.once_per_turn: bool`; `CollectTriggers` records matches per event before next event in same batch.
- `Game.Conditions.OncePerTurn(tag)`.
- 3 tests.

## Fix: untap-step triggers

- `AdvanceTurnStep::Untap` routes through real pipeline but with `PipelineExit::AdvanceTurnStep` so no priority window opens (CR 502.4).
- `ConditionPrimitive::{BeginningOfYourUntap, BeginningOfEachUntap}`.
- 3 tests.

## Fix: submit_discard triggers

- `submit_discard` stashes departed abilities and routes through `CollectTriggers` with `PipelineExit::CleanupSbaCheck`.
- `ConditionPrimitive::{DiscardsCard, EachDiscardsCard}`.
- 2 tests.

## Slice: intervening-if (merged)

- Branch `phase5/intervening-if`.
- Scope: CR 603.4 intervening-if clause.
- Verify: 3 integration + 4 unit tests, 476 total, 0 failed.
- Key changes:
  - `InterveningIfPrimitive::{ControllerAtOrBelowLife(i32)}`.
  - `InterveningIfSpec::Primitive` (Script deferred).
  - `RegisteredAbility.intervening_if: Option<InterveningIfSpec>`; `StackObject.intervening_if` baked at push.
  - `collect_triggers` checks at trigger time; `ResolveStackTop` rechecks at resolution and ORs into fizzle.
  - `Game.InterveningIf.AtOrBelowLife(n)` and `:SetInterveningIf(tag)`.
- Deviations:
  - ~70 `RegisteredAbility` struct literals + 3 `StackObject` sites needed new field.
- Unresolved:
  - `InterveningIfSpec::Script` and more primitive shapes (later fixed by trigger-completeness-cleanup).

## Slice: trigger-completeness-cleanup (merged)

- Branch `phase5/trigger-completeness-cleanup`.
- Scope: close remaining trigger gaps: scry/surveil, filter-scoped dies/leaves, script intervening-if.
- Verify: 13 tests, 546 total, 0 failed.
- Key changes:
  - `ConditionPrimitive::{Scries, EachScries, Surveils, EachSurveils}`.
  - `ConditionPrimitive::{DiesMatching, LeavesBattlefieldMatching(FilterSpec)}` (live-source loop, not departed loop).
  - `InterveningIfSpec::Script(ScriptHandle)`; `ScriptRuntime::invoke_intervening_if(handle, state, source, controller)`.
  - `state_to_lua` snapshot passed as 4th arg to `invoke_condition` and used by `invoke_intervening_if`.
- Deviations:
  - `ConditionPrimitive`/`ConditionSpec` lost `Copy` due to `FilterSpec` field; `ConditionTag`/`OncePerTurnTag` dropped `Copy`.

## Slice: ability-stable-id (merged)

- Branch `phase5/ability-stable-id`.
- Scope: `PendingTrigger` used to reference its `RegisteredAbility` by position ("the nth ability
  registered to this source") — computed once at `collect_triggers` time but consulted later, after
  an `AwaitTriggerOrderChoice` pause and potentially arbitrary intervening game actions. A position
  is safe only as long as nothing ever removes/reorders a single ability from a still-live source's
  list independent of the others — true today (`retire_object` only ever purges a *whole* source's
  abilities in bulk) but not once CR 613.1f's "loses all abilities" (already flagged, unimplemented —
  see `06-abilities.md`) actually strips individual `RegisteredAbility` entries.
- Verify: 2 new unit tests inside `engine.rs`'s own `#[cfg(test)] mod tests`, full workspace suite
  green (0 failed).
- Key changes:
  - `AbilityId(u64)` (types/ids.rs) — minted via `GameState::allocate_ability_id`, which draws from
    the *same* counter `allocate_sequence_no` already does rather than adding a dedicated
    `next_ability_id` field to `GameState` (which is built as a full struct literal in 100+ test
    fixtures — reusing the existing counter avoids that mechanical fallout entirely, same "one
    counter, more than one purpose" precedent `GameObject.timestamp` already set).
  - `RegisteredAbility.id: AbilityId` — real ids minted at every production registration site
    (`script.rs`'s builder-drain in `run_card_script`, `engine.rs`'s Clue/Food token abilities,
    `scenario/src/lib.rs`'s `mana_ability`/`trigger_operation` construction, the latter seeding a
    local counter and bumping the final `GameState.next_sequence_no` past it so scenario-seeded ids
    can never collide with a real one minted later in the same game).
  - `PendingTrigger.ability_index: u32` → `ability: AbilityId`; `commands::find_ability_by_id` (new,
    matches by id) replaces the position-based lookup at `PutTriggersApnap`'s stack-push site, for
    both the live `state.abilities` case and the departed-abilities LKI fallback.
  - `GameState.once_per_turn_triggered: HashSet<AbilityId>` (was `HashSet<(ObjectId, u32)>`).
  - `triggers::collect_triggers` simplified as a bonus: the per-source position-counting `HashMap`
    (both the live loop and the departed-abilities loop) is gone entirely — matching by `ability.id`
    needs none of that machinery.
- Scoping note (deliberate, found and confirmed mid-implementation): `PlayerActionKind::
  ActivateAbility.ability_index` was **not** touched, despite being the same "position into
  same-source abilities" shape `commands::find_ability` also serves. It's chosen and consumed in the
  same synchronous engine step (submit → immediate `step()` execution), never spanning a pause the
  way `PendingTrigger` does — its only consumer (`find_ability`) is never called across a real time
  gap, so it was never actually at risk. Fixing it too would have touched ~35 additional test files
  for no correctness gain.
- Unresolved: none new — this closes the "ability index positional fragility" note `06-abilities.md`
  carried since `activate-ability-mana`/`activated-ability-costs`.

## Slice: trigger-target-selection (branch open)

- Branch `phase5/trigger-target-selection`.
- Scope: CR 603.3d - a triggered ability that names a target ("when this dies, destroy target
  creature") genuinely pauses for its controller to choose one before going on the stack, instead of
  always being pushed with `targets: Vec::new()` and fizzling at the CR 608.2b recheck. This was
  named in `triggers_pipeline.rs`'s own long-standing TODO (mislabeled `CR 603.3c` there - that rule
  is actually about *modal* triggered abilities' mode choice; the real citation for a triggered
  ability's own target selection is CR 603.3d, "identical to the process for casting a spell...").
  Directly unblocks the previous slice's own named gap: Vesuvan Doppelganger's granted upkeep ability
  was real and correctly shaped in `state.abilities` but could never actually resolve through ordinary
  gameplay.
- Key changes:
  - `commands::legal_target_candidates` (legality.rs) - a generic candidate scan against any
    `TargetRequirement`, the same "walk every object/player, filter through `target_is_legal`" shape
    `attach::legal_aura_entry_candidates` already established, generalized off any requirement
    instead of just an Aura's own.
  - `commands::validate_single_target_requirement` - a thin `pub(crate)` wrapper exposing the
    existing private `validate_one_target_requirement` to `engine::`, which has no `TargetSpec`
    wrapper to hand the existing `validate_targets` (nothing initiated a trigger with player-submitted
    targets in the first place).
  - `ProcessState`/`StepStatus::AwaitingTriggerTargets` - new pause, same shape
    `AwaitTriggerOrderChoice` already established one level up (the trigger-push queue survives the
    pause).
  - `triggers_pipeline.rs`'s old unpaused `for trigger in ordered { ... }` push loop became
    `Engine::push_ordered_triggers(remaining: VecDeque<PendingTrigger>) -> StepStatus` - resumable,
    one trigger at a time; pauses the moment a `TargetSpec::Primitive`-shaped trigger has real
    candidates to choose from (computed via `legal_target_candidates`), and silently skips the push
    entirely (CR 603.3d's own "the ability is simply removed from the stack") when the requirement's
    own minimum can't be met. `push_trigger_stack_object` is the shared tail (constructs the real
    `StackObject`/`StackObjectPushed` event) both the no-target fast path and `submit_trigger_targets`'s
    resume call into.
  - `Engine::submit_trigger_targets(chosen: Vec<Target>) -> Result<(), TriggerTargetsError>` - answers
    the pause; on success, pushes the stack object immediately (same "submit_* mutates now" convention
    every other choice-pause resume already follows) and defers the rest of the queue to the next
    `step()` by reusing `PutTriggersApnap`'s own empty-`pending_groups` branch rather than a third call
    site for the same loop.
- Deliberately narrow, named in code comments: `TargetSpec::Primitive` only (a single target word) -
  every real triggered ability this engine has ever modeled is this shape; `TargetSpec::Multiple` on a
  trigger stays a named gap, still pushed with an empty target list same as before this slice. Modal
  triggered abilities (CR 603.3c itself) untouched. The pre-existing sacrifice/discard-as-cost
  departed-source gap (`retire_object`'s own doc comment) is unrelated and untouched.
- Verify: 5 new tests (`phase5_trigger_target_selection.rs`) - a real end-to-end pause/choose/resolve
  cycle (`Destroy` against a chosen creature), the CR 603.3d zero-candidate removal case, both
  `submit_trigger_targets` error paths, and `vesuvans_granted_upkeep_ability_now_really_fires_and_
  becomes_a_copy` - the flagship proof, driving Vesuvan's exact granted-ability shape through a real
  `BeginningOfYourUpkeep` collection (no manual re-registration-as-Activated workaround needed
  anymore) and confirming the copy actually happens. Full workspace suite green.
- Unresolved: `TargetSpec::Multiple` on a trigger. Modal triggered abilities (CR 603.3c). The
  departed-source dies-trigger gap already named above.

## Slice: trigger-multi-target (branch open)

- Branch `phase5/trigger-multi-target`.
- Scope: `trigger-target-selection`'s own named narrowing - CR 601.2c's multi-word target form
  ("destroy target creature and target land") on a triggered ability. The previous slice's pause
  carried a single `TargetRequirement`; this one generalizes it to a full `TargetSpec`
  (`Primitive`/`Multiple`), reusing the exact `commands::validate_targets`/
  `split_targets_by_requirement` machinery `CastSpell`/`ActivateAbility` already validate their own
  submitted targets through, rather than re-deriving per-word splitting for triggers.
- Key changes:
  - `commands::legal_target_candidates_for_spec(state, script_runtime, spec, caster, source) ->
    Vec<Vec<Target>>` (legality.rs) - one legal-candidate list per target word, mapping
    `legal_target_candidates` over `TargetSpec`'s one or many `TargetRequirement`s.
  - `commands::target_spec_minimums(spec) -> Vec<u32>` - one minimum-candidate-count per word (`0`
    for `up_to`, `count` otherwise), paired positionally with the candidates list above.
  - `commands::validate_targets` widened from `pub(in crate::commands)` to `pub(crate)` -
    `engine::triggers_pipeline` is outside the `commands` module tree and needed it directly; the
    narrower `validate_single_target_requirement` wrapper the previous slice added is deleted as
    redundant (this function already handles the single-word case identically, dispatching through
    its own `TargetSpec::Primitive` arm).
  - `ProcessState`/`StepStatus::AwaitingTriggerTargets`'s `requirement: TargetRequirement`/
    `candidates: Vec<Target>` became `spec: TargetSpec`/`candidates: Vec<Vec<Target>>` - a shape
    change to a variant introduced last slice, not a new one; `push_ordered_triggers`'s own
    satisfiability check now zips `candidates`/`target_spec_minimums(&spec)` per word instead of a
    single scalar comparison. `submit_trigger_targets` accepts one flat `Vec<Target>` (same contract
    as before) and hands it straight to `validate_targets`, which already knows how to split it per
    word for a `Multiple` spec.
- Deliberately unchanged: CR 603.3c (modal triggered abilities) - a different rule, still unmodeled.
  The all-or-nothing stance (one word with zero legal candidates voids the *whole* trigger, matching
  CR 601.2c/e for spells) was already implicit in `validate_targets`'s own existing behavior, just
  newly exercised by a trigger for the first time.
- Verify: 2 new tests in `phase5_trigger_target_selection.rs`
  (`triggered_ability_with_multiple_target_words_pauses_once_and_resolves_both`/
  `triggered_ability_with_multiple_target_words_and_one_unsatisfiable_is_removed_entirely`), plus the
  5 tests from the previous slice updated for the new pause shape (mechanical - field rename plus
  indexing into the now-nested `candidates`). Full workspace suite green.
- Unresolved: CR 603.3c (modal triggered abilities). Nothing else new named by this slice.

## Slice: trigger-modal-choice (branch open)

- Branch `phase5/trigger-modal-choice`.
- Scope: CR 603.3c - a *modal* triggered ability's controller announces the mode choice when
  putting the ability on the stack, the exact gap `trigger-multi-target` named and left open
  ("Deliberately unchanged: CR 603.3c ... a different rule, still unmodeled"). `RegisteredAbility`
  had no `modes` field at all before this slice - confirmed via `commands/activate.rs`'s own
  long-standing exhaustiveness-only comment on `AbilityEffect::Modal`: "RegisteredAbility has no
  modes field, so this variant can't actually originate from an ability today."
- Key changes:
  - `RegisteredAbility.modes: Vec<SpellMode>` / `.mode_choice: ModeChoice` - reused wholesale from
    `CardDefinition`'s own modal shape rather than a parallel type (identical per-option data: a
    target requirement + effect per mode, chosen `mode_choice` of them). `SpellMode::additional_cost`
    is never populated/consulted on a trigger-declared mode - CR 700.2h's additional-cost text is a
    casting-cost concept (601.2b/f-h) with no equivalent for a triggered ability going on the stack.
    ~124 `RegisteredAbility` struct-literal construction sites across 50 files needed the two new
    fields (no `Default` impl) - the same ripple class `modal-multi-mode`/`modal-chosen-by-opponent`
    already paid on `CardDefinition` for the identical reason, done mechanically here too.
  - `commands::combine_chosen_modes(modes, chosen_indices) -> Result<(Option<TargetSpec>,
    Option<AbilityEffect>), String>` (legality.rs) - the CR 700.2c/d mode-combination logic (single
    mode passes through bare; 2+ combine into `TargetSpec::Multiple`/`AbilityEffect::Modal`, gated by
    `is_modal_resolvable`) extracted out of `commands::cast::execute_cast_spell`'s own caster-chosen
    branch so a modal trigger's mode choice can reuse it instead of a third hand-written copy.
    `ModeChoice::bounds(&self) -> (min, max, repeatable)` extracted alongside it for the same reason
    (previously inlined separately in `execute_cast_spell`).
  - `ProcessState`/`StepStatus::AwaitingTriggerModeChoice` - new pause, same "queue survives the
    pause" shape `AwaitingTriggerTargets` already established; checked in
    `Engine::push_ordered_triggers` *before* the existing `ability.target` check (CR 603.3c's mode
    choice precedes CR 603.3d's target choice, mirroring 601.2b before 601.2c for a cast spell).
  - `Engine::submit_trigger_mode_choice(chosen: Vec<u32>) -> Result<(), TriggerModeChoiceError>` -
    answers the pause: validates count/duplicate against `mode_choice.bounds()`, combines via
    `combine_chosen_modes`, then either chains straight into the *existing*
    `ProcessState::AwaitingTriggerTargets` pause (if the combined mode(s) need a target - no new
    target-selection machinery needed) or pushes the ability immediately (reusing
    `push_trigger_stack_object`) if not. A combined target requirement with no legal candidates
    removes the ability from the stack (CR 603.3d), same all-or-nothing stance the plain
    (non-modal) target check already takes.
  - `ProcessState::AwaitingTriggerTargets` gained a `modal_effect: Option<Option<AbilityEffect>>`
    field (outer `Option`: was this pause reached via a modal choice at all; inner: the real,
    possibly-`None`, combined effect) so `submit_trigger_targets`'s existing resume can push the
    *combined* modal effect instead of re-deriving from the ability's own (meaningless, for a modal
    ability) `effect` field - `None` for the pre-existing non-modal path, unchanged.
  - `push_trigger_stack_object` gained a `modal_override: Option<(Option<TargetSpec>,
    Option<AbilityEffect>)>` parameter - `None` (both existing call sites) preserves the original
    "derive from `ability.target`/`ability.effect`" behavior exactly; `Some(pair)` (both new modal
    call sites) uses the already-resolved pair verbatim instead.
- Deliberately narrow, named in code/doc comments: CR 700.2e "chosen by opponent" isn't supported
  for a triggered ability's own modes - `push_ordered_triggers`/`submit_trigger_mode_choice` don't
  special-case `ModeChoice::ExactlyChosenByOpponent`/`UpToChosenByOpponent`, so a trigger declaring
  either would incorrectly prompt its own controller instead of the other player; no real card needs
  this combination yet. CR 700.2h per-mode additional cost is likewise unwired for triggers (see
  above). Combining `*ChosenByOpponent` with a trigger's own modes was never attempted.
- Verify: 8 new tests (`phase5_trigger_modal_choice.rs`) - single mode chosen (targetless), 2+ modes
  combined and chaining into a real `AwaitingTriggerTargets` pause, a single chosen mode with its own
  target chaining into real target selection end-to-end (Destroy resolving against the chosen
  creature), CR 603.3d removal when the chosen mode's target has zero legal candidates, and four
  `submit_trigger_mode_choice` rejection paths (not-awaiting, out-of-range count, duplicate mode,
  bad index). Full workspace suite green (163 test binaries, 0 failed).
- Unresolved: CR 700.2e for a triggered ability's own modes. CR 700.2h for a triggered ability's own
  modes. No Lua exposure - `TriggeredAbilityBuilder` gains no mode-declaration method this slice
  (proven via direct `RegisteredAbility` construction only, same posture `trigger-target-selection`
  itself took - that builder still has no `:SetTarget` either). No scenario TOML wiring.

## Slice: trigger-target-lua-exposure (merged)

- Branch `phase5/trigger-target-lua-exposure`. `Game.CreateTriggeredAbility(c)` gained a real
  `:SetTarget(table | function)`, mirroring `Game.CreateActivatedAbility`'s own exactly - CR 603.3d
  was engine-complete but Lua-inaccessible. See `05-script-bridge.md`'s own entry for the full
  writeup (this slice's real home is the script bridge, not the trigger pipeline itself - nothing
  in `triggers_pipeline.rs`/`process_state.rs` changed).

## Slice: trigger-mode-chosen-by-opponent (branch open)

- Branch `phase5/trigger-mode-chosen-by-opponent`.
- Scope: CR 700.2e - "a player other than [the ability's] controller chooses a mode for it" - for a
  triggered ability's own modes, the first of the two gaps `trigger-modal-choice` named and left
  open. **CR 700.2h turned out not to apply here at all** - its own rule text says "some modal
  *spells*" and "paid *as the spell is cast*," with no "or ability" clause the way 700.2a/700.2e both
  have (checked against `comp_rules.md` directly before starting) - so there was nothing left to
  build for that citation on a trigger; it's a correctly-inapplicable rule, not an unbuilt one.
- Key changes:
  - `ProcessState`/`StepStatus::AwaitingTriggerModeChoice` gained a `chooser: PlayerId` field -
    previously the dispatch side in `engine.rs` always reported `trigger.controller` as the asker;
    now `push_ordered_triggers` resolves the *real* chooser once, at the point the pause is raised
    (mirroring `CastSpell`'s own `PendingModeChoice::chooser`), and the dispatch side just reports
    whatever was resolved.
  - `push_ordered_triggers`'s modal check: for `ModeChoice::ExactlyChosenByOpponent`/
    `UpToChosenByOpponent`, validates the exact same narrowing `CastSpell`'s own branch already
    enforces (every mode targetless and costless; exactly one other non-eliminated player) before
    ever constructing the pause. Unlike `CastSpell` (which rejects an invalid combination as a
    `CommandError` at cast time, since casting is a real player-initiated command that can fail), a
    triggered ability has no "reject the cast" step to fall back on - an invalid combination just
    means this ability can never validly go on the stack, so it's skipped outright (`continue`), same
    CR 603.3d "no legal choice -> removed from the stack" stance a trigger with no legal target
    already takes a few lines below.
  - `Engine::submit_trigger_mode_choice` itself needed **no functional change** - it was already
    player-unauthenticated (like every other `submit_*` in this engine), so the resolved `chooser`
    only matters for who the caller *prompts*, not for anything the engine validates.
- Verify: 6 new tests (`phase5_trigger_mode_chosen_by_opponent.rs`) - the pause reports the other
  player, not the trigger's own controller; the other player's chosen mode(s) still resolve for the
  trigger's own controller (single mode, and 2+ combined - the latter incidentally proving 2+
  all-targetless chosen modes still chain into a real, trivially-satisfied `AwaitingTriggerTargets`
  pause, same as a spell's own multi-mode cast would); both narrowing violations (a mode with its own
  target, a 3-player game) silently skip the trigger instead of pushing anything; a duplicate-mode
  rejection (CR 700.2e doesn't grant CR 700.2d's own repeat exception, same as the spell side).
  Full workspace suite green (165 test binaries, 0 failed).
- Unresolved: nothing new under CR 700.2/603.3c - this and `trigger-modal-choice` together close
  every named mode-choice gap for triggered abilities except Lua mode-declaration (a separate,
  bigger, more novel gap - see `trigger-target-lua-exposure`'s own note on why modes weren't bundled
  with `:SetTarget`) and scenario TOML wiring.

## General trigger notes

- Trigger ordering: `group_apnap` groups by controller in APNAP order; intra-group ordering pauses via `AwaitTriggerOrderChoice`.
- Departed-abilities stash is per-pipeline-run, cleared in `end_pipeline_run`.
