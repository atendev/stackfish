# Objects in id order

`GameState::objects` is a `BTreeMap<ObjectId, GameObject>`. Every `values()`, `keys()` and `iter()` over it runs in ascending `ObjectId` order, so a candidate list or ledger built from a walk is the same on every run.

## What it fixed

- Four walks built Decision candidates in hash order: `replacement::pending_devour_choice`, `pending_amplify_choice`, `pending_copy_choice` and `commands::attach::legal_entry_attach_candidates`. They return ascending ids with no code change of their own.
- The proof game reaches none of the four, so the pinned replay hash in `crates/host/tests/replay.rs` did not move and `BEHAVIOUR_REV` stays 3.

## What stayed

- The explicit sorts in `sba.rs`, `commands/attach.rs`, `commands/legality.rs`, `engine/pregame.rs`, `engine/turn.rs` and `engine/mod.rs` stay. Their comments say why the order matters. A host or the ledger sees it.
- The other `HashMap` fields on `GameState` (`lands_played_this_turn`, `last_active_turn_number`, `linked_ability_generation`, `ability_function_zones`) and `PlayerState::commander_casts` are keyed lookups. A lint against hash-ordered walks is ticket 075.

## Tests

- `crates/engine/tests/candidate_order.rs`: 24 candidates at scattered ids for Devour, Amplify and copy, and an `open_decision` check for a Devour pause and an entry-attach pause.
- `commands::legality::tests::entry_attach_candidates_come_in_ascending_id_order` covers `legal_entry_attach_candidates`, which is `pub(crate)`. It sits in `legality.rs` to reuse that module's `object` and `state_with` fixtures.
- All six failed on `HashMap` before the change.
