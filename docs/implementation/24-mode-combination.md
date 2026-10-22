# 24-mode-combination

Ticket 036, branch `phase5/mode-combination`. Ticket 024 decided that the modal entries and the cast validation share one predicate. This slice adds it.

## The predicate

`commands/legality.rs` holds it. `mode_can_join_others(mode)` is false when the mode has an empty `TargetSpec::Multiple` requirement or an effect that `is_modal_resolvable` refuses. `combine_chosen_modes` fails on exactly those two properties, so the rule is per mode and not per pair. Two modes that each pass always combine, because target words are deduplicated per word (`validate_targets`).

A mode that fails is "solo only": it can be the single chosen mode and cannot sit beside another. `solo_only_modes(modes)` lists their indices in ascending order.

These callers use it:

- `combine_chosen_modes` (the caster-chosen cast, the pawprint branch and the modal triggered ability).
- `submit_mode_choice` (the opponent's choice). The hand-written copy of the `is_modal_resolvable` loop is gone, and `is_modal_resolvable` is private again.
- `cast_modes_and_targets` (the entries) and `mode_range`.
- `decision/open.rs`, which fills `solo_only` on the trigger and opponent `Modes` shapes.

The comprehensive rules allow any legal pair (CR 700.2), so a solo-only mode is a resolver-coverage gap and not a rule.

## Entries and the facade

- `ModeRange` and `ReplyShape::Modes` have `solo_only: Vec<u32>`. It is empty for almost every card.
- A card is listed when it has a legal selection. With a minimum of 0 or 1, any usable mode counts. With a minimum of 2 or more, only usable combinable modes count, and a repeatable range needs one of them, because choosing a solo-only mode twice is still two modes.
- `check_modes` refuses a selection of two or more that contains a `solo_only` index, as `InvalidChoice` with the Decision still open.
- The trigger and opponent pauses are checked by the engine (`UnsupportedModeCombination`), which the facade maps to `InvalidChoice`. The shape lists `solo_only` so a host sees it first.
- The wire carries `solo_only` on `WireModeRange` and on the `modes` shape. It is skipped when empty, so the field is additive and `PROTOCOL_VERSION` stays 1.
- The proof bot never puts a solo-only mode in a selection of two or more.

## Tests

- `engine/tests/phase5_mode_combination.rs` (10): a choose-two card with one combinable mode is not listed; a three-mode card lists `[1]` and accepts `[0, 2]`; "one or both" accepts `[1]` and refuses `[0, 1]`; the empty `Multiple` mode is solo only; a repeatable range needs one combinable usable mode; a pawprint budget takes a solo-only mode alone but not paired; a modal trigger and an opponent-chosen spell show `solo_only` and refuse a pair; and a cross-check where the facade and `submit_action` accept the same selections over four small cards.
- `host/src/wire/decision.rs` (2): `solo_only` reaches the wire on the range and on the shape, and an empty list is omitted.
- Existing modal tests pass unchanged. `host/tests/phase5_error_split.rs` is deleted. See `22-error-split.md`.

## Open items

- `TODO(modal-resolver)` on `mode_join_refusal`: `is_modal_resolvable` still refuses `ForEachPlayer`, `Cascade`, `MadnessCastOrDiscard`, `ReturnSelfWithCounter`, `AddMana*`, `Script` and a nested `Modal`. Each kind the resolver learns shrinks `solo_only`.
- An empty `TargetSpec::Multiple` is a card-definition mistake. A load-time check should reject it. This slice does not add one.
- Madness and Cascade casts carry their own `modes` and do not go through the entries' `check_modes`. They still reach `combine_chosen_modes`.
- The proof bot reads a pawprint budget as a plain count (existing TODO).
