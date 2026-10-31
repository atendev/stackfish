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
Every ticket runs this loop without waiting on me, except for the two button prompts. Subagents are `haiku` or `sonnet`, your pick per job. A fork or subagent cannot spawn agents, so it runs the audit inline.

### Implementation tickets
1. Write a requirements report:
   - What does a successful implementation look like?
   - What requirements from the comp rules are there?
   - What TODO's are we going to need to mark since they aren't implemented yet?
   - What tests will we want to add so that we can make sure this section works properly?
2. Spawn a subagent (default `haiku`) to audit the report. It is read-only: it may not edit files. It returns `APPROVE` or a list of required changes. Apply the changes and re-audit until it approves.
3. Create a local branch `phase5/<feature-name>`. Do not push it to origin.
4. Implement the feature. Mark every stubbed part with a TODO (eg: PlayLand has no "play additional lands" effects yet, so the PlayLand action gets a TODO). Commit on the branch at sensible points without asking.
5. Spawn a subagent (default `sonnet`) to audit the work. It is read-only. It runs the tests, checks the code against the approved report and the comp rules, checks the comment rules and, for Lua API changes, the game stub in the docs. It returns `APPROVE` or a list of problems. Fix them and re-audit until it approves.
6. Report in the chat as the workflow document describes, then ask with `AskUserQuestion`: options `Merge` and `Don't merge`.
7. On `Merge`: merge into master, push master, then delete the local branch. On `Don't merge`: keep the branch and wait for my instructions.

### Grilling tickets
1. Write a report listing the ticket's questions and what is needed to answer them.
2. Spawn a read-only subagent (default `haiku`) to audit the report. Same `APPROVE` or changes loop.
3. Spawn a subagent (default `sonnet`) to answer the questions from the docs, comp rules and code. It does not ask me anything.
4. Give me the answers in the chat, then ask with `AskUserQuestion`: options `Approve` and `Deny`.
5. On `Approve`: fill in the ticket's Resolution and close it per `.plan/README.md`. On `Deny`: ask what to change and redo step 3.

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
