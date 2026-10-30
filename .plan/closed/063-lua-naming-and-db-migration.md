---
id: 063
title: Standardize Lua parameter names and migrate stackfish-db scripts
type: build
mode: HITL
status: closed
blocked_by: [062]
claimed: false
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

Parameter names are standardized (`6b8cb97`): `counter` and `amount` keys, `AddManaChoice(colors, count?)`, `"C"` for colorless in color strings (still rejected as a single color, CR 105.2c), lowercase `"you"`/`"owner"` for reanimation control, PascalCase `"Hand"`/`"Battlefield"` for `Search` destinations. The unbound `ReplaceEvent`, `PayCost` and `AddContinuousEffect` header references are removed and `docs/lua/` and `docs/lua/stubs/game.lua` match.

The merge left three active rows in `../stackfish-db/stackfish.db` on the old API (00000372 argument order, 00003639 raw `discard_chosen` result, 00029796 v2 `enchanted_only`), although their `scripts/*.lua` files were migrated. The smoke test read only the files, so it passed. `e3371a6` makes it read the active database rows, fail on an active row with no script id, and skip when the database is absent or an unpulled LFS pointer. A second test fails when a row differs from `scripts/m<script_id>.lua`.

Decisions made with the user: update the three database rows in place, and keep the whitespace and CRLF-insensitive comparison. The rows were updated with a backup at `/tmp/stackfish.db.bak-before-063`; nothing was committed in `stackfish-db`, which still has other uncommitted changes.

All 25 active rows load under the strict API. Workspace run: 1907 passed in the engine crate, 0 failed. Audit: five rounds, no `APPROVE`; each round found a smaller point and all were fixed, the last two without a further audit. Not covered: the missing-file, drift and null-script-id failures are checked by reading only. No CI exists in the repo.

Branch: `phase5/db-script-drift`.
