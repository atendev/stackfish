---
id: 109
title: Close the proof game coverage gaps
type: task
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Build the proof cards and assertions the byte-level bot does not exercise yet.

## Notes

- Origin: the Proof game and Zone permissions entries in `99-backlog.md`.
- Gaps to cover:
  - A main proof card with menace, and one with a must-attack rule (`ATTACKS_EACH_COMBAT`).
  - A main proof card with a sacrifice or discard cost. `cost_proof_game.rs` has its own set.
  - An assertion that the bot casts off-turn under Vedalken Orrery at least once in the seed loop.
  - The bot's Phyrexian handling, which assumes 20 starting life. Read the life total from the setup instead.
- Adding a card to a deck moves the seed loop, so expect `BEHAVIOUR_REV` to change. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- Read `docs/implementation/19-proof-game.md`, `33-ordered-choice-proof-cards.md` and `30-payment-candidates.md`.

## Resolution

<Filled in at close.>
