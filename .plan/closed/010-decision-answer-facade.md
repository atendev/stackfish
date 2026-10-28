---
id: 010
title: Build B1 Decision/Answer facade in the engine
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Build the Decision/Answer facade in the engine. Answers are a tagged enum that the Session routes through a facade over the existing `submit_*` methods, and rejections carry a code and a message.

## Notes

- Branch: `phase5/decision-answer-facade`. Follow the CLAUDE.md slice workflow: requirements report first, approval before every commit, closing report, user merges.
- Decisions: 002 (Decision shape, Answer enum, rejection codes, Session counter id) and 005 (candidates and Pick/Target Answers use `Object(ObjectId) | Player(PlayerId)`, Order Answers are candidate indices).
- Terms from `CONTEXT.md` (Session, Seat, Decision, Answer).
- Mark any stubbed parts with TODO.

## Resolution

Built on `phase5/decision-answer-facade`, merged to master as 5dcfbb6 (commit e79c4d6). Not pushed yet.

- `crates/engine/src/decision/`: `Engine::open_decision()` builds a Decision from `ProcessState`, and `Engine::answer(player, Answer)` checks and routes it to the existing `submit_*` and `decline_*` methods. See `docs/implementation/16-decision-facade.md`.
- Rejections run in order: `not_open`, `stale_decision`, `wrong_seat`, `wrong_answer_shape`, `invalid_choice`. A rejected Answer changes nothing.
- Deviations from 002 and 005: `Candidate` is `Target | Replacement` (reuses `Target`, which has `Ability`). Madness and Cascade use a `MayCast` shape. Unless-paid shares the `Payment` shape. `Answer` is a struct of Decision id plus a `Reply` enum.
- The Decision id counter lives on `Engine` and moves into the Session in B3.
- Left for later: priority entry lists (B2), illegal priority actions are accepted by `submit_action` and fail during `step()` (B2 or the atomicity item), nested mana abilities during a Payment Decision, `Target::Ability` candidates for script picks, friendlier rejection text (B3).
- Tests: 24 unit tests in `engine/decision_tests.rs`, 3 integration tests in `tests/phase5_decision_answer_facade.rs`. Workspace: 1894 passed, 0 failed.
