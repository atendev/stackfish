# Decision report: Build player hexproof and shroud

Ticket `.plan/tickets/141-build-player-hexproof-and-shroud.md`. The requirements are in `141-build-player-hexproof-and-shroud-requirements.md`. The decision behind this ticket is "Ward with non-mana costs, player hexproof and hexproof from".

The work ran across two threads. The first wrote the requirements, got them approved and built the feature at `1200323`. It then handed off because its implementation auditor hit a usage limit before returning. The second thread ran the implementation audit, fixed its findings and wrote this report.

## Process

- The repo `CLAUDE.md` defines the ticket loop, so its gates applied: a requirements report audited by `haiku`, an implementation audit by `sonnet`, and `tools/check.sh --full` before each audit round.
- The wayfinder-master brief replaced the merge question. The branch commits without asking, never merges, and gets pushed for the master to merge.
- The branch is `phase5/player-hexproof-shroud-2`. It was cut from the handoff commit `26f9f20` because another worktree still holds `phase5/player-hexproof-shroud`.

## Decisions

### 1. Lua shape of the static

- Question: how does a card script give a player hexproof or shroud?
- Options: a player-side `AddKeyword`, or a separate player modifier builder.
- Choice: `Game.CreatePlayerKeywordModifier(c)` with `:SetAffected`, `:SetKeyword("Hexproof" | "Shroud")`, `:SetFunctionsFrom` and `:Register`. It has the same shape as `HandSizeModifierBuilder`.
- Reason: players have no characteristics, so `AddKeyword` does not apply to them. "You and permanents you control have hexproof" takes two registrations: this modifier plus an `AddKeyword` on your permanents.
- Made by: the implementer, following the earlier decision ticket.
- Audit: requirements approved in round 2. Implementation approved in round 2.

### 2. Shared affected type

- Question: does the player keyword modifier get its own affected enum?
- Options: a third copy of the `Filter`/`EnchantedPlayer` enum, or one shared type.
- Choice: one `PlayerAffected { Filter(FilterController), EnchantedPlayer }` with an `applies_to` matcher. `HandSizeAffected` and `HandRevealAffected` are now type aliases of it.
- Reason: the `HandRevealAffected` doc said to extract a shared type when a third mechanism needed this shape, and this is the third. The aliases keep existing code and tests compiling unchanged.
- Made by: the implementer.
- Audit: approved.

### 3. Whose "you"

- Question: does "you" read the controller stamped at registration or the source's current controller?
- Choice: the source's live controller through `layers::effective_controller`, so Leyline of Sanctity protects whoever controls it after a control change (CR 109.5). If the source no longer exists, the check falls back to the stamped controller, because `effective_controller` panics on an unknown id.
- Made by: the implementer. The requirements auditor added the CR 109.5 citation in round 1.
- Audit: approved. Test 12 covers the control change.

### 4. Where the check runs

- Question: which target paths enforce player hexproof and shroud?
- Choice: `player_blocked_by_hexproof_or_shroud` in `commands/legality.rs` runs in the `Player`, `PlayerScript` and `AnyTarget` player arms of `target_is_legal_impl`. Each call is gated on `enforce_targetability`. That one path covers cast, activation, the trigger candidate scan and the CR 608.2b recheck.
- Shroud blocks every caster, the player included (CR 702.18a). Hexproof blocks only other players (CR 702.11c). Any matching modifier blocks, so redundant instances need no special handling (CR 702.11h, 702.18b).
- `attachment_target_is_legal` passes `enforce_targetability = false`. An Aura already attached to a player stays when the player gains shroud, because the Aura permanent does not target (CR 115.1b). An Aura spell cast at that player is blocked.
- Made by: the implementer, following the ticket notes.
- Audit: approved.

### 5. TODOs

- The player TODO above `target_blocked_by_hexproof_or_shroud` and the header TODO in `tests/hexproof_shroud.rs` are removed, because this ticket resolves them.
- `// TODO: protection on players (CR 702.16b)` sits at all three player arms. Before, the `PlayerScript` arm had a plain comment saying the same thing.
- The "no team model" TODO is repeated above the new function. The "hexproof from" and "as though it didn't have hexproof" TODOs stay for "Build hexproof from".
- Made by: the implementer. The requirements auditor asked for the protection TODO in round 1.
- Audit: approved.

### 6. Test 8 wording (implementation audit, round 1)

- Question: the requirements said the `PlayerScript` test proves "the script predicate is not consulted", but its predicate always returns `true`.
- Options: make the predicate observable, or correct the wording.
- Choice: correct the wording. A Lua predicate error goes through `unwrap_or(false)`, so a skipped predicate and one that returns `false` give the same result, and no test can tell them apart. Test 8 now says the target is blocked even when the predicate returns `true`, and the existing test proves exactly that.
- Made by: the main thread, using an alternative the auditor offered.
- Audit: approved in round 2.

### 7. Stale Lua doc (implementation audit, round 1)

- Finding: `docs/lua/game-constants.md` still listed player hexproof and shroud as not modeled.
- Fix: removed that clause and linked to the new `player-modifiers.md` section.
- Made by: the main thread.
- Audit: approved in round 2.

## Verification

- 16 tests in `crates/engine/tests/player_hexproof_shroud.rs` cover all 15 items in the requirements.
- `tools/check.sh --full` passed before round 1 and again after the round-1 fixes: fmt, clippy and 2688 tests with 0 failures. `db_scripts_smoke.rs` skipped because `stackfish-db` is not checked out.
- Lua docs updated: `stubs/game.lua`, `player-modifiers.md`, `game-constants.md`, `not-available-yet.md` and `index.md`, plus `docs/DESIGN.md`.
