# Zone permissions

An effect that lets a player play lands or cast spells from another zone (Crucible of Worlds, Future Sight) is a `ZonePermission` in `GameState.zone_permissions`, beside `timing_permissions` and `land_play_modifiers`. The decisions are in `.plan/closed/080-zone-permissions.md` and `.plan/closed/083-library-linked-exile-zone-permissions.md`.

## Shape

- `ZonePermission { source, controller, affected, zone, owners, kind, cards, library_top, linked, reveals_top, duration }` in `types/player.rs`, with `ZonePermissionKind { PlayLand, Cast }`.
- `zone` is `Graveyard`, `Exile`, or `Library` with `library_top`. `owners` matches `card.owner` relative to `controller`, because a graveyard belongs to its owner and exile is shared. `cards` is `FilterSpec::ANY` unless a script calls `:SetFilter`, and a filter with `extra` never matches.
- It is not a field on `TimingPermission`. Timing answers when and zone answers where, and an effect that grants both registers both.

## Library top and linked exile

- `library_top` matches a card only when `is_top_of_library` says it is the first card of its owner's library (`types/player.rs`). The top is never stored, so a new top opens as soon as the old one is cast or played, and a card cast from the top is already on the stack when the next one opens (CR 601.2a).
- `linked` matches a card only when `GameState.exiled_with` holds `LinkedExile { source, exiled }` for this permission's source (CR 607.2a). `Game.Ops.ExileLinked` writes the record while the ability's source is on the battlefield. `exiled_with` is separate from `linked_exiles`, which `return_linked_exiles` drains.
- `retire_object` and `remap_abilities` drop a record when its source or its card moves. The cast and land commands move a card with `remap_abilities` alone, so the purge has to sit in both.
- `linked` is an `ExileLink`: `None`, `Source` (the `exiled_with` rule above) or `Cards(ids)`. A `Cards` row covers exactly its ids and ignores `owners`, so a `Source` or `None` row is unchanged (CR 400.7j).
- `Game.Ops.ExileTopLinked(n)` exiles through `ScriptedZoneMoveKind::ExileLinked`, whose commit pushes each card that landed in exile onto `GameState.exiled_this_resolution`. A grant built with `:SetExiledByThisEffect()` copies that list when it resolves (`resolve/direct_effects.rs`). The list is cleared when a resolution starts and when `StackObjectResolved` or `Fizzled` commits, so it is empty whenever a player has priority. A card a replacement sends elsewhere never reaches it.
- `prune_linked_cards` (`commands/attach.rs`) drops a card id from every `Cards` row in both purges, so a card that is played, cast or leaves exile is no longer covered.
- Both predicates refuse a `face_down` card outside the hand and command zone. Nothing makes a card face down yet, so a face-down linked card is never offered (CR 601.3f).
- `has_zone_permission` takes the `GameState`, since it reads the library order and `exiled_with`.

## Seeing the top

- `library_top_visible_to(state, card, viewer)` is true when a `library_top` permission covers the card (its `owners` and `cards` filters included) and either sets `reveals_top` or affects the viewer. `None` is a spectator, who sees only a revealed top.
- The host reads it in `view.rs`. A Seat's snapshot shows the one top card of a library it may see, and an opaque Decision keeps that id. The chooser's full Decision already sends the identity of every hidden card it offers.
- Entries are built for the acting player, so an opponent's list never holds the top unless a permission affects that opponent.
- Events are unchanged (005): a card cast or played from the top shows an opponent only its new id, even when the top was revealed.

## Two predicates

- `can_cast_from_zone(state, player, card)` and `can_play_land_from_zone(state, player, card)` in `commands/legality.rs` cover the actor's own hand, the CR 903.8 commander case (cast only) and a matching permission.
- `validate_cast_spell` checks `can_cast_from_zone` when no alternative cost is chosen. With one chosen (flashback, the cascade free cast) it checks that cost's `from_zone`, as before. `cast_entries` lists `normal` from the same predicate, so a graveyard card with flashback under a cast permission lists both `normal` and the flashback index.
- `validate_play_land` checks `can_play_land_from_zone`. `play_land_entries` lists hand lands first, in hand order, then permitted lands from other zones in id order.
- Timing is unchanged: a cast still passes `can_cast_now`, and a land play still needs `is_sorcery_speed` and an unused land play (CR 305.2a).

## Moving the card

- The cast and the land play remove the card from its real zone. `from_owner` is the card's owner for any non-exile zone (it was the actor before, which was right only for a hand). `LandPlan` carries `from_zone` and `from_owner`, and the land's `ZoneChange` reports its real `from`.
- A card cast or played from another player's graveyard is controlled by the actor (CR 601.2a, 110.2), so both apply paths set `controller` on the new object.

## Leaving

`retire_object` and `remap_abilities` drop a source's static zone permissions (Timed grants stay, see `37-expiring-permissions.md`), as they do timing permissions (CR 611.3b), and the `exiled_with` records of a source or card that moved (CR 607.2a, 400.7).

## Lua

`Game.CreateZonePermission(c)` returns a builder with `:SetFunctionsFrom`, `:SetAffected` (required, no `EnchantedPlayer`), `:SetZone` (required, `"Graveyard"`, `"Exile"` or `"Library"`), `:SetKind` (required, `"PlayLand"` or `"Cast"`), `:SetOwner` (default `Game.Affected.You`, no `EnchantedPlayer`), `:SetFilter(Game.Filter({...}))`, `:SetFromLibraryTop`, `:SetLinked`, `:SetExiledByThisEffect` (grants only, `"Exile"` only, not with `:SetLinked`), `:SetRevealTop` and `:Register`. `:Register` refuses `"Library"` without `:SetFromLibraryTop` and the reverse, `:SetLinked` outside `"Exile"`, and `:SetRevealTop` without `:SetFromLibraryTop`. `Game.Ops.ExileTopLinked(n)` is the library-top form, an impulse draw. `Game.Ops.ExileLinked` is a bare op tag: targeted exile from the battlefield or a graveyard that records the link. The drain in `run_card_script` stamps the source's live controller. Docs: `docs/lua/player-modifiers.md`, the stub in `docs/lua/stubs/game.lua`, `docs/LUA_API.md`.

## Tests

- `legality.rs` unit tests: hand and opponent's hand, the commander owner, and a permission's kind, zone, affected players, owners and filter.
- `commands/tests.rs`: `retire_object` and `remap_abilities` drop the old source's zone permission.
- `zone_permissions.rs`: a graveyard land refused and unlisted without a permission and played with one (the `from` zone, the controller, the land count), the land limit, the opponent's turn, own versus opponent's graveyard, a play-land permission not casting, an instant and a sorcery keeping their timing, flashback plus a permission listing both casts, an exile cast, a graveyard permission not opening exile, and a bounced source taking its permission with it.
- `script_zone_permission.rs`: the Lua round trip, `:SetOwner` and `:SetFilter`, the three required setters, rejected zones, kinds and `EnchantedPlayer`, and function zones.
- `scenarios/permissions/crucible_graveyard_land.toml`: Crucible cast and resolved, then a graveyard land played and a second one refused by the land limit.
- `crates/host/src/view.rs`: every Seat sees both ids and the identity of a land played from a graveyard.
- Library top and linked exile: `legality.rs` unit tests for `is_top_of_library`, `library_top_visible_to` and the face-down skip; `commands/tests.rs` for the `exiled_with` purges; `zone_permissions.rs` for a top land play and cast, only the top being open, the next top opening, an opponent's entries, an opponent's library, a linked cast, a bounced source dropping its links and a face-down card; `exile_linked.rs` for the op recording a link and recording none when the source left first; `script_zone_permission.rs` for the three setters and the `Register` refusals; `scenarios/permissions/linked_exile_cast.toml`; `exile_top_linked.rs` for impulse draw (the three cards, cleanup, the scratch list, a replacement, a player target, a returned card, a short library, the `exiled_with` record and the builder refusals) and `scenarios/permissions/act_on_impulse.toml`; and the `view.rs` tests for the snapshot, the opaque Decision and a played top card's event (no library id for an opponent, revealed or not).

## Proof game

- Crucible of Worlds (id 31) replaces one Grizzly Bear in the red-green deck and is in `SPELLS`.
- No seed from 0 to 29 plays a land from a graveyard (an assertion for it failed on all 30), since basic lands rarely reach one. The scenario fixture covers the play, and the proof game adds no graveyard assertion.
- Future Sight (id 32) replaces one Swamp in the blue-black deck and is in `SPELLS`. Its script registers a play-land and a cast permission over the revealed top of its controller's library.
- `BEHAVIOUR_REV` is 8 and the seed 9 replay pin moved with Future Sight.

## Not built

Face-down linked cards (CR 601.3f), the stand-alone "play with the top card revealed" effect (CR 401.5), links for cards exiled by a replaced source copy (CR 607.2a), an untargeted `ExileLinked`, a face-down or per-player `ExileTopLinked`, a "your next turn" duration for impulse draw (Light Up the Stage), "until end of turn" grants (CR 611.2) each have a TODO. "Exile it instead" riders are built in `38-zone-permission-riders.md`.
