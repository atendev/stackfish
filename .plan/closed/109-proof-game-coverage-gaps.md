---
id: 109
title: Close the proof game coverage gaps
type: task
mode: HITL
status: closed
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

Built on `phase5/proof-game-coverage-gaps`.

- Night Stalker (menace) and Raging Boar (attacks each combat) join the main decks. The block Decision now lists `min_blockers` (`{ attacker, count }`), so the bot drops a block that leaves a menace attacker short, and `illegal.rs` sends a one-blocker menace declaration that only the engine's whole-declaration check refuses.
- Bone Altar (`{1}`, sacrifice a creature: draw a card) gives the main decks a sacrifice cost. `Bot::costs_paid()` counts accepted payments by kind.
- The 30-seed proof loop asserts that both creatures attack, that a sacrifice is paid, and that a non-instant is cast off its owner's turn while that owner controls Vedalken Orrery.
- Before its first `advance` the bot polls seat 0, then reads every player's life from seat 0's snapshot. `Bot::with_starting_life` is gone.
- The 500-seed loop found an engine panic: a creature sacrificed while attacking stayed in `CombatState`, and `band_entries` asked for its characteristics. `band_entries` now skips departed attackers. The general CR 506.4 rule (leaving the battlefield removes a creature from combat) is a TODO and a backlog line.
- `BEHAVIOUR_REV` is 9.
