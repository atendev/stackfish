# Zone permissions

An effect that lets a player play lands or cast spells from another zone (Crucible of Worlds) is a `ZonePermission` in `GameState.zone_permissions`, beside `timing_permissions` and `land_play_modifiers`. The decision is in `.plan/closed/080-zone-permissions.md`.

## Shape

- `ZonePermission { source, controller, affected, zone, owners, kind, cards }` in `types/player.rs`, with `ZonePermissionKind { PlayLand, Cast }`.
- `zone` is `Graveyard` or `Exile`. `owners` matches `card.owner` relative to `controller`, because a graveyard belongs to its owner and exile is shared. `cards` is `FilterSpec::ANY` unless a script calls `:SetFilter`, and a filter with `extra` never matches.
- It is not a field on `TimingPermission`. Timing answers when and zone answers where, and an effect that grants both registers both.

## Two predicates

- `can_cast_from_zone(state, player, card)` and `can_play_land_from_zone(state, player, card)` in `commands/legality.rs` cover the actor's own hand, the CR 903.8 commander case (cast only) and a matching permission.
- `validate_cast_spell` checks `can_cast_from_zone` when no alternative cost is chosen. With one chosen (flashback, the cascade free cast) it checks that cost's `from_zone`, as before. `cast_entries` lists `normal` from the same predicate, so a graveyard card with flashback under a cast permission lists both `normal` and the flashback index.
- `validate_play_land` checks `can_play_land_from_zone`. `play_land_entries` lists hand lands first, in hand order, then permitted lands from other zones in id order.
- Timing is unchanged: a cast still passes `can_cast_now`, and a land play still needs `is_sorcery_speed` and an unused land play (CR 305.2a).

## Moving the card

- The cast and the land play remove the card from its real zone. `from_owner` is the card's owner for any non-exile zone (it was the actor before, which was right only for a hand). `LandPlan` carries `from_zone` and `from_owner`, and the land's `ZoneChange` reports its real `from`.
- A card cast or played from another player's graveyard is controlled by the actor (CR 601.2a, 110.2), so both apply paths set `controller` on the new object.

## Leaving

`retire_object` and `remap_abilities` drop a source's zone permissions, as they do timing permissions (CR 611.3b).

## Lua

`Game.CreateZonePermission(c)` returns a builder with `:SetFunctionsFrom`, `:SetAffected` (required, no `EnchantedPlayer`), `:SetZone` (required, `"Graveyard"` or `"Exile"`), `:SetKind` (required, `"PlayLand"` or `"Cast"`), `:SetOwner` (default `Game.Affected.You`, no `EnchantedPlayer`), `:SetFilter(Game.Filter({...}))` and `:Register`. The drain in `run_card_script` stamps the source's live controller. Docs: `docs/lua/player-modifiers.md`, the stub in `docs/lua/stubs/game.lua`, `docs/LUA_API.md`.

## Tests

- `legality.rs` unit tests: hand and opponent's hand, the commander owner, and a permission's kind, zone, affected players, owners and filter.
- `commands/tests.rs`: `retire_object` and `remap_abilities` drop the old source's zone permission.
- `zone_permissions.rs`: a graveyard land refused and unlisted without a permission and played with one (the `from` zone, the controller, the land count), the land limit, the opponent's turn, own versus opponent's graveyard, a play-land permission not casting, an instant and a sorcery keeping their timing, flashback plus a permission listing both casts, an exile cast, a graveyard permission not opening exile, and a bounced source taking its permission with it.
- `script_zone_permission.rs`: the Lua round trip, `:SetOwner` and `:SetFilter`, the three required setters, rejected zones, kinds and `EnchantedPlayer`, and function zones.
- `scenarios/permissions/crucible_graveyard_land.toml`: Crucible cast and resolved, then a graveyard land played and a second one refused by the land limit.
- `crates/host/src/view.rs`: every Seat sees both ids and the identity of a land played from a graveyard.

## Proof game

- Crucible of Worlds (id 31) replaces one Grizzly Bear in the red-green deck and is in `SPELLS`.
- No seed from 0 to 29 plays a land from a graveyard (an assertion for it failed on all 30), since basic lands rarely reach one. The scenario fixture covers the play, and the proof game adds no graveyard assertion.
- `BEHAVIOUR_REV` is 7 and the seed 9 replay pin moved.

## Not built

Top-of-library and linked-exile permissions (CR 401.5), a "may look" check for a face-down exiled card (CR 601.3f), "until end of turn" grants (CR 611.2) and "exile it instead" riders (CR 608.2n) each have a TODO.
