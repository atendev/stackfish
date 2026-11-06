---
id: 114
title: Build Lua modal spells and triggers
type: task
mode: AFK
status: open
blocked_by: [90]
claimed: false
---

## Question

Build the spell and trigger half of what 090 decided: the mode-table decoder, the `Game.ModeChoice` tags, `s.modes()` in `load_script_declarations` and the scenario crate, `:SetModes` on `TriggeredAbilityBuilder`, the load errors, the handle-0 reservation, the docs, the stub and the tests.

## Notes

- Origin 090. The full decision, including the load-error list and the required tests, is in `closed/090-lua-modal-builders.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- Do not add `:SetModes` to `ActivatedAbilityBuilder`. That and the wire change belong to "Build Lua modal activated abilities".
- The Lua API changes, so update `docs/lua/stubs/game.lua` and the docs 090 lists, and keep `lua_api_doc_sync` passing.
- Starting the handle counter at 1 may move the replay pin. Bump `BEHAVIOUR_REV` if it does.
- Remove the stale TODO at `types/ability.rs:1355`.
- A solo chain mode (`Chain` or `ChainEach`) must resolve on the spell and trigger paths, with `check_chain_words` run per mode. Add `// TODO: combine a chain mode with other modes`, and move "a chain as a mode" in `docs/implementation/25-chain-operations.md` from the not-built list to the solo-only notes.
- Handle 0: re-check every reader of the placeholder (`script/decode.rs`, `engine/resolve/misc_effects.rs`), narrow or remove the TODO at `misc_effects.rs:199`, and add the test where a function mode is the runtime's first function.
- `ManaAbilityBuilder` gets no `:SetModes`. Add `// TODO: modal mana abilities` with CR 605.3b beside it, and say in `mana-abilities.md` that modes are not available.
- Proof deck: a modal instant and a modal enters trigger, inline Lua in `crates/host/src/proof_cards.rs`.
- Docs beyond the Lua pages: a `DESIGN.md` note on where mode declarations come from, and a `scenario_format.md` note that a scripted fixture can use `s.modes()`.

## Resolution

<Filled in at close.>
