# 12-coroutines

## Slice: script-coroutines (merged)

- Branch `phase5/script-coroutines`.
- Scope: first player-input-mid-script via Lua coroutines — `Game.SelectTargets`.
- Verify: 2 spike + 6 integration tests, 490 total, 0 failed.
- Key changes:
  - `ScriptRuntime::invoke_operation` uses `Lua::create_thread`/`Thread::resume`.
  - `Game.SelectTargets(filter)` is a plain Lua function that calls `coroutine.yield(...)`.
  - `ScriptChoiceRequest::SelectTarget { filter: TargetFilter }`.
  - `OperationOutcome::{Finished, NeedsChoice}`.
  - `ScriptRegistry.suspended_threads: HashMap<ScriptHandle, RegistryKey>`.
  - `ProcessState::AwaitingScriptChoice`; `StepStatus::AwaitingScriptChoice`; `Engine::submit_script_choice`.
  - `ResolveStackTop` pauses on yield, resumes with chosen target, validates via `target_is_legal`.
- Unresolved:
  - Object-only, count=1 this slice.
  - May abilities, modal spells, player targets, nested asks deferred.

## Slice: script-target-players (merged)

- Branch `phase5/script-target-players`.
- Scope: `Game.SelectTargets` widened to `Target::Player` and "any target".
- Verify: 5 new tests, 495 total, 0 failed.
- Key changes:
  - `decode_target_filter_table` gained `type` discriminator: `"object"`, `"player"`, `"any"`.
  - `resume_operation`/`submit_script_choice` widened from `ObjectId` to `Target`.
  - Resumed Lua value changed to tagged table `{ kind = "object"|"player", id = n }`.
- Unresolved:
  - `TargetFilter::Player` controller restriction (target opponent vs any player).

## Slice: script-may-abilities (merged)

- Branch `phase5/script-may-abilities`.
- Scope: CR 608.2d "may" abilities — yes/no choice mid-resolution.
- Verify: 7 new tests, 502 total, 0 failed.
- Key changes:
  - `ScriptChoiceRequest::MayChoose`.
  - `Game.MayChoose()` Lua binding.
  - `ScriptRuntime::resume_operation_bool`.
  - `Engine::submit_script_may_choice(bool)`.
  - `ScriptChoiceError::WrongRequestKind`.
- Unresolved:
  - Modal spells.
  - Nested/repeated asks untested.

## Slice: script-modal-spells (merged)

- Branch `phase5/script-modal-spells`.
- Scope: CR 700.2a modal spell mode selection at cast time.
- Verify: 8 new tests, 510 total, 0 failed.
- Key changes:
  - `SpellMode { target_requirement: Option<TargetSpec>, effect: Option<AbilityEffect> }`.
  - `CardDefinition.modes: Vec<SpellMode>`.
  - `CastSpell` resolves chosen mode index before target validation; chosen mode's requirement/effect replace card's singular fields.
  - `PlayerActionKind::CastSpell.modes: Vec<u32>` and TOML `cast_spell.modes = []` already existed as dead plumbing.
- Deviations:
  - ~60 `CardDefinition` struct literals needed `modes: Vec::new()`.
- Unresolved:
  - Modal activated/triggered abilities; choose more than one mode; kicker/buyback; pre-filter illegal modes.

## Slice: payment-unwind (merged)

- Branch `phase5/payment-unwind`.
- Scope: CR 601.6 — caller can abandon a cast/activation when unable to pay.
- Verify: 4 new tests + 1 TOML fixture, 445 total, 0 failed.
- Key changes:
  - `Engine::unwind_payment()`.
  - Spell: reuses `move_resolving_spell(stack_id, Hand, false)` to return card to hand under fresh id.
  - Activated ability: just removes stack entry.
  - `GameEventKind::CastingUnwound { stack_id }`.
  - Scenario `ActionSpec::UnwindPayment`.
- Explicitly not modeled:
  - True CR 733 state rewind (append-only ledger); `commander_casts` increment not rolled back.

## Slice: mass-choice-effects (merged)

- Branch `phase5/mass-choice-effects`.
- Scope: four generic primitives for Balance-shaped effects ("each player chooses N, sacrifices/
  discards the rest") — none tailored to Balance specifically. Balance itself built as the
  validating end-to-end test, entirely in Lua on top of the four primitives.
- Verify: 8 new tests (7 primitive tests + Balance itself), 1363 total, 0 failed.
- Key changes:
  - `Game.ChooseN(filter, n, opts)` — pure-Lua loop on top of `Game.SelectTargets`/`Game.MayChoose`,
    no new coroutine request shape. `opts.up_to` asks a `MayChoose`-shaped "one more?" before each
    pick (including the first).
  - `ScriptChoiceRequest::SelectTarget` gained `exclude_ids: Vec<ObjectId>` (checked in
    `submit_script_choice`, not folded into `TargetFilter` — that struct is constructed at ~50 call
    sites, only this one request shape needs per-ask exclusion).
  - `decode_target_filter_table` gained a `zone` key (was hardcoded to `None`/Battlefield) — needed
    for `Game.ChooseN({ zone = "Hand", ... }, n)` (CR 701.9 discard).
  - `Game.Ops.SacrificeChosen(list)`/`DiscardChosen(list)` — hand-written Lua, build a
    `sacrifice_chosen`/`discard_chosen` result-table key. `ScriptRuntime::handle_operation_result`
    decodes those into a new `OperationOutcome::PendingZoneMoves(Vec<(ScriptZoneMoveKind, Vec<
    ObjectId>)>)` variant (the generalization the pre-existing `gain_life`-only TODO comment flagged)
    rather than mutating `GameState` directly — `ScriptRuntime` has no access to
    `Engine::continue_scripted_zone_move`'s replacement-checked pipeline, one layer up.
  - `ScriptedZoneMoveKind` gained `Discard` (Hand→Graveyard) — `continue_scripted_zone_move`
    generalized from a hardcoded Battlefield source to `kind.from_zone()`; `commit_scripted_zone_move`
    gained a `Discard` arm (can't reuse `sba::commit_death` — that hardcodes removing from
    Battlefield too).
  - `AbilityEffect::ForEachPlayer` (marker, CR 101.4) — `Game.Ops.ForEachPlayer(fn)` is the one
    `Game.Ops.*` entry that isn't a static tag; it's a `create_function` closure
    (`ForEachPlayerTag`) that registers `fn` immediately and pairs the marker with a *real*
    `ScriptHandle`, unlike every other tag's `ScriptHandle(0)` placeholder.
  - `triggers::apnap_player_order` factored out of `group_apnap` (same rotation, reused for
    `ForEachPlayer`'s own loop instead of trigger placement).
  - `EachPlayerLoop` (new `ProcessState::AwaitingScriptChoice.each_player: Option<...>` field) —
    carries `remaining_players`/`sacrifice_ids`/`discard_ids` across a pause so
    `engine::resolve::each_player::{resolve_for_each_player_effect, settle_script_operation_outcome,
    advance_or_finish_each_player}` can re-invoke the same operation per player and only perform the
    real zone moves once every player has answered (CR 101.4 "then simultaneously" — ids accumulate,
    nothing moves until the loop's queue is empty).
  - `ScriptedZoneMoveThen::ThenScriptedZoneMove` — chains a second zone-move batch (e.g. sacrifice
    then discard) after the first's own queue and any CR 616.1 ordering pause settles;
    `Engine::chain_pending_zone_moves` builds the chain from a `PendingZoneMoves` list.
  - `Game.Ops.SacrificeRandom(filter, count)`/`DiscardRandom(count)` (CR 701.9b/"at random") —
    static tags, resolved directly in Rust (`engine::resolve::destruction`/`discard`) via
    `Engine::next_rng` (widened from private to `pub(in crate::engine)` — same deterministic,
    `RngDrawn`-logged source `shuffle_library` already uses).
  - `invoke_operation` now passes a third `state` argument to every operation function (the same
    `state_to_lua` snapshot condition/CDA scripts already get) — needed for a `ForEachPlayer` body to
    compute something like "the fewest lands any player controls" before choosing. Backward
    compatible (Lua ignores extra call arguments).
  - `card_summary_to_lua` gained a `card_types` field (was `is_creature`-only) — needed to tell lands
    apart from the snapshot at all.
- Deviations from the original plan:
  - `Game.Ops.ForEachPlayer` only wraps a custom function, not a static tag directly (e.g. "each
    player discards at random" needs a one-line wrapper) — no real card needed the combination this
    slice, named as a follow-up rather than built speculatively.
  - `Game.ChooseN`'s `filter` is the flat option-bag shape only, no scripted-predicate `filter = fn`
    form (that's `:SetTarget`/`target_requirement()`'s own thing) — not needed by Balance.
- Unresolved / explicitly deferred:
  - `Game.Ops.SacrificeRandom`/`DiscardRandom` can't be composed inside a `ForEachPlayer` body (see
    "deviations" above).
  - `DiscardRandom` has no `filter` parameter (`SacrificeRandom` does) — no real card discards a
    filtered random selection yet.
  - `Game.ChooseN`'s `up_to` only allows fewer picks, no `at_least`/"N or more" shape.
