---
id: 105
title: How the loader treats design non-goal card types
type: research
mode: AFK
status: open
blocked_by: []
claimed: false
---

## Question

Which planeswalker, battle, saga and face-down cards are in `../stackfish-db/stackfish.db`, and what does `stackfish-carddb` do with each? Multi-face cards already fail with `CardLoadError::MultiFace`, but `types_line.rs` still maps `Planeswalker` and `Battle` to type bits.

## Notes

- Origin: backlog item 12. The backlog asks that the loader skip or reject non-goal cards explicitly.
- Report counts per card type, whether a deck containing one loads, plays and breaks silently, and a recommended rule (reject at load, reject at deck setup, or allow inert). Do not change code.
- Capture the findings in `docs/research/` and link them from the ticket Resolution. Use the `research` skill.
- The database may be absent on some machines. If so, say that and stop.

## Resolution

<Filled in at close.>
