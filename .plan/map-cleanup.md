# Map: Cleanup and Lua API hardening

## Goal

Bring the codebase up to the comment and organization rules in `CLAUDE.md`, replace test binary sprawl with a clean layout, and harden the Lua API so card scripts have one canonical spelling per concept and fail loudly on unknown keys.

## Notes

- All build and task work follows the `CLAUDE.md` vertical slice workflow: requirements report first, `phase5/<name>` branch, user check before commit, merge and push when instructed.
- Every comment pass is checked with `python3 tools/comment_diff.py` so no code changes slip in and no `TODO` is lost.
- Current counts from discovery: `crates/engine/src/engine/mod.rs` is 9,593 lines, `crates/engine/src/script/globals.rs` is 2,340 lines, `crates/scenario/src/lib.rs` is 1,924 lines, `crates/engine/tests/` holds 224 files, and `phase5_` starts 229 test file names.
- All tickets on `map.md` and `map-fixes.md` are closed (001 through 037). Numbering continues at 038.
- Closed tickets on `map-fixes.md` cover:
  - 026: The 12 combat evasion failure scenarios in `cards/tests/failure_scenarios/combat_evasion/`. Audited the 12 combat evasion failure scenarios, rewrote the 4 scenarios that failed for unrelated deck/setup reasons (`393593f`), and deleted the obsolete `combat_evasion` failure bundle (`ee60676`); all 11 remaining scenarios pass.
  - 036: Replacing the `stackfish-carddb` test script fixture so tests stop depending on the live `cards/scripts/` tree (`a45158e`).

## Decisions so far

- 038: Split `crates/scenario/src/lib.rs` (1,924 lines) into `lib.rs` (types and `load_from_toml_str`), `setup.rs` (board construction), `runner.rs` (scripted action execution and shortcuts) and `assertions.rs` (`verify_expectations`).
- 039: Kept `crates/engine/src/types.rs` (1,123 lines) as one file. A clean domain split (`card.rs`, `ability.rs`, `effect.rs`, `replacement.rs`) is recorded in the closed ticket if the file grows further.
- 040: Split `crates/engine/src/script/globals.rs` (2,340 lines) into a `globals/` directory (`mod.rs`, `condition.rs`, `target.rs`, `ops.rs`, `builders.rs`, `helpers.rs`) by Lua namespace and builder type.
- 041: Split `crates/engine/src/engine/mod.rs` (9,593 lines) into 10 submodules (`combat.rs`, `Zone_and_sba.rs`, `layers.rs`, `triggers.rs`, `targeting.rs`, `costs.rs`, `stack.rs`, `commands.rs`, `decisions.rs`, `script_hooks.rs`) leaving `mod.rs` at 979 lines.
- 042: Trimmed comments across all 69 files under `crates/engine/src/` to the `CLAUDE.md` comment rules (comment lines 9,858 -> 6,797, all 187 `TODO`s preserved, `comment_diff.py` exit 0).
- 043: Trimmed comments across `crates/carddb/src`, `crates/scenario/src`, `crates/testutil/src` and `crates/host/src` to the `CLAUDE.md` comment rules (comment lines 2,400 -> 1,004, all 10 `TODO`s preserved, `comment_diff.py` exit 0).
- 044: Trimmed comments across all 237 test files in `crates/engine/tests`, `crates/host/tests`, `crates/scenario/tests` and `crates/carddb/tests` to the `CLAUDE.md` comment rules (comment lines 17,977 -> 4,769, all 52 `TODO`s preserved, `comment_diff.py` exit 0).
- 045: Measured `stackfish-engine` test build times (7.65s wall / 1m 53s CPU after touching `lib.rs`, 95% spent linking 224 test binaries totaling 16 GiB) and decided to proceed with both halves of 046: consolidating `crates/engine/tests/` into a single test binary (`tests/main.rs` with modules) and renaming `phase5_*` test files by feature.
- 046: Consolidated 224 engine integration test files into a single binary (`crates/engine/tests/main.rs` with `autotests = false`) and stripped `phase5_` from all 229 Rust test files across `crates/engine/tests/` and `crates/host/tests/`. Incremental test rebuild after touching `crates/engine/src/lib.rs` dropped from 7.65s (1m 53s CPU) to 2.43s (4.64s CPU).
- 047: Audited `crates/engine/src/` for missing local unit tests. About 200 pure helpers across 35 files lack `#[cfg(test)]` coverage; the densest gaps are in `types/`, `commands/legality.rs`, `commands/combat/*`, `script/decode.rs`, `sba.rs`, `replacement.rs`, and `triggers.rs`. [closed/047-missing-unit-tests-audit.md]
- 048: Added local `#[cfg(test)]` coverage for pure helpers in `color_identity.rs` and `types/{zone,object,player,counter,card,mana,cost}.rs`. [closed/048-add-unit-tests-pure-helpers.md]
- 049: Added `#[cfg(test)]` coverage for `layers.rs`, `commands/legality.rs` and `commands/combat/predicate.rs` with minimal state fixtures. Open gaps: `FilterSpec::matches`, `ReplacementEffect`, `ContinuousEffect` helpers and `script::decode`. [closed/049-add-unit-tests-stubbed-helpers.md]
- 050: Audited `crates/engine/src/script/`. Every table decoder ignores unknown keys except `decode_entry_options`; strict validation touches about 12 call sites plus one helper. Operation results and effect shapes each have a tag and a raw-key spelling, three method names take different argument families, and several decoders swallow wrong-typed values. `cards/` is absent from this checkout, so no card script was read. [closed/050-audit-lua-api-surface.md]
- 051: Audited the 25 active Lua scripts in the sibling `stackfish-db` database. Eleven basic-land scripts repeat the mana-ability builder, target tables repeat too little to hide, and Balance's custom counting is card-specific. [closed/051-audit-card-script-idioms.md]
- 052: Decided on canonical Lua API forms and strict validation: require `Game.Ops.*` and `Game.Effects.*` constructors (remove raw-key return tables, use `Ops.Chain` for multi-op results, allow string or list in `Effects.AddKeyword`); unify `InterveningIf` into `Game.Conditions` and the 4 `*Affected` tables into `Game.Affected` with explicit `Game.Filter({...})` for filter tables; use flat filter keys in target tables with one shared filter schema; reject unknown keys, wrong-typed values, and unknown counter strings at both load time and game time; remove `Conditions.Always` and `Tokens.ArtifactCreature`, keep `RegisterKeyword`/`RegisterWard` and explicit `ManaAbility.New()`; standardize parameter names (`count`, `amount`, `counter`), casing, `"C"` colorless, and `AddMana`/`AddManaChoice` argument order; migrate both `crates/` inline scripts and the 25 active scripts in `../stackfish-db/stackfish.db`. [closed/052-decide-lua-api-canonical-forms.md]
- 053: Callbacks return `Game.Ops.*` and `Game.Effects.*` values or `nil`, raw-key tables error, `DealDamageTo`, `AddCountersTo` and `DoublePT` added, and a returned value holds at most one pending kind. [closed/053-constructor-only-ops-and-effects.md]
- 061: Merged `InterveningIf` into `Game.Conditions`, collapsed the four `*Affected` tables into `Game.Affected` plus `Game.Filter({...})`, flattened target filter keys behind one shared decoder covering only engine-backed keys, and removed `Conditions.Always` and `Tokens.ArtifactCreature`. Unbacked filter keys are TODOs; the key set feeds 062. [closed/061-unify-lua-namespaces-and-filters.md]
- 053 split: the original 053 was too large for one slice, so it became 053, 061, 062 and 063, ordered so the filter schema settles before strict key sets are written. 054 and 058 now wait on 063.
- Renumbering: open tickets from the Lua track onward moved up by one (old 049-059 are now 050-060) because 049 was used for the follow-up unit-test ticket.

## Tracks

### A. File splits and comment cleanup

Split oversized files along responsibility lines first, then run the comment pass so each file is touched once in its final shape.

- 038 Split `crates/scenario/src/lib.rs` into submodules. Type task, HITL. Closed.
- 039 Decide whether to split `crates/engine/src/types.rs`. Type grilling, HITL. Closed.
- 040 Split `crates/engine/src/script/globals.rs` by Lua namespace. Type task, HITL. Closed.
- 041 Split `crates/engine/src/engine/mod.rs` by subsystem. Type task, HITL. Closed.
- 042 Comment pass on `crates/engine/src/`. Type task, HITL. Blocked by 039, 040, 041. Closed.
- 043 Comment pass on `crates/carddb`, `crates/scenario`, `crates/testutil` and `crates/host`. Type task, HITL. Blocked by 038. Closed.
- 044 Comment pass on test files across all crates. Type task, HITL. Closed.

### B. Tests

Stop the test binary sprawl, drop the `phase5_` prefix, and fill in missing module unit tests.

- 045 Measure test build and decide on consolidating test binaries. Type grilling, HITL. Closed.
- 046 Consolidate engine test binaries and rename `phase5_*` files by feature. Type task, HITL. Blocked by 044 and 045. Closed.
- 047 Audit `crates/engine/src/` modules for missing `#[cfg(test)]` unit tests. Type research, AFK. Blocked by 041. Closed.
  - List pure helpers and data structures in `crates/engine/src/` that have no local unit tests and are only exercised end-to-end.
- 048 Add unit tests for pure engine helpers. Type build, HITL. Blocked by 047. Closed. A follow-up, 049, covered helpers that need a small `GameState` or layer stubs. Closed.
- 060 Decide which repetitive Rust tests become TOML scenarios and whether to rename `phase5_*.toml`. Type grilling, HITL. Blocked by 046.
  - Inspect the reorganized test suite and `crates/engine/tests/scenarios/phase5/phase5_*.toml` to decide which table-style Rust tests should move to TOML scenarios and how the TOML scenario directory should be named.

### C. Lua API audit and cleanup

Make the Lua surface area small, documented, and strict before scaling up card script generation.

- 050 Audit Lua API surface area, aliases, and silent table keys. Type research, AFK. Blocked by 040. Closed.
  - Inventory every `Game.*` constructor, builder method, and table decoder in `crates/engine/src/script/`. List duplicate ways to express the same ability, inconsistent parameter shapes, and every table decoder that ignores unknown keys.
- 051 Audit `cards/scripts/` for idioms that belong in engine helpers. Type research, AFK. Closed.
  - The complete `cards/scripts/` source is absent from this checkout; the audit used the 25 active scripts in `../stackfish-db/stackfish.db`.
  - Scan existing Lua scripts for multi-line workarounds, repeated filter tables, or boilerplate that a small Lua helper or engine primitive would replace.
- 052 Decide on Lua API removals, canonical forms, and strict key validation. Type grilling, HITL. Blocked by 050 and 051. Closed.
  - Pick the single canonical spelling for each concept, decide which aliases to cut, and agree on how unknown table keys fail at load time.
- 053 Constructor-only operation and effect results (`Ops.Chain`, `AddKeyword` list). Type build, HITL. Blocked by 052. Closed.
- 062 Strict key, type and counter-name validation in Lua decoders. Type build, HITL. Blocked by 061.
- 063 Standardize parameter names, casing and mana argument order, fix stub drift, migrate the 25 `stackfish-db` scripts. Type build, HITL. Blocked by 062.
- 054 Document the canonical Lua API in `docs/LUA_API.md`. Type task, HITL. Blocked by 063.

### D. Architecture and housekeeping

Check module visibility, workspace hygiene, and developer tooling once the splits settle.

- 055 Audit `pub` vs `pub(crate)` visibility and dead helpers across `crates/`. Type research, AFK. Blocked by 038, 040, 041.
- 056 Audit workspace `Cargo.toml` dependencies, lints, and formatting. Type research, AFK.
  - Check shared `[workspace.dependencies]`, unused dependencies, `cargo clippy` warnings, and `cargo fmt --check` status across all crates.
- 057 Tighten visibility, remove dead helpers, and apply workspace lints. Type task, HITL. Blocked by 055 and 056.
- 058 Update `docs/DESIGN.md` to reflect the new module layout. Type task, HITL. Blocked by 038, 040, 041, 046, 063.
- 059 Decide on a pre-commit or CI check for `fmt`, `clippy`, and `comment_diff.py`. Type grilling, HITL. Blocked by 057.

## Frontier

Open and unblocked:

- 062, 055, 056, 060

Suggested order. Work 062, 063 in sequence; they edit the same script files. Research tickets 055 and 056 and grilling 060 can run in parallel.

## Not yet specified

- Whether `crates/carddb` and `crates/host` need any file splits of their own (`host/src/session.rs` is 935 lines, `host/src/wire/decision.rs` is 559 lines). Revisit after 043.
