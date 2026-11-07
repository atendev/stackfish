---
id: 120
title: Build scenario assertions and fixture fields
type: task
mode: AFK
status: open
blocked_by: [95]
claimed: false
---

## Question

Build what 095 decided: setup-order timestamps, `GameEventKind::name()`, the `expect_error` variant name, the `damage_marked`, `event_count`, `stack_targets`, `stack_size` and `characteristics` assertions, and the `alternative_cost` and `pay_buyback` cast fields with script cost loading.

## Notes

- Origin 095. The full decision, the commit order and the test conditions are in `closed/095-scenario-schema-additions.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- The timestamp commit comes first and passes the full scenario suite alone. Record before and after results for `crates/engine/tests/scenarios/layers/` and the whole suite.
- Add `// TODO: a static ability's effect takes its host's timestamp (CR 613.7a)` at the `run_card_script` drain.
- Update `docs/scenario_format.md`, `docs/lua/not-available-yet.md`, `99-backlog.md`, `25-chain-operations.md` and `DESIGN.md`. No Lua API change, so the game stub stays.
- Check that the seed 9 replay pin does not move.

## Resolution

<Filled in at close.>
