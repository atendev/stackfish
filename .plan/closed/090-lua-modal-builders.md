---
id: 90
title: Modal builders in Lua for spells, triggers and activated abilities
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

How does a card script declare modes? Modal spells ("choose one") and modal triggered abilities (CR 603.3c) have no Lua builder, and a modal activated ability cannot exist at all because `RegisteredAbility` has no `modes` field (CR 700.2b).

## Notes

- Origin: backlog item 7, `docs/lua/not-available-yet.md` (triggered abilities and modal spells entries), the TODOs at `types/card.rs:255` and `commands/activate.rs:507`.
- Decide the builder shape, how a mode's target requirement and operation are declared, and whether `RegisteredAbility` gains a `modes` field or activated abilities reuse the spell path.
- The engine already supports mode choice for cards built in Rust, including `solo_only` modes (ticket 036) and opponent-chosen modes. Read `docs/implementation/24-mode-combination.md` and `12-coroutines.md` first.
- Lua API changes mean the game stub in `docs/lua/stubs/game.lua` must be updated by the build.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

Decided in an auto-mode grilling run. A subagent stood in for the user and took every recommendation, adding a few constraints of its own. The builds are "Build Lua modal spells and triggers" and "Build Lua modal activated abilities", in that order.

**Premise corrections.** `RegisteredAbility` already has `modes` and `mode_choice`, and the trigger pipeline already opens the mode Decision before targets (CR 603.3c) and routes `*ChosenByOpponent` to the one other Seat. So the gap is the Lua surface for spells and triggers, plus the engine path for activation. The TODO at `types/ability.rs:1355` is stale. A modal activated ability falls under CR 700.2a, not 700.2b, so the TODOs at `commands/activate.rs:507` and `types/card.rs:255` are mis-cited.

**A mode is a plain table.** `{ target = ..., operation = ..., cost = ..., pawprint = n }`. Only `operation` is required. `target` takes the same table or array as `s.target_requirement()` and `:SetTarget` (CR 700.2c, 700.2f), and `operation` takes a `Game.Ops.*` value or a function. A mode with several effects uses one `Game.Ops.Chain` or `ChainEach`. Unknown keys are a load error, through `reject_unknown_keys`. One decoder serves every kind.

**The count is a value tag.** `Game.ModeChoice.Exactly(n)`, `UpTo(n)`, `ExactlyRepeatable(n)`, `UpToRepeatable(n)`, `ExactlyChosenByOpponent(n)`, `UpToChosenByOpponent(n)` and `PawprintBudget(n)`, one per `ModeChoice` variant (CR 700.2a, 700.2d, 700.2e, 700.2i). There is no option-table form.

**Spells use `s.modes()`.** A no-argument module function returning `{ choice = Game.ModeChoice.Exactly(1), modes = { {...}, {...} } }`, a sibling of `s.operation()`. `register_abilities` cannot serve an instant or sorcery, which never becomes a battlefield object. `load_script_declarations` reads it, caches it per script source, and fills `CardDefinition::modes` and `mode_choice` only when the Rust definition has none. The scenario crate patches it the way `register_script_operation` patches `operation()`, through the engine's own runtime so function handles stay valid. A script defining `s.modes()` together with `s.operation()` or `s.target_requirement()` logs a script error and loads no modes.

**Triggers and activated abilities use `:SetModes(choice, { mode, ... })`.** `:Register()` needs exactly one of `:SetOperation` or `:SetModes`, and `:SetTarget` beside `:SetModes` is an error, since each mode owns its targets. A modal ability keeps `target` and `effect` at `None` and the placeholder `operation`, as Rust-built modal triggers do. `:SetCondition`, `:SetInterveningIf`, `:AddCost` and `:SetFunctionsFrom` work unchanged.

**Activation announces modes in the action.** `PlayerActionKind::ActivateAbility` gains `modes: Vec<u32>`, validated by the same `combine_chosen_modes` as casting, because CR 602.2b runs activation through CR 601.2b-i and targets in the same action depend on the modes. The entries list shows an activated ability's mode range like a spell's, and the wire adds `modes` as a skip-if-empty field, so `PROTOCOL_VERSION` stays 1. There is no separate mode Decision for activation.

**Function and chain modes are allowed and solo only.** A function packs into `AbilityEffect::Script(handle)`, and `is_modal_resolvable` refuses it, `Chain` and `ChainEach`, so each can be the single chosen mode but cannot join another. The docs say so plainly. The build makes a solo chain mode resolve on the spell and trigger paths, and `check_chain_words` runs per mode. TODO: combine a chain mode with other modes. Teaching the resolver to do that stays in Not yet specified.

**Per kind.**
- Opponent-chosen (CR 700.2e): accepted by `s.modes()` and the triggered builder. The activated builder rejects it at `:Register()`. TODO: CR 700.2e opponent-chosen modes for an activated ability.
- Per-mode `cost` (CR 700.2h), `pawprint` and `PawprintBudget` (CR 700.2i): accepted only in `s.modes()`. Under `PawprintBudget` every mode needs `pawprint` of at least 1, and `pawprint` is refused under any other choice.
- Mana abilities (CR 605.1a, 605.3b): `ManaAbilityBuilder` gets no `:SetModes`, and a colour choice stays `AddManaChoice`. TODO: modal mana abilities, CR 605.3b.
- Mode text: out of scope. The wire keeps sending indices and a host labels modes from card text.

**Load errors.** Each has its own negative test, and its text names the card script and the mode index: fewer than two modes (CR 700.2); `Exactly(n)` or `UpTo(n)` with n of 0, or n above the mode count when not repeatable; a `PawprintBudget` of 0; a mode with no `operation`; a `ChainEach` whose words do not match its mode's `target`; unknown keys; an opponent-chosen mode with a target or cost; and the per-kind refusals above.

**Handle 0 is reserved.** `ScriptRegistry` hands out handles from 0, and `ScriptHandle(0)` is also the "no function" placeholder, so a real function that is the runtime's first can be mistaken for it. The counter starts at 1. The build re-checks every reader of handle 0 (`script/decode.rs`, `engine/resolve/misc_effects.rs`), narrows or removes the TODO at `misc_effects.rs:199`, adds a test where a function mode is the runtime's first function, and checks whether the replay pin or `BEHAVIOUR_REV` moves.

**Split.** The first build covers spells, triggers, the handle reservation, their docs, tests and proof cards, and moves "a chain as a mode" in `25-chain-operations.md` from the not-built list to the solo-only notes. It adds no `:SetModes` to `ActivatedAbilityBuilder`, so it has no wire change. It removes the stale `types/ability.rs:1355` TODO. The second covers the activation action, entries, wire, proof bot, `:SetModes` on the activated builder, its docs and tests, and removes the `activate.rs:507` and `card.rs:255` TODOs. Each keeps `lua_api_doc_sync` passing.

**Docs.** The stub `docs/lua/stubs/game.lua` gains a `Mode` class, a `ModeChoice` alias, the `Game.ModeChoice` table, `:SetModes` on each builder that gets it, and `s.modes()`. `docs/LUA_API.md` lists `Game.ModeChoice`. `card-script-basics.md` gets a "Writing `modes()`" section and a table row. `triggered-abilities.md`, `activated-abilities.md` and `game-constants.md` get modal sections, and `mana-abilities.md` says modes are not available. `not-available-yet.md` drops its two mode entries and lists the remaining gaps: opponent-chosen activated modes, modal mana abilities, function modes being solo only, and mode text. `scenario_format.md` notes that a scripted fixture can use `s.modes()`, and `DESIGN.md` says where mode declarations come from.

**Tests.** Inline Lua only, never `../stackfish-db`. The first build: a choose-one spell through `new_game` and through the scenario crate, a choose-two spell, a pawprint budget, a per-mode cost, an opponent-chosen spell, a modal trigger including an opponent-chosen one, a function mode alone and refused beside another, a chain mode alone on a spell and on a trigger and refused beside another, the first-handle test, every load error, and proof-deck cards for a modal instant and a modal enters trigger. The second: a modal activated ability with per-mode targets through `ActivateAbility`, the entries list and wire showing its mode range, its load errors, and a TOML scenario under `tests/scenarios/modal/` that drives it end to end.

**Glossary.** `CONTEXT.md` gains **Mode** and **Solo-only mode**.
