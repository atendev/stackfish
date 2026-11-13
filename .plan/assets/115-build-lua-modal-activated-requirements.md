# 115 Build Lua modal activated abilities: requirements

Decision: `.plan/closed/090-lua-modal-builders.md`. The spell and trigger half shipped in 114.

## What success looks like

A card script writes

```lua
local e = Game.CreateActivatedAbility(c)
e:AddCost(Game.Costs.TapSelf)
e:SetModes(Game.ModeChoice.Exactly(1), {
    { target = { type = "object", card_types = { "Creature" }, count = 1 }, operation = Game.Ops.Damage(1) },
    { operation = Game.Ops.GainLife(2) },
})
e:Register()
```

and the player activates it with `ActivateAbility { modes: [0], targets: [creature], .. }`. The engine checks the modes and the targets for those modes in the same action, pushes one stack object carrying the combined effect, and resolves it. The priority entries list the ability with its mode range and per-mode target slots, as a modal spell's cast entry does. The host wire carries both and stays at `PROTOCOL_VERSION` 1. The proof bot activates modal abilities, and a proof card puts one into the proof game.

## Comp rules

- CR 602.2b: activation runs CR 601.2b-i, so modes are announced (601.2b) before targets (601.2c) in the one activation action. There is no separate mode Decision.
- CR 700.2a: the controller chooses the modes as part of activating. A mode with no legal targets can't be chosen. The entries list lists a modal ability only when some legal selection exists, as for spells.
- CR 608.2c: chosen modes resolve in the order written on the card, whatever order the player announced them in. The engine sorts the indices, as casting does.
- CR 700.2d: the count comes from `ModeChoice::bounds` and a repeat is refused unless the choice is repeatable, the same as casting.
- CR 700.2c, 700.2f: only the chosen modes' targets are announced. `combine_chosen_modes` builds the target spec and effect, so solo-only modes (function, `Chain`, `ChainEach`, `ForEachPlayer`) behave as they do for spells: each can be the one chosen mode and none can join another.
- CR 700.2e: another player choosing an activated ability's modes is not built. The builder refuses `*ChosenByOpponent`, and `ActivateAbility` refuses a Rust-built ability with one.
- CR 700.2h, 700.2i: per-mode costs and pawprints are spell-only. The builder refuses `cost`, `pawprint` and `PawprintBudget`. `ActivateAbility` also refuses a Rust-built `PawprintBudget` ability.
- CR 605.1a, 605.3b: a mana ability gets no modes (090, "Per kind"). `ManaAbilityBuilder` stays without `:SetModes`. This build adds one guard 090 leaves implicit: a mode of an activated ability whose operation is `AddMana` or `AddManaChoice` is a load error. Without it a solo mana mode would reach the mana-ability branch of `ActivateAbility` and resolve without the stack, which is a modal mana ability by another route.

## Engine changes

1. `PlayerActionKind::ActivateAbility` gains `modes: Vec<u32>`. Every constructor and pattern updates (about 110 sites, most of them tests that add `modes: Vec::new()`).
2. `validate_activate_ability`: a non-modal ability with modes announced is rejected. A modal ability validates the count, repeats and indices, sorts the indices, runs `combine_chosen_modes`, and unpacks a function mode with `unpack_script_effect`. The combined target spec feeds `validate_targets` and the stack object. `chosen_modes` on the `StackObject` holds the sorted indices. A shared helper checks count, repeats and indices, so the cast and activation rules cannot drift.
3. `entries.rs`: `ActivateEntry` gains `modes: Option<ModeRange>`, and `targets` becomes `EntryTargets` (`Fixed` or `PerMode`). `mode_range` and the per-mode listing are shared between cast and activate entries. An opponent-chosen or pawprint activated ability is not listed, matching what the command refuses.
4. `decision/check.rs`: `check_activate` checks modes and per-mode targets with the same `check_modes` as casting.
5. `script/decode.rs`: `ModeOwner::Activated` refuses `cost`, `pawprint`, `PawprintBudget` and `*ChosenByOpponent`, each with its own message naming the script context and mode index. A mode whose operation is `AddMana` or `AddManaChoice` is a load error for an activated ability.
6. `script/builders.rs`: `ActivatedAbilityBuilder:SetModes(choice, modes)`. `:Register()` needs exactly one of `:SetOperation` or `:SetModes` and refuses `:SetTarget` beside `:SetModes`, as the triggered builder does. A modal ability keeps `target` and `effect` at `None` and the placeholder `operation` (handle 0), as modal triggers do. `:AddCost` and `:SetFunctionsFrom` work unchanged. The `*ChosenByOpponent` refusal fires inside `:SetModes` through `ModeOwner::Activated`, before `:Register()`. 090 places it at `:Register()`; both are script load errors with the same text, and checking in the decoder keeps every per-kind refusal in one place.
7. Scenario crate: `activate_ability` gains an optional `modes` field (default empty), passed through and printed in the report.
8. Host wire: `WireAction::ActivateAbility` gains `#[serde(default)] modes`. `WireEntry::Activate` gains `modes` (skipped when `None`) and `mode_targets` (per-mode slots, skipped when empty). `targets` keeps its flat array, empty for a modal ability. All three are additive, so `PROTOCOL_VERSION` stays 1.
9. Proof bot: `activate` reads `modes` and `mode_targets` through the same mode picker as casting. A new proof card with a modal tap ability goes into a deck. The proof game changes, so `BEHAVIOUR_REV` moves from 10 (its value in `crates/engine/src/lib.rs` after 114) to 11 and the replay pin moves.
10. TODO removal: `commands/activate.rs` ("no modes on activated abilities", CR 700.2b) and `types/card.rs` (CR 700.2b, a modal activated or triggered ability). The `AbilityEffect::Modal` arm comment in `activate.rs` that cites CR 700.2b changes too, since a modal ability's resolved effect can now be `Modal`.

## TODOs to add

- `// TODO: opponent-chosen modes for an activated ability (CR 700.2e)` where the decoder and `ActivateAbility` refuse it. The ticket words it with the citation first; CLAUDE.md puts the citation at the end, so the CLAUDE.md form wins.
- The existing `TODO: modal mana abilities, CR 605.3b` stays. A note next to the new AddMana-mode refusal points at it.

## Docs

- `docs/lua/stubs/game.lua`: `ActivatedAbilityBuilder:SetModes` and the `:Register` note.
- `docs/lua/activated-abilities.md`: a "Modal activated abilities" section with an example and the refusals.
- `docs/lua/not-available-yet.md`: the modal activated entry becomes "opponent-chosen activated modes".
- `docs/lua/game-constants.md`: note which `ModeChoice` tags an activated ability accepts.
- `docs/scenario_format.md`: `activate_ability.modes`.
- `docs/implementation/39-lua-modes.md`, `17-legal-actions.md`, `18-host-session.md` (wire), `DESIGN.md` where it says where modes come from, and the backlog line if one names this.
- Already done by 114 and not repeated: `mana-abilities.md` ("Modes are not available"), `LUA_API.md` (`Game.ModeChoice`), `card-script-basics.md`, `triggered-abilities.md` and `CONTEXT.md` (Mode, Solo-only mode). Each is only touched here if it says activated abilities can't be modal.

## Tests

- `lua_modes.rs` (or a sibling file): a modal activated ability with per-mode targets activates through `ActivateAbility` and resolves the chosen mode; choose-two resolves in printed order; a function mode and a chain mode each resolve alone; a function mode beside another mode is refused; modes on a non-modal ability are refused; a wrong count, a repeat and an index past the end are refused; a target belonging to an unchosen mode is refused.
- Entries: the activate entry shows the mode range and per-mode slots; a modal ability whose only usable mode needs a missing target is not listed when no selection is legal.
- Load errors, each its own case through the activated builder: `ExactlyChosenByOpponent`, `UpToChosenByOpponent`, per-mode `cost`, `pawprint`, `PawprintBudget`, an `AddMana` mode, an `AddManaChoice` mode, `:SetOperation` with `:SetModes`, `:SetTarget` with `:SetModes`, `:Register()` with neither, and fewer than two modes (one case showing the shared decoder runs for this builder). The other shared decoder errors (count of 0, count above the mode count, no `operation`, unknown keys, `ChainEach` word mismatch, opponent-chosen with a target or cost) are already pinned once each by 114's spell and trigger tests on the same `decode_modes`, so they are not repeated per builder.
- Host wire: a unit test that the activate entry serializes `modes` and `mode_targets` only for a modal ability, and that an `activate_ability` reply with `modes` parses.
- Proof bot: the proof game runs with the new card and the replay pin moves.
- TOML scenario `crates/engine/tests/scenarios/modal/modal_activated_ability.toml`: an inline scripted artifact with a modal tap ability, activated for its target mode, then its other mode, asserting life and zones. No `../stackfish-db`.
- `lua_api_doc_sync` keeps passing with the stub change.
