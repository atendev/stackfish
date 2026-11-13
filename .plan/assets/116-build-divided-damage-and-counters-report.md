# 116 Build divided damage and counters: decision report

Branch `phase5/divided-damage-counters`. Two child sessions built this ticket. The first wrote and got approval for the requirements report (`116-build-divided-damage-and-counters-requirements.md`, haiku auditor, round 3) and built the engine side. The second picked up at b0c51a8 and built the host side, docs, TODOs and missing tests, then ran the code audit. The decisions from 091 and the requirements report are not repeated here.

## Process choices

- **Which ticket loop.** The repo's CLAUDE.md vertical slice loop defines the gates, so it wins over the runner where they overlap. The code auditor ran on sonnet, as CLAUDE.md asks. The master's brief set the audit cap at five rounds, which overrides the two-round cap in CLAUDE.md. The master also told the child to commit without asking, so the runner's commit question was skipped. The master merges, and no PR was opened.
- **Picking up the previous branch.** `git merge --ff-only phase5/divided-damage-counters` failed, because this worktree started at a newer master tip (b039425) that already held three other merged tickets. The options were a rebase of the earlier commits, a normal merge, or stopping. The main thread chose a normal merge, because it rewrites no history and the remote branch did not exist yet. One merged test from another ticket (`battlefield_card_scripts.rs`) built an `ActivateAbility` without `division`, so the merge needed a one-line fix.

## Implementation decisions

1. **Division on snapshot stack entries.** The snapshot already drops a target the Seat can't see. The options were to send the full division (misaligned with the shown targets, and it leaks how many targets are hidden), to drop a share together with its hidden target, or to omit the division when any target is hidden. The implementer chose to drop the share with its target, so `division` stays index for index with the shown `targets`.
2. **How the bot splits.** The bot picks a target count up to the smaller of the slot's `max` and the total. It then starts every share at 1 and hands out the rest one at a time to random targets. A word with no total or with fewer than two targets gives 0s, and the bot sends an empty division when no word divides. The engine wants exactly that flat form. Chosen by the implementer, following the requirements report.
3. **Where Arc Lightning goes.** The red-green proof deck must stay at 40 cards (`both_decks_hold_forty_cards`). The implementer replaced one of the two Shocks, since it has the same color and job. Adding a 41st card or swapping out an unrelated card were the other options.
4. **Seed-loop check.** `Bot::divided_casts()` counts accepted `cast_spell` actions with a non-empty division, as `modal_activations()` does for modes. `the_games_reach_attacks_sacrifice_and_off_turn_casts` asserts at least one across seeds 0 to 29.
5. **Replay round-trip test.** Snapshotting the live game mid-run could disturb the outboxes the bot reads. The implementer chose to cut the exported replay's `answers` list at the divided cast instead and load that. The test finds the first seed with a divided cast and checks that the loaded stack entry shows the recorded division. It then loads the whole replay, compares every omniscient event with the original game, and checks that the spell's damage amounts are its shares in order.
6. **Replay pin.** The pin did not move, so `BEHAVIOUR_REV` stays 10. Seed 9's game never draws the swapped deck slot, and an older replay can't hold a division or Arc Lightning.
7. **The `:SetTarget` TODO.** `decode_target_spec_value` builds every word with `count: 1`, and `decode_modes` uses the same decoder for a mode's `target`. The TODO names both, so only a non-modal spell's `s.target_requirement()` gives a Lua divided effect more than one target. The docs say the same.
8. **Missing items from the requirements report.** The untargeted division TODO (CR 608.2d) and the entry test for `divide_total` and the capped `max` were missing. The implementer added both.
9. **Plain `Damage` comments.** The `AbilityEffect::Damage` doc said it hits the first chosen target. The resolver hits every legal target. The implementer corrected the doc.

## Audit

The sonnet code auditor read the diff and the `tools/check.sh --full` output without rerunning it.

Round 1 returned five problems, all fixed by the main thread:

1. A comment line the implementer added to the `Damage` arm repeated the existing comment. It was deleted.
2. A test comment in `wire/reply.rs` used a history phrase. It was reworded.
3. `check_divided_shape` refused `DividedCounters` on a script-filtered object word, although its candidates are objects. The auditor suggested refusing only player and any-target words. The main thread chose an allow-list of `Object` and `Script` instead, because that also refuses `Ability` words, and a stack ability can't hold counters. The docs and a unit test cover both cases.
4. No test put an undivided mode beside a divided one, so the "share outside a divided word" rejection was never exercised. A new test covers it, along with a nonzero share on a lone divided target and an all-zero division.
5. Two comments put rule text after `CR 601.2d`. They now cite the number and give the reason on a second line.

The main thread also took four of the auditor's non-blocking notes. It moved a unit test below the `use` lines, renamed the bot's `division` helper to `flat_division`, added a TODO for a TOML division in scenario actions, and updated `99-backlog.md` and `17-legal-actions.md`. It did not add a Lua-level test for the Chain rejection. The requirements report did not list one, and the auditor confirmed the behaviour is correct.

Round 2 returned APPROVE. The auditor traced the new modal test through `validate_effect_division` by hand. It noted that `99-backlog.md` called the branch merged before the master merges it, so that line now says "Built on".

## Checks

`tools/check.sh --full` exited 0 after the round 1 fixes: 30 test binaries, 2672 passed, 0 failed, 2 ignored, and the 38 Python tool tests passed. `lua_api_doc_sync` passes.

## Left open (TODOs in code)

- An X-dependent divided total (CR 601.2d, 107.3).
- Untargeted division chosen at resolution (CR 608.2d).
- Divided amounts in Chain and function operations.
- A copy that gets new targets re-announcing its division (CR 707.10c).
- `Game.Ops.CastSpell` announces no division.
- `:SetTarget` and mode `target` words take one target.
- "One, two or three targets" has no minimum of one.
- Scenario TOML casts and activations can't carry a division.
- A future redirect effect (ticket 103) must keep the announced division. This is written in `07-targeting.md`, not in code.
