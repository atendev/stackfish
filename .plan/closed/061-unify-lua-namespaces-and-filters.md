---
id: 061
title: Unify Lua namespaces, filter schema and target declarations
type: build
mode: HITL
status: closed
blocked_by: [053]
claimed: false
---

## Question

Apply sections 2, 3 and the removals of section 5 of [052](file:///home/aten/Documents/Github/stackfish/.plan/closed/052-decide-lua-api-canonical-forms.md).

## Notes

- Map: `.plan/map-cleanup.md`, track C. Second of four slices.
- Scope:
  - Merge `Game.InterveningIf` into `Game.Conditions`.
  - Collapse the four `*Affected` tables into `Game.Affected`, with explicit `Game.Filter({...})` for filter tables.
  - Flat filter keys in target tables, one shared filter decoder (reconcile `decode_filter` and `decode_target_requirement`), and merge `decode_subtypes` with `decode_card_type_names`.
  - Remove `Conditions.Always` and `Tokens.ArtifactCreature`. Keep `RegisterKeyword`, `RegisterWard` and `ManaAbility.New()`.
  - Migrate inline scripts and fixtures under `crates/`. Update stubs and `docs/lua/`.
- The reconciled filter key set is the input for 062.
- Same `CLAUDE.md` slice workflow as 053.

## Resolution

`Game.InterveningIf` is merged into `Game.Conditions`. A trigger `:SetCondition` rejects an intervening-if tag and `:SetInterveningIf` rejects an event tag, each naming the other method. `Conditions.Always` and `Tokens.ArtifactCreature` are removed.

`Game.Affected` replaces the four player-modifier `*Affected` tables: `Self`, `Enchanted`, `EnchantedPlayerControls`, `You`, `Opponent`, `Any`, `EnchantedPlayer`. Each builder validates which tags it accepts. `Game.Filter({...})` is required by the builder setters `SetAffected` and the cost modifier `SetFilter`, and rejects `self_only`, `enchanted_only` and `enchanted_player_controls`. A raw table passed to either setter is an error with a migration hint.

One decoder, `decode_object_filter_keys`, reads flat filter keys for every consumer. Target tables take the keys beside `count` and `type`; a nested `filter` table is an error and a `filter` function still works. `decode_subtypes` and `decode_card_type_names` are merged into `decode_name_list`.

Decisions made with the user:
- The shared key set covers only keys the engine backs: card_types, excluded_card_types, subtypes, excluded_subtypes, supertypes, excluded_supertypes, controller, zone, mana_value_equals/at_most/at_least, random. The unbacked 052 keys (keywords, colors, name, other, power/toughness bounds, is_token, tapped, attacking, blocking, and the target-only min_count, max_count, optional, player, zones) are marked `// TODO:` in `decode.rs`.
- `self_only`, `enchanted_only` and `enchanted_player_controls` became the `Game.Affected.Self/Enchanted/EnchantedPlayerControls` tags.
- Op, cost and condition constructors (`DestroyAll`, `Search`, `DiscardRandom`, `Costs.Sacrifice`, `DiesMatching`, `ControlsAtLeast`) keep raw flat tables.

The 052 text treated the four `*Affected` tables as already merged; they were real tables and this ticket collapsed them. The static `:SetAffected` function predicate keeps its fifth `enchanted` argument.

Inline scripts, fixtures, scenario TOMLs, `proof_cards.rs`, `docs/lua/` and `docs/lua/stubs/game.lua` are migrated. 19 new tests in `tests/lua_namespace_unification.rs`; workspace run 2292 passed, 0 failed.

Open for 062: reject unknown and wrong-typed filter keys and add the mutual-exclusion errors. Open for 063: parameter names and the `stackfish-db` script migration.
