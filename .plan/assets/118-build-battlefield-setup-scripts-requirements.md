# 118 Build battlefield setup card scripts: requirements

Decision: `.plan/closed/093-scenario-battlefield-scripts.md`.

## What success looks like

A `[[setup]]` card placed on the battlefield whose resolved `definition.script_ref` is a non-empty `Inline` source has its `s.register_abilities(c)` run once before the first step. Its static abilities apply from the first assertion, its activated abilities can be activated, and its triggered abilities wait for their events. Setup is the starting state, not an entry, so "when this enters" triggers and "enters with" replacements never fire and no event is written.

Pieces:

1. `pub fn Engine::run_battlefield_card_scripts(&mut self) -> Vec<(ObjectId, ScriptError)>` in `crates/engine/src/engine/mod.rs`. It collects battlefield objects whose `script_ref` is a non-empty `Inline`, sorts them by `ObjectId`, calls `script_runtime.run_card_script` for each, and returns the failures. It writes no ledger event (no `record_script_error`), does not insert into `off_battlefield_scripts_run`, and does not call `apply_enters_battlefield_modifiers`. The doc comment states the trap: call it once on a freshly built state, because a second call double-registers. There is no guard field on `GameState`, since it would join the loop fingerprint and the snapshot for a harness-only concern. The method is not called from `Engine::new` (hand-built test states would double-register) or from `sync_off_battlefield_scripts` (it would re-run every permanent that entered for real). Only the scenario harness calls it.
2. `crates/scenario/src/report.rs` `run()`: after `Engine::new(state)` and after every `Pending*` registration, it calls the method. Each failure panics with the card key (inverse lookup in `card_ids`) and the `ScriptError`. The `[game]` path (`build_new_game_engine`) does not change, because it places no battlefield cards.
3. `crates/scenario/src/build.rs` `build_initial_state`: for a battlefield setup entry with a resolved non-empty script (`resolved_script.is_some()`), it panics when the spec also sets `trigger_operation`, `mana_ability` or `continuous_effect`. `mana_ability` is a list that defaults to empty, and `build.rs` already treats an empty list as unset (it registers nothing), so "sets" means non-empty for that field. `mana_ability = []` stays legal beside a script. The message names the card key and the field. It reads `resolved_script`, so a `db_script_id` card is covered. The check applies only to the battlefield zone.
4. Kess (`crates/engine/tests/scenarios/permissions/kess_rider_cast.toml`) starts on the battlefield. The cast-Kess steps (cast, submit_payment, pass_priority) and the pre-Kess refusal of `rite` are dropped. The header comment loses the "refused before Kess resolves" clause. The graveyard rider assertions stay.
5. Docs. `docs/scenario_format.md`, `script` section: a battlefield setup card's script runs once at setup with no entry, so ETB triggers and replacements do not fire. The "pick one" rule names all three shortcuts (`trigger_operation`, `mana_ability`, `continuous_effect`) and says a battlefield setup card that pairs one with a script panics. `docs/DESIGN.md` gets one line on the method under Verification. `docs/lua/` has no page that names the cast-first workaround (checked by grep), so it is unchanged. No Lua API change, so the game stub is unchanged.

## Comp rules

- CR 604.1, CR 113.6: a permanent's static and activated abilities function while it is on the battlefield. A setup permanent therefore needs its abilities registered.
- CR 603.2, CR 603.6a: a triggered ability triggers only on an event that happens after it exists. Setup is not an event, so an ETB trigger on a setup card never fires.
- CR 614.1c, CR 614.12: an "enters with" replacement applies only to an actual entry, so setup does not apply it.
- CR 400.7: a setup card that leaves the battlefield becomes a new object. The hand sweep (`sync_off_battlefield_scripts`) registers the new object once, because its id is not in `off_battlefield_scripts_run`.

## TODOs to mark

- None new. The decision keeps the command-zone TODO at `engine/mod.rs` (`run_card_script_for`, CR 113.6p) and the six `build.rs` TODOs as they are.
- The precise shortcut check (count what the script registered) is already a line in `map.md` Not yet specified. No code TODO, since the panic is the intended behaviour for now.

## Tests

Engine integration tests (`crates/engine/tests/battlefield_card_scripts.rs`, built with `stackfish_testutil`):

- Two scripted battlefield objects are registered in `ObjectId` order, even when the battlefield zone lists them in reverse. The ability ids rise with the source id.
- A `ByKey` battlefield object and an empty `Inline` one are skipped: no failure is returned and nothing registers.
- A script with a syntax error returns `ScriptError::LoadError` for that object. A `register_abilities` that calls `error()` returns `ScriptError::RuntimeError`. Other objects still register.
- The call writes no ledger event (`ledger.len()` unchanged, including on a failure) and leaves `off_battlefield_scripts_run` empty.
- Hand, graveyard and stack objects are not run.
- A battlefield setup card with a hand ability (`:SetFunctionsFrom("Hand")`) and a self-bounce activated ability: after the bounce the new hand object has exactly one registered ability (CR 400.7).

Scenario tests (`crates/scenario/tests/battlefield_setup_scripts.rs`, through `parse` and `run`):

- A scripted "+0/+1 to creatures you control" permanent on the battlefield keeps a 1-toughness creature with 1 marked damage alive through the first state-based action check, so the buff applies from the first step (CR 704.5g). The scenario format has no power or toughness assertion, so `assert_object_zone` carries the check.
- A battlefield setup card with an ETB "gain 1 life" trigger: after passing priority through a step, life stays 20 and no trigger went on the stack.
- A scripted activated ability ("{0}: gain 1 life" or similar) on a battlefield setup card is activated with `activate_ability` and resolves.
- `#[should_panic]` tests: a battlefield setup script that fails to load, one that raises at runtime (message names the card key), and each of the three shortcut pairs (`trigger_operation`, a non-empty `mana_ability` list, `continuous_effect` beside `script`).
- A hand setup card with `script` and `trigger_operation` does not panic in `build_initial_state` (the check is battlefield-only).
- A battlefield setup card with `script` and `mana_ability = []` does not panic in `build_initial_state`.

Existing suites:

- `crates/engine/tests/scenarios.rs` runs every TOML scenario, including the moved Kess fixture.
- The host proof-game seed 9 pin must not move. Proof games do not use scenario setup, and `tools/check.sh --full` confirms it.
