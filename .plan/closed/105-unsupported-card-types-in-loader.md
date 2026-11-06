---
id: 105
title: How the loader treats design non-goal card types
type: research
mode: AFK
status: closed
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

Findings are in `docs/research/non-goal-card-types.md`. The database is present.

- 518 planeswalkers, battles and sagas (323, 2 and 193) and 38 Class cards load today. `MultiFace` already stops 102 more. They cast, enter the battlefield and do nothing: no loyalty, defense or lore counters, no chapter triggers, no state-based action. They break silently. None has a script or a `card_tests` row.
- Face-down cards have no database marker. 231 single-face cards can be cast face down (Morph, Megamorph, Disguise) and play correctly face up. 73 make face-down permanents (Manifest, Manifest dread, Cloak), and that effect does nothing. 2 cards are in both groups.
- Recommendation: reject at load with a new `CardLoadError` in `build_definition`, beside `MultiFace`, for planeswalkers, battles, sagas, Class cards and the 73 producers. Let the other 229 load. The host already turns a load error into a setup refusal naming the card, and the proof-game fixtures never use `CardDb`.
- This departs from backlog item 12, which names all face-down permanents. The choice, plus keyword-less face-down cards, face-down exile, melds and other unmodeled layouts, goes to ticket 112.
