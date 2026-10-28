# Map: Cleanup and Lua API hardening

## Destination

The codebase has no oversized files and no tirades in comments. The backlog, `DESIGN.md` and `docs/lua/` match the code. The test suite builds quickly. The Lua API lets a custom function express anything a `Game.Ops.*` primitive can, and it has been tried against 50 to 100 varied real cards.

## Notes

- This map follows `map.md` (Host protocol, complete) and `map-fixes.md` (leftovers, in progress). It does not repeat map-fixes tickets. Where an item here overlaps one there, the text says so.
- Ticket numbers continue from 038. No ticket files exist yet. Create each file in `tickets/` from `TICKET.template.md` when it is picked up.
- Build tickets are `phase5/<name>` slices that follow the CLAUDE.md slice workflow. That means a requirements report first, approval before every commit, a closing report, and the user merges.
- Written text follows the unslop skill, including commit messages and comments.
- Numbers behind this map, measured on 2026-10-27. All Rust is 129,933 lines, of which 93,675 are code, 25,392 are comments and 10,866 are blank. Tests are 79,566 lines. The engine's non-test source is 40,409 lines and 15,454 of them (38%) are comments. `crates/scenario/src/lib.rs` is 3,222 lines. `crates/engine/tests/` holds 224 files, and `phase5_` starts 229 test file names.
- The comment-only diff check. Strip comments from the source before and after a pass and diff the results. An empty diff proves no code changed. `tools/comment_diff.py` is that script (ticket 039).

## Decisions so far

- 038 Split `crates/scenario/src/lib.rs`. Now 46 lines of docs and re-exports, with the code moved unchanged into ten modules by concern. Public paths are unchanged and only shared helpers became `pub(crate)`. [closed/038-split-scenario-lib.md]
- 039 Comment rules and the comment-only diff check. The rules are in the Comments section of `CLAUDE.md`. `tools/comment_diff.py` strips comments, ignores whitespace, and fails on any code change or lost `TODO`. [closed/039-comment-rules-and-diff-check.md]
- 040 Comment pass on the small crates and scenario. Six crates, six commits on `phase5/comment-pass-small-crates` (not yet merged). Comment lines 1,805 to 740, no code change, `TODO`s kept and three added. The check counts prose mentions of "TODO", and a few rule gaps are listed for 041 to 044. [closed/040-comment-pass-small-crates.md]
- 041 Comment pass on engine types and script. Comment lines 6,960 to 3,554 across 27 files, no code change, engine tests pass. 33 new one-line TODOs came from prose notes. [closed/041-comment-pass-engine-types-script.md]
- 042 Comment pass on engine core. `engine/`, `commands/` and `decision/` are done and merged to `master` (5098bbf). No code change, `comment_diff.py` exits 0, and 2,024 engine tests pass. The CR 111.7 token cessation note was removed because `sba.rs` enforces it. [closed/042-comment-pass-engine-core.md]

- 043 Comment pass on engine top-level modules. `sba.rs`, `replacement.rs`, `layers.rs`, `triggers.rs`, `script_api.rs`, `engine_context.rs` and `color_identity.rs` are done and merged to `master` (5a8ab4a). Comment lines 1,631 to 400, no code change, 2,024 engine tests pass. Six new `TODO`s came from prose notes about real gaps. [closed/043-comment-pass-engine-top-level.md]
- 044 Comment pass on test files. All 237 test files across engine, scenario, carddb, and host done and merged to `master` (8550105). Comment lines 8,230 to 1,727, no code change, all crate tests pass. 49 new `TODO`s captured from test gap notes. [closed/044-comment-pass-test-files.md]
- 045 Measure test build and decide on consolidating test binaries. Clean engine test compile is 16.9s, incremental after engine touch is 7.7s (~1m53s CPU), and 224 test binaries consume 16 GiB in debug deps. Consolidate into a single test runner (tests/main.rs with modules) in 046. [closed/045-measure-test-build.md]

## Not yet specified

- Whether repetitive Rust tests should become TOML scenarios. Decide after 046, when the test files are reorganized.
- Backlog items 8 to 10 (division of damage or counters under CR 601.2d, `Revealed` and `face_down` producers, rules polish). They are tracked in `99-backlog.md` and map-fixes ticket 027. They get tickets after 047 shows what is still true.
- Build tickets for composition operations (`If`, `ForEach`, `Repeat`) and named targets. They are created from the decisions in 057 and 058.

## Tickets

Ticket types and modes follow `README.md`.

### A. Code hygiene

- 038 Split `crates/scenario/src/lib.rs`. Type task, HITL.
  - 3,222 lines in one file. Split by concern (fixture parsing, card templates, builders) with no behavior change. The scenario tests must pass unchanged.
- 039 Comment rules and the comment-only diff check. Type grilling, HITL.
  - Decide the rules for `CLAUDE.md`. Proposed. Comment why, not what. Cite a rule in one line as `// CR 704.5g` and never quote or paraphrase its text. Public items get one or two lines of doc. No ticket numbers or history in comments, because the numbered notes hold that. Keep every `TODO`.
  - Write the script that strips comments and diffs a before and after tree, and decide where it lives.
- 040 Comment pass on the small crates and scenario. Type task, HITL. Blocked by 038 and 039.
  - Covers `carddb`, `cli`, `godot`, `host`, `scenario` and `testutil`. This is the pilot, so it proves the rules and the diff check before the engine.
  - One crate per commit, with permission asked before each.
- 041 Comment pass on engine types and script. Type task, HITL. Blocked by 040.
  - `crates/engine/src/types/` and `crates/engine/src/script/`.
- 042 Comment pass on engine core. Type task, HITL. Blocked by 040.
  - `crates/engine/src/engine/`, `commands/` and `decision/`.
- 043 Comment pass on engine top-level modules. Type task, HITL. Blocked by 040.
  - `sba.rs`, `replacement.rs`, `layers.rs`, `triggers.rs`, `script_api.rs`, `engine_context.rs`, `color_identity.rs` and `lib.rs`.
- 044 Comment pass on test files. Type task, HITL. Blocked by 040.
  - 8,270 comment lines under `tests/` directories. Keep comments that say which rule a test pins.

### B. Tests

- 045 Measure the test build and decide on consolidating test binaries. Type grilling, HITL.
  - Cargo builds each file in `tests/` as its own program, and the engine has 224. Time a clean `cargo test --no-run` and an incremental one after touching one engine file.
  - If the time is tolerable, close this ticket with "leave it" and drop 046's consolidation half.
- 046 Consolidate engine test binaries and rename `phase5_*` files by feature. Type task, HITL. Blocked by 044 and 045.
  - One `tests/main.rs` with modules, names by feature (`fight_bite`, `hexproof_shroud`). Mechanical, with the same tests passing before and after. If 045 says leave it, only the renames remain.

### C. Docs

- 047 Backlog audit. Type task, HITL. Blocked by 040, 041, 042 and 043.
  - Check every item in `99-backlog.md` against the code. Delete what is done, move history into the numbered notes, and reconcile the priority list with `map-fixes.md`. Run after the comment passes, because the `TODO`s in code are one input.
  - Overlaps map-fixes 017 (stale notes). Close the stale-note part of 017 here.
- 048 `DESIGN.md` audit and implementation notes review. Type task, HITL. Blocked by 047.
  - Compare each section of `DESIGN.md` with the code. Decide whether the 26 numbered notes stay as they are or merge by subsystem.
- 049 `docs/lua/` and game stub audit. Type task, HITL. Blocked by 047.
  - Stale prose, for example "Protection from an attached Aura/Equipment" in `not-available-yet.md`. Check `stubs/game.lua` against every `Game.*` entry in the code. Run again after 052.

### D. Lua API

- 050 How custom function returns differ from primitives. Type research, AFK.
  - Read `script/decode.rs`, `script/builders.rs` and `script/globals.rs`. List which effect shapes a `Game.Ops.*` tag can produce that a custom function's return table cannot, and where the two decode paths diverge. The result feeds 051.
- 051 One effect vocabulary. Type grilling, HITL. Blocked by 050.
  - Proposal. Every `Game.Ops.*` call returns a plain table, and a custom function may return any of those tables or a list of them, through the same Rust decoder.
- 052 Build the unified effect vocabulary. Type task, HITL. Blocked by 051.
  - Phase5 slice. Update the game stub in `docs/lua/stubs/game.lua`.
- 053 Lua event visibility. Type task, HITL.
  - Let conditions see damage, life, tap, untap and mill events. Make `GainLife` and combat damage emit `LifeChanged`. Backlog item 6, and it unblocks the most card scripts. Update the game stub.
- 054 Modal builders in Lua. Type task, HITL.
  - Modal spells and triggered abilities from a card script, then modal activated abilities in `commands/activate.rs` (its CR 700.2b `TODO`). Backlog item 7. Update the game stub.
- 055 Choose the stress-test cards. Type grilling, HITL.
  - 50 to 100 cards from `../stackfish-db` that cover varied shapes. Equipment, auras, ETB and dies triggers, modal spells, X spells, damage and life triggers, copy and layer effects. Add a note for each card saying which shape it tests.
- 056 Write the stress-test scripts and record friction. Type task, HITL. Blocked by 052, 053, 054 and 055.
  - Write each script in `../stackfish-db/scripts`, run it, and log what was awkward or impossible. The log is the evidence for 057 and 058.
- 057 Composition operations. Type grilling, HITL. Blocked by 056.
  - Decide whether `If`, `ForEach` and `Repeat` belong next to `Chain`, using the friction log.
- 058 Named targets. Type grilling, HITL. Blocked by 056.
  - Decide whether targets get names that operations refer to, replacing `ChainEach`'s counted target words and Fight's special array shapes.

## Out of scope

- New card mechanics outside the Lua work above. Those follow their own `phase5/<feature-name>` slices.
- Everything left in `map-fixes.md`. That map keeps its own frontier.
- The Godot UI.

## Frontier

Open tickets, unblocked and unclaimed.

- 046, 047, 050, 053, 054, 055

Suggested order. 046 unblocks immediately after 045. Run 047 to 049 after the comment passes (047 is next). Run the Lua work (050 to 058) in the order 050, 051, 052, with 053 and 054 anywhere before 056. Doing hygiene before Lua keeps comment-pass diffs from overlapping Lua changes.

Blocked:

- 048 and 049 wait on 047.
- 051 waits on 050. 052 waits on 051.
- 056 waits on 052 to 055. 057 and 058 wait on 056.
