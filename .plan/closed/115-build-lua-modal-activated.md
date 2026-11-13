---
id: 115
title: Build Lua modal activated abilities
type: task
mode: AFK
status: closed
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

Built on `phase5/lua-modal-activated` and audited (requirements by haiku, implementation by sonnet, each APPROVE in round 2). CI is green on b3b8b1c. PR #5 targets `wayfinder/host-protocol-master`.

`ActivateAbility` carries `modes` (CR 602.2b, 700.2a). It checks them with `announced_modes_in_order`, which casting now shares, then combines them with `combine_chosen_modes`. `ActivatedAbilityBuilder:SetModes` decodes through `ModeOwner::Activated`. That refuses opponent-chosen choices (TODO, CR 700.2e), per-mode `cost`, `pawprint`, `PawprintBudget`, and a mode that adds mana (CR 605.1a), each with its own negative test. Both builders share one `operation_or_modes` check at `:Register()`. `ActivateAbility` also refuses a Rust-built ability with an opponent-chosen or pawprint choice, and the entries list leaves such an ability out.

`ActivateEntry` gains `modes` and per-mode `EntryTargets`. The wire adds `modes` and `mode_targets` to an `activate` entry and `modes` to an `activate_ability` reply, all additive, so `PROTOCOL_VERSION` stays 1. Both mis-cited CR 700.2b TODOs are gone.

The proof bot activates modal abilities, and Forked Totem joins the red-green deck. The pinned replay never reaches that slot, so `BEHAVIOUR_REV` stays 10. Tests: nine activated cases in `lua_modes.rs`, wire and bot unit tests, and `scenarios/modal/modal_activated_ability.toml`. The docs and stub are updated, and `39-lua-modes.md` holds the notes. Decision report: `.plan/assets/115-build-lua-modal-activated-report.md`.
