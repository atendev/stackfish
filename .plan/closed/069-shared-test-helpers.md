---
id: 069
title: Move duplicated engine test helpers into stackfish_testutil
type: task
mode: HITL
status: closed
blocked_by: [060]
claimed: false
---

## Question

Pull the near-identical setup helpers (`creature_definition`, `vanilla_creature`, `bolt_definition`, `base_state` and similar) that each protection and targeting test file declares into `stackfish_testutil`, without changing what any test asserts.

## Notes

- Optional. Found by 060.
- Clusters: `crates/engine/tests/protection_from_*.rs` (about 50 tests) and `crates/engine/tests/targeting_*.rs`. `layer_engine_*`, `modal_*` and `script_ops_*` may share helpers too.
- Only move a helper when the copies are actually the same. Keep test-specific variants local.

## Resolution

Added 18 helpers to `stackfish_testutil` and deleted 448 local copies from 171 files in `crates/engine/tests/`. A copy moved only when its body matched the shared one exactly, ignoring comments and whitespace.

- `engine_ext.rs`: `pass_both_and_drain`, `pay_free_and_drain`, `cast_spell`, `try_cast_spell`, `declare_attackers`, `declare_blockers`, `assign_combat_damage`, `pass_to_declare_blockers`, `life`, `last_zone_change`, `progressed`.
- `object.rs`: `put_on_battlefield`, `permanent`. `card.rs`: `creature_definition` (vanilla 2/2, id 1, script `bear`). `state.rs`: `two_player_main_phase_state`. New `ability.rs`: `dies_ability`, `any_target_requirement`, `no_filter`.
- Some local helpers had a different name but the same body, so their call sites were renamed: `pass_both` became `pass_both_and_drain`, `step_until_pause` became `drain`, and the no-argument `two_player_state()` and `base_state()` became `two_player_main_phase_state()`.
- `vanilla_creature`, `bolt_definition`, `land_definition`, `combat_state` and most `creature_definition` copies have too many variants (for example 72 variants across 130 `creature_definition` copies), so they stay local, as the ticket notes ask.
- The engine test list is unchanged at 1908 tests. The only assert lines removed are the 71 `pay_free_and_drain` copies, whose assert now lives in the shared helper. No `TODO` was lost, and the rationale comments on deleted copies moved to the shared doc comments.

The grouping script is not committed. Rerunning a body-hash grouping over `crates/engine/tests/` finds the next candidates if the variants converge later.
