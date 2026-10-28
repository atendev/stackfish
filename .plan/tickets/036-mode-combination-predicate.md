---
id: 036
title: Mode-combination predicate
type: task
mode: HITL
status: open
blocked_by: []
claimed: true
---

## Question

`combine_chosen_modes` refuses some modes at cast time, but the modal entries still list a card or a pair it then refuses. How do the entries, the cast, a modal trigger and the opponent's mode choice share one rule?

## Notes

- Origin 011. Decided in 024. Branch `phase5/mode-combination`.
- Decisions taken at the requirements report (user said to go with the recommendations):
  - Scope is the cast, the modal trigger and the opponent's mode choice, with `solo_only` on `ReplyShape::Modes` as well as on `ModeRange`.
  - The host test `phase5_error_split.rs` is deleted. Its coverage moves to the engine-level menace test in `phase5_combat_predicates.rs` and the Session test in `phase5_host_session.rs`.
  - `solo_only: Vec<u32>` and not a per-mode `combinable: Vec<bool>`.
- The rule is per mode, not per pair. A mode fails when it has an empty `TargetSpec::Multiple` or an effect `is_modal_resolvable` refuses.

## Resolution

`mode_can_join_others` in `commands/legality.rs` is the one predicate. `combine_chosen_modes`, `submit_mode_choice`, the entry builder and the two `Modes` pauses use it. `ModeRange` and `ReplyShape::Modes` carry `solo_only`, the wire carries it as an additive field, and `check_modes` refuses a selection of two or more that contains one. A card with no legal selection is not listed. See `docs/implementation/24-mode-combination.md`.
