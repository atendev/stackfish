---
id: 93
title: Scenario battlefield setup runs card scripts
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Should a card placed on the battlefield by a scenario fixture run its script, so its static and triggered abilities register? Today it does not, and fixtures that need a static ability cast the card from hand first.

## Notes

- Origin: `map.md` Not yet specified (found by 088). The Kess scenario works around it by casting Kess.
- Decide whether setup always runs `register_abilities`, runs it only when the card spec asks, or stays as it is. Check how many existing scenarios would change behaviour or replay pins.
- Read `docs/scenario_format.md` and `crates/scenario/src/build.rs` (six TODOs).
- Use the `grilling` and `domain-modeling` skills.

## Resolution

Decided in an auto-mode grilling run. A subagent stood in for the user over two rounds and took every recommendation. A haiku auditor approved the question report in its fourth round. The build is "Build battlefield setup card scripts".

**Scope.** Only the `[[setup]]` path places cards on the battlefield. `build_new_game.rs` fills libraries and commanders only. Hand, library, graveyard and exile setup cards already register lazily through `sync_off_battlefield_scripts`. The battlefield is the one zone where `s.register_abilities(c)` never runs, while `s.operation()` and `s.target_requirement()` already load at setup.

**Impact.** A per-entry parse of every TOML scenario found 115 battlefield setup entries, none scripted. All 13 scripted setup entries start in hand. The inline Rust scenarios put only `db_script_id = "grizzly_bears"` on the battlefield, and its DB row has no script. No existing scenario changes behaviour. Host proof games never use scenario setup, so `BEHAVIOUR_REV` and the seed 9 pin are out of reach.

**Rule.** Every battlefield setup card whose resolved `definition.script_ref` is a non-empty `Inline` source runs `register_abilities` once (CR 604.1, 113.6). That covers a TOML `script` and a `db_script_id` card with a script row. `ByKey` and empty sources are skipped, as `record_script_error` already skips them. There is no opt-in field.

**No entry.** Setup is the starting state, not an entry. "When this enters" triggers and "enters with" replacements register and never fire (CR 603.2). `apply_enters_battlefield_modifiers` does not run, and no event is written, so `ledger_len` assertions keep their values.

**Engine method.** `pub fn Engine::run_battlefield_card_scripts(&mut self) -> Vec<(ObjectId, ScriptError)>` runs `run_card_script` for battlefield objects in `ObjectId` order. It returns the failures, writes no ledger event and does not insert into `off_battlefield_scripts_run`, matching the `action.rs` entry path. A bounced setup card is a new object (CR 400.7), and the hand sweep registers it. The doc comment states the trap: call it once on a freshly built state, because a second call double-registers. There is no guard flag, since a `GameState` field would join the loop fingerprint and the snapshot for a harness-only concern. Running it inside `Engine::new` or from `sync_off_battlefield_scripts` was ruled out. The first double-registers on hand-built test states. The second re-runs every permanent that entered for real, because the entry path never marks the set.

**Harness.** `report.rs` calls the method after `Engine::new(state)` and after every `Pending*` registration, so existing ability ids do not move. Each failure panics with the card key and the `ScriptError`, `LoadError` and `RuntimeError` alike, as every other setup failure in `build.rs` panics.

**Shortcut check.** `build_initial_state` panics when a battlefield setup card has a resolved non-empty script and its spec sets `trigger_operation`, `mana_ability` or `continuous_effect`. It reads `resolved_script` after the DB lookup, so it needs no Lua load and covers `db_script_id` cards. It rejects the pair even when the script defines only `operation()`. No fixture uses any pair. A precise check that counts what the script registered is a line in `map.md` Not yet specified.

**Out of scope.** Command-zone setup cards keep the TODO at `engine/mod.rs:500` (CR 113.6p). None of the six `build.rs` TODOs is touched.

**Tests.** A scenario with a scripted static-ability permanent on the battlefield whose buff applies from the first step. A battlefield setup card with an ETB trigger that does not fire. A scripted activated ability on a battlefield setup card that can be activated. A bounced setup card that registers once in hand. Unit tests for the method: `ObjectId` order, skipped `ByKey` and empty sources, a returned `LoadError` and `RuntimeError`, and no ledger event. Panic tests for a failing setup script and for each of the three shortcut pairs.

**Fixtures and docs.** Kess (`permissions/kess_rider_cast.toml`) starts on the battlefield in place of the cast-first workaround. `docs/scenario_format.md` describes battlefield setup running the script, and its "pick one" rule covers all three shortcuts. Check `docs/lua/` for any page that names the cast-first workaround. No Lua API change, so the game stub does not change. `DESIGN.md` gets a line on the method.
