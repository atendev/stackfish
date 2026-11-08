---
id: 98
title: Timing permission follow-ups
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Which of the timing permission gaps get built next? Conditional flash, "as though it were a main phase" and instant-speed equip are not built, and `Permanent`, `WhileConditionHolds` and "until the source leaves" durations are refused for a grant.

## Notes

- Origin: the Timing permissions entry in `99-backlog.md` and 076's Resolution. Land plays and Equip still use `is_sorcery_speed`.
- Decide the order and the shape of each. A condition on a permission needs the same evaluation point as `can_cast_now`. Instant-speed equip touches the activation path, not the cast path.
- CR 611.2b is why the refused durations are refused. Decide whether any real card needs them.
- Read `docs/implementation/35-timing-permissions.md` and `37-expiring-permissions.md`.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

Conditional flash is built first, then instant-speed equip. "As though it were a main phase" and land-play timing are ruled out, because no printed card uses either. Every refused duration stays refused on a Timing permission. Decided in auto mode.

The card counts come from oracle text in `../stackfish-db/stackfish.db` with reminder text stripped.

- Condition: `TimingPermission` gains `condition: Option<InterveningIfSpec>`, set by `:SetCondition` through `decode_state_condition`, as static effects do. A script condition needs the runtime, so `can_cast_now` takes `&ScriptRuntime`. Both callers hold one. A script error counts as false, as in `layers.rs` (CR 601.3d, 611.3a).
- Evaluation point: `can_cast_now` at `cast.rs:142` and `entries.rs:107`. Payment runs through `submit_payment` and never reads it, so a cast already begun is not re-checked. `// CR 601.6a` goes at the check.
- A card's own flash (14 cards, such as Illusion Spinners and Crashing Tide): a hand-functioning permission with a new `own_card_only` flag, set by a builder method. It matches only `card.object_id == source`. A `FilterSpec` entry cannot express this, since `matches` has no source. The row keeps `WhileSourceOnBattlefield`, because `source_bound` drives the purge, and the doc says so.
- Conditions: one new primitive, `Game.Conditions.DuringYourTurn`. Everything else is a Lua predicate, so `state_to_lua` gains `active_player`, `phase`, `step`, `turn_number` and a per-card `tapped`. Of the five permanents with a conditional grant, Quicksilver and Final-Word Phantom become buildable. Kianne needs current power, and the snapshot holds base values. Captain Mar-Vell needs a spells-cast-this-turn count. Azula needs an exile link.
- Timed grants: `Game.Ops.GrantTimingPermission` refuses a condition and `own_card_only`, because no printed card needs either. A Timed `own_card_only` row would match nothing once the card moves (CR 400.7).
- Instant-speed equip (Leonin Shikari, Forge Anew): `TimingPermission.spells` becomes `TimingSubject::{Spells(FilterSpec), EquipAbilities}` in the same list, so the purge, expiry and loop-watch code is not duplicated. `activate.rs:129` and `entries.rs:304` read one shared helper. Forge Anew uses `DuringYourTurn` (CR 702.6a, 602.5d).
- Durations: no timing grant reads "for as long as" or "until the source leaves", so all stay refused. `WhileConditionHolds` is covered by the condition field on a static row. The citation is too narrow today: `Permanent` and `UntilSourceLeaves` are CR 611.2a, and `WhileConditionHolds` and `WhileSourceOnBattlefield` are CR 611.2b. `Custom` has no rule. The error string in `timed_grant_duration` and the TODO at `types/player.rs:342` change with the 125 build.
- Zone permission durations go to a new ticket. 67 cards read "for as long as it remains exiled" and 8 read "for as long as you control". A `Permanent` linked row does not work for the exile case, because the card's id changes when it leaves exile (CR 400.7, 607.2a).
- Not built here: flash bought with an extra cost (10 cards) needs the additional-cost slot a non-modal card lacks, so it gets a new ticket. Conditions that depend on cast choices (4 cards, CR 601.3b), "next spell" grants (4 cards) and flash through a Zone permission (2 cards) stay in Not yet specified.
- "Activate only as a sorcery" has no engine or Lua support on any ability but Equip. 521 printed cards use it, so it gets a new ticket.
- TODOs in the 125 build: `legality.rs:42` and `:43` go, because expiry and conditions will exist, and the commit message says so. `legality.rs:31` stays, reworded to say no printed card needs it. `legality.rs:30` goes with 126, and the `legality.rs:41` doc changes with it.
- Tests for 125: `can_cast_now` units, one `timing_permissions.rs` integration test per condition kind, a Lua round trip and an Illusion Spinners scenario. No proof-deck card unless the replay pin moves. The fingerprint hashes `timing_permissions` through Debug and the state a condition reads, so no `ledger_reads` change is needed.

Builds are 125 then 126. New grilling tickets are 127, 128 and 129.
