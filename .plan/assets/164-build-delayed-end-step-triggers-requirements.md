# Requirements: Build delayed end-step triggers

Ticket: `.plan/tickets/164-build-delayed-end-step-triggers.md`. Decision: `.plan/closed/132-delayed-triggered-abilities.md`. Branch: `phase5/delayed-end-step-triggers`.

## What success looks like

Unearth (build 165) and the die and leave riders (build 130) are not in this build.

A Lua script can write `Game.Ops.Chain(Game.Ops.ReturnToBattlefield(), Game.Ops.AtNextEndStep(Game.Ops.Exile(), "entered"))`. When the ability resolves, the return happens first. Then a row goes into `GameState.delayed_triggers` naming the creature that entered. At the beginning of the next end step the row triggers once. It goes on the stack as an ordinary triggered ability, ordered with the other end-step triggers in APNAP order, and exiles the creature if it is still on the battlefield. The row is gone after the push.

## Engine shape

- `types/game.rs`: `pub struct DelayedTrigger { id: AbilityId, source: ObjectId, source_lki: Option<GameObject>, controller: PlayerId, when: DelayedWhen, effect: AbilityEffect, refers: Vec<(ObjectId, ZoneKind)>, created_after: EventId, triggered: bool }` and `pub enum DelayedWhen { NextEndStep }`. `GameState.delayed_triggers: Vec<DelayedTrigger>`. Every `GameState` literal (15 sites) gets `delayed_triggers: Vec::new()`. The decision writes `ZoneType`; the engine's type is `ZoneKind`.
- `AbilityEffect::AtNextEndStep { effect: Box<AbilityEffect>, refers: DelayedRefers }` with `DelayedRefers { Targets, SelfObject, Entered }`.
- `created_after` is `EventId(state.next_event_id)` when the row is made. A row matches an event only when `event.event_id.0 >= created_after.0`, so no earlier event matches (CR 603.7a), and the end step a row is made in can never trigger it (CR 513.2).
- `triggers::collect_triggers` also walks `state.delayed_triggers`: an untriggered row whose `when` matches (`StepStarted { step: End }`) and whose `created_after` allows the event gives a `PendingTrigger { source, controller, ability: row.id, once_per_turn: false, triggering_stack: None }`. The early "no triggered ability" return also checks the list.
- Ordering (CR 603.3b): rows give ordinary `PendingTrigger`s in the same `collect_triggers` pass, so `group_apnap` groups them by controller with the normal end-step triggers and each controller orders their group at the existing trigger-order Decision. One Decision at a time.
- `resolve_collect_triggers_state` marks the matched row `triggered = true` right after the match, beside the `once_per_turn_triggered` insert (CR 603.7b). A marked row matches nothing, so `recollect_triggers_since` cannot match it twice.
- `push_ordered_triggers`: when the id is not in `abilities` or the departed stash, it looks in `delayed_triggers`, removes the row, skips it when the controller has left the game (CR 800.4a, 800.4d), and pushes a `StackObject` with kind `TriggeredAbility`, `source`, `source_lki`, `controller`, `effect: Some(row.effect)`, `targets` = the refers ids as `Target::Object`, `target_requirement: None`, and a new `StackObject.delayed_refers: Vec<(ObjectId, ZoneKind)>`. It logs no `BecameTarget`, since the ability does not target. `ability_text` is a fixed description of the delayed op.
- Deviation, stated: the decision says the stack object gets "`targets = refers` after the filter". The filter runs at resolution, and the stack object must keep each recorded zone until then, which `targets: Vec<Target>` cannot hold. So `targets` gets every referred id at push (the host sees what the ability refers to) and the new `delayed_refers` field keeps the zones for the resolution filter. `StackObject` is not on the host wire: `WireStackEntry` copies chosen fields, so the new field changes nothing there.
- `resolve_stack_top`: when `delayed_refers` is not empty, the legal list is each referred id still in its recorded zone (CR 603.7c, 400.7). For a `Sacrifice` effect, an object whose effective controller is not the stack object's controller is also dropped (CR 701.21a). No fizzle, since the requirement is `None`. With nothing left the ability still resolves and does nothing.
- Creation: the Chain loop in `resolve/modal.rs` runs an `AtNextEndStep` entry itself. `ModalContinuation` gains `since_ledger: usize`, taken from `pipeline_context` when the chain starts and carried across pauses. `AtNextEndStep` is not synchronous, so earlier entries' events are committed before it reads the ledger. Source and controller are the resolving stack object's (CR 603.7d, 603.7e). `source_lki` is the live source object if one exists, else `top.source_lki`. The id comes from `allocate_ability_id`.
- `refers` at creation:
  - `"targets"`: each legal object target the entry sees.
  - Addition, not in the decision: a target an earlier entry of this resolution moved to a public zone is followed to its new object through the `ZoneChange` events since `since_ledger` (CR 400.7j). The move gave it a new id (CR 400.7), so the id the entry sees would be stale when the row is made, and the decision's flicker case ("exile target creature, return it at the beginning of the next end step") could never find the card in exile.
  - `"self"`: the resolving object's source, with its current zone. For a permanent's ability that is the permanent. For a spell it is the card on the stack, which moves to the graveyard as the spell finishes and gets a new id (CR 400.7), so the row resolves and does nothing. Spell scripts use `"targets"` or `"entered"`.
  - `"entered"`: every object a `ZoneChange` to the battlefield since `since_ledger` names, both returned cards and created tokens. A card a replacement sent elsewhere has no such event, so it is not included.
  - Only ids that exist when the row is made are recorded, each with its current zone.
- Allowed delayed ops: `Exile`, `Sacrifice`, `Destroy`, `ReturnToHand`, `ReturnToBattlefield`. Any other op is a Lua error. `AtNextEndStep` is accepted only as a `Chain` or `ChainEach` entry; on its own, as a mode, or inside `ForEachPlayer` it is a load error.
- Deviation, stated: the decision names `Chain` only. `ChainEach` shares the `chain_entry` check, and rejecting it there would add a special case for no rules reason. In `ChainEach`, `"targets"` means the target words the entry owns, as for any other entry. The dispatcher arm in `resolve_ability_effect` is a defensive no-op.
- Check: `ReturnToBattlefield` moves an unchecked exile target. `ScriptedZoneMoveKind::Reanimate` reads the source zone from the object, so it should. A test confirms it.
- `retire_object` never touches rows. `sba::remove_players_objects` drops rows the leaving player controls (CR 800.4a).
- `loop_watch::fingerprint` adds the rows without `id` and `created_after`.
- No host wire change. The stack entry is `StackEntryKind::TriggeredAbility`. `BEHAVIOUR_REV` moves only if the replay pin moves.

## Lua

- `Game.Ops.AtNextEndStep(op, refers)`: `op` is a called `Game.Ops.*` value from the allowed list. `refers` is `"targets"`, `"self"` or `"entered"`; anything else is a Lua error.
- It returns its own userdata tag, which only `chain_entry` accepts, so no other place can take it by mistake.
- Docs: `docs/lua/game-constants.md`, `docs/lua/triggered-abilities.md`, `docs/lua/not-available-yet.md`, `docs/lua/stubs/game.lua`. `docs/LUA_API.md` lists `Game.Ops.*` (checked: yes), so it gets the constructor. `docs/DESIGN.md` gets `delayed_triggers` in its data model section. `CONTEXT.md` already has "Delayed trigger".
- Addition: `docs/implementation/42-delayed-triggers.md` with a line in `docs/implementation/index.md`. The project CLAUDE.md asks future sessions to read that index first, and every build before this one (35 to 41) added a note. `docs/implementation/99-backlog.md` gets one line listing the new TODOs.

## Comp rules

- CR 603.7: delayed triggers are made by an effect.
- CR 603.7a: made during resolution, triggers only after it exists.
- CR 603.7b: triggers once, the next time its event happens.
- CR 603.7c: a referred object that left its expected zone is not affected; one that left and came back is a new object (CR 400.7).
- CR 603.7d, 603.7e: source and controller.
- CR 513.2: made during the end step, it waits for the next turn's end step.
- CR 603.3b: ordered with the other triggers by controller, in APNAP order.
- CR 701.21a: a player can't sacrifice a permanent they don't control.
- CR 800.4a, 800.4d: a leaving player's delayed triggers leave with them.
- CR 400.7j: other parts of an effect can find an object it moved to a public zone.

## TODOs to mark

- `DelayedWhen`: other timings: your next upkeep, your next end step, end of combat, "when that creature dies or leaves this turn".
- CR 603.7b: stated durations ("this turn") and the simultaneous-event choice.
- CR 603.7f, 603.7g, 603.7h: delayed triggers made by a replacement, by a static permission action, or by a resolve count.
- A chain that must act on only one of two entered groups.
- Showing pending rows to players (host view).
- `resolve_sacrifice_effect`: `// TODO: Sacrifice does not check that the resolving controller controls the target (CR 701.21a)`.
- Encore (CR 702.141a), dash (CR 702.109a), blitz (CR 702.152a), mobilize (CR 702.181a): one TODO line each on `AbilityEffect::AtNextEndStep` in `types/ability.rs`.

## Tests

TOML scenario fixtures under `crates/engine/tests/scenarios/delayed/` where the scenario steps can express them. A Rust integration test `crates/engine/tests/delayed_triggers.rs` covers what a fixture cannot reach (direct state checks, a player leaving, the fingerprint).

1. Whip-style: return a creature card from the graveyard, exile it at the next end step.
2. A row made during the end step waits for the next turn's end step (CR 513.2).
3. An end-step event before the row exists does not trigger it (CR 603.7a).
4. The row triggers once: it is gone after the push, and the next end step does nothing (CR 603.7b).
5. An object that leaves and returns before the end step is not affected, and the ability still resolves (CR 603.7c, 400.7).
6. An object flickered between push and resolve is skipped.
7. The row survives its source spell leaving the stack (CR 603.7d).
8. The stack object's controller is the resolving controller (CR 603.7d, 603.7e).
9. Tokens named by `"entered"` are sacrificed.
10. A card a replacement sent elsewhere is not "entered".
11. A creature another player gains control of before the end step stays on the battlefield (CR 701.21a).
12. A row matched before a trigger-order pause goes on the stack once.
13. The row of a player who left the game is dropped (CR 800.4d).
14. A delayed and a normal end-step trigger are ordered together in APNAP order (CR 603.3b).
Additions beyond the decision's list (15 covers the `"targets"` addition; 16 and 17 cover rules the decision states but does not list as tests):

15. Flicker: exile a target creature, return it at the next end step through `"targets"` (CR 400.7j, plus the unchecked exile target check).
16. Load errors: `AtNextEndStep` alone, as a mode, with a disallowed op, with a bad `refers`.
17. `loop_watch` fingerprint includes the rows and ignores `id` and `created_after`.

No proof deck change.
