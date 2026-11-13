# 174 Modal mana abilities: decision report

Grilling ticket, decided in auto mode. Branch `phase5/modal-mana-abilities`. The questions are in [174-modal-mana-abilities-questions.md](174-modal-mana-abilities-questions.md) and the answers with their evidence in [174-modal-mana-abilities-answers.md](174-modal-mana-abilities-answers.md).

## Outcome

The load error on mana modes in an activated `:SetModes` stays. No printed card has a modal activated mana ability, so building one would add engine paths, wire fields and tests that no card exercises. The ticket records the design a build would follow if such a card appears. While answering, the main thread found and proved a separate hole. `Game.Ops.AddMana` and `AddManaChoice` load on a spell, a trigger or a spell or trigger mode, and then resolve from the stack as a no-op. That hole becomes a build ticket.

## Decisions

1. **Build modal mana abilities now?** Options: build them (A), keep the load error and record the shape as out of scope (B), or leave the ticket open (C). Chosen: B. A scan of the card database found 30 modal activated abilities on 30 cards, and none has a mode that adds mana. The near misses are Item Crate (a random token, not a mode choice) and Chandra, Flameshaper's +2 (a loyalty ability, excluded by CR 605.1a). The existing TODOs and docs stay. Recommended by the main thread, accepted by the grilling subagent, and the auditor approved.
2. **Where the mode is announced.** Options: on the existing `ActivateAbility.modes` field in the same action (A), or a later mode Decision (B). Chosen: A, as a design only. CR 601.2b and 700.2a put the mode announcement inside activation (CR 602.2b), and CR 605.3b leaves no later point for a Decision. Recommended by the main thread, accepted by the grilling subagent, and the auditor approved.
3. **Which modal abilities count as mana abilities.** Options: judge the printed ability as a whole (A), or judge each activation by its chosen modes (B). Chosen: B, as a design only. CR 700.2c treats an unchosen mode's targets as absent, so an activation whose chosen modes have no target and could add mana meets CR 605.1a. The main thread first recommended A. The auditor objected under CR 700.2c, and the main thread changed the recommendation to B before the grilling subagent accepted it. A build would move the `mana_ability` flag in `commands/entries.rs` from load time to activation time.
4. **How a Payment Decision lists one, and the proof bot.** Options: give the mana listing the `modes: Option<ModeRange>` that `ActivateEntry` already has, limited to the modes that have no target and add mana (A), or refuse modal mana abilities during payment (B). Chosen: A, as a design only. B would break CR 605.3a. The bot's pick among the allowed modes stays open until a card exists, and the proof deck gains no card. Recommended by the main thread, accepted by the grilling subagent, and the auditor approved.
5. **The no-op mana hole.** Options: surface a build ticket that resolves mana from the stack (A), surface a ticket that turns it into a load error (B), or fix it in this ticket (C). Chosen: A, with the grilling subagent's amendment. CR 605.5a and 605.5b say spells and ordinary triggers that add mana use the stack and resolve normally. About 70 instants and sorceries and about 125 triggers add mana on resolution. Triggered mana abilities (CR 605.1b, 605.4a) skip the stack and are not built, so they get their own follow-up. The build ticket must measure how many of the trigger cards are triggered mana abilities before it sets its scope. It must also decide how a spell or trigger announces the color for `AddManaChoice`, since `chosen_colors` exists only on `ActivateAbility`. Recommended by the main thread, amended by the grilling subagent, and the auditor approved.

## Evidence

- The main thread ran a throwaway test, since deleted. It cast a sorcery with the mode `{ operation = Game.Ops.AddMana("R", 3) }`, chose that mode and resolved the spell. P0's pool held 0 red afterwards. `resolve_misc_or_script_effect` (`engine/resolve/misc_effects.rs:190`) returns with no pool change, and `is_modal_resolvable` (`commands/legality.rs:1135`) leaves `AddMana*` off its allow-list, so such a mode is solo-only.
- The answers file's appendix records the card scan, which can be re-run.

## Audit

- Questions report, haiku auditor, three rounds. Round 1 asked for five changes: cite CR 700.2c in Q3, cite CR 601.2b for the mode announcement, drop an unsourced Gatherer claim, drop an unexplained bot strategy, and record the queries. Round 2 asked for the full database path. Round 3: APPROVE.
- Answers, the same auditor, two rounds. Round 1 asked for five changes: reconcile the card counts, restate Item Crate from its text, record the scan, say the trigger split is unmeasured, and cite the code behind `is_modal_resolvable`. The main thread made them. Round 2: APPROVE.

## Gates changed by the brief

- Auto mode: the user was not interviewed. A sonnet grilling subagent answered the main thread's questions. The CLAUDE.md Approve / Deny and Merge buttons were skipped, as the brief directs. The master merges.
- The commit gate ran without a question, as the brief directs.
- The CLAUDE.md cap of two audit rounds was replaced by the brief's five-round cap. The questions audit took three rounds.
- No code changed. The only engine change was the throwaway probe, deleted before the first commit.

## Surfaced

- Resolve mana added by spells and stack abilities | build | none. Resolve `AddMana` and `AddManaChoice` from the stack into the controller's pool for spells and ordinary triggers (CR 605.5a, 605.5b). Make an `AddMana` mode joinable, decide how a spell or trigger announces its `AddManaChoice` color, and measure the triggered mana abilities among the trigger cards first.
- Triggered mana abilities | grilling | none. Decide how a trigger that fires from a mana ability's activation resolves at once inside payment (CR 605.1b, 605.4a). The TODO at `commands/activate.rs:247` and `docs/implementation/26-special-mana-payment.md` already name the gap.
