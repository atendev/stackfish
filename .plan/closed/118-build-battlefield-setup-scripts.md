---
id: 118
title: Build battlefield setup card scripts
type: task
mode: AFK
status: closed
blocked_by: [93]
claimed: false
---

## Question

Build what 093 decided: `pub fn Engine::run_battlefield_card_scripts(&mut self) -> Vec<(ObjectId, ScriptError)>`, the harness call after `Engine::new(state)` that panics on a failure, and the `build_initial_state` panic for a scripted battlefield card that also sets `trigger_operation`, `mana_ability` or `continuous_effect`.

## Notes

- Origin 093. The full decision, including the call order, the skipped sources, the once-only doc comment and the tests, is in `closed/093-scenario-battlefield-scripts.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- Move the Kess scenario (`permissions/kess_rider_cast.toml`) to a battlefield setup and drop its cast-first workaround.
- Update `docs/scenario_format.md` (the `script` field and the "pick one" rule) and `DESIGN.md`. No Lua API change.
- No proof-game card. Check that the seed 9 pin does not move.

## Resolution

<Filled in at close.>
