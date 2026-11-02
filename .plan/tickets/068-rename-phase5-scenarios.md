---
id: 068
title: Rename the phase5 TOML scenarios into feature folders and fix their references
type: task
mode: HITL
status: open
blocked_by: [060]
claimed: true
---

## Question

Move the 39 files in `crates/engine/tests/scenarios/phase5/` into feature folders under `crates/engine/tests/scenarios/` without the `phase5_` prefix, and update every reference to them.

## Notes

- Layout and example names are in `closed/060-rust-tests-to-toml-scenarios.md`. `mechanics/banding/` and the four top-level files stay.
- `crates/engine/tests/scenarios.rs` walks the directory recursively, so no Rust code changes.
- Update the cross-reference comments inside the TOML files and the paths in `docs/scenario_format.md`. Fix the stale ones there too (paths missing the `phase5/` folder, old `phase5_*.rs` names).
- Reword comments that name a branch (`phase5/ward` and similar) rather than retargeting them.
- Use `git mv` so history follows the files. `tools/check.sh --full` must pass.

## Requirements

Success: `crates/engine/tests/scenarios/phase5/` no longer exists, its 39 files sit in feature folders with `phase5_` dropped from the name and nothing else renamed, every live reference points at a real path, and `tools/check.sh --full` passes with the same scenario count as before (39 phase5 files, 58 scenario files in total).

Mapping (`git mv`, prefix drop only):

- `combat/`: `automatic_combat_damage`, `combat_damage`, `combat_declarations`, `keyword_primitives`
- `commander/`: `commander`, `commander_damage`, `commander_tax`
- `costs/`: `activate_ability_mana`, `cast_spell_payment`, `payment_unwind`, `x_costs`
- `layers/`: `conditional_continuous_effects`, `continuous_effect_filter_subtype_supertype`, `layer_engine_basics`, `layer_engine_cda`, `layer_engine_consumers`, `layer_engine_types_colors`
- `modal/`: `modal_pawprint_budget`, `modal_spell_toml`
- `phases/`: `beginning_phase`, `ending_phase`
- `sba/`: `sba_basics`
- `scripting/`: `continuous_effects_script`, `mass_choice_balance`, `mass_destroy_effect`, `script_bridge_primitives`, `script_bridge_trigger`, `script_operation`, `script_self_registration`, `static_ability_self_registration`
- `shortcuts/`: the six `scenario_shortcuts_*` files
- `targeting/`: `script_target_requirement`, `spell_target_script`, `targeting_basics`

Unchanged: `mechanics/banding/` and the four top-level files.

Reference updates:

- The 17 cross-reference comments (in 16 of the moved TOML files) name a sibling as `phase5_<name>.toml`. Each becomes the sibling's new path relative to `tests/scenarios/` (for example `costs/cast_spell_payment.toml`).
- Nine comments in the moved TOML files name a branch (`phase5/automatic-combat-damage`, `phase5/cascade`, `phase5/ward`, `phase5/mass-choice-effects`, `phase5/register-abilities-and-script-operation`, `phase5/scripted-target-filter`). Reword each to say the feature or rule instead, with no branch name.
- `docs/scenario_format.md`: every scenario path becomes the new path, including the stale ones missing `phase5/` and the bare names `phase5_layer_engine_consumers.toml` and `phase5_layer_engine_types_colors.toml` (lines 410 and 413). The intro line naming `phase5/` and `card_tests/` (neither will exist) as example subdirectories names real folders instead (`combat/`, `mechanics/banding/`). The stale Rust names `phase5_x_costs_activated_ability.rs`, `phase5_layer_engine_basics.rs`, `phase5_targeting_basics.rs`, `phase5_script_modal_spells.rs` and `phase5_mana_ability_during_payment.rs` point at the current files under `crates/engine/tests/`.

No comp rules apply: this is a file move. No TODOs are expected. No Rust code changes: `crates/engine/tests/scenarios.rs` walks the folder recursively.

Tests: none added. The existing `all_scenarios_have_no_hard_failures` test is the check, and the file count before and after must match.

Out of scope: the dated notes under `docs/implementation/` (they record past sessions, so their old paths stay), the Lua comment in `crates/scenario/tests/card_db_lookup.rs` that names a branch, and the branch names in `docs/DESIGN.md` and `docs/lua/`. None of these refer to a scenario file.

## Resolution

Moved the 39 files with `git mv` into `combat/`, `commander/`, `costs/`, `layers/`, `modal/`, `phases/`, `sba/`, `scripting/`, `shortcuts/` and `targeting/`, dropping only the `phase5_` prefix, so the names still match their Rust counterparts. The 17 sibling references in the TOML comments and every path in `docs/scenario_format.md` point at the new files. That includes 10 doc paths already missing `phase5/`, two bare names and five old `phase5_*.rs` names. The nine branch-name comments now state the rule or feature instead. A stale pointer to the removed `phase5_balance.rs` was dropped from `scripting/mass_choice_balance.toml`. No Rust changed, the scenario count stays 58, and `tools/check.sh --full` passes. Commit `6619188` on `phase5/rename-phase5-scenarios`. A Haiku auditor approved the requirements after one round, and a Sonnet auditor approved the implementation.
