# Lua modes

A spell declares its modes with `s.modes()`, and a triggered or activated ability with `:SetModes`. The decision is in `.plan/closed/090-lua-modal-builders.md`.

## Shape

- `script::decode::decode_modes` reads every form. `ModeOwner::Spell` allows `cost`, `pawprint` and `PawprintBudget`, and `ModeOwner::Trigger` and `ModeOwner::Activated` refuse them. `Activated` also refuses `*ChosenByOpponent` and a mode whose operation is `AddMana` or `AddManaChoice`, since a mana ability can't be modal. Each mode's `target` decodes like `s.target_requirement()`, or like `:SetTarget` for a function. `check_chain_words` runs per mode.
- `Game.ModeChoice.*` builds a `ModeChoiceTag` for each `ModeChoice` variant. A count of 0 is a load error for every variant.
- A per-mode cost takes only an additional `Game.Costs.*` value. A `Mana` cost is a load error with a TODO (CR 700.2h).
- `load_script_declarations` reads `s.modes()` once per script and fills `CardDefinition::modes` only when the definition has none. Beside `s.operation()` or `s.target_requirement()` it records a script error and loads no modes. The scenario crate does the same through `register_script_modes` and panics on the mix.
- `TriggeredAbilityBuilder:Register()` and `ActivatedAbilityBuilder:Register()` share `operation_or_modes`: exactly one of `:SetOperation` or `:SetModes`, and no `:SetTarget` beside `:SetModes`. A modal ability keeps `target` and `effect` at `None` and the placeholder operation.

## Activation

- `PlayerActionKind::ActivateAbility` carries `modes` (CR 602.2b, 700.2a). `validate_activate_ability` checks them with `announced_modes_in_order`, the count and repeat check casting also uses, combines them with `combine_chosen_modes`, and unpacks a function mode. The stack object's `chosen_modes` holds the sorted indices.
- `ActivateAbility` refuses a Rust-built ability whose choice is `*ChosenByOpponent` or `PawprintBudget`, and the entries list leaves such an ability out.
- `ActivateEntry` has `modes: Option<ModeRange>` and `targets: EntryTargets`, built by the same `modal_modes_and_targets` as a cast entry. `decision::check` checks both through `check_modes_and_targets`.
- Wire: `activate_ability` takes `modes`, and an `activate` entry adds `modes` and `mode_targets` for a modal ability. All three are skipped or defaulted when empty, so `PROTOCOL_VERSION` stays 1.

## Function handles

- `ScriptRegistry` hands out handles from 1, so `ScriptHandle(0)` means only "no function". Before, a spell function worked only when it held handle 0.
- A spell or mode has no `operation` field, so `AbilityEffect::packed` folds a function into the effect: `Script(handle)`, or `ForEachPlayer(Some(Script(handle)))` for a `ForEachPlayer` function. `commands::unpack_script_effect` splits it back at cast, at the trigger push and at the opponent's mode answer.
- Function, `Chain`, `ChainEach` and `ForEachPlayer` modes are solo only, since `is_modal_resolvable` refuses them beside another mode.
- The handle change moved the replay pin, so `BEHAVIOUR_REV` is 10.

## Tests

- `lua_modes.rs` (17): `new_game` loading a spell's modes, modes beside `operation()`, choose one, choose two in printed order, a pawprint budget, a per-mode cost, an opponent-chosen spell with a function mode, the first-handle function mode, a function or chain mode refused beside another, a chain mode alone, a Fight mode, a Bite mode, a modal enters trigger, an opponent-chosen trigger, function and chain modes on a trigger, and every spell and trigger load error.
- `scenarios/scripting/script_modes.toml`: a fixture's `s.modes()` through the scenario crate.
- `lua_modes.rs` activated cases (9): a mode and its targets announced in the activation, choose two recorded in printed order, function and chain modes alone and refused beside another, refused selections, modes on a non-modal ability, the entry's range and per-mode slots, an unlisted ability with no usable mode, a priority Answer checked against the entry, and every activated load error.
- `scenarios/modal/modal_activated_ability.toml`: a scripted modal activated ability through the scenario crate.
- Proof deck: Fork in the Road (a Lua modal instant), Crossroads Scout (a modal enters trigger) and Forked Totem (a modal activated ability) in `crates/host/src/proof_cards.rs`. `proof_game.rs` checks some game activates Forked Totem with a mode. The pinned replay game never reaches its deck slot, so `BEHAVIOUR_REV` stayed 10.

## Open

- Opponent-chosen activated modes (CR 700.2e).
- Modal mana abilities (CR 605.3b), a per-mode mana cost (CR 700.2h) and mode text.
- A chain or function mode combined with other modes.
- Three or more players choosing which opponent picks the modes (CR 700.2e).
