# Expiring permissions

A resolving spell or ability can grant a timing or zone permission for a stated duration, such as "you may play lands from your graveyard this turn". The row is a Timed grant in the same `timing_permissions` and `zone_permissions` lists as a static permission. The decision is in `.plan/closed/084-expiring-permissions.md`.

## Shape

- `TimingPermission` and `ZonePermission` each have `duration: EffectDuration`. A static permission from `:Register` carries `WhileSourceOnBattlefield`. A Timed grant carries `UntilEndOfTurn`, `UntilYourNextTurn` or `UntilEndOfCombat`.
- `source_bound()` on each struct is true for `WhileSourceOnBattlefield`. Readers ignore the duration: a row in the list is in force.
- Not a shared expiry list and not `ContinuousEffect`. The two lists already have their own readers and purges, and a continuous effect is a layer payload.

## Installing

- `AbilityEffect::GrantTimingPermission(Box<TimingPermission>)` and `GrantZonePermission(Box<ZonePermission>)` carry the finished row, boxed so `AbilityEffect` keeps its size. `resolve_grant_timing_permission` and `resolve_grant_zone_permission` in `engine/resolve/direct_effects.rs` push it with `source` set to the resolving `top.source` and `controller` to `top.controller`, fixed at resolution as `GainControl` is. No event is written.
- Both variants are targetless, synchronous and modal-resolvable, and `activate.rs` lists them with the other stack effects.

## Expiring

- `expire_permissions(state, expired)` in `engine/turn.rs` drops rows from both lists by duration and stored controller.
- `begin_cleanup_step` expires `UntilEndOfTurn` beside the continuous-effect drain (CR 514.2). The end-of-combat arm expires `UntilEndOfCombat` (CR 511.3). The next-turn arm expires `UntilYourNextTurn` when the stored controller becomes active, with no source lookup, so a grant whose source is gone still ends.
- `retire_object` and `remap_abilities` purge only `source_bound` rows. A Timed grant outlives its source (CR 611.2a, 611.3b).

## Linked grants

`ExileLinked` records links only under a battlefield source, and a link ends when its source moves. A Timed linked grant from a permanent that stays matches its cards. One from a spell matches nothing, which is a TODO on `ZonePermission::linked` (CR 607.2a).

## Lua

- `Game.Ops.GrantTimingPermission(builder, duration)` and `Game.Ops.GrantZonePermission(builder, duration)` take a finished builder and a `Game.Durations.*` tag. They call the builder's `build`, which `:Register` shares, so the same required setters and refusals apply. They never push into the pending static lists.
- `timed_grant_duration` in `script/globals.rs` refuses any duration but the three drained ones (CR 611.2b). `Game.Durations` already had all three.
- The builder's source id is replaced on resolution, so a spell's `s.operation()`, which has no `c`, may pass any id.
- Docs: `docs/LUA_API.md`, `docs/lua/player-modifiers.md`, `docs/lua/game-constants.md`, `docs/lua/index.md` and the stub in `docs/lua/stubs/game.lua`.

## Tests

- `permission_expiry.rs`: a resolved grant takes the spell and its controller, an end-of-turn timing grant survives combat and expires at cleanup, a zone grant expires at cleanup, an end-of-combat grant expires as combat ends, a three-player "until your next turn" grant with no source survives two other turns and ends on its controller's, a static row is not expired by cleanup, a grant from an activated ability taking the permanent as its source, a revealed library top hidden again after cleanup, both linked cases, the Lua round trip for both ops, the refused `Permanent` duration (the only other one `Game.Durations` binds) and missing setter, and a grant op that registers nothing.
- `commands/tests.rs`: `retire_object` and `remap_abilities` keep a Timed row.
- `loop_watch_tests.rs`: the fingerprint sees a duration change.

## Proof game

No proof card. The seed 9 replay pin did not move, so `BEHAVIOUR_REV` stays 8.
