---
id: 132
title: Delayed triggered abilities
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

How does the engine represent a delayed triggered ability (CR 603.7), and which printed shapes does the first build cover?

## Notes

- Origin: "Riders beyond leaving the stack". Unearth (CR 702.84a) and Whip of Erebos need "exile it at the beginning of the next end step", and the engine has no delayed triggers.
- About 506 printed cards mention "at the beginning of the next end step". Encore's token sacrifice is one of them.
- Rules to settle: created during resolution (CR 603.7a), triggers once unless a duration is stated (CR 603.7b), loses a moved object (CR 603.7c), and its source and controller (CR 603.7d, 603.7e).
- Decide whether unearth as a keyword belongs here or in a build after it.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

A delayed triggered ability is a row in a new `GameState.delayed_triggers` list, apart from `state.abilities`. The first build covers "at the beginning of the next end step" only. Decided in auto mode, then approved by the user. Builds are 164 "Build delayed end-step triggers", then 165 "Build unearth" after it and 130 "Build die and leave riders".

- Why a separate list: the source of a delayed ability is usually a spell that has left the stack (CR 603.7d). `collect_triggers` skips an ability whose source object is gone (`triggers.rs:69`), and `retire_object` purges abilities whose source leaves. The riders decision chose a separate list for the same reason.
- Row: `{ id: AbilityId, source: ObjectId, source_lki: Option<GameObject>, controller: PlayerId, when: DelayedWhen, effect: AbilityEffect, refers: Vec<(ObjectId, ZoneType)>, created_after: EventId, triggered: bool }`. `id` comes from `allocate_ability_id`. `push_ordered_triggers` finds it as a third lookup after `abilities` and the departed stash. `source` and `controller` are the resolving stack object's (CR 603.7d, 603.7e), read at creation. `retire_object` never touches rows.
- `DelayedWhen::NextEndStep` is the only variant. It matches `StepStarted` for the end step. Each other shape gets a TODO: "your next upkeep" (62 cards, mostly pacts that need an unless-cost), "your next end step", end of combat, "when that creature dies/leaves this turn".
- CR 603.7a: a row matches only events whose id is later than `created_after`, the next `EventId` at creation. This also gives CR 513.2: a row made during an end step or cleanup cannot match that turn's end-step event, so it waits for the next turn's.
- CR 603.7b: every row triggers once. `collect_triggers` only reads `&GameState`, so `resolve_collect_triggers_state` marks a matched row `triggered: true` right after the match, as it records `once_per_turn_triggered` (`triggers_pipeline.rs:133`). A marked row matches nothing, so a re-collect after a trigger-order or target pause (`recollect_triggers_since`) cannot match it twice. `push_ordered_triggers` removes the row when it builds the stack object, and still finds it by id after a pause. TODO: stated durations ("this turn") and the simultaneous-event choice.
- Ordering (CR 603.3b): rows produce ordinary `PendingTrigger`s in the same `collect_triggers` pass. `group_apnap` groups them by controller with normal end-step triggers, and each controller orders their group at the existing trigger-order Decision. One Decision at a time.
- CR 603.7c: `refers` stores each object id with the zone it was in at creation. The resolve path filters at resolution, not at trigger time: an id no longer in its recorded zone is dropped. Ids are fresh after a zone change (CR 400.7), so the check is "same id, same zone". With every object gone the ability still goes on the stack and does nothing, per the example printed after CR 603.7c.
- Effect: the stack object gets `targets = refers` after the filter, as `Target::Object`, with `target_requirement: None`. The resolve path then takes no legality recheck and no CR 608.2b fizzle, since the ability does not target. Allowed ops: `Exile`, `Sacrifice`, `Destroy`, `ReturnToHand`, `ReturnToBattlefield` (from exile, for flicker). Others are a load error. The build checks that `ReturnToBattlefield` accepts an unchecked target in exile.
- Sacrifice (CR 701.21a): a player can't sacrifice a permanent they don't control. When the delayed effect is `Sacrifice`, the resolve path drops each referenced object the row's controller does not control at resolution, so that object stays on the battlefield. `resolve_sacrifice_effect` gets `// TODO: Sacrifice does not check that the resolving controller controls the target (CR 701.21a)`.
- Players leaving (CR 800.4a, 800.4d): `sba::remove_players_objects` drops rows the leaving player controls. `push_ordered_triggers` also skips a row whose controller has left. A countered or departed source keeps its row, since only `source_lki` is read.
- Lua: `Game.Ops.AtNextEndStep(op, refers)`, a `Chain` entry. `refers` is `"targets"`, `"self"` or `"entered"`. `"entered"` is every object the resolution put onto the battlefield, both returned cards and created tokens, read from the ledger since the resolution began. The entry is non-synchronous, so the chain commits earlier entries first, and `ModalContinuation` carries `since_ledger` across pauses. A card a replacement sent elsewhere is not "entered". TODO: a chain that must act on only one of two entered groups. A single-op ability writes `Chain(op, AtNextEndStep(...))`. Docs: `game-constants.md`, `triggered-abilities.md`, `not-available-yet.md` and the stub in `docs/lua/stubs`.
- Unearth (CR 702.84a) is its own build after this one and "Build die and leave riders". It needs an ability that works from the graveyard, sorcery timing, a haste grant, the delayed exile and the leave rider. Encore (CR 702.141a), dash (CR 702.109a), blitz (CR 702.152a) and mobilize (CR 702.181a) get a TODO in the build. Their delayed operations (sacrifice, return to hand) are all in the allowed list.
- Host: the ability uses `StackEntryKind::TriggeredAbility`, so the stack wire does not change. The list joins the `loop_watch.rs` fingerprint, hashing everything but `id` and `created_after`, since `next_event_id` is outside the fingerprint too. TODO: show pending delayed triggers to players. `BEHAVIOUR_REV` moves only if the replay pin moves.
- TODOs: CR 603.7f (made by a replacement), 603.7g (made by a static permission action), 603.7h (resolve-count).
- Tests, scenario fixtures, no proof deck change: a Whip-style return exiled at the next end step; a row made during the end step waits a turn (CR 513.2); an event before creation does not trigger it (CR 603.7a); the row triggers once (CR 603.7b); an object that leaves and returns is not affected, and the ability still resolves (CR 603.7c, 400.7); an object flickered between push and resolve is skipped; the row survives its source spell leaving the stack (CR 603.7d); the controller is the resolving controller; tokens from `"entered"` are sacrificed; a card a replacement sent elsewhere is not "entered"; a creature another player gains control of before the end step stays on the battlefield (CR 701.21a); a row matched before a trigger-order pause goes on the stack once; a row of a player who left is dropped (CR 800.4d); a delayed and a normal end-step trigger are ordered together in APNAP.
- Printed card counts from `stackfish.db`, approximate: 506 "at the beginning of the next end step" (89 exile it, 116 sacrifice it or them, about 66 return to the battlefield), 62 "your next upkeep", 25 "your next end step", 20 "when that creature dies this turn", 69 unearth.
- Terms: `CONTEXT.md` adds "Delayed trigger".
