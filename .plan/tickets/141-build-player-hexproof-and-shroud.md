---
id: 141
title: Build player hexproof and shroud
type: task
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Add a player static that gives a player hexproof or shroud, as "Ward with non-mana costs, player hexproof and hexproof from" decided.

## Notes

- Read `closed/101-ward-costs-and-player-hexproof.md`.
- A new builder in the shape of `HandSizeModifierBuilder`, with `Game.Affected.You`, `Opponent`, `Any` and `EnchantedPlayer`.
- Enforce it in the `TargetFilter::Player` and `PlayerScript` arms and the any-target player branch of `target_is_legal_impl`. `attachment_target_is_legal` skips it (CR 115.1b).
- Hexproof stops only other players (CR 702.11c). Shroud stops the player too (CR 702.18a).
- Test the Aura spell that targets a player, and a recheck on resolution (CR 608.2b).
- Close the player TODO above `target_blocked_by_hexproof_or_shroud`.
- Update the game stub in `docs/lua/stubs/game.lua`, and `docs/lua/not-available-yet.md`.

## Resolution

<Filled in at close.>
