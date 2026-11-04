# Proof cards for ordered choices

Four proof cards make bot games open the four Decisions whose candidates come from a walk over every object (`replacement::pending_devour_choice`, `pending_amplify_choice`, `pending_copy_choice` and `commands::attach::legal_entry_attach_candidates`). A walk that loses its `ObjectId` order now shows up as a transcript mismatch in the proof game.

## The cards

| Card | Deck | Mechanic | Route |
|---|---|---|---|
| Ravenous Ooze, {1}{G} 1/1 Ooze | red-green, 2 | Devour 1 | cast, `permanent_entry.rs` |
| Rat Herald, {2}{B} 1/1 Rat | blue-black, 2 | Amplify 1 | cast, `permanent_entry.rs` |
| Mimic Pool, {3}{U} 0/0 Shapeshifter | blue-black, 2 | enters as a copy | cast, `permanent_entry.rs` |
| Armory Call, {1}{R} sorcery | red-green, 2 | search for an Equipment, enters attached | `library_choices.rs` |

- The three replacement cards use the `WouldMoveZone(nil, "Battlefield")` block from `docs/lua/replacement-effects.md`.
- Grave Rat is a Rat, so Rat Herald has cards in hand to reveal. Iron Blade went from 1 to 2 in the red-green deck so Armory Call has more to find.
- Cuts to keep 40 cards: red-green Grizzly Bear 4 to 2, Stone Giant 2 to 1, Cinder Duelist 4 to 3, Shock 3 to 2. Blue-black Wind Drake 4 to 3, Counterspell, Bounce and Divination 2 to 1.
- The search opens Attach on entry only with two or more creatures on the battlefield. One candidate attaches with no pause, so this Decision is the rarest. With one Armory Call, 1 seed in 40 reached it; with two, 3 in 30 do.

## Tests

- `the_same_seeds_give_a_byte_identical_game` plays seeds 0 to 10 twice each and asserts `Bot::mechanics_seen` holds all four labels across them. Seeds 8 and 9 open Amplify, and seed 10 opens Attach on entry.
- `the_games_cast_every_card_of_the_set_and_play_every_land` lists the four new spells.
- The proof game moved, so `BEHAVIOUR_REV` is 4 and the pin in `crates/host/tests/replay.rs` changed in a separate commit.
- The ignored 500-seed run finishes with no rejected Answer.

## Not covered

- No proof card creates an Equipment token, since `Game.Tokens` has no Equipment shape, so the token route into `legal_entry_attach_candidates` (`resolve/tokens.rs`) is covered only by engine tests. TODO in `proof_cards.rs`.
- Devour and Amplify are not offered on token creation or a land play (existing TODOs in `resolve/tokens.rs` and `resolve/mod.rs`).
- Amplify matches any shared subtype, not only creature types. TODO on `pending_amplify_choice`.
