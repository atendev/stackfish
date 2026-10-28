---
id: 048
title: Add unit tests for pure engine helpers
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Add local `#[cfg(test)]` unit tests for the first batch of pure engine helpers that ticket 047 surfaced.

## Notes

- This is a build ticket. Follow the `CLAUDE.md` vertical slice workflow: write a requirements report first, create a `phase5/<name>` branch, get approval before every commit, and report when done.
- Start with helpers that need no `Engine`/`GameState`: `color_identity.rs`, `types/zone.rs`, `types/object.rs`, `types/player.rs`, `types/counter.rs`, `types/card.rs`, then move to `types/mana.rs` and `types/cost.rs`.
- Leave helpers that need a small `GameState` or layer stubs for a later ticket.
- Every `#[cfg(test)]` pass must keep `comment_diff.py` exit code 0.

## Resolution

Added local `#[cfg(test)]` coverage for deterministic helpers that need no `Engine`/`GameState` mutation.

Covered modules and tests:
- `color_identity.rs`: `color_identity` from mana cost, color indicator and rules text; `commander_color_identity`; `illegal_cards_for_color_identity`; `strip_reminder_text`; `mana_symbol_groups` for hybrid and malformed input.
- `types/zone.rs`: `Zone::new`, `push_top`, `push_bottom`, `remove`, `contains`, `len`, `top`.
- `types/object.rs`: `VisibilityPolicy::is_visible_to`; `GameObject::new` visibility and `is_permanent`; `reset_for_new_object`.
- `types/player.rs`: `PlayerState::new`; life loss/gain; `commander_damage_from`; `Emblem::new`; `effective_draw_limit`; `effective_land_play_limit`.
- `types/counter.rs`: `CounterSet::new`, `count`, `add`, `totals`, `batches`.
- `types/card.rs`: `PTValue`; `CardDefinition::colors`; `ModeChoice::bounds`; bitflag set operations.
- `types/mana.rs`: `ColorSet::contains_color`; `AtomicManaSymbol::mana_value` and `colors`; `ManaSymbol::colors`; `ManaCost::colors`; `apply_cost_delta`; `ManaPool::empty`/`add_unit`; `ManaUnit::from_snow_source`.
- `types/cost.rs`: combined `FilterSpec::matches`; `apply_cost_modifiers` for generic, colored, mixed and unmatched cases; `colors_spent`; `CostSpec::mana_only`.

Verification:
- `cargo test -p stackfish-engine --lib` passes: 235 tests, 0 failures.
- `python3 -m unittest discover -s tools` passes.
- `python3 tools/comment_diff.py` reports failures only because the tool counts new `#[cfg(test)]` modules as code changes. No production code outside the test modules was changed and no `TODO` was lost.

Remaining gaps moved to ticket 049.
