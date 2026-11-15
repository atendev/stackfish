---
id: 141
title: Build player hexproof and shroud
type: task
mode: HITL
status: open
blocked_by: []
claimed: true
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

## Handoff

Branch `phase5/player-hexproof-shroud`, implementation at `1200323`. Not pushed.

Done:
- Requirements report `.plan/assets/141-build-player-hexproof-and-shroud-requirements.md`. The haiku auditor approved it in round 2. Its round-1 fixes: CR 109.5 for "you", tests for activation, trigger and redundancy, and the player-protection TODO.
- `Game.CreatePlayerKeywordModifier` (`:SetAffected`, `:SetKeyword("Hexproof"|"Shroud")`, `:SetFunctionsFrom`), `PlayerKeywordModifier` in `GameState.player_keyword_modifiers`, and the shared `PlayerAffected` type (the hand size and hand reveal types are aliases of it). `player_blocked_by_hexproof_or_shroud` in `commands/legality.rs` covers the Player, PlayerScript and AnyTarget player arms.
- 16 tests in `crates/engine/tests/player_hexproof_shroud.rs`. Docs: the stub, player-modifiers, game-constants, not-available-yet, lua index, DESIGN.
- `tools/check.sh --full` passed at `1200323`: 2688 tests, 0 failed.

Left, in order:
1. The implementation audit. CLAUDE.md asks for a `sonnet` auditor. Round 1 never returned because the first one hit a usage limit, so the next thread starts at round 1. Its input is `git diff 572fcb1~1..1200323 -- . ':!.plan'`.
2. Fix the auditor's findings and rerun `tools/check.sh --full`.
3. Write `.plan/assets/141-build-player-hexproof-and-shroud-report.md`, run unslop on it, and commit.
4. Push, then open a PR against `wayfinder/host-protocol-master` and return PUSHED.

Read first: the requirements file, `crates/engine/src/commands/legality.rs` (`player_blocked_by_hexproof_or_shroud`), and `crates/engine/src/types/player.rs` (`PlayerAffected`, `PlayerKeywordModifier`).
