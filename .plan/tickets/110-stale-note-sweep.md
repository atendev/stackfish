---
id: 110
title: Sweep stale notes and plan prose
type: task
mode: AFK
status: open
blocked_by: []
claimed: true
---

## Question

Which statements in the notes and plans no longer match the code, and what do they say now? Fix each one.

## Notes

- Origin: backlog item 11, which says "this file has gone stale before". Known stale points found while charting these tickets:
  - `docs/lua/not-available-yet.md` says `GainLife`, damage to a player and lifelink emit no `LifeChanged`. They do (`31-snapshot-fold-events.md`).
  - The same file says protection from an attached Aura or Equipment is missing. `14-protection-attach.md` says only CR 702.16b remains.
  - `99-backlog.md` priority items 1 to 4 and 9 still read "awaiting review" for work that is merged, and item 5 lists tickets 020 to 028 that are closed.
  - `map-fixes.md` Frontier lists 031 to 034 and 072 to 075 as open. All are closed.
  - `.plan/README.md` describes one `map.md`, but the folder also holds `map-fixes.md` and `map-cleanup.md`.
- Check each claim against the code before editing. Run `python3 tools/comment_diff.py` only if a Rust file changes, which it should not.
- Prose only. Run `unslop` on every edit. Commit on a `phase5/` branch.

## Resolution

<Filled in at close.>
