# Battlefield setup scripts

A scenario card placed on the battlefield by `[[setup]]` runs its script once, so its abilities exist from the first step (CR 604.1, 113.6). The decision is in `.plan/closed/093-scenario-battlefield-scripts.md`.

## Shape

- `Engine::run_battlefield_card_scripts` runs `run_card_script` for every battlefield object with a non-empty `Inline` source, in `ObjectId` order, and returns the failures. It writes no event, skips `ByKey` and empty sources, and leaves `off_battlefield_scripts_run` alone. A second call registers every ability again, so a host calls it once on a fresh state. No `GameState` flag guards it.
- Placement is not an entry. Enters triggers register and never fire, `apply_enters_battlefield_modifiers` does not run, and `ledger_len` assertions keep their values (CR 603.6a, 614.1c).
- `report.rs` calls it after `Engine::new` and the `Pending*` registrations, so their ability ids do not move, and panics on the first failure with the card key. The `[game]` path places no battlefield cards and does not call it.
- `build_initial_state` panics when a battlefield setup card has a resolved script and sets `trigger_operation`, a non-empty `mana_ability` or `continuous_effect`. It reads the resolved definition, so a `db_script_id` row's script counts.
- A bounced setup card is a new object (CR 400.7). The hand sweep registers it once.
- No proof game uses scenario setup, so `BEHAVIOUR_REV` stays 10.

## Tests

- `battlefield_card_scripts.rs` (5): `ObjectId` order, skipped sources, returned `LoadError` and `RuntimeError` with no ledger event, no enters trigger, a bounced card registering once in hand.
- `scenario/tests/battlefield_setup_scripts.rs` (10): a setup static ability saving a damaged creature, a setup enters trigger staying quiet, a setup activated ability, panics for a failing script and each shortcut pair, and the two allowed cases (`mana_ability = []`, a hand card).
- `kess_rider_cast.toml` starts Kess on the battlefield.

## Open

- Command-zone setup cards do not run their script (CR 113.6p).
- The shortcut check rejects the pair even when the script registers nothing. A precise check waits for a fixture that needs the pair.
