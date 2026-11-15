# Delayed triggers

A resolving spell or ability can make a one-shot triggered ability for the beginning of the next end step (CR 603.7). The decision is in `.plan/closed/132-delayed-triggered-abilities.md`. The requirements are in `.plan/assets/164-build-delayed-end-step-triggers-requirements.md`.

## Shape

- `GameState.delayed_triggers: Vec<DelayedTrigger>`. A row holds `id` (from `allocate_ability_id`), `source`, `source_lki`, `controller`, `when: DelayedWhen`, `effect`, `refers: Vec<(ObjectId, ZoneKind)>`, `created_after: EventId` and `triggered`. `DelayedWhen::NextEndStep` is the only timing.
- Lua: `Game.Ops.AtNextEndStep(op, refers)` returns a `DelayedTag`, which only the `Chain` and `ChainEach` entry decoder accepts. Anywhere else, `decode_operation_value` rejects it with a load error. `op` must be `Exile`, `Sacrifice`, `Destroy`, `ReturnToHand` or `ReturnToBattlefield`. It decodes to `AbilityEffect::AtNextEndStep { effect, refers: DelayedRefers }`.
- Creation: the chain loop in `resolve/modal.rs` makes the row itself (`resolve/delayed.rs`, `create_delayed_trigger`), because `"entered"` and a moved target read the ledger from where the resolution began. `ModalContinuation.since_ledger` carries that position across pauses. The dispatcher arm in `resolve_ability_effect` is a no-op.
  - `"targets"`: the entry's legal object targets. A target an earlier entry moved to a public zone is followed to its new object through the `ZoneChange` events since `since_ledger` (CR 400.7j).
  - `"self"`: `top.source`. A spell's card has moved off the stack by the end step, so a spell's `"self"` row does nothing.
  - `"entered"`: every object a `ZoneChange` to the battlefield since `since_ledger` names.
  - Only ids that exist when the row is made are recorded, each with its zone at that moment.
  - `source_lki` is the live source, else `top.source_lki`, else the last known information from the source's own move since `since_ledger`. A spell whose earlier entry paused is already in its graveyard when the row is made.
- `created_after` is `EventId(next_event_id)` at creation. `collect_triggers` matches an untriggered row only against an event at or after that id (CR 603.7a), so the end step a row is made in never triggers it (CR 513.2). The match gives an ordinary `PendingTrigger`, so rows join the APNAP grouping and the trigger-order Decision (CR 603.3b).
- `resolve_collect_triggers_state` marks the row `triggered` at the match. `push_ordered_triggers` removes it and pushes a `TriggeredAbility` stack object with `targets` naming every referred id, `target_requirement: None`, no `BecameTarget`, and `delayed_refers` holding the zones. A row whose controller has left the game is dropped at the push (CR 800.4a, 800.4d).
- Resolution: with `delayed_refers` set, the legal list is each referred id still in its recorded zone (CR 603.7c). A `Sacrifice` row also drops a permanent its controller does not control (CR 701.21a). The ability never fizzles; with nothing left it does nothing.
- `retire_object` never touches rows. `sba::remove_players_objects` drops the leaver's rows.
- `loop_watch::fingerprint` includes the rows without `id` and `created_after`.
- No wire change: `WireStackEntry` copies chosen fields and does not carry `delayed_refers`. `BEHAVIOUR_REV` stays 10.

## Tests

- `tests/delayed_triggers.rs` (18): the Whip-style return and exile, the row outliving its spell, the controller, a row made during the end step, an older end-step event, triggering once, an object that left and came back, an object flickered between push and resolve, `"entered"` tokens, a card a replacement sent elsewhere, a delayed sacrifice of a stolen creature, a trigger-order pause, APNAP order with a normal end-step trigger, a leaving player, a flicker through `"targets"`, and the load errors.
- `engine/loop_watch_tests.rs`: the fingerprint ignores row ids and `created_after` and sees `triggered`.
- Scenarios under `tests/scenarios/delayed/`: a returned creature exiled at the end step, and an exiled creature returned at the end step.

## Open

- Other timings: your next upkeep, your next end step, end of combat, "when that creature dies or leaves this turn".
- Stated durations and the CR 603.7b choice when several events happen at once.
- CR 603.7f, 603.7g and 603.7h.
- A chain that acts on only one of two entered groups.
- Showing pending rows to players.
- `resolve_sacrifice_effect` has no general control check (CR 701.21a). Only the delayed path checks.
- Encore, dash, blitz and mobilize.
- A spell's card moves to its graveyard before its later chain entries finish when an earlier entry pauses (CR 608.2n).
