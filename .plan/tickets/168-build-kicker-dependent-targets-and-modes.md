---
id: 168
title: Build kicker-dependent targets and modes
type: task
mode: AFK
status: open
blocked_by: [166, 114]
claimed: false
---

## Question

Let a target slot or a mode range depend on an optional cost being paid, so a spell chooses a kicker-only target only if it was kicked (CR 702.33g, 601.2c).

## Notes

- Origin 159. The full decision is in `closed/159-optional-additional-costs-and-kicker.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- A target slot gets `requires_paid: Option<usize>`, and a modal card gets `paid_mode_range: Option<(usize, ModeRange)>` (Inscription of Abundance, Pyrrhic Strike's blight).
- One step: the Entry lists every slot with its tag, and the priority Answer carries the counts and the targets together (decision 003). The cast rejects a target for an unpaid slot and a missing target for a paid one, and changes nothing (decision 001).
- TODO: "another target for each time it was kicked" (Comet Storm).
- About 64 cards. Recount the shape before building. Lua API change: update the docs and `docs/lua/stubs/game.lua`.

## Resolution

<Filled in at close.>
