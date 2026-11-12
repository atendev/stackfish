---
id: 110
title: Sweep stale notes and plan prose
type: task
mode: AFK
status: closed
blocked_by: []
claimed: false
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

Every claim was checked against master before editing. The five known points were stale, plus four found along the way.

- `docs/lua/not-available-yet.md`: removed the attached Aura/Equipment protection bullet and the enchanting-a-player bullet, and the sentence saying `GainLife`, damage to a player and lifelink emit no `LifeChanged`. An Aura grants protection through `Game.Affected.Enchanted` (a throwaway test passed and was reverted). Protection removes attachments through `attachment_target_is_legal`, and `attached_to_player` exists.
- `docs/lua/static-abilities.md`, `docs/lua/game-constants.md`, `14-protection-attach.md`: protection against attachments cites CR 702.16c (an Aura goes to the graveyard) and 702.16d (Equipment unattaches), not 702.16b, and nothing is listed as open.
- `99-backlog.md`: the priority list marks merged branches as merged, item 5 and loop detection as done, and the line 860 citation as CR 702.16c, 702.16d.
- `map-fixes.md`, `map-cleanup.md`, `README.md`: neither older map has an open ticket, and the README lists all three maps.
- The Rust comments cite CR 702.16c for targeting and 702.16d for blocking. Ticket 171 fixes them, since this ticket changes prose only.
