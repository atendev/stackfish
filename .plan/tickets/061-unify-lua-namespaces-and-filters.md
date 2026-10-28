---
id: 061
title: Unify Lua namespaces, filter schema and target declarations
type: build
mode: HITL
status: open
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

<Filled in at close.>
