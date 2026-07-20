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
