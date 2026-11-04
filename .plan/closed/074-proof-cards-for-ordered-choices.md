---
id: 074
title: Proof cards for ordered choices
type: task
mode: HITL
status: closed
blocked_by: [073]
claimed: false
---

## Question

Add proof or fixture cards that reach Devour, Amplify, Clone and Equipment attach from a search or token, and add them to the seed loop in `the_same_seeds_give_a_byte_identical_game`.

## Notes

- Origin 028. The proof set has no card that reaches the 4 formerly unsorted walks, so a regression there is invisible to the 500-seed run.
- Moves the proof game: re-pin the hash at `crates/host/tests/replay.rs:371` and bump `BEHAVIOUR_REV` by hand, in its own commit.
- Blocked by 073 so the new cards prove the fixed order, not the old one.

## Resolution

Four proof cards reach the four walks: Ravenous Ooze (Devour 1), Rat Herald (Amplify 1), Mimic Pool (enters as a copy) and Armory Call (search for an Equipment, enters attached). They are in both 40-card decks, so the 30-game, three-player and 500-seed runs reach them too. `Bot::mechanics_seen` counts answered Decisions by mechanic, and the seed loop plays seeds 0 to 10 and asserts all four Decisions open. Armory Call is two copies, because one reached Attach on entry in 1 seed of 40.

The cards found an engine bug: a Devour creature was its own sacrifice candidate, which wrote its sacrifice before its entry and broke the snapshot fold. It is still entering and not on the battlefield (CR 614.12), so it is no longer a candidate. The user chose to fix it in this slice. `BEHAVIOUR_REV` is 4 and the replay pin moved, in commits of their own. The token route into entry attach has no proof card, since `Game.Tokens` has no Equipment shape (TODO). The same self-candidate question for Clone is 077. Notes in `docs/implementation/33-ordered-choice-proof-cards.md`.
