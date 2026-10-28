# Stackfish
This is an mtg-engine based on the ygo-pro engine.

## Commits
Commits should be straight and to the point, but in readable and understandable language that the user can understand.

### Commit Structure
1. A single line starting with the action and a sentence explaining what was changed.
  - eg: 'add narrow script bridge with one real lua triggered ability implementation'
  - eg: 'fix card ids not working properly with new_game init'
  - eg: 'update scenario with new_game setup method to properly walk through pre-game'
2. A more detailed explanation of the changes made.
> [IMPORTANT] Do not add anything else. No attribution line, nothing.

## Vertical Slice Instructions
- all slices start with a detailed requirements report of the following information:
  - What does a successful implementation look like?
  - What requirements from the comp rulse are there?
  - What TODO's are we going to need to mark since they aren't implemented yet?
  - What tests will we want to add so that we can make sure this section works properly?
- create a branch `phase5/<feature-name>` and push to origin.
- implement the given instructions for the feature.
- As you implement, make sure to mark any stubbed parts that aren't implemented with TODO marks for later.
  - Eg: Implementing PlayLand, we don't have continuous effects for 'play additional lands' effects, so we mark the PlayLand action with a TODO so we know to implement that later.
- make sure to stop and commit at appropritate times, but always check for permission before committing.
- When done with implementation, give a detailed report in the chat as indicated in the workflow document.
- Don't delete the branch, as I will manually inspect the code after for changes.
- If all is good, I will instruct to commit, merge, and push.

## IMPORTANT
- Always use the unslop skill
- If we are making changes to the Lua API, you need to check the game stub in the docs to make sure things are updated properly

## Comments
These rules cover Rust source and Rust tests.
- Say why, not what. Skip a comment the code already shows.
- Cite a rule as `// CR 704.5g`: the number only. Never quote or paraphrase rules text, and add no label after the number. `docs/comp_rules.md` has the text.
- No history. Do not mention tickets, backlog items, phases, slices or branch names. Do not write "used to", "previously", "now", "new", "before X existed" or "see note N". If the reason still matters, state the reason.
- Public items get a doc comment of two lines. Up to about five lines is fine for an invariant, a rule interaction or a trap. Private items get a comment only for a why the code cannot show.
- A `//!` module doc is three lines or fewer. It says what the module owns and points to `DESIGN.md` for the rest.
- Keep every `TODO`. The format is `// TODO: <what is missing>` on one line, with an optional CR citation at the end. A pass may trim the wording but never deletes a `TODO` or the rule it names.
- A test comment is a single `// CR 704.5g` line naming the rule the test pins, plus a why line only when the setup is not obvious.
- After a comment-only pass, run `python3 tools/comment_diff.py [paths]` and get exit code 0 before asking to commit. It compares the working tree with `HEAD`, ignores comments and whitespace, and fails on any code change or a lost `TODO`. Its tests run with `python3 -m unittest discover -s tools`.

## Notes
Look in the docs folder for the following information:
- [DESIGN.md](/docs/DESIGN.md) | Bigger design document that should be updated as new things are created.
- [index.md](/docs/implementation/index.md) | Notes from your previous sessions about where we are and things yet to be done, mistakes, changes, etc. This is something you should always be checking.
- [backlog.md](/docs/implementation/99-backlog.md) | The backlog of what items we are currently working on or are listed in TODOs.
- [workflow.md](/docs/workflow.md) | The main workflow of how we are going to be talking back and forth. Current phase is listed in implementation.
- [ygo_pro_reference.md](/docs/ygo_pro_reference.md) | A reference guide for how the ygo-pro engine works.
- [mtg_adaptation_reference.md](/docs/mtg_adaptation_reference.md) | Reference guide for how some of the ygo-pro engine mechanics can translate over to mtg.
- [comp_rules.md](/docs/comp_rules.md) | The entire comprehensive magic the gathering ruleset. Check this when wondering exactly how any rules interaction is supposed to work.
- [scenario_format.md](/docs/scenario_format.md) | The TOML scenario/fixture schema used by tests and the CLI.
- [lua/index.md](/docs/lua/index.md) | Full Lua scripting API reference (card script structure, every `Game.*` table, one page per ability/effect kind) - start here for anything about writing card scripts.
