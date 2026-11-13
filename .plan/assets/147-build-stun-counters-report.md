# Decision report: Build stun counters

Branch `phase5/build-stun-counters`, implementation commit `2b37481`. The spec is the Resolution of the closed decision "Stun counters and tap hooks" (`closed/102-stun-counters-and-tap-hooks.md`). One sonnet auditor checked the full diff and returned APPROVE on round 1.

## Workflow choices

1. **Which loop runs the ticket.** The options were the CLAUDE.md slice loop (requirements report, requirements audit, Merge prompt) or the runner loop with the master's changes. The main thread ran the runner loop as the master asked. It wrote the requirements itself into this report and skipped the separate requirements audit and the Merge / Don't merge prompt, because the master merges. Audit verdict: not audited, a process choice.
2. **Auditor model.** The runner names haiku, and the repo's slice workflow names sonnet for an implementation audit. The main thread used sonnet, because repo rules win for gates. Audit verdict: n/a.
3. **Branch name.** The worktree started on `t3/build-stun-counters`. The main thread created `phase5/build-stun-counters` from it to follow the CLAUDE.md naming. Audit verdict: n/a.

## Implementation decisions

4. **Where the helper lives.** The options were `engine/mod.rs`, `turn.rs` or `resolve/direct_effects.rs`. The implementer put `Engine::attempt_untap` in `turn.rs` as `pub(super)`, next to `untap_step`. `flip_tapped` in `resolve/` can reach it. Audit verdict: APPROVE.
5. **Helper precondition.** The helper could check zone and tapped state itself, or the callers could. The callers check, as 102 says. `untap_step` filters for tapped battlefield permanents, and `flip_tapped` keeps its early return, so an untapped permanent keeps its stun counter. The helper checks only `phased_out`. Audit verdict: APPROVE.
6. **Pending stun count.** The helper sums `CounterAdded { Stun }` deltas for the same object in the event list it is given. In the untap step that list is the step's own list. For an effect it is the resolution's list. A mutation that dropped the pending sum failed the two-untaps test, so the test checks that path. Audit verdict: APPROVE.
7. **Test for two untaps in one resolution.** No Lua op runs two ops in sequence. The implementer used a modal instant with one Untap mode and `ModeChoice::ExactlyRepeatable(2)`, cast with the same target twice (CR 700.2d). Audit verdict: APPROVE.
8. **The `Untap` TODO in `types/ability.rs`.** 102 asked for it to be narrowed, not deleted. The new text is "no non-stun would become untapped replacement event, so only a stun counter stops an untap (CR 614, 616.1)". CR 122.1d moved to the helper, where stun is applied. Audit verdict: APPROVE.
9. **The "no condition tag for a counter being removed" TODO.** It sits beside the `Each*` TODO under `ConditionPrimitive`, where tags are defined. Audit verdict: APPROVE.
10. **`BEHAVIOUR_REV`.** It moves only when the replay pin moves. The full suite passed with the pin unchanged, so it stays 10. Audit verdict: APPROVE.
11. **Docs.** The implementer updated the Lua stub (the `Stun` field, the `"Stun"` name in `Ops.Counters`, and a note that `amount` is negative for a removal), `game-constants.md`, `not-available-yet.md`, the backlog item 10 list and tap/untap entry, `scenario_format.md`, and a new `docs/implementation/40-stun-counters.md` with an index line. Audit verdict: APPROVE.
12. **Wiki submodule.** `docs/wiki/Game-Constants-Reference.md` lists counter names but is a separate repo. The main thread reverted its edit there, because changes outside this repo need the user's approval. Audit verdict: n/a.

## Tests

`crates/engine/tests/stun_counters.rs` has 10 tests. They cover the untap step with one stun counter, with two (2, then 1, then 0, then untapped), an untapped permanent keeping its counter, and a phased-out permanent. They also cover `Untap` and `UntapAll`, a replaced untap firing no `BecomesUntapped` (with a control case that does fire), two untaps in one resolution, a creature entering tapped with a stun counter, and the Lua names. `scenario::build` and `host::wire::event` each gained a unit test for the `"stun"` name. `tools/check.sh --full` passed.

## Left open

- Stale TODOs at `types/ability.rs` say there is no Lua "becomes tapped" or "becomes untapped" condition, but those tags exist since "Bridge condition events into Lua". This ticket did not touch them. A comment pass can narrow them.
- `docs/wiki` still omits `Stun` from its counter list.
