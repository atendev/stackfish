---
id: 165
title: Build unearth
type: task
mode: AFK
status: open
blocked_by: [130, 160, 164]
claimed: false
---

## Question

Build the unearth keyword (CR 702.84a) from the delayed end-step trigger, the battlefield leave rider and sorcery-speed activation.

## Notes

- Origin 132. See `closed/132-delayed-triggered-abilities.md` and `closed/099-riders-beyond-leaving-the-stack.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- Unearth has five parts: an activated ability that works from the graveyard, "activate only as a sorcery" (160), the return with a haste grant, the exile at the next end step (164), and the "if it would leave the battlefield, exile it instead" rider (130).
- About 69 printed cards. Decide in the requirements report whether unearth is a carddb keyword that expands to these parts or a Lua helper.

## Resolution

<Filled in at close.>
