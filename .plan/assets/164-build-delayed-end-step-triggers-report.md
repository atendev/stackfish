# Decision report: Build delayed end-step triggers

Branch `phase5/delayed-end-step-triggers-3`, cut from the handoff commit `bfc11ec` because another worktree still holds `phase5/delayed-end-step-triggers-2`. Requirements: `164-build-delayed-end-step-triggers-requirements.md` (approved by the haiku auditor in round 2). Implementation note: `docs/implementation/42-delayed-triggers.md`.

Gates: the repo CLAUDE.md loop ran (requirements report, haiku audit, `phase5/` branch, `tools/check.sh --full`, sonnet implementation audit). Its `AskUserQuestion` merge gate was skipped, because the wayfinder master merges. The two-round audit cap in CLAUDE.md gave way to the five-round cap in the user's global rules.

## Decisions

| # | Question | Options | Choice | Made by | Audit |
|---|---|---|---|---|---|
| 1 | Where does `"targets"` find an object an earlier chain entry moved? | Use the stale id; follow the `ZoneChange` to the new object | Follow it through the events since the resolution began (CR 400.7j). Without this the flicker case could never find the card in exile. | Implementer (requirements) | Approved, requirements round 2 |
| 2 | How does the stack object keep each referred zone until resolution? | Filter at push into `targets`; a new `StackObject.delayed_refers` | `delayed_refers` holds the zones, `targets` names every referred id. `StackObject` is not on the wire, so the wire is unchanged. | Implementer (requirements) | Approved, requirements round 2 |
| 3 | Does `ChainEach` accept `AtNextEndStep`? | `Chain` only, as the decision says; both | Both. They share the entry check, and rejecting `ChainEach` would add a special case with no rules reason. | Implementer (requirements) | Approved, requirements round 2 |
| 4 | Where does a spell's `source_lki` come from when an earlier entry paused? | Live source; `top.source_lki`; the source's own `ZoneChange` | All three in that order. A paused spell is already in its graveyard when the row is made, so the move's last known information is the only snapshot. | Implementer | Approved, implementation round 2 |
| 5 | How is the "as a mode" load error tested? | A mode test through `build_modes`; leave it out | A `build_modes` script whose first mode is `AtNextEndStep` fails with the "must be an entry of Game.Ops.Chain" error. | Implementer | Approved, implementation round 2 |
| 6 | Can scenario fixtures reach the end step? | TOML fixtures; Rust tests only | TOML. `advance_to_step = "end"` and card-id tracking across zone changes make both the return-then-exile and the exile-then-return cases expressible. | Implementer | Approved, implementation round 2 |
| 7 | Does a row with no referred objects resolve safely? | Leave it untested; run it through the end step | Run it through. The replacement test now resolves its empty row and checks that nothing moves, since each op acts only on the legal list. | Implementer | Approved, implementation round 2 |
| 8 | Which TODOs go in code? | Docs only for some; all as `// TODO:` lines | All of them. The auditor found three only in docs (CR 603.7b durations, CR 603.7f-h, the host view); they now sit on `DelayedTrigger`. | Auditor, round 1 | Approved, implementation round 2 |
| 9 | Is the push-time skip for a departed controller tested? | A three-player test; cite only CR 800.4a on the SBA test | The citation. In a two-player game the elimination ends the game before the end step, so the push skip cannot be reached there. It stays as a guard with no test. | Auditor's fallback, round 1 | Approved, implementation round 2 |
| 10 | `BEHAVIOUR_REV` | Bump; keep 10 | Keep 10. The replay pin did not move. | Implementer | Approved, implementation round 2 |

## Results

- `tools/check.sh --full`: exit 0, all full checks passed. `db_scripts_smoke.rs` skipped (no `stackfish-db`).
- Tests: 18 in `tests/delayed_triggers.rs`, one fingerprint test in `loop_watch_tests.rs`, two scenarios in `tests/scenarios/delayed/`.

## Surfaced

- A spell's card reaches its graveyard before its later chain entries finish when an earlier entry pauses (CR 608.2n). Found while building decision 4.
- The general CR 701.21a control check in `resolve_sacrifice_effect`. Only the delayed path checks control.
