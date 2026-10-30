---
id: 054
title: Document the canonical Lua API in docs/LUA_API.md
type: task
mode: HITL
status: open
blocked_by: []
claimed: true
---

## Question

Write a single canonical reference document for card script authors that captures every constructor, builder method, and table schema decided in 052 and implemented in 053, 061, 062 and 063. The doc must live at `docs/LUA_API.md` and be the one place a script author checks when writing a new card.

## Notes

- Map: `.plan/map-cleanup.md`, track C. Open after 063 closes because the canonical forms only settle once the migration is done.
- Source material:
  - `docs/lua/index.md` is the old sprawling API reference.
  - `.plan/closed/052-decide-lua-api-canonical-forms.md` records the decisions.
  - `.plan/closed/053-constructor-only-ops-and-effects.md`, `061-unify-lua-namespaces-and-filters.md`, `062-strict-lua-validation.md`, `063-lua-naming-and-db-migration.md` record the implementation.
  - `crates/engine/src/script/` contains the actual decoders and constructors.
- Scope:
  - Replace or supersede `docs/lua/index.md` with `docs/LUA_API.md` at the repo root so it is easy to find.
  - One page, grouped by namespace: `Game.Conditions`, `Game.Affected`, `Game.Filter`, `Game.Target`, `Game.ManaAbility`, `Game.Costs`, `Game.Ops`, `Game.Effects`, `Game.Abilities`, `Game.Triggers`, `Game.Replacements`, `Game.Continuous`.
  - For each constructor: required arguments, optional arguments, return shape, common mistakes, and one inline example.
  - Flattened filter key schema (shared by Target, Affected, Filter, ChooseN, Search, etc.).
  - Strict validation rules: unknown keys error, wrong types error, counter names must be known, `Ops.Chain` for multi-op results.
  - Color and mana syntax (`"C"`, hybrid, Phyrexian, snow).
  - Migration notes from the old `index.md` forms.
- Non-goals:
  - No engine internals.
  - No exhaustive CR citations (use `docs/comp_rules.md`).
  - No Godot or host protocol details.
- If this doc changes the Lua API surface, update the game stub in `docs/` as required by `CLAUDE.md`.

## Resolution

<Filled in at close.>
