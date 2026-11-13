# Lua modes

A spell declares its modes with `s.modes()` and a triggered ability with `:SetModes`. The decision is in `.plan/closed/090-lua-modal-builders.md`. Modal activated abilities are not built.

## Shape

- `script::decode::decode_modes` reads both forms. `ModeOwner::Spell` allows `cost`, `pawprint` and `PawprintBudget`, and `ModeOwner::Trigger` refuses them. Each mode's `target` decodes like `s.target_requirement()`, or like `:SetTarget` for a function. `check_chain_words` runs per mode.
- `Game.ModeChoice.*` builds a `ModeChoiceTag` for each `ModeChoice` variant. A count of 0 is a load error for every variant.
- A per-mode cost takes only an additional `Game.Costs.*` value. A `Mana` cost is a load error with a TODO (CR 700.2h).
- `load_script_declarations` reads `s.modes()` once per script and fills `CardDefinition::modes` only when the definition has none. Beside `s.operation()` or `s.target_requirement()` it records a script error and loads no modes. The scenario crate does the same through `register_script_modes` and panics on the mix.
- `TriggeredAbilityBuilder:Register()` needs exactly one of `:SetOperation` or `:SetModes`, and refuses `:SetTarget` beside `:SetModes`. A modal trigger keeps `target` and `effect` at `None` and the placeholder operation.

## Function handles

- `ScriptRegistry` hands out handles from 1, so `ScriptHandle(0)` means only "no function". Before, a spell function worked only when it held handle 0.
- A spell or mode has no `operation` field, so `AbilityEffect::packed` folds a function into the effect: `Script(handle)`, or `ForEachPlayer(Some(Script(handle)))` for a `ForEachPlayer` function. `commands::unpack_script_effect` splits it back at cast, at the trigger push and at the opponent's mode answer.
- Function, `Chain`, `ChainEach` and `ForEachPlayer` modes are solo only, since `is_modal_resolvable` refuses them beside another mode.
- The handle change moved the replay pin, so `BEHAVIOUR_REV` is 10.

## Tests

- `lua_modes.rs` (17): `new_game` loading a spell's modes, modes beside `operation()`, choose one, choose two in printed order, a pawprint budget, a per-mode cost, an opponent-chosen spell with a function mode, the first-handle function mode, a function or chain mode refused beside another, a chain mode alone, a Fight mode, a Bite mode, a modal enters trigger, an opponent-chosen trigger, function and chain modes on a trigger, and every spell and trigger load error.
- `scenarios/scripting/script_modes.toml`: a fixture's `s.modes()` through the scenario crate.
- Proof deck: Fork in the Road (a Lua modal instant) and Crossroads Scout (a modal enters trigger) in `crates/host/src/proof_cards.rs`.

## Open

- Modal activated abilities, their wire field and opponent-chosen activated modes.
- Modal mana abilities (CR 605.3b), a per-mode mana cost (CR 700.2h) and mode text.
- A chain or function mode combined with other modes.
- Three or more players choosing which opponent picks the modes (CR 700.2e).
