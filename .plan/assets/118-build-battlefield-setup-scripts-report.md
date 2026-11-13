# 118 Build battlefield setup card scripts: decision report

Decision: `.plan/closed/093-scenario-battlefield-scripts.md`. Requirements: `118-build-battlefield-setup-scripts-requirements.md`. Branch `phase5/battlefield-setup-card-scripts`.

## Gates

The CLAUDE.md slice loop ran in full. A haiku auditor approved the requirements report in round 3. A sonnet auditor reviewed the diff and the `tools/check.sh --full` output and approved in round 2. The master's brief replaced the Merge / Don't merge prompt and the commit question: this branch commits every time and never merges, and the master runs CI on the branch.

## Decisions

1. **What "sets `mana_ability`" means.** The options were to panic on any `mana_ability` key, or only on a non-empty list. The main thread chose non-empty. The field defaults to an empty list, and `build.rs` already treats an empty list as unset. `mana_ability = []` stays legal beside a script, and a test pins it. The haiku auditor asked for the reading to be written down and tested, then approved.
2. **Carrying the rejected alternatives.** The haiku auditor asked that the report state that the method has no `GameState` guard field, and that it is not called from `Engine::new` or `sync_off_battlefield_scripts`. The main thread added both. The code follows them: only `report.rs` calls the method.
3. **Which failure the harness reports.** The options were to collect every failure into one panic, or to panic on the first. The main thread chose the first. The decision asks for a panic on a failure, and every other setup failure in `build.rs` panics on its first error. The sonnet auditor accepted it.
4. **Where the iteration order comes from.** `GameState.objects` is a `BTreeMap`, so the method walks it directly and gets `ObjectId` order without a sort. A comment says why the order matters: each run allocates ability ids. A test places the higher id first in the battlefield zone to show that zone order does not leak in.
5. **The scenario load-error test.** A syntax error never reaches the new method, because the existing `target_requirement()` loader in the `Pending*` pass panics on it first. The implementer used `s.register_abilities = 5`, which parses and which only `run_card_script` rejects as a `LoadError`. The engine test still covers a syntax error. The sonnet auditor accepted this.
6. **The bounce test count.** The first draft gave the card a hand-only ability beside the bounce ability and expected one registration in hand. The engine keeps undeclared abilities in hand too, so the count was two, both registered once. The implementer dropped the hand ability so the test checks one copy against two.
7. **The stack case.** The sonnet auditor found that the skip test left out the stack object the report listed. The implementer added one, and the auditor approved in round 2.
8. **Kess fixture.** Kess starts on the battlefield. The cast, payment and resolve steps and the refusal of the graveyard sorcery before Kess resolves are gone, and the header comment matches. The graveyard rider assertions are unchanged.
9. **Docs.** `scenario_format.md` describes the setup run and the battlefield-only pick-one panic. The `db_script_id` paragraph names the exception. `DESIGN.md` gets one line under Verification. Implementation note 38 lost its stale Open bullet, and note 41 records this build. No page in `docs/lua/` named the workaround, and the Lua API did not change, so the game stub is unchanged.

## Checks

`tools/check.sh --full` passes. The proof-game and replay tests pass, including `the_build_id_revision_moves_with_the_proof_game`, so the seed 9 pin did not move and `BEHAVIOUR_REV` stays 10.

## Left open

- Command-zone setup cards still do not run their script (CR 113.6p). The TODO stays.
- The shortcut check rejects the pair even when the script registers nothing. That line is already in the map's Not yet specified.
