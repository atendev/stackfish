---
id: 077
title: A Clone offered as a copy of itself
type: task
mode: AFK
status: closed
blocked_by: []
claimed: false
---

## Question

Should `replacement::pending_copy_choice` leave the entering object out of its own candidates, as `pending_devour_choice` does?

## Notes

- Origin 074. A Devour creature was offered as its own sacrifice, and 074 removed it from its own candidates because it is still entering and not on the battlefield (CR 614.12). `pending_copy_choice` still lists the entrant ("`object` included" in its doc), and `docs/implementation/09-layers.md` says self-copy is not special-cased.
- CR 707.6 and CR 614.12a: a permanent that enters as a copy chooses as it would enter. Check whether the entrant can be a legal choice.
- Mimic Pool is in the proof decks, so a fix moves the proof game: re-pin the hash in `crates/host/tests/replay.rs` and bump `BEHAVIOUR_REV` in its own commit.
- CLAUDE.md implementation loop on a `phase5/<name>` slice.

## Resolution

Yes. `pending_copy_choice` skips the entering object by id, in the first round and in a chained CR 707.9e round. The engine moves the resolving spell into the battlefield zone before the copy pause, so a zone check alone listed it, but the choice is made before the permanent enters (CR 614.12a). A Clone with no other creature enters as itself with no Decision, and answering with the entrant is refused with `InvalidCandidate`. Event hashes moved for 29 of seeds 0 to 39. The pinned seed 9 did not, so `BEHAVIOUR_REV` is 5 and the pinned hash is unchanged. Notes in `docs/implementation/34-clone-self-copy.md`.
