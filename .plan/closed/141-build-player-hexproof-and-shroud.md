---
id: 141
title: Build player hexproof and shroud
type: task
mode: HITL
status: closed
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

Built `Game.CreatePlayerKeywordModifier(c)` with `:SetAffected` (`You`, `Opponent`, `Any`, `EnchantedPlayer`), `:SetKeyword("Hexproof" | "Shroud")`, `:SetFunctionsFrom` and `:Register`. Rows live in `GameState.player_keyword_modifiers`. `player_blocked_by_hexproof_or_shroud` in `commands/legality.rs` enforces them in the `Player`, `PlayerScript` and `AnyTarget` player arms, so cast, activation, the trigger scan and the CR 608.2b recheck all respect them. Attachment legality ignores them (CR 115.1b). Shroud stops the player too (CR 702.18a), and hexproof stops only other players (CR 702.11c). "You" reads the source's live controller (CR 109.5). The hand size and hand reveal modifiers now share one `PlayerAffected` type. Each player arm carries `// TODO: protection on players (CR 702.16b)`.

The sonnet auditor approved in round 2, and CI passed on PR 8. Requirements: [141-build-player-hexproof-and-shroud-requirements.md](../assets/141-build-player-hexproof-and-shroud-requirements.md). Decision report: [141-build-player-hexproof-and-shroud-report.md](../assets/141-build-player-hexproof-and-shroud-report.md).
