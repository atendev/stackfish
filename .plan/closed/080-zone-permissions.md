---
id: 80
title: Zone permissions (play or cast from another zone)
type: grilling
mode: HITL
status: closed
blocked_by: [79]
claimed: false
---

## Question

How does an effect let a player play a land or cast a spell from a zone other than hand, such as "play lands from your graveyard", so the commands and `legal_actions` read one rule? Decide whether it extends `TimingPermission` or sits beside it.

## Notes

- Origin 076. It touches `required_zone` in `commands/cast.rs`, `validate_play_land` and the entry scans in `entries.rs`, plus the CR 601.3 and 305.1 rules after the cast.
- `LandPlayModifier`'s `SetFunctionsFrom` names where the source ability functions, not where the played card comes from, so it is not the precedent.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

An effect that lets a player play a land or cast a spell from another zone becomes a Zone permission. It is a new list, `GameState.zone_permissions`, beside `timing_permissions` and `land_play_modifiers`. It is not a field on `TimingPermission`, because timing answers when and zone answers where, and one permission serves both land plays and casts. Each row names the source, who may use it, the zone, whose cards it covers, whether it plays lands or casts spells, and a card filter. Two predicates in `commands/legality.rs`, `can_cast_from_zone` and `can_play_land_from_zone`, replace the hand and commander tests in the cast command, `validate_play_land` and both entry scans, so `legal_actions` and the commands agree. Flashback stays on the alternative cost, and the chosen cost decides which source is checked. The land limit, `can_cast_now` and the sorcery-speed rule are unchanged. A permission is removed when its source leaves, through `retire_object` and `remap_abilities`. Graveyard and exile are in scope. Top of library, "cast from among cards exiled with this", end-of-turn grants and exile-instead riders are follow-ups. Build 082 does the rest.

The full answer set is below.

- Representation: `ZonePermission { source, controller, affected: FilterController, zone: ZoneKind, owners: FilterController, kind: ZonePermissionKind { PlayLand, Cast }, cards: FilterSpec }` in `types/player.rs`, and `has_zone_permission(permissions, kind, player, card)` beside `has_timing_permission`. `owners` matches `card.owner` relative to `controller`, because a graveyard is per owner and exile is shared. `cards` defaults to `FilterSpec::ANY`, and a filter with `extra` never matches. The new field is wired into `engine/loop_watch.rs` (destructure and fingerprint), `testutil/src/atomic.rs` (`snapshot_diff` list) and every `GameState` literal.
- Predicate: `pub(in crate::commands) fn can_cast_from_zone(state, player, card)` and `can_play_land_from_zone(state, player, card)` cover hand, the CR 903.8 commander case and permissions. They are read by the zone check in `cast.rs` (the `required_zone` local and `cast_from_command_zone`), `from_owner` in `cast.rs` (owner of any non-exile zone, not the actor), `validate_play_land`, `apply_play_land` (remove from the real zone and report `from` truthfully), `play_land_entries` (hand loop first so entry order holds, then other zones in id order) and `cast_entries`. The TODOs at `entries.rs:5` and in the `can_cast_now` doc block go.
- Alternative costs: flashback and other `Replacement { from_zone }` entries stay on the alternative cost (CR 113.6b, 702.34a). With an alternative chosen, its `from_zone` is checked. Without one, `can_cast_from_zone` is checked. A graveyard card with both lists `normal` and the flashback index. The TODO at `cast.rs:115` stays.
- Scope: graveyard and exile only, with any owner by filter. The builder rejects Library, Hand, Battlefield, Stack and Command. Both zones are public, so 004 and 005 need no change, and a host test pins that an opponent Seat sees a graveyard land play. Top of library and linked exile go to 083.
- Land limit: no change. `apply_play_land` already counts every play (CR 305.2a), and `LandPlayModifier` has no zone field. `SetFunctionsFrom` names where the source ability functions (CR 113.6), not where the card comes from.
- Timing: orthogonal. A cast still passes `can_cast_now` (CR 601.3b, 117.1a), and a land play still needs `is_sorcery_speed` (CR 305.3). An effect that grants both calls both builders.
- Lifetime: purged by source in `retire_object` (CR 611.3b) and dropped in `remap_abilities`, at both `attach.rs` sites that handle timing permissions. Expiring grants go to 084.
- Lua: `Game.CreateZonePermission(c)` returns a builder with `SetFunctionsFrom`, `SetAffected` (required, `EnchantedPlayer` rejected), `SetZone` (required, `Graveyard` or `Exile`), `SetKind` (required, `PlayLand` or `Cast`), `SetOwner` (default You), `SetFilter` and `Register`. Wiring is `script/builders.rs`, `script/globals.rs`, `script/types.rs` (`pending_zone_permissions`) and the drain in `script/runtime.rs`. Docs are `docs/LUA_API.md`, `docs/lua/stubs/game.lua`, `docs/lua/player-modifiers.md`, `docs/lua/index.md`, `docs/lua/game-constants.md`, `docs/DESIGN.md` and a new `docs/implementation/36-zone-permissions.md`.
- After resolution: the permission carries no destination rule, so a spell cast through it follows CR 608.2n. Exile-instead riders go to 085.
- TODOs: top-of-library and linked-exile permissions (CR 401.5), a face-down exiled card offered with no "may look" check (CR 601.3f), no expiry for "until end of turn" grants (CR 611.2), and no "exile it instead" rider (CR 608.2n).
- Tests: `has_zone_permission` and the two predicates as unit tests, the purges in `commands/tests.rs`, `tests/zone_permissions.rs` (graveyard land with and without a permission, the land limit, opponent's graveyard, the `from` zone, instant and sorcery timing, flashback plus a permission, source leaving, an exile permission), `tests/script_zone_permission.rs` and a host view test.
- Proof: Crucible of Worlds (`{3}` artifact) replaces one Grizzly Bear. The replay pin and `BEHAVIOUR_REV` (6 to 7) move in their own commit. If no proof seed plays a graveyard land, a scenario fixture covers it and the proof assertion stays.
- Terms: `CONTEXT.md` adds "Zone permission" and edits "Timing permission". No ADR, since the choice is easy to reverse.
- Build: ticket 082. Follow-ups 083, 084 and 085.
