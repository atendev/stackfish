# 21-atomicity

Ticket 025, branch `phase5/atomicity-harness`. Ticket 024 set the contract: a rejected input changes nothing, and the engine keeps that by validating everything before it mutates anything. It does not snapshot and restore. This note records the harness that checks the contract, the audit of every rejecting code path, and the bugs the audit found.

## The contract

- A rejected `PlayerAction` or Answer leaves `GameState`, the ledger and the engine's own fields exactly as they were. The player keeps priority and may try again (CR 733.1, 733.2).
- The guarantee covers input rejection only. A script error during resolution is still logged as a `ScriptError` event and resolution continues. A broken engine invariant (a panic or a failed `.expect(`) is still fatal. Ticket 035 splits `CommandError` so the two can be told apart.
- The old "no rollback" comments are replaced. They were on `StepStatus::Error` (`engine/process_state.rs`), in `handle_pending_action` (`engine/action.rs`) and on `record_script_error` (`engine/mod.rs`).

## Derives

`Clone`, `PartialEq` and `Debug` are derived unconditionally on every struct and enum declared under `types/`, not behind a test feature. The probe compiled with no other change. 63 declarations gained at least one derive. Two notes:

- `FullTextOverride` keeps its hand-written `PartialEq`, which compares by name.
- `ScriptHandle` is a plain `u64`, so Lua does not block the derives. The Lua state itself stays on `Engine` and is not compared.

`Debug` was also added to the engine-internal types the pipeline holds (`ProcessState`, `PipelineContext`, `PipelineExit`, `PendingPaymentRequest`, `ModalContinuation`, `DecisionTracker`, `DyingCandidate` and four small enums), because the snapshot below compares them through their `Debug` text.

## The harness

- `Engine::snapshot()` returns an `EngineSnapshot`: a clone of `GameState`, `next_command_id`, the Lua handle count (`ScriptRuntime::handle_count`), and the `Debug` text of the fields that have no `PartialEq` (pipeline stage and context, pending payment, suspended payments, modal continuation, Decision tracker). `Debug` text is stable for a value nobody modified, which is the case being checked.
- `stackfish-testutil` (`atomic.rs`) has `assert_unchanged_after`, `assert_action_rejected` (drives `submit_action` and `step()`, accepts a rejection from either) and `snapshot_diff`, which names what changed.
- `Session::snapshot()` exposes the same snapshot to host tests.
- `Bot::injecting_illegal_answers()` sends illegal replies before each real Answer (`crates/host/src/illegal.rs`).

Test files:

| File | What it covers |
| --- | --- |
| `engine/tests/phase5_atomicity.rs` | 45 tests, at least one illegal input for each of the 9 command arms, plus nested mana abilities |
| `engine/tests/phase5_atomicity_submit.rs` | Every `submit_*` and `decline_*` method (33 rows) asked when it is not waiting, a pending payment, a cleanup discard, Madness and Cascade |
| `engine/src/engine/atomicity_tests.rs` | 56 bad answers to 23 kinds of hand-built pause (scry, surveil, search, attach choices, devour, amplify, trigger order, targets and modes, replacement, legend rule, copy, discard choice, unless paid, script choices, mulligan, bottom, spell mode, payment). Each must be refused with the engine unchanged and not with a "not awaiting" error |
| `engine/tests/phase5_atomicity_harness.rs` | Meta-tests: the harness fails when state, a counter or the ledger changes, including a mutate-then-`Err` call |
| `engine/tests/phase5_sba_event_order.rs` | Regression for the `HashMap` ordering bug below |
| `host/tests/phase5_atomicity_proof_game.rs` | Whole games with illegal Answers injected at every Decision |

## Audit of `execute_command`

| Arm | Result | Notes |
| --- | --- | --- |
| `PassPriority` | validate-first | Read-only. |
| `MakeChoice` | not applicable | A stub that accepts any call and mutates nothing but the pass counter. It rejects nothing today. |
| `PlayLand` | fixed | Allocated ids and removed the card from the hand before looking up the battlefield zone. |
| `CastSpell` | fixed | Allocated ids before looking up the source zone. The one-shot grants taken at the top (`pending_timing_bypass`, `pending_free_cast`) are `None` outside Madness and Cascade, so taking them is no change. Both callers clear them again on `Err`. |
| `ActivateAbility` | fixed | The free mana-ability path tapped the source before looking up the actor's player state. |
| `DeclareAttackers` | fixed | Checked each attacker before tapping any, but accepted the same creature twice. |
| `DeclareBands` | validate-first | Replaces any earlier declaration instead of rejecting a second one. Not a rollback problem. |
| `DeclareBlockers` | validate-first | Per-pair checks, menace and banding all run before `combat.blocks` is touched. |
| `AssignCombatDamage` | validate-first | `plan_damage_splits` takes `&GameState`. The first write is the merge into `pending_damage_splits`, after every check. |

## Audit of the `submit_*` methods

Every method below takes the same shape: match the pause, check the answer, then `mem::replace` the process state and mutate. Each check precedes the first mutation. "Read" means I read the whole validation path. All 28 methods besides `submit_action`, plus the three `decline_*` methods and `unwind_payment`, also ran through a script that flags any `Err` after the first mutation. It flagged only the Madness and Cascade casts (fixed) and two script-error logging branches (not rejections).

| Method | Result |
| --- | --- |
| `submit_action` | fixed (see below) |
| `submit_payment`, `unwind_payment` | validate-first, read |
| `submit_scry_choice`, `submit_surveil_choice`, `submit_search_choice` | validate-first, read |
| `submit_entry_attach_choice`, `submit_search_devour_choice`, `submit_search_amplify_choice` | validate-first, read |
| `submit_mulligan_decision`, `submit_bottom_cards` | validate-first, read |
| `submit_trigger_order`, `submit_trigger_targets`, `submit_trigger_mode_choice` | validate-first, read |
| `submit_discard`, `submit_discard_choice` | validate-first, read |
| `submit_attach_choice`, `submit_token_attach_choice` | validate-first, read |
| `submit_script_choice`, `submit_script_may_choice` | validate-first, read. A script error on resume is logged, not rejected. |
| `submit_devour_choice`, `submit_amplify_choice` | validate-first, read |
| `decline_madness_cast`, `decline_cascade_cast`, `decline_unless_paid` | validate-first, read |
| `submit_madness_cast`, `submit_cascade_cast` | fixed (see below) |
| `submit_unless_paid_payment` | validate-first, read |
| `submit_mode_choice`, `submit_copy_choice` | validate-first, read |
| `submit_replacement_choice`, `submit_legend_rule_choice`, `submit_commander_return_choice` | validate-first, read. `submit_commander_return_choice` accepts both booleans, so nothing can be rejected. |

## Bugs found and fixed

Each has a regression test that fails without the fix. I checked this by reverting the fix.

1. `execute_play_land` allocated an object id and a sequence number, removed the card from the hand, and only then looked up the battlefield zone. A missing zone returned `Err` with the card gone. Test: `play_land_without_a_battlefield_zone_changes_nothing`.
2. `execute_cast_spell` allocated both ids before it looked up the zone to remove the card from. Test: `cast_without_the_source_zone_changes_nothing`.
3. `execute_activate_ability` tapped the source of a free mana ability before it looked up the actor's `PlayerState`. Test: `activate_a_mana_ability_without_player_state_changes_nothing`.
4. `execute_declare_attackers` accepted one creature listed twice, then tapped it and logged `AttackerDeclared` twice. Test: `declare_the_same_attacker_twice_is_rejected`.
5. `handle_pending_action` took a command id before running the command, so every rejection at `step()` used one up. A replay of only the accepted Answers would have numbered its commands differently, which breaks the byte-identical replay in `DESIGN.md`. The id is now taken once the command is accepted. Test: `a_rejected_action_does_not_use_up_a_command_id`. Without the fix 40 of the 45 tests in that file fail, because every rejection moved the counter.
6. `submit_madness_cast` and `submit_cascade_cast` had the same command id leak when `execute_command` returned `Err`. Both now put the counter back. Tests: `an_illegal_madness_cast_keeps_the_pause_and_the_command_id`, `an_illegal_cascade_cast_keeps_the_pause_and_the_command_id`.
7. `submit_action` suspended the payer's pending payment before its last two rejection checks (wrong player holds priority, pregame action). The checks now run first. I found no way to reach this through the public API, because the payer and the priority holder are the same player in every state the engine can produce. No regression test. The fix is by inspection.
8. Not an atomicity bug, but the proof-game test found it. The state-based action collectors for illegally attached Auras and Equipment, creature Auras, self-enchanted Auras and the World rule walked `GameState::objects` (a `HashMap`) without sorting. The same seed sometimes ordered two `Unattached` events differently, which breaks replay (found at seed 29, about one run in three). All five now sort by object id. Test: `equipment_that_falls_off_does_so_in_object_id_order` (24 Equipment, fails every run without the sort). This is part of ticket 028's `HashMap` audit. Other unordered walks may remain (`objects.values()` appears in 8 more places outside `sba.rs`; I did not check them).

## The nested mana ability

CR 601.2g and 605.3a let a player activate a mana ability while paying for a spell. `submit_action` suspends the pending payment, and the nested action's rejection resumes it. Two tests cover it:

- `a_rejected_nested_mana_ability_leaves_the_payment_intact`: a tapped land activated mid-payment is rejected at `step()`, the snapshot is unchanged, and `submit_payment` then pays and the spell reaches the stack.
- `another_action_during_a_payment_is_rejected_without_touching_it`: a pass, another player's mana ability and a missing ability index are each rejected at `submit_action`.

## The proof-game test

`illegal_answers_at_every_decision_change_nothing_and_the_game_ends_the_same` plays seeds 0 to 11 twice, with and without injection. Before each real Answer the injected bot sends the replies from `illegal.rs`: a reply of the wrong shape, and a reply of the right shape with illegal content (a made-up id, an out-of-range mode or order index, 50 colorless mana for a payment, an extra attacker or blocker, one point too much damage). For each, the bot checks that the Session refuses it, that the same Decision is still open, and that `Session::snapshot()` is equal before and after. At the end of the game the two runs must have the same winner, answer count, per-seat events, final `GameState` and `next_command_id`.

The twelve seeds send about 43,000 illegal Answers. The test asserts that each of these Decisions was attacked at least once: priority, payment, declare attackers, declare blockers, assign combat damage, Scry, Search, cleanup discard, mulligan bottom, the "may" yes/no and the mulligan yes/no. A three-player game with seed 7 passes the same comparison.

The first version of this test attacked only the first seven of those. Scry and Search never opened, in any seed, because `Engine::new_game` did not load a spell's `operation()` and `target_requirement()` from its script (see "The inert proof spells" below). The `order`, `modes`, `targets` and `may_cast` shapes still do not occur: no proof card has a triggered ability with targets or several triggers at once, and none has Madness or Cascade. They have injection code but no coverage is claimed.

## The inert proof spells

Every spell in the proof set declares its effect in Lua (`s.operation()`) and its targets in `s.target_requirement()`. `CardDefinition.effect` and `target_requirement` stayed `None`, so Shock was cast with no target and dealt no damage, Peek never scried and Rampant Growth never searched. The scenario crate wired these two values for its own fixtures. The host builds every card through `Engine::new_game`, which did not. Nothing flagged it, because an inert spell raises no script error.

`Engine::new_game` now loads both for every card with an inline script (`load_script_declarations` in `engine/pregame.rs`). A value already set in Rust wins. A script that fails to load logs a `ScriptError` event. Tests: `phase5_new_game_script_declarations.rs` (3).

Making spells work exposed a visibility leak. Raise Militia's token, returned to a hand by Bounce, ceased to exist there, and `token_ceased` carried its hand id to the opponent. `view.rs` now remembers where a token entered a hidden zone and sends `token_ceased` only to its owner and omniscient Seats. Tests: two in `session.rs`.

Because the facade checks an Answer against the Decision's entries first (`decision/check.rs`), most injected replies never reach the engine. The engine-level coverage comes from the per-arm tests above.

## Open items

- Done in ticket 035 (`22-error-split.md`): command validation runs in `submit_action`, and the `.expect(` calls under `commands/` became `Internal` errors or rejections. About 30 `.expect(` calls under `engine/` stay as panics that the `Session` catches.
- `TODO(026)`: menace is checked only in `execute_declare_blockers`, and must-attack is not checked at all. `legal_blockers` does not know either rule.
- `TODO(036)`: `combine_chosen_modes` decides which mode pairs combine at cast time only. The modal entries can offer a pair that is then rejected.
- `MakeChoice` accepts any call. It should reject when no choice is pending.
- `DeclareBands` accepts a second declaration and overwrites the first.
- Every pause with something to refuse now gets bad answers from `atomicity_tests.rs`. Those pauses are built by hand, not reached by playing, which is enough because a rejection happens before the code reads anything beyond the pause. Only Scry, Search, payment, discard, Madness and Cascade are also reached by playing.
- `Engine::new_game` does not load `alternative_costs()` or `buyback_cost()` from a script. They are the same kind of declaration as `operation()`.
- Cascade and Madness casts bypass `submit_action`, so their rejections are `IllegalCast` errors from the `submit_*` method. They are atomic now, but they do not go through the facade checks.
