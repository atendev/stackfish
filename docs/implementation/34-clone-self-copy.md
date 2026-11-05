# Clone is not offered as a copy of itself

`replacement::pending_copy_choice` leaves the entering object out of its own candidates, as `pending_devour_choice` does.

## Why

- `continue_permanent_entry` moves the resolving spell into the battlefield zone before it checks for the copy pause, so a zone check alone listed the entrant.
- The choice is made before the permanent enters (CR 614.12a), and the copy's controller makes it (CR 707.6). The entrant's entry is not complete, so it is not "a creature on the battlefield" to copy. The filter excludes it by id, not by zone.
- A chained round (CR 707.9e) calls the same function while the entry is still in progress, so it excludes the entrant too.
- A Clone with no other creature on the battlefield gets no Decision and enters as itself. A 0/0 Clone then dies to state-based actions.

## Tests

- `clone_enters_as_copy.rs`: the candidates are every other creature on the battlefield, a lone Clone enters with no pause, and answering with the entrant's id is refused with `InvalidCandidate` and changes nothing.
- `chained_copy_effects.rs`: `a_chained_round_does_not_offer_the_entrant` checks the first and the chained round.
- `candidate_order.rs` counts one fewer copy candidate.

## Proof game

- Mimic Pool is in the blue-black deck, so the bot's copy choices change. Event hashes moved for 29 of seeds 0 to 39. Seed 9, the one `replay.rs` pins, never opens a copy choice and kept its hash.
- `BEHAVIOUR_REV` is 5, so a replay recorded on build 4 is refused. The pinned hash is unchanged.
