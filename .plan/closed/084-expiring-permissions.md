---
id: 84
title: Expiring timing and zone permissions
type: grilling
mode: HITL
status: closed
blocked_by: [82]
claimed: false
---

## Question

How does an "until end of turn" grant expire, for timing permissions and zone permissions alike (CR 611.2)?

## Notes

- Origin 076 and 080. Both permission lists last only while their source is on the battlefield.
- Weigh a duration field on both structs against a shared expiry list.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

An "until end of turn" grant reuses `EffectDuration`. `TimingPermission` and `ZonePermission` each get a `duration: EffectDuration` field. A static ability installs a permission with `WhileSourceOnBattlefield`, which is today's behaviour. A resolving spell or ability installs a Timed grant through a new `AbilityEffect`, with `UntilEndOfTurn`, `UntilYourNextTurn` or `UntilEndOfCombat`. A Timed grant is independent of its source and survives the source leaving. The existing drains in `engine/turn.rs` expire it. The full answer set is below.

- Storage: a duration field on both structs, not a shared expiry list. The two lists already have separate readers and purgers, and a shared list would need a second lookup on every legality check. `land_play_modifiers` and `cost_modifiers` stay static-only (ticket 081 keeps them purged by source). `ContinuousEffect` is not reused, because it is a layer payload and these are legality rows. Both fields go into the `loop_watch.rs` fingerprint and the `snapshot_diff` list in `testutil/src/atomic.rs`.
- Durations: the three drained variants plus `WhileSourceOnBattlefield` as the default. `UntilSourceLeaves` means the same as the default, so the builders and ops reject it. `WhileConditionHolds`, `Custom` and `Permanent` are rejected, since none has a drain or a condition field (CR 611.2b). Nothing in scope needs them.
- Expiry: in `begin_cleanup_step`, next to the continuous-effect drain, so both expire together (CR 514.2). A shared helper `expire_permissions(state, pred)` in `engine/turn.rs` serves cleanup, combat end and the next-turn site. "Until your next turn" compares the stored `controller` with the new active player. It does not look up the source, so a source that has left cannot strand the grant. This is better than the existing continuous-effect drain, which leaves such an effect inert.
- Source leaving: CR 611.2a gives only a duration, and CR 611.3b ties only static-ability effects to the source. So a Timed grant outlives its source. `retire_object` and the two `attach.rs` sites purge only rows whose duration is `WhileSourceOnBattlefield`. `remap_abilities` drops only those rows too.
- Static versus resolution: a static ability never carries a timed duration. CR 611.3d (play a land or cast a permanent spell) does not change this, because "as long as stated" for a static ability is covered by the source test. Resolution installs through `AbilityEffect::GrantTimingPermission` and `GrantZonePermission`, handled in `engine/resolve/direct_effects.rs`. The row takes `source` from the resolving object and `controller` from the resolving controller, fixed at resolution as `GainControl` does.
- Lua: no `:SetDuration`. `Game.Ops.GrantTimingPermission(builder, duration)` and `Game.Ops.GrantZonePermission(builder, duration)` take a finished builder plus a `Game.Durations.*` tag, decoded through `DurationTag`. `Game.Durations` gains `UntilEndOfCombat` and `UntilYourNextTurn` if the table lacks them. A static `:Register` keeps the default. Docs to update: `docs/LUA_API.md` (the `lua_api_doc_sync` test), `docs/lua/stubs/game.lua`, `docs/lua/player-modifiers.md`, `docs/lua/index.md`, `docs/implementation/35-timing-permissions.md`, `36-zone-permissions.md` and `docs/DESIGN.md`.
- Fingerprint and replay: the duration is hashed through `{:?}`. A static permission differs only by the new field. If the seed 9 replay pin moves, re-pin and bump `BEHAVIOUR_REV` from 7 to 8 in its own commit. If it does not move, the revision stays 7.
- Tests: expiry at cleanup for both kinds, no expiry at end of combat for an `UntilEndOfTurn` grant, `UntilEndOfCombat` and `UntilYourNextTurn` drains, a static row purged on source exit, a Timed grant kept on source exit, the default unchanged, a Lua round trip for both ops, and a fingerprint change. Put them in `tests/permission_expiry.rs`, plus a `commands/tests.rs` purge case.
- Proof card: none. No clean until-end-of-turn grant card fits the deck without an exile-instead rider (Past in Flames and Yawgmoth's Will need ticket 085). Test-only Lua scripts and scenario fixtures cover the path. The proof deck stays unchanged.
- TODOs: remove the expiry TODO at `types/player.rs:330`. Add `// TODO: Permanent, WhileConditionHolds and UntilSourceLeaves durations are rejected for permissions (CR 611.2b)` beside it. Keep the 083 and 085 TODOs, the replacement-effect duration TODO in `turn.rs`, and the `remap_abilities` TODO.
- CONTEXT.md: edit "Timing permission" and "Zone permission" to mention duration, and add "Timed grant" (a permission a resolving effect installs, independent of its source). No ADR.
- Build: ticket 087.
