---
id: 063
title: Standardize Lua parameter names and migrate stackfish-db scripts
type: build
mode: HITL
status: open
blocked_by: [062]
claimed: true
---

## Question

Apply section 6 and the stub cleanup of section 5 of [052](file:///home/aten/Documents/Github/stackfish/.plan/closed/052-decide-lua-api-canonical-forms.md), then migrate the 25 active scripts in `../stackfish-db/stackfish.db` to the final API.

## Notes

- Map: `.plan/map-cleanup.md`, track C. Last of four slices.
- Scope:
  - `count` vs `amount`, the `counter` key, lowercase player references, PascalCase zones, `"C"` for colorless, and `AddMana(symbols, count?)` / `AddManaChoice(colors, count?)` argument order.
  - Remove the unbound `Game.ReplaceEvent`, `PayCost` and `AddContinuousEffect` header references in `globals.rs`, and fix the stub and doc drift listed in 050.
  - Migrate the DB scripts in one pass and check each loads under the strict API. DB scripts stay on the old API during 053, 061 and 062, so `stackfish-db` is expected to be broken against `master` until this closes. CI does not depend on it.
- Same `CLAUDE.md` slice workflow as 053.

## Resolution

<Filled in at close.>
