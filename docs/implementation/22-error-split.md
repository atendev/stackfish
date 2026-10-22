# 22-error-split

Ticket 035, branch `phase5/error-split`. Ticket 024 decided that a rejected input changes nothing and an engine bug is fatal. Ticket 025 built the harness that checks the first half. This slice makes the two outcomes different types, moves every command rejection into `submit_action`, and teaches the `Session` to survive a rejection and to catch a panic.

## Types

- `commands::Rejected` has two variants, `InvalidTarget` and `IllegalAction(String)`. Only validation code builds one. Every `validate_*` function returns `Result<_, Rejected>` and every `apply_*` function returns `Result<_, Internal>`, so a function that mutates cannot report a rejection.
- `commands::Internal(String)` is a broken invariant found after validation passed, such as a zone that validation just confirmed is gone. It is fatal.
- `commands::CommandError` is `Rejected(Rejected)` or `Internal(Internal)`. Only `execute_command` and the Madness, Cascade and script-cast callers see it.
- `StepStatus::Error` now carries `Internal`. A host that sees it must stop using the engine.
- `ActionError` gained `Rejected(Rejected)`. `ActionError::IllegalAction(String)` and `NotYourTurn` stay for the checks about the engine's own state (mid-pipeline, wrong priority holder, pregame actions).
- `MadnessChoiceError::IllegalCast` and `CascadeChoiceError::IllegalCast` carry `Rejected`. Both gained an `Internal` variant.
- `decision::RejectionCode::Internal` is new. It is the one Answer refusal that does not promise "nothing changed".
- `Engine::fatal_error()` returns the first `Internal` the engine hit. After it is set, `step()` returns `StepStatus::Error` and does nothing else. `Engine::poison` records one.

## Validate, then apply

Each arm of `execute_command` split into a validation half and an apply half.

| Arm | Plan type the validator returns |
| --- | --- |
| `PassPriority` | the actor |
| `MakeChoice` | none (it still accepts any call) |
| `PlayLand` | `LandPlan` |
| `CastSpell` | `CastPlan`: cost, zones, mode effect, target requirement |
| `ActivateAbility` | `ActivatePlan`: `FreeMana`, `PaidMana` or `Stack` |
| `DeclareAttackers` | `AttackersPlan`, with the vigilance decision made per attacker |
| `DeclareBands` | `BandsPlan` |
| `DeclareBlockers` | `BlockersPlan`, with the banding blocks already added |
| `AssignCombatDamage` | `DamagePlan`: `AlreadyAssigned`, `Record` or `Deal` with the damage events built |

`commands::validate_command(&GameState, &ScriptRuntime, actor, action)` returns a `Validated` plan. `apply_command(&mut GameState, Validated)` consumes it. `execute_command` calls both, so callers outside `submit_action` (Madness, Cascade, a script cast, the automatic combat damage step) still work.

`submit_action` calls `validate_command` after its own state checks and before it suspends a payment or queues anything. The command is therefore validated twice, once at submit and once in `step()`. The second run can only fail if the state changed between the two calls. `handle_pending_action` treats that as an `Internal` error and poisons the engine.

Two details came from the split:

- `CastSpell` used to `take()` the one-shot grants `pending_timing_bypass` and `pending_free_cast` at the top. The validator now reads them and `apply_cast_spell` clears them. A rejected cast leaves them set, and the Madness, Cascade and script-cast callers still clear them on `Err`.
- The `todo!()` at the end of `execute_command` for the variants that never reach it (`SubmitPayment`, the pregame actions) is gone. The validator returns `Rejected::IllegalAction` for them.

## Session

- `Session::handle` runs each request under `catch_unwind` (`Session::guarded`). A panic sets the phase to `Dead`. The Session answers that request and every later one, `poll` included, with the code `engine_panic` and never touches the engine again. `Session::create` also catches a panic and returns an `engine_panic` response.
- A `Rejected` error never reaches the Session as a fatal. `Engine::answer` returns the refusal, the Decision stays open and the response is `decision_open` with `invalid_answer`.
- An `Internal` error from `step()`, or `RejectionCode::Internal` from `answer`, sets the phase to `Halted`. The code is `pipeline_error`, and `poll` still drains the outbox, as before.
- The wire text for both fatal cases is the fixed sentence "the engine hit an internal error and the game cannot continue". The detail (the `Internal` text or the panic message) is kept in `Session::internal_error()` for the host's log. It can name a hidden card, so it never goes on the wire.
- `ErrorCode::EnginePanic` (`engine_panic`) is a twelfth error code. It is additive, so `PROTOCOL_VERSION` stays 1.

## Godot adapter

`Core` keeps its own `catch_unwind` as a backstop. The `Session` now catches an engine panic first, so a panic inside `handle` returns an `engine_panic` response and `last_error()` stays empty. The backstop still covers a panic outside the `Session`'s guard, such as in response serialization.

## Tests

- `engine/src/engine/atomicity_tests.rs` (4 new): `submit_action` refuses with `ActionError::Rejected` and changes nothing; a zone that vanishes between validation and apply is `Internal`; an action rejected after it was queued is fatal and `step()` stays fatal; `execute_command` keeps the two error kinds apart.
- `host/src/session.rs` (3 new or changed): a halted Session shows a generic message and keeps the detail; `guarded` turns a panic into `engine_panic` and refuses every later request including `poll`; a panic while building the game is an error response.
- `host/tests/phase5_error_split.rs` (1): a modal cast the entries list but `combine_chosen_modes` refuses comes back as `invalid_answer` with the same Decision open, the snapshot unchanged, and a legal Answer accepted afterward.
- The rejection assertions in about 60 engine test files moved from `StepStatus::Error(CommandError::X)` after `step()` to `submit_action` returning `Err(ActionError::Rejected(Rejected::X))`. Three scripts did most of the rewrite and I fixed the rest by hand. `assert_action_rejected` in `stackfish-testutil` now requires the refusal at `submit_action` and returns the `ActionError`.

I did not check that the new Session test fails on the old code. The old `Session::run` turned every `StepStatus::Error` into a halt, which this test would catch, but I did not revert to confirm it.

## Open items

- `TODO(026)`: menace and must-attack. `validate_declare_blockers` and `validate_declare_attackers` are where the shared predicate goes.
- `TODO(036)`: the mode-pair predicate. The Session test above relies on the entries still listing a pair `combine_chosen_modes` refuses, and stops being able to reach that refusal when 036 lands. Its replacement needs another reply that passes the facade and fails the engine.
- About 30 `.expect(` calls under `engine/` stay as panics, mostly in `pregame.rs` (16) and `library_choices.rs` (7), in functions that return no `Result`. The `Session` catches them. The ones in `commands/` became `Internal` errors or `ok_or` rejections.
- A panic leaves the `Engine` inside the dead `Session` undropped until the `Session` drops. If the `Engine`'s drop can panic, `Core::die` still leaks it, but a plain `Session` user does not.
- The proof-game injection bot has no reply that passes the facade and fails the engine. The Session test covers one hand-built case.
- `MakeChoice` still accepts any call, and `DeclareBands` still overwrites an earlier declaration.
