---
id: 101
title: Ward with non-mana costs, player hexproof and hexproof from
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

How do targeting protections grow past permanents with plain keywords? Ward takes only a mana cost, hexproof and shroud work only on permanents, and "hexproof from [quality]" does not exist.

## Notes

- Origin: backlog item 10 and `docs/lua/not-available-yet.md` (Ward, Hexproof and Shroud).
- Gaps to rule on:
  - Ward with life, discard or sacrifice costs, and a cost that depends on X.
  - Granting Ward to another permanent through a continuous effect.
  - Hexproof on a player.
  - "Hexproof from [quality]".
  - `IgnoreKeyword("Hexproof")`, which is accepted but unread.
  - Team rules stay a design non-goal.
- Read `docs/implementation/07-targeting.md` and `14-protection-attach.md`. The Ward payer queue TODO is in `types/ability.rs:402`.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
