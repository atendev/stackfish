---
id: 053
title: Constructor-only operation and effect results
type: build
mode: HITL
status: closed
blocked_by: [052]
claimed: false
---

## Question

Make `Game.Ops.*` and `Game.Effects.*` the only way a Lua callback returns an operation or continuous effect, per section 1 of [052](file:///home/aten/Documents/Github/stackfish/.plan/closed/052-decide-lua-api-canonical-forms.md).

## Notes

- Map: `.plan/map-cleanup.md`, track C. First of four slices split from the original 053 (053, 061, 062, 063).
- Scope:
  - Remove raw-key return tables from `handle_operation_result` (`runtime.rs`) and `invoke_continuous_effect`.
  - `Effects.AddKeyword` accepts a string or a list of strings.
  - Multi-operation results use `Game.Ops.Chain({ ... })`, including `Balance`.
  - Migrate inline scripts and fixtures under `crates/` that return raw-key tables.
  - Update `docs/lua/stubs/game.lua` and `docs/lua/` for the touched entries.
- Design decision (this session): `Ops.*` values returned from a `:SetOperation` callback decode into a runtime result type that maps to `OperationOutcome`, so `PendingZoneMoves`, `PendingCast` and `PendingEvents` stay reachable. `Game.Ops.Chain(a, b, ...)` stays variadic and declarative, which amends the list form written in 052. Callback-level combining uses the same constructor family.
- `stackfish-db` scripts are migrated in 063, not here.
- Follows the `CLAUDE.md` vertical slice workflow: requirements report, `phase5/<name>` branch, approval before every commit, closing report, the user merges.
- Input: [050](file:///home/aten/Documents/Github/stackfish/.plan/closed/050-audit-lua-api-surface.md), [051](file:///home/aten/Documents/Github/stackfish/.plan/closed/051-audit-card-script-idioms.md).

## Resolution

Callbacks return `Game.Ops.*` / `Game.Effects.*` values or `nil`; raw-key tables raise an error. Added `Ops.DealDamageTo`, `Ops.AddCountersTo`, `Effects.DoublePT`, and `AddKeyword` accepts a name or list. `Ops.Chain` stays variadic and builds a runtime result when it holds a script-pick value. A returned value may hold one pending kind (zone moves, draw, events or cast) plus `GainLife`; two kinds are an error until `OperationOutcome` can carry several requests. `layers.rs` still drops a bad continuous-effect return silently, tracked in 062. Notes: `docs/implementation/27-constructor-only-results.md`.
