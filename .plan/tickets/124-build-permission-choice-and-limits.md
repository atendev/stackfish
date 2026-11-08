---
id: 124
title: Build permission choice and once-per-turn limits
type: task
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Build the player's choice between matching zone permissions and the once-per-turn limit, as "Player choice between matching permissions and once-per-turn limits" decided.

## Notes

- Origin: the Resolution of [closed/097-permission-choice-and-limits.md](../closed/097-permission-choice-and-limits.md), which holds the rules for every part below.
- Open builds 117 and 119 also change `ZonePermission`, so run these one at a time.
- `types/player.rs`: add `once_per_turn` and `your_turns_only` to `ZonePermission`, add `ZonePermissionUse`, and replace `cast_permission_rider` with a function that returns the option list. An option collapses rows on rider and use key only, and its `source` is the first row's label. Spent and turn-restricted rows are filtered outside `covers`.
- `types/game.rs`: add `zone_permission_uses`. Clear it in `engine/turn.rs` beside `once_per_turn_triggered`. Add it to the `loop_watch.rs` destructure and format string, and to the `testutil` atomicity destructure.
- `commands/legality.rs`, `cast.rs`, `land.rs`, `entries.rs`: `ZoneCastSource::Permission` carries the chosen option. Validate `permission` per the Resolution. Record a land use at apply.
- `types/stack.rs`: the use key on `StackObject` beside `leave_stack_rider`. Record it at the `SpellCast` log in `engine/payment.rs`.
- Internal casts send `None`, take a rider-less option then row order, and spend no use. Keep a TODO naming the missing player choice (CR 601.2).
- Lua: `:SetOncePerTurn(group)` and `:SetYourTurnsOnly()` on the zone permission builder. `Register` refuses an empty group, and a Timed grant refuses `:SetOncePerTurn` with a TODO. Update `docs/LUA_API.md`, `docs/lua/player-modifiers.md` (the Kess example at line 190 and the gap at line 245) and the stub in `docs/lua/stubs/game.lua`.
- Host: `permission` on both wire actions, `permissions` on both wire entries, `PROTOCOL_VERSION` still 1. The bot picks uniformly from two or more options.
- Scenarios: `permission = n` on `cast_spell` and `play_land` in `docs/scenario_format.md` and `crates/scenario`.
- Tests: a required field with two options and rejection when absent, out of range, unneeded, or sent with `alternative_cost`; the rider-less test in `zone_permissions.rs` becomes one that picks; identical rows collapse and two copies of a source do not; a shared group across a land row and a cast row; the limit resets next turn and a new source object has a fresh limit; `your_turns_only` on another player's turn; a cast abandoned under CR 601.6 spends nothing and a countered spell refunds nothing; the next Decision drops a spent option; an internal cast spends no use; the Lua setters and refusals; a Kess scenario that refuses a second cast.
- Proof: two fixture cards with overlapping instant and sorcery cast rows, one with a rider and one limited. The seed loop asserts a two-option entry opens and that the option disappears once spent. Bump `BEHAVIOUR_REV` if the pin moves.
- Remove the TODOs at `commands/legality.rs:82` and `types/player.rs:371` with the code that answers them. Update `38-zone-permission-riders.md` and the Zone permissions entry in `99-backlog.md`.
- Follow the CLAUDE.md implementation loop on a `phase5/` branch.

## Resolution

<Filled in at close.>
