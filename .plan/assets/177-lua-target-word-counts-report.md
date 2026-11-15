# 177 Let Lua target words take a count: decision report

Branch `phase5/lua-target-word-counts`. The requirements are in `177-lua-target-word-counts-requirements.md`. The requirements auditor (haiku) approved in round 2. The implementation auditor (sonnet) approved in round 1 and confirmed the comment-only nit fixes. `tools/check.sh --full` passed.

## Gates

The repo's CLAUDE.md slice loop sets the gates. The loop ran with a requirements report, a haiku audit, a `phase5/` branch, the build, `check.sh --full` and a sonnet audit. The master thread's instructions replaced the last two steps. Commits went in without asking, and the branch was pushed with no merge question. The master merges.

## Decisions

### 1. Which words needed a change

- Question: the ticket says both `:SetTarget` words and mode `target` words always take one target. Do both need work?
- Options: change both decoders, or only the one that is fixed at one.
- Choice: only `:SetTarget`. A mode `target` table already goes through `decode_target_spec_table`, the `s.target_requirement()` decoder, which requires `count` and reads `up_to`, `at_least`, `exclude_source` and `filter`. Only a function-valued mode word is fixed at one target. A test now pins a two-target divided mode word.
- Made by: the implementer, from reading `decode.rs`. The requirements auditor accepted it.

### 2. Shape of the count on `:SetTarget`

- Question: how does a script ask for a count on `:SetTarget`?
- Options: (a) the same flat keys as `s.target_requirement()`, with `count` optional and defaulting to 1; (b) the same keys with `count` required; (c) a separate argument such as `:SetTarget(filter, count)`.
- Choice: (a). Every existing script still loads, a script author learns one vocabulary, and the array form works with no extra rules. A bare function stays one exact target. With a predicate, a script writes `{ count = 3, filter = fn }`.
- Made by: the implementer. Both auditors accepted it.

### 3. Mode arrays keep a required `count`

- Question: should a mode `target` table default `count` to 1 as well?
- Options: default it, or keep it required.
- Choice: keep it required. Changing the spell decoder is outside the ticket. The docs now say a mode `target` takes the `s.target_requirement()` shape with `count` required, and that `:SetTarget` defaults it.
- Made by: the implementer, after the requirements auditor asked for the difference to be stated.

### 4. New load errors in the shared decoder

- Question: what should happen with `up_to` and `at_least` together, and with `count = 0` without `at_least`?
- Options: leave them unchecked, as before, or refuse them at load.
- Choice: refuse both, for `:SetTarget`, `s.target_requirement()` and mode tables. `TargetRequirement` documents the two flags as mutually exclusive. An exact or "up to" zero picks nothing. `count = 0, at_least = true` ("any number of targets", CR 601.2c, 115.6) still loads. No script or test wrote either refused shape.
- Made by: the implementer, prompted by the requirements auditor asking where the rule was enforced and how `count = 0` behaves.

### 5. A count on a `"triggering"` word

- Question: what does `{ type = "triggering", count = 2 }` mean?
- Options: ignore the keys, or refuse them.
- Choice: refuse them at load. The engine fills that word with exactly the spell or ability that caused the trigger (`triggers_pipeline.rs`), so a count has no meaning.
- Made by: the implementer.

### 6. CR citations

- Question: which rules back the count keys?
- Choice: CR 601.2c for a variable number of targets, CR 115.6 only for allowing zero targets, CR 115.4 for `exclude_source`, and CR 601.2d, 602.2b and 603.3d for division on abilities. A first draft cited CR 702.21a for the triggering word and CR 115.6 for "N or more". The requirements auditor flagged both, and the report dropped them.
- Made by: the requirements auditor flagged the errors and the implementer fixed them.

### 7. Docs

- Question: which docs change for this Lua API change?
- Choice: the stub gains a `SetTargetTable` class, and both `:SetTarget` params use it. `targeting-and-choices.md` drops "a `:SetTarget` word and a mode's `target` word always take one target", explains the count keys and adds an ability divided example. `activated-abilities.md`, `triggered-abilities.md`, `LUA_API.md` and the `DividedDamage` row in `game-constants.md` are updated, and `07-targeting.md` gets a slice entry. Line 275 of `targeting-and-choices.md` is about `Game.SelectTargets` and stays. The requirements auditor first said the quoted sentence did not exist. It does exist, split across a line break at lines 234-236, and the auditor accepted the correction.
- Made by: the implementer. The implementation auditor found no stale claims left.

### 8. Comment nits

- Question: the implementation auditor raised two optional nits, a short line in a rewrapped doc comment and a descriptive doc comment on a private test helper.
- Choice: fix both, comment-only. `tools/comment_diff.py` exits 0 and the TODO count is unchanged.
- Made by: the implementer.

## TODOs

- Removed: `decode.rs`, "a :SetTarget or mode target word takes exactly one target".
- Kept: the other open divided TODOs from 116 (an X total, CR 608.2d, Chain and function ops, CR 707.10c, `Game.Ops.CastSpell`, and no minimum of one for "one, two or three targets").
- Added: none.

## Tests

In `crates/engine/tests/divided_effects.rs`, all loading real Lua:

- A Lua activated ability splits +1/+1 counters over a `count = 3, up_to = true` word and refuses a bad split.
- A Lua enters trigger splits damage over an `any` word and refuses a bad split.
- A Lua modal activated ability's mode word with `count = 2` takes two targets and a split.
- `:SetTarget` defaults `count` to 1, reads each key, keeps a scripted `filter`, keeps a bare function at one target and gives each array entry its own count.
- A function-valued mode word stays one exact target.
- Load errors: a count on `"triggering"`, a negative count, `up_to` with `at_least`, `count = 0` without `at_least`, and `count = 4` over a divided total of 3.

## Surfaced

None. The open limits are already TODOs from 116.
