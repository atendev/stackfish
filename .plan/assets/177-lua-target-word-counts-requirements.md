# 177 Let Lua target words take a count: requirements

Follows 116 (`closed/116-build-divided-damage-and-counters.md`). Branch `phase5/lua-target-word-counts`.

## What a successful build looks like

A card script can write a triggered or activated ability with one target word that takes more than one target, such as "this deals 3 damage divided as you choose among up to three targets" or "tap up to two target creatures". `:SetTarget` accepts the same count keys that `s.target_requirement()` does: `count`, `up_to`, `at_least` and `exclude_source`. Each one sits flat beside the filter keys, and each one works in a bare table or in any entry of the multi-word array. `count` is optional and defaults to 1, so every existing script loads unchanged. A Lua-built divided ability then activates or triggers with two or three targets and a split, the same way the Rust-built abilities in `divided_effects.rs` already do. The engine below the decoder needs no change, because `TargetSlot`, `validate_targets`, the trigger target pause and the division checks already read any `TargetRequirement`.

## Finding: mode words already take a count

The ticket says mode `target` words always make a one-target word. That is only true for a mode `target` given as a bare function. A mode `target` table already goes through `decode_target_spec_table`, the `s.target_requirement()` decoder, so it already requires `count` and accepts `up_to`, `at_least`, `exclude_source` and a `filter` function. A function-valued word keeps count 1, as it does for `:SetTarget`, and a script that wants a count with a predicate writes `{ count = 3, up_to = true, filter = fn }`. No mode decoder change. The build adds a Lua test that pins a divided mode word with a count above one, and fixes the docs that say otherwise.

## Comp rules

- CR 601.2c: a spell or ability announces how many targets it will choose when the number is variable, then chooses them. The same player, object or ability can't be chosen twice for one word (CR 115.3, existing dedup).
- CR 602.2b: 601.2c and 601.2d apply to activated abilities.
- CR 603.3d: a triggered ability's targets are chosen as it goes on the stack, using the same 601.2c rules.
- CR 601.2d: a divided effect's split is announced with the targets. This is already enforced by `check_divided_shape` and `validate_effect_division`.
- CR 601.2c also covers a variable number of targets ("up to N", "N or more", "any number"): the player announces how many, then chooses them. CR 115.6: a word may allow zero targets (`up_to`, or `at_least` with `count = 0`). `TargetRequirement` documents `up_to` and `at_least` as mutually exclusive, but no decoder enforces that today.
- CR 115.4: "another target" (`exclude_source`).
- A `type = "triggering"` word is not a rules shape of its own. The engine fills it with the one spell or ability that caused the trigger (`triggers_pipeline.rs`), so a count on it means nothing.

## Engine changes

1. `script/decode.rs`: split `decode_target_requirement_table` so the count keys decode once, with `count` either required (`s.target_requirement()`, mode tables) or defaulting to 1 (`:SetTarget`). The count keys are read from the table first, and the filter decoder then gets `type`, `count`, `up_to`, `at_least` and `exclude_source` as its allowed extra keys. Today `decode_target_spec_value` passes only `&["type"]`, which is why a count key on `:SetTarget` is an unknown-key error. The error text for a missing `count` on `target_requirement()` stays the same.
2. `decode_target_spec_value` decodes a table, or each table entry of an array, through the defaulting form. A bare function, or a function entry in an array, stays `count: 1` with no flags. `:SetTarget` arrays default each entry's `count` to 1, while a mode `target` array (through `decode_target_spec_table`) still requires `count` per entry. The docs say so.
2a. The shared count-key decode refuses `up_to` and `at_least` together, and `count = 0` without `at_least` (an exact or "up to" zero picks nothing). This also applies to `target_requirement()` and mode tables. No script or test writes either shape today.
3. A `type = "triggering"` word with `count` other than 1, `up_to`, `at_least` or `exclude_source` is a load error, since the engine fills that word with exactly the triggering object.
4. A non-integer or negative `count` is a load error, as it is today for `target_requirement()` (mlua's `u32` conversion).
5. Remove the TODO at `decode_target_spec_value`. Fix the stale doc comments: `decode_target_spec_value` ("A plain filter table or function is one target word (count 1)"), `decode_target_filter_value`, and the "single exact count" and "nothing consumes more" comments on `TriggeredAbilityBuilder`, `ActivatedAbilityBuilder` and the trigger `Register()`.
6. `check_divided_shape` and `check_chain_words` need no change. They already read the word's `count` and `up_to`.

## Docs (Lua API change)

- `docs/lua/stubs/game.lua`: a `SetTargetTable : FilterTable` class with `type?`, `count?` (default 1), `up_to?`, `at_least?`, `exclude_source?` and `filter?`. Both `:SetTarget` params use it.
- `docs/lua/targeting-and-choices.md`: drop "A `:SetTarget` word and a mode's `target` word always take one target" (lines 234-236, split across a line break), and show a `:SetTarget` divided example. Line 275's "Always exactly one target per call" is about `Game.SelectTargets` and stays.
- `docs/lua/activated-abilities.md:80` ("Every target word is exactly one target, with no "up to N"") and `triggered-abilities.md:62-67` ("Every word is an exact-count target ... For "up to N" words, construct a `RegisteredAbility` directly in Rust"): replace with the count keys. Correct the trigger mode note so it says a mode `target` table takes the `target_requirement()` shape, with `count` required.
- `docs/lua/game-constants.md`: the `DividedDamage` row's TODO drops the `:SetTarget`/mode clause.
- `docs/LUA_API.md`: check the `:SetTarget` lines and the mode shape line.
- `docs/implementation/07-targeting.md`: a short slice entry. The "Not changed" and "Open" lines in earlier entries are history and stay.
- `lua_api_doc_sync` keeps passing.

## TODOs

- Remove: `decode.rs` "a :SetTarget or mode target word takes exactly one target".
- Keep: every other divided TODO from 116 (X total, CR 608.2d, Chain and function ops, CR 707.10c, `Game.Ops.CastSpell`, no minimum of one).
- Add: none expected. "One, two or three targets" still has no minimum of one, and that TODO is already recorded.

## Tests

In `crates/engine/tests/divided_effects.rs`, reusing its `load_error`, `state_with_ability` and `resolve` helpers and loading real Lua scripts:

1. A Lua activated ability, `:SetTarget({ type = "object", card_types = { "Creature" }, count = 3, up_to = true })` with `DividedCounters`, activates on two creatures with a split of 1 and 2, and a bad split is refused. CR 601.2d, 602.2b.
2. A Lua triggered ability with `{ type = "any", count = 3, up_to = true }` and `DividedDamage(3)` pauses for targets, refuses a bad split and resolves a good one. CR 603.3d.
3. Decode unit tests: a bare table with no `count` gives count 1. `count`, `up_to`, `at_least` and `exclude_source` land on the requirement. Array entries each carry their own count. `{ count = 2, filter = fn }` keeps the script filter.
4. Load errors, through the existing `load_error` helper: `type = "triggering"` with `count = 2`. A negative `count`. `up_to = true, at_least = true`. `count = 0` with no `at_least`. And `count = 0, at_least = true` loads.
4a. A function-valued mode `target` stays one exact target.
5. A Lua modal activated ability whose mode `target` is `{ type = "object", card_types = { "Creature" }, count = 2 }` with `DividedCounters` takes two targets and a split. This pins that mode words already take a count.
6. A `:SetTarget` word with `count = 4` and `DividedDamage(3)` is a load error (existing check, now reachable from `:SetTarget`).
