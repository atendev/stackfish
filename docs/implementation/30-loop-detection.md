# 30-loop-detection

Ticket 029, branch `phase5/infinite-loop-detection`. The ticket asks the engine to detect a loop of mandatory actions and end the game as a draw (CR 104.4b). The step budget only stops a runaway from freezing the host. This report chooses the detection method.

## Rules

- CR 104.4b: in a game without the limited range of influence option, a "loop" of mandatory actions that repeats a sequence of events with no way to stop is a draw. A loop that contains an optional action is not.
- CR 104.4f: the same loop under limited range of influence is a draw only for the players who control an object in the loop and the players in their range. The game goes on for the rest.
- CR 732.3: a fragmented loop (each player does an independent action and the same state recurs) must be broken by the active player, or the first involved player in turn order, making a different choice.
- CR 732.4: a loop of only mandatory actions is a draw.
- CR 732.5: no player can be forced to take an action that would end the loop, other than actions that objects in the loop call for.

CR 104.4f needs a range-of-influence setting. `GameSettings` (`crates/engine/src/types/config.rs`) has none, so every game is a game "not using the limited range of influence option" and 104.4b applies to all players. CR 732.3 is covered only as far as a player holding a non-mana option never gets a draw. The engine does not pick a different choice on anyone's behalf.

## Detection method

The engine ends the game as a draw when the same game state recurs at a priority pass that nobody has a way to avoid. No real player choice sits between the two occurrences, so the sequence between them is mandatory and repeats forever.

### Where it hooks

`Engine::submit_action` (`crates/engine/src/engine/action.rs`), after `validate_command` succeeds and before `process_state` becomes `PendingAction`. Every priority action reaches the engine through it, including the `Engine::answer` facade (`crates/engine/src/decision/answer.rs`).

### What counts as a forced pass

The action is `PlayerActionKind::PassPriority`, and the passer's `legal_actions()` (`crates/engine/src/decision/legal_actions.rs`) holds only `Entry::Pass` and plain tap mana abilities. A plain tap mana ability is an `Entry::Activate` with `mana_ability: true` whose ability cost is `{T}` alone: no mana cost and no additional cost besides `TapSelf`. A mana ability with no cost counts too, because the entry builder treats it as tapping its source (`commands/entries.rs`). The passer is `state.priority_player`, which `submit_action` has already checked.

`legal_actions()` does not test affordability (its doc comment says so). A card the player cannot pay for still counts as a way out. That can only miss a loop and never invent one. `mana_ability` is set from the effect alone (`commands/entries.rs`), so a mana ability with a sacrifice, discard, life or mana cost is still an `Entry::Activate`. Such an ability changes the game in a way the loop does not undo, so it counts as a way out. A plain tap mana ability only taps a permanent and floats mana. No condition watches a tap, so it cannot stop the loop (CR 732.5).

### The watch

`Engine` gets a private `loop_watch: Vec<Fingerprint>`.

- A forced pass with a non-empty stack builds a fingerprint of the state before the pass is applied. If it equals an earlier entry in the watch, `process_state` becomes `ProcessState::GameOver { winner: None }` and `submit_action` still returns `Ok(())`. The pass is not applied. The next `step()` returns `StepStatus::GameOver { winner: None }` through the existing `ProcessState::GameOver` arm in `advance`. This is the same terminal state CR 104.4a uses (`game_over_check` in `engine/replacement_resume.rs`). If there is no match, the fingerprint is pushed.
- Any other accepted action clears the watch: a pass that is not forced, a pass with an empty stack, and every non-pass action (casting, activating, playing a land, combat declarations).
- `step()` clears the watch when `advance()` returns a pause other than `AwaitingAction` or `Progressed` (a target, order, payment, replacement, scry or similar choice). A player choice sits between the two states, so the sequence is not mandatory. Doing it in `step()` covers every `submit_*` path without hooking each one. `GameOver` and `Error` need no clearing.

Empty-stack passes are skipped. When both players pass on an empty stack the step advances, and `turn_number` and the step never repeat a state. Skipping them also keeps the cost out of normal play: the watch and `legal_actions()` run only while the stack is non-empty.

A replay feeds the same actions to a fresh engine, so the watch fills identically. It is not part of `EngineSnapshot` because a rejected input returns before the hook runs and cannot touch it.

### The fingerprint

Plain `GameState` equality never sees a trigger loop repeat. Every trigger put on the stack gets a fresh `stack_id` from `allocate_object_id`, and the ledger and the id counters only grow. The fingerprint is a `String` built from `GameState` (`crates/engine/src/types/game.rs`, `Clone + PartialEq + Debug`, not `Hash`):

1. Leave out `ledger`, `next_event_id`, `next_sequence_no` and `next_object_id`. Those grow with every event and say nothing about what the game can do next.
2. Format every other field with `{:?}`.
3. Find every `ObjectId(n)`, `AbilityId(n)`, `EventId(n)` and `SequenceNo(n)` token across the whole text, rank the values of each kind in numeric order, and replace each value with its rank. This is equality up to an order-preserving renaming of those identifiers. A fresh stack id that is always the largest in play gets the same rank each lap.
4. `HashMap` and `HashSet` fields print in an unstable order, so format each entry on its own, relabel it with the shared rank table, and sort the entries before joining. The rank table is built from every entry first. The implementer greps `types/` for every `HashMap` and `HashSet` field, including those inside `PlayerState` (`commander_casts`) and `GameObject`. The ones on `GameState` today are `lands_played_this_turn`, `last_active_turn_number`, `objects`, `once_per_turn_triggered`, `commander_return_offered`, `linked_ability_generation`, `script_ability_ids`, `ability_function_zones` and `off_battlefield_scripts_run`. `CombatState.pending_damage_splits` (`types/combat.rs`) is another.
5. `rng` stays in. A loop that shuffles never matches, because the draw counter moves.
6. Fold in every value the engine derives from the ledger to make a rules decision, because dropping the ledger would otherwise let two states with different futures compare equal and give a wrong draw. Today these are `cards_drawn_this_turn` per player (`types/player.rs`, CR 121.2b), `opponent_damaged_this_turn` per player (`replacement.rs`, CR 702.54a). `find_departed_lki` (`triggers.rs`, CR 603.10) is left out on purpose. It runs only at trigger collection for a departed source with an intervening-if (`triggers_pipeline.rs`), which finishes before any player gets priority. Its result is stored in `StackObject.source_lki`, which item 2 covers. The grep result is the authority, not this list. The implementer greps for every reader of `state.ledger` outside the event pipeline, adds each derived value, and lists them in the fingerprint's doc comment so a new reader is added there.

The cost of one fingerprint is one full `{:?}` of the state. That runs only on forced passes with a non-empty stack. The watch is unbounded between clears, and the step budget stops a non-repeating chain such as a growing counter. A cap on the watch would miss long loops, so there is none.

### Why not the other methods

- Hashing a state instead of storing the string risks a false draw on a collision. A draw ends a game and must not be wrong, so the watch keeps full strings.
- Counting steps or stack depth is the existing step budget. It cannot tell a long finite game from a loop, which is the point of the rule.
- Tracking the ledger for a repeated event pattern is defeated by the same fresh ids, and compares events instead of states, which is not what CR 104.4b describes.

### Check against the code

I checked the proposed design against the code and kept it. No concrete flaw turned up. Two things the report depends on:

- `submit_action` runs `validate_command` before any mutation, and its only early arms are the payment suspension (a non-pass action) and the combat turn-based actions (not passes). The hook sits cleanly between validation and the `PendingAction` assignment.
- `Entry` is `PlayLand`, `Cast`, `Activate` or `Pass` (`crates/engine/src/decision/entry.rs`), and `ActivateEntry` has the `mana_ability` flag. The forced-pass test needs nothing else.

No `ConditionPrimitive` (`crates/engine/src/types/ability.rs`) watches a tap or an untap, so the test fixture below uses a death-and-return loop.

## What success looks like

- A two-permanent mandatory trigger loop with a non-empty stack and no optional action ends the game: the next `step()` returns `StepStatus::GameOver { winner: None }`.
- A loop where some player could do something else, such as cast an instant, never draws.
- A repeat that makes progress (a counter or life total that changes) never draws, because the fingerprint differs.
- A trigger loop whose only difference between laps is a fresh `stack_id` is recognised.
- No normal game changes. `submit_action` costs nothing extra when the stack is empty, and a pass with a non-empty stack where a player has a real option clears the watch.
- A host reports the draw as it reports a CR 104.4a draw (see below).

## TODOs to mark

One line each, in the format `// TODO: <what is missing> (CR n)`, at the code they affect.

- Forced non-priority Decisions, such as a one-candidate target or trigger order, clear the watch today, so a loop through one is missed (CR 732.4).
- Affordability: a spell or ability the passer cannot pay for still counts as a way out, so a loop is missed (CR 104.4b).
- Loops that cross extra combat phases or steps pass on an empty stack and are not watched (CR 104.4b).
- The limited range of influence variant has no setting in `GameSettings`, so the draw covers every player (CR 104.4f).
- CR 732.3 fragmented loops: the engine never makes a different choice for the active player.

## Tests

### Engine, in `crates/engine/src/engine/` or `crates/engine/tests/`

Fixtures: `stackfish_testutil` (`two_player_state`, `card_object`, `creature_definition`, `put_on_battlefield`, `pass_both_and_drain`, `drain`) and hand-built `RegisteredAbility` values as in `tests/discarded_trigger.rs`. `engine/tests.rs` has `minimal_state` and `trigger` for tests inside the crate. The fingerprint test lives next to the fingerprint code because the type is private.

The loop fixture is one 0/0 creature with a `Dies` trigger whose effect is `AbilityEffect::ReturnSelfWithCounter` with a counter kind that does not change toughness. CR 704.5f kills it, the trigger returns it, and it dies again. Counters do not survive the zone change (CR 400.7), so each lap ends where the last one began, apart from fresh object, stack and ability ids. This uses existing primitives only, so there is no Lua API change. No tap or untap condition exists, which rules out a two-permanent tap cycle.

Because the creature changes zones each lap, the fingerprint also relabels `AbilityId(n)` tokens: the zone change registers its abilities under fresh ids.

1. `a_mandatory_trigger_loop_is_a_draw`: the 0/0 returning creature, with no optional action for either player. Drive passes with `submit_action` and `step()` until `StepStatus::GameOver`. Assert `winner: None`, and that it arrives within a small bound (a few laps), well below the step budget.
2. `a_castable_instant_keeps_the_game_going`: same loop, but a player holds an instant that `legal_actions()` lists. Assert no draw after many more laps than test 1 needs.
3. `a_finite_repeat_is_not_a_draw`: a mandatory trigger that changes the state on each lap, such as a life total that keeps moving. Assert no draw while the counter grows, and that the game is still `AwaitingAction`.
4. `the_fingerprint_relabels_a_fresh_stack_id`: two states that differ only in a `stack_id`, and in `next_object_id`, `ledger`, `next_event_id` and `next_sequence_no`, give equal fingerprints. States that differ in `rng`, a life total or a permanent's tapped flag give different ones. A `HashMap` field filled in two insertion orders gives one fingerprint.
5. `a_player_choice_clears_the_watch`: a target or order pause between two otherwise equal states leaves the watch empty, so no draw.
6. `a_pass_on_an_empty_stack_is_not_watched`: repeated all-pass rounds with an empty stack never fill the watch.
7. `a_rejected_action_leaves_the_watch_alone`: an illegal action returns an error and the watch is unchanged.
8. `a_mana_ability_with_a_sacrifice_cost_keeps_the_game_going`: the loop from test 1, with a passer holding a "{T}, Sacrifice this: add one mana" artifact. No draw.
9. `the_fingerprint_sees_ledger_derived_values`: one case per reader. Two states equal apart from the ledger give different fingerprints when the ledger changes `cards_drawn_this_turn` for a player, and when it changes `opponent_damaged_this_turn`.

### Host, in `crates/host/tests/`

- The 500-seed proof run (`five_hundred_seeds_all_finish_without_a_rejection` in `proof_game.rs`, `#[ignore]`) still finishes with no rejection. Its outcome has no field that says why a game ended, so the check is that the run is unchanged: record the `transcript_hash` of each seed from 1000 to 1499 on master before the change, and compare after. A hash that moves means the rule fired or the hook changed play. Run it with `cargo test -p stackfish-host --test proof_game -- --ignored`.
- How the host reports the draw: `Session` maps `StepStatus::GameOver { winner }` to `Outcome::GameOver(winner)` (`crates/host/src/session.rs`), then `Response::game_over(events, winner.map(|p| p.0))` (`crates/host/src/envelope.rs`), so a loop draw is `Status::GameOver` with `winner: Some(None)`, the same as a CR 104.4a draw. No host change is needed. A host test needs a card set that makes a loop, which the proof setup does not, so the engine tests carry the proof and the host check stays a no-regression run.

`tests/player_elimination.rs` already pins the CR 104.4a draw at the engine level and is the model for the assertion on `StepStatus::GameOver { winner: None }`.

## Comment and doc rules for this slice

- Cite rules as `// CR 104.4b` with the number only, no quoted text.
- Rust comments say why. The new module gets a `//!` of three lines or fewer that points to `DESIGN.md`.
- `docs/DESIGN.md` gets a short section on the watch and the fingerprint.
- Lua API: no change.
- Run `tools/check.sh --full` before the audit.

## What changed while building

- The loop fixture is a Lua card (`Game.Ops.ReturnSelfWithCounter(Game.Counter.Charge)` on a `Dies`
  trigger). Hand-built abilities are not carried across a zone change, so a hand-built fixture loops
  only once. Because a rerun script mints fresh `ScriptHandle`s, the fingerprint relabels
  `ScriptHandle(n)` by rank too. The card's script text stays in the fingerprint, so the rank lines up
  only for the same script rerun. Lua globals a script keeps are not compared, which is a TODO.
- The loop exposed an engine bug. The `ReturnSelfWithCounter` mover took the card out of the
  graveyard zone but left its graveyard object in `state.objects`, so every Persist or Undying return
  left a ghost object behind. It now calls `commands::retire_object`, and
  `persist_retires_the_graveyard_object_it_returns` pins it.
- `BEHAVIOUR_REV` moves to 2, because a looping game now ends where it used to run to the step budget.
  The proof-game hash is unchanged.
- The rejected-action test caps its passes. An uncapped version ran the loop until the process ran
  out of memory while detection was still failing.
- Results: `tools/check.sh --full` passes. All 500 proof seeds (1000 to 1499) give the same
  transcript hash on master and on this branch, and none ends in a draw.

The plan's tests 5 and 7 landed as `a_player_choice_clears_the_watch` and
`a_priority_window_keeps_the_watch` (in-crate, since the watch is private) and as
`a_rejected_action_leaves_the_watch_alone` (played). A plain `{T}` mana ability test,
`a_plain_tap_mana_ability_does_not_stop_the_draw`, was added beside test 8.

Test 6 became the unit test `a_pass_on_an_empty_stack_clears_the_watch`, beside
`a_forced_pass_with_a_stack_is_watched`. A played version could only show that no draw happened,
and that holds even with a broken hook, because empty-stack passes advance the step.
