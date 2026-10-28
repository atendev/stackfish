---
id: 050
title: Audit Lua API surface area, aliases, and silent table keys
type: research
mode: AFK
status: closed
blocked_by: []
claimed: false
---

## Question

Which `Game.*` constructors, builder methods and table decoders in `crates/engine/src/script/` offer more than one way to express the same ability, take inconsistently shaped parameters, or silently ignore unknown table keys?

## Notes

- Map: `.plan/map-cleanup.md`, track C. This feeds ticket 052 (decide canonical forms and strict key validation), together with 051.
- Code lives in `crates/engine/src/script/`: `globals.rs` (Lua namespaces), `builders.rs` (builder methods), `decode.rs` (table decoders), `encode.rs`, `runtime.rs`, `tags.rs`, `types.rs`.
- Start from `docs/lua/index.md` and `docs/lua/stubs/` for the documented surface, then read the code and compare.
- Deliver an inventory, not changes: no edits to `crates/` or `cards/`.

## Resolution

Read-only audit of `crates/engine/src/script/` against `docs/lua/` and the stubs. Line refs are to `decode.rs` (D), `globals.rs` (G), `builders.rs` (B) and `runtime.rs` (R).

**Silent unknown keys.** Every table decoder reads keys with `table.get("key")` and never iterates, so unknown keys are ignored. The only strict one is `decode_entry_options` (G:1178). Unknown builder methods already error. Sizing for strict validation: 9 dedicated decoders (D:157, 203, 457, 493, 544, 675, 766, 861, 1031), 3 inline option tables (`Search` G:412, `ChooseN`, `CastSpell`), and 2 game-time return-table readers (R:796 `handle_operation_result`, R:544 `invoke_continuous_effect`). About 12 call sites plus one shared `reject_unknown_keys` helper. The two filter decoders (D:203, D:675) need their allowed key sets reconciled first, and the game-time readers change behavior if made strict.

**Duplicate spellings.**
- Operation results exist as `Ops.*` tags and as raw returned keys (`gain_life`, `draw_cards`, `deal_damage`, `add_counters`, `sacrifice_chosen`, `discard_chosen`, `cast_spell`).
- Static effects exist as `Effects.*` tags and as function-shape keys (`set_pt`, `add_keyword`, ...). The shapes differ: tag `AddKeyword(name)` takes one string, key `add_keyword` takes a list.
- `RegisterKeyword` and `RegisterWard` are documented sugar.
- `Tokens.ArtifactCreature` equals `Tokens.Creature` with `additional_types`.
- Omitting `:SetCondition` equals `Conditions.Always`.
- Target declaration has three entry points (`:SetTarget`, `target_requirement()`, flat vs nested `filter=`).
- `decode_subtypes` (D:392) and `decode_card_type_names` (D:412) are byte-identical.

**Inconsistent shapes.**
- Same method name takes different families: `:SetCondition` (Conditions vs InterveningIf), `:SetFilter` (EventFilters tag vs table), `:SetAffected` (table vs tag).
- Four identical `*Affected` tag tables.
- Amount naming varies (`n`, `amount`, `count`), and `AddMana(letter, count)` vs `AddManaChoice(count, colors)`.
- Zone is positional in some ops, `opts.destination` in `Search`, `zone=` in target filters, and absent in the effect filters.
- Case conventions mix ("you" vs "You", capitalized vs lowercase zones).
- Colour `"C"` is accepted in some places and rejected in others.
- Counter kind is `kind`, `counter` or `counter_kind` depending on site.

**Silent failures worth fixing regardless of the key policy.**
- `parse_counter_kind_name` (D:315) turns a typo into `CounterKind::Custom`.
- `.ok().flatten()` and `if let Ok(Some(..))` swallow wrong value types in `decode_creature_token_opts`, `decode_landwalk_quality` and `decode_copy_exceptions`.
- `decode_affected_value` (D:766): `enchanted_only` and `enchanted_player_controls` short-circuit and drop the other keys.
- `handle_operation_result` (R:796-893): a typo like `gain_lfe` returns `Finished` with no error, and `draw_cards` plus `deal_damage` drops the damage. The docs say all keys combine.
- `invoke_continuous_effect` (R:544-649): first match wins, and bad names return `Ok(None)` while the tag form errors at load.

**Docs and stub drift.**
- Missing from the stub or docs: `Conditions.Cycled`, `MadnessExiled`, `InterveningIf.Escaped`, `HadNoCounters`, `Ops.BecomeBlocked`, `MadnessCastOrDiscard`, `EventFilters.WouldDieEnchanted`, `WouldEnterEnchantedPlayerGraveyard`, `ReplacementPayloads.EntersWithCountersEscape`, `UmbraArmor`.
- In the docs but not the stub: `Ops.Cascade`, `Ops.CastSpell`, `Ops.ReturnSelfWithCounter`, `Effects.AddSupertype`, `Effects.SetCardTypes`.
- Stub tables that undercount keys: `OperationResultTable` (1 of 7), `EffectShapeTable`, `ReplacementAffectedTable` (3 of 7), `FilterTable` (advertises keys the decoders ignore), `BecomeCopyExceptions`.
- `G:42-49` header names unbound `Game.ReplaceEvent`, `PayCost` and `AddContinuousEffect`.

**Ranked cleanup candidates.**
1. Merge the filter decoders and error on unknown keys.
2. Pick one of tag or raw-key for operation results and effect shapes, then validate it.
3. Unify the tag family and argument shape of `:SetCondition`, `:SetFilter` and `:SetAffected`.
4. Collapse the four `*Affected` tables and the magnitude setters.
5. Stop swallowing wrong-typed values.
6. Standardise amount names, zone passing, case and counter keys.
7. Minor: `ArtifactCreature`, omit-vs-`Always`, stub gaps.

**Caveat for 051 and 053.** `cards/` does not exist in this checkout, so no real `.lua` script was read. How many scripts rely on an ignored key is unknown, and scripts that do will start failing under strict validation. Ticket 051 needs the card scripts restored first or has to read them from wherever they live.
