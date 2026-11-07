---
id: 95
title: Scenario schema assertions and fixture fields
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Which assertions and fixture fields does the TOML scenario schema gain? 28 of the 32 targeting and layer tests stay in Rust because the schema cannot express them.

## Notes

- Origin: the "Scenario assertions" entry in `99-backlog.md`, the alternative-cost and buyback entry in `docs/lua/not-available-yet.md`, and the "no field for a chain" item from `25-chain-operations.md`.
- Gaps to rule on:
  - `expect_error` with a rejection variant.
  - An exact `damage_marked` assertion, and a `CardSpec` resolution effect with no Lua script.
  - Ledger event kinds present or absent, and a stack object's targets.
  - Effective power, toughness and keywords after layers, and a per-effect timestamp on `continuous_effect`.
  - A mid-scenario state edit that makes a chosen target leave play.
  - `[cards.alternative_costs]`, `buyback_cost`, and chain fields.
- Decide which are worth the schema weight and which tests stay in Rust. Ticket 071 already moved the tests the schema could express.
- Read `docs/scenario_format.md`. Use the `grilling` and `domain-modeling` skills.

## Resolution

Decided in an auto-mode grilling run. A sonnet subagent stood in for the user over two rounds, took every recommendation and added the conditions below. A haiku auditor approved the question report in its third round. The builds are "Build scenario assertions and fixture fields" and then "Move the targeting and layer tests to TOML".

**Count.** The four files held 32 tests. Ticket 071 moved two and deleted two duplicates, which leaves 28. Of those, 27 move and 1 stays.

**Gains a field or assertion.**
- `expect_error` takes `true` or a Rust variant name such as `"InvalidTarget"`. A name passes when the action errors and the error's Debug text holds that name as a whole identifier, so `InvalidTarget` does not match `InvalidTargetCount`. There is no closed list across the 13 `submit_*` error enums, and the name does not say which enum. A typo can only fail a step, never pass one.
- `damage_marked { card, equals }` and `assert_damage_marked`. It reads the object the card key points to now, in any zone. A dead creature's graveyard object reads 0 (CR 400.7), and the failure message names the zone.
- `event_count { kind, equals }` and `assert_event_count` over the whole ledger. `kind` is a snake_case variant name (`"fizzled"`, `"stack_object_resolved"`), and absent is `equals = 0`. Names come from a new `GameEventKind::name() -> &'static str`, an exhaustive match with no wildcard arm and a unit test that the names are unique. An unknown `kind` is a load error.
- `stack_targets { card, equals }` compares the full ordered target list of the spell whose card key is `card`, through the existing `resolve_target` parser. `stack_size { equals }` counts stack objects.
- `characteristics { card, power, toughness, keywords }` reads `layers::effective_characteristics` on the live state. At least one field is required, and an omitted field is unchecked. `keywords` is the exact effective set, so `keywords = []` means no keywords. A contains-style `has_keywords` was proposed and dropped, because no moved test needs it. If the script runtime is not reachable from `&Engine`, the build adds a one-line `pub` accessor.
- Setup timestamps, with no field. Each `[[setup]]` object takes a stamp from the build-time counter that `AbilityId`s use, in setup order (CR 613.7d). A TOML `continuous_effect` takes its host card's stamp (CR 613.7a). This models only permanents that entered in setup order, and the host-stamp rule covers only TOML effects. A card holds at most one TOML effect, and 093 panics on a script beside one, so setup gives no stamp ties. `next_sequence_no` is seeded after the last stamp. A test covers every stamp and fixed `AbilityId` the build hands out and checks that none share a value. Effects made in play by a resolving spell or ability keep the engine's own stamps (CR 613.7b).
- `cast_spell` gains `alternative_cost` (an index into the card's alternative costs) and `pay_buyback`, which replace the two TODOs in `crates/scenario/src/actions.rs`. The manual scenario build loads a script's `s.alternative_costs()` and `s.buyback_cost()` through the existing `Engine::build_alternative_costs` and `build_buyback_cost`, as it loads `s.operation()`. CI tests use inline scripts, and db-backed cases skip in CI as they do today. An out-of-range index is the engine's rejection, reported as `Errored`. If the engine panics, the build fixes that as an engine bug with its own engine test. The ticket "`new_game` loads alternative costs and buyback" covers `Engine::new_game`, and this build covers the manual build path. Both call the same two `Engine` functions, and neither blocks the other.

**Stays out of the schema.**
- A TOML resolution effect. A one-line inline `script` whose `s.operation()` returns a `Game.Ops` tag builds the same `AbilityEffect` (`register_script_operation`). The cost is a short script per fixture card, and `Game.Ops` stays the one syntax for effects.
- A state-edit step that removes a target. The fizzle scenarios cast a response instant with `s.target_requirement()` and `Game.Ops.Destroy`, so the target leaves through the engine (CR 608.2b). The Rust tests' "target vanishes with no event" setup is not reproducible, and no legal game reaches it.
- `[cards.alternative_costs]` and `buyback_cost` tables. Lua already has the cost syntax.
- Chain fields. An inline script with `Game.Ops.Chain` covers it, as `scripting/mass_choice_balance.toml` shows.

**What moves.** All 8 tests in `targeting_basics.rs`, all 6 in `targeting_up_to_n.rs`, all 6 in `targeting_at_least_n.rs`, and 7 of 8 in `layer_engine_basics.rs`. `cda_power_and_toughness_are_still_skipped_not_guessed` stays in Rust, because it builds a `*` power with no script, which a `CardSpec` cannot express. A Rust test is deleted only once its TOML version asserts the same thing, not a weaker one, and a fizzle scenario must pass before its Rust test goes.

**Lua statics.** A `Game.CreateStaticAbility` effect is stamped when `run_card_script` drains it, not with its host's stamp. A Lua static against a TOML `continuous_effect` is therefore not ordered by CR 613.7a. The build adds `// TODO: a static ability's effect takes its host's timestamp (CR 613.7a)` at the drain, and the docs say so. No moved test mixes the two sources. Fixing the drain is out of scope, since it moves in-game timing and the replay pin.

**Build conditions.** The timestamp stamping is the first commit and passes the full scenario suite on its own, with before and after results for the six `layers/` scenarios and the whole suite in the build report. Then `GameEventKind::name()` and the runtime accessor, then the fields and docs. Every new assertion gets a `crates/scenario` unit test with a failing case that reports a hard failure. `expect_error` gets tests for a wrong name and a substring-only name, and `alternative_cost` gets one for an out-of-range index. No Lua API changes, so `docs/lua/stubs/game.lua` stays as it is, and the audit confirms that. The seed 9 replay pin must not move.

**Docs.** `docs/scenario_format.md` documents each field and gains a "Resolution effects, chains and removing a target" section with an inline damage spell, a `Game.Ops.Chain` script and a response-spell fizzle. New text says "fixture name" for a `[[cards]]` `key`. The "no field for a chain" lines leave `99-backlog.md` and `25-chain-operations.md` with a pointer to that section. The alternative-cost entry in `docs/lua/not-available-yet.md` is rewritten to the cast fields. The "Scenario assertions" backlog entry is replaced by the two build tickets. `DESIGN.md` gains one line on the scenario assertions.

**Vocabulary.** The new names reuse engine words: `stack_targets` and `stack_size` for stack objects, `event_count` for ledger events, and `characteristics` for the values after layers (CR 109.3). `CONTEXT.md` gains a flagged ambiguity: a scenario's `key` is a fixture name local to one file, not a **Card key**.
