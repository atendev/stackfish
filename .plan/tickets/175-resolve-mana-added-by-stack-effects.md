---
id: 175
title: Resolve mana added by spells and stack abilities
type: task
mode: AFK
status: open
blocked_by: []
claimed: false
---

## Question

An `AddMana` effect on a spell, a triggered ability that is not a mana ability, or a mode resolves today with no mana added. Make a resolving stack object add its mana to the controller's pool (CR 106.3, 605.5b).

## Notes

- Found by 174, which confirmed the bug with a throwaway test. See `closed/174-modal-mana-abilities.md` and `.plan/assets/174-modal-mana-abilities-report.md`.
- Start with a failing test: a Dark Ritual style spell resolves and the pool holds the mana. Cover a non-mana triggered ability and a spell mode as well.
- Follow the CLAUDE.md implementation loop on a `phase5/` branch. Check the replay pin and bump `BEHAVIOUR_REV` if it moves.

## Resolution

<Filled in at close.>
