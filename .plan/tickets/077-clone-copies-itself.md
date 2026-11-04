---
id: 077
title: A Clone offered as a copy of itself
type: task
mode: AFK
status: open
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

<Filled in at close: the decision and the reason for it.>
