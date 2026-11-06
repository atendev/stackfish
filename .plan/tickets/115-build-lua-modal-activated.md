---
id: 115
title: Build Lua modal activated abilities
type: task
mode: AFK
status: open
blocked_by: [114]
claimed: false
---

## Question

Build the activated half of what 090 decided: `modes: Vec<u32>` on `PlayerActionKind::ActivateAbility` validated by `combine_chosen_modes`, the activated ability's mode range in the entries list, the additive `modes` wire field, the proof bot, `:SetModes` on `ActivatedAbilityBuilder` with its refusals, the docs, the stub and the tests.

## Notes

- Origin 090. The full decision is in `closed/090-lua-modal-builders.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- Modes are announced in the activation action, not a separate Decision (CR 602.2b, 700.2a). `PROTOCOL_VERSION` stays 1 because the field is skip-if-empty.
- The activated builder refuses `*ChosenByOpponent`, per-mode `cost`, `pawprint` and `PawprintBudget`, each with a negative test. Add `// TODO: CR 700.2e opponent-chosen modes for an activated ability`.
- Remove the TODOs at `commands/activate.rs:507` and `types/card.rs:255`, which cite CR 700.2b where 700.2a applies.
- A TOML scenario under `crates/engine/tests/scenarios/modal/` drives a scripted modal activated ability end to end. Fixtures stay inline, never `../stackfish-db`.

## Resolution

<Filled in at close.>
