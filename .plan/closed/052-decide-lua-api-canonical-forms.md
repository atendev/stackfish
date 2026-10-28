---
id: 052
title: Decide on Lua API removals, canonical forms, and strict key validation
type: grilling
mode: HITL
status: closed
blocked_by: [050, 051]
claimed: false
---

## Question

Which duplicate Lua API spellings and aliases should be removed, what is the single canonical form for each ability/effect/operation/filter construct, how should strict table key and value validation fail, and what is the scope of script migration for ticket 053?

## Notes

- Map: `.plan/map-cleanup.md`, track C. Unblocked by [050](file:///home/aten/Documents/Github/stackfish/.plan/closed/050-audit-lua-api-surface.md) and [051](file:///home/aten/Documents/Github/stackfish/.plan/closed/051-audit-card-script-idioms.md). Blocks 053 (implementation & migration) and 054 (`docs/LUA_API.md`).
- Code under review: `crates/engine/src/script/{globals.rs,builders.rs,decode.rs,runtime.rs,tags.rs}`.

## Resolution

### 1. Canonical operation results and continuous effect shapes
- **Constructor-only (`Game.Ops.*` and `Game.Effects.*`):** Remove raw-key return tables (`{ gain_life = ... }`, `{ set_pt = ... }`, etc.) from `handle_operation_result` and `invoke_continuous_effect`. Every operation callback and continuous effect callback must return a value constructed via `Game.Ops.*` or `Game.Effects.*`.
- **Multi-keyword static effect:** `Game.Effects.AddKeyword` accepts either a single keyword string or a Lua list of keyword strings.
- **Multi-operation composition:** Require `Game.Ops.Chain({ ... })` whenever an operation performs multiple effects (including combining `Ops.SacrificeChosen` and `Ops.DiscardChosen` in `Balance`). Do not merge raw keys across operation tables.

### 2. Unified tag namespaces and builder setters
- **Single `Game.Conditions` namespace:** Merge `Game.InterveningIf` into `Game.Conditions` so `:SetCondition` takes tags from one namespace (`Game.Conditions.*`).
- **Single `Game.Affected` namespace:** Collapse `EffectAffected`, `PreventAffected`, `CantAffected`, and `ReplacementAffected` into a single `Game.Affected` table.
- **Explicit `Game.Filter({...})` constructor for filter tables:** Builder setters (`:SetAffected`, `:SetFilter`) no longer accept raw untyped Lua tables interchangeably with tags. Table-based object filters are wrapped in `Game.Filter({...})` (or passed to `Game.Affected.Filter({...})` where appropriate), while event filters on triggers and replacement effects use `Game.EventFilters.*`.

### 3. Target declarations and unified filter schema
- **Flat filter keys in target tables:** `s.target_requirement` (as a function or table) and `:SetTarget` accept a single target requirement table or a list of target requirement tables using flat filter keys (remove the nested `filter = { ... }` wrapper).
- **Single filter decoder:** Reconcile `decode_filter` and `decode_target_requirement` into one canonical set of filter keys (`zone`, `controller`, `card_types`, `subtypes`, `supertypes`, `colors`, `keywords`, `name`, `other`, `power_max`, `power_min`, `toughness_max`, `toughness_min`, `mana_value_max`, `mana_value_min`, `is_token`, `tapped`, `attacking`, `blocking`, `enchanted_only`, `enchanted_player_controls`), plus target-only fields (`count`, `min_count`, `max_count`, `optional`, `up_to`, `player`, `zones`) only on target requirement tables.
- **Deduplicate subtype/card-type decoders:** Merge byte-identical `decode_subtypes` and `decode_card_type_names`.

### 4. Strict key, type, and enum validation
- **Reject unknown table keys everywhere:** Add a shared `reject_unknown_keys` check across all load-time decoders, inline option tables (`Search`, `ChooseN`, `CastSpell`), and game-time callback return tables (`handle_operation_result`, `invoke_continuous_effect`).
- **Reject wrong-typed values:** Replace `.ok().flatten()` and `if let Ok(Some(..))` patterns in `decode_creature_token_opts`, `decode_landwalk_quality`, `decode_copy_exceptions`, and other decoders so present keys with the wrong Lua type raise a Lua error instead of silently falling back to `None`/defaults.
- **No silent short-circuiting:** In `decode_affected_value` / `Game.Filter`, error if mutually exclusive keys (such as `enchanted_only` combined with conflicting filter keys) are passed together instead of silently dropping extra keys.
- **Strict counter names:** `parse_counter_kind_name` errors on unknown counter strings instead of silently producing `CounterKind::Custom`. Custom counters require an explicit `Game.Counter.Custom(name)` constructor (or explicit syntax) if needed.

### 5. Sugar, aliases, and helper policy
- **Remove `Conditions.Always`:** Omitting `:SetCondition` is the single canonical way to express an unconditional ability/effect.
- **Remove `Tokens.ArtifactCreature`:** Express artifact creature tokens via `Tokens.Creature` with `additional_types = { "Artifact" }`.
- **Keep `Game.RegisterKeyword` and `Game.RegisterWard`:** Retain these two documented top-level sugar functions.
- **Keep explicit `ManaAbility.New()`:** Do not add a basic-land helper; the 12 land scripts remain explicit `ManaAbility.New()` registrations.
- **Clean up dead/missing stub and doc entries:** Reconcile all drift identified in 050 (`docs/lua/` and `docs/lua/stubs/game.lua`) during tickets 053 and 054, and remove the unbound `Game.ReplaceEvent`, `PayCost`, and `AddContinuousEffect` header references in `globals.rs`.

### 6. Parameter naming, casing, and argument order
- **Quantities vs magnitudes:** Use `count` for discrete counts (cards, tokens, counters, targets) and `amount` for damage, life, and numeric magnitudes.
- **Counter key:** Standardize on `counter` across all tables and constructors (replacing `kind` and `counter_kind`).
- **Casing:** Standardize on lowercase player references (`"you"`, `"opponent"`, `"each"`, `"target"`, `"active"`) and PascalCase zone names (`"Battlefield"`, `"Graveyard"`, `"Hand"`, `"Library"`, `"Stack"`, `"Exile"`, `"Command"`).
- **Colorless `"C"`:** Accept `"C"` everywhere mana colors/symbols are parsed where colorless is valid, with consistent validation.
- **Mana constructors:** Standardize argument order across `Game.Ops.AddMana(symbols, count?)` and `Game.Ops.AddManaChoice(colors, count?)` (colors/symbols first, optional count second defaulting to 1).

### 7. Migration scope for ticket 053
- Ticket 053 will enforce the canonical API and strict validation in `crates/engine/src/script/`, update `docs/lua/stubs/game.lua`, migrate all inline Lua scripts and test fixtures in `crates/`, and migrate the 25 active card scripts in `../stackfish-db/stackfish.db`.
