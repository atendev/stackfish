---
id: 073
title: Make GameState objects a BTreeMap
type: task
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Change `GameState::objects` from `HashMap<ObjectId, GameObject>` to `BTreeMap<ObjectId, GameObject>` so every walk is ascending `ObjectId`, and pin the candidate order of the 4 unsorted walks.

## Notes

- Origin 028. Slice `phase5/objects-btreemap`, CLAUDE.md implementation loop.
- Edit sites: the declaration at `crates/engine/src/types/game.rs:65` plus its import; `HashMap::new()` literals at `game.rs:276`, `types/object.rs:213`, `types/ability.rs:2078`, `testutil/src/state.rs:27,85`, `commands/tests.rs:49`, `engine/tests.rs:41`; type annotations at `engine/pregame.rs:35` and `scenario/src/build.rs:530`.
- Keep the existing sorts. Reword the comments that say `objects` is a `HashMap`: `sba.rs`, `commands/attach.rs`, `commands/legality.rs`, `engine/pregame.rs`, `engine/mod.rs`, `engine/turn.rs`, `types/ids.rs:7`, `docs/DESIGN.md:213-214`, and the "8 more walks" line in `docs/implementation/99-backlog.md`.
- Tests: for `pending_devour_choice`, `pending_amplify_choice`, `pending_copy_choice` and `legal_entry_attach_candidates`, build 24 candidates at scattered ids and assert ascending order. Add an `open_decision` order check for Devour and EntryAttach. Precedent: `crates/engine/tests/sba_event_order.rs`.
- Acceptance: `tools/check.sh --full` green and the pinned pair at `crates/host/tests/replay.rs:371` unchanged. If it moves, bump `BEHAVIOUR_REV` by hand.
- Mark any stub with a TODO. Lua API is untouched.

## Resolution

<Filled in at close: the decision and the reason for it.>
