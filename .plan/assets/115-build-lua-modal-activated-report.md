# 115 Build Lua modal activated abilities: decision report

Branch `phase5/lua-modal-activated`. Requirements: `115-build-lua-modal-activated-requirements.md` (haiku auditor, APPROVE in round 2). Implementation: sonnet auditor (verdict below). `tools/check.sh --full` exits 0, and the on-demand 500-seed proof sweep passes.

## Decisions

1. **Where activation checks the mode count.** Options: copy the cast branch, or share a helper. Chosen: `legality::announced_modes_in_order`, used by both cast and activation, so the two cannot drift. Cast's rejection texts became kind-neutral ("choose between 1 and 1 mode(s)"); no test depended on the old wording. By the implementer.
2. **Rust-built abilities with an opponent-chosen or pawprint choice.** Options: let them through, or refuse. Chosen: `ActivateAbility` refuses both, and the entries list leaves them out so the list matches the command. The opponent case carries `// TODO: opponent-chosen modes for an activated ability (CR 700.2e)`. By the implementer.
3. **Where the Lua refusals live.** Decision 090 put the opponent-chosen refusal at `:Register()`. Options: `:Register()`, or `decode_modes` through a new `ModeOwner::Activated`. Chosen: the decoder, beside the other per-kind refusals. Both give the same load error. By the implementer; the requirements auditor accepted it.
4. **Mana modes.** 090 says mana abilities get no modes but does not cover an `AddMana` mode on an activated ability. Without a guard, a solo mana mode would reach the mana-ability branch and skip the stack. Chosen: a load error for `AddMana` and `AddManaChoice` modes on activated abilities (CR 605.1a, 605.3b). By the implementer; the requirements auditor asked for the justification and both tests.
5. **Shared `:Register()` check.** The activated builder needed the same "exactly one of `:SetOperation` or `:SetModes`, no `:SetTarget` beside `:SetModes`" rule as the trigger builder. Chosen: one private `operation_or_modes` helper for both. By the implementer.
6. **Entry shape.** Options: change `ActivateEntry.targets` on the wire, or add fields. Chosen: the engine's `ActivateEntry` gains `modes: Option<ModeRange>` and `targets: EntryTargets`, as `CastEntry` has. The wire keeps `targets` as a flat list (empty for a modal ability) and adds `modes` and `mode_targets`, both skipped when empty. The reply's `modes` defaults to empty. `PROTOCOL_VERSION` stays 1. By the implementer, per 090.
7. **Clippy `large_enum_variant` on `ActivatePlan`.** Options: box a field, or allow the lint. Chosen: allow, with a comment saying the plan is already boxed in `Validated::ActivateAbility`. By the implementer.
8. **Proof card and `BEHAVIOUR_REV`.** Forked Totem ("{1}, {T}: choose one: 1 damage to target creature, or gain 2 life") takes one Raging Boar slot. The requirements report expected `BEHAVIOUR_REV` 10 to 11, but the pinned replay hash did not move because that game never reaches the swapped slot. Chosen: keep 10, and explain it in `39-lua-modes.md`. The bot counts accepted modal activations, and `proof_game.rs` asserts some game makes one. By the implementer; the implementation auditor accepted it.
9. **TODO wording.** The ticket words the TODO with the citation first. CLAUDE.md puts it at the end, and CLAUDE.md wins. Raised by the requirements auditor.
10. **Load-error coverage.** The activated tests pin every activated-only refusal, plus "fewer than two modes" to show the shared decoder runs. The other shared decoder errors are already pinned by the spell and trigger tests from "Build Lua modal spells and triggers". Raised by the requirements auditor; the implementer checked the existing cases.

## Audits

- Requirements (haiku): round 1 asked for nine changes (citations, TODO format, test list, AddMana justification, docs split, `BEHAVIOUR_REV` wording). Round 2: APPROVE.
- Implementation (sonnet): round 1 found two comment-rule problems in tests (a misplaced `// CR 702.22c` line in `bot.rs` and the word "new" in a `lua_modes.rs` doc). Both were fixed, `comment_diff.py` passed, and the full check reran green. Round 2: APPROVE.

## Gates skipped or changed

- The parent thread ran the Jev audit, so this run skipped it.
- No AskUserQuestion commit or merge prompts. The parent asked for commits without a question, and the master thread merges.
- The audit round cap is five rounds (user rules and the parent), not the two in CLAUDE.md.
