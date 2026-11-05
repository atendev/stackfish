# Timing permissions

An effect that lets a player cast spells as though they had flash (Vedalken Orrery) is a `TimingPermission` in `GameState.timing_permissions`, beside `land_play_modifiers` and `cost_modifiers`.

## Shape

- `TimingPermission { source, controller, affected: FilterController, spells: FilterSpec }` in `types/player.rs`. `spells` is `FilterSpec::ANY` unless a script calls `:SetFilter`. A filter with `extra` never matches.
- `affected` is a plain `FilterController`, not a one-variant enum like `LandPlayAffected`, because `reject_enchanted_player` already returns one and nothing needs another form.
- FLASH is not granted through the layers. CR 601.3b makes it a permission, and the layers do not filter by zone, so a granted keyword would reach every keyword reader.

## One predicate

- `can_cast_now(state, player, card)` in `commands/legality.rs` serves `validate_cast_spell` and `cast_entries`, so `legal_actions` lists a cast exactly when the command accepts it.
- An instant, a Flash card or a card a permission covers needs priority only. Anything else needs `is_sorcery_speed`.
- The permission's subject is the caster `player`. `card.controller` is used only as the spell's controller inside the spell filter. For a card in hand the two are the same. For a commander its owner casts from the command zone they can differ, so the caster is the right subject. This refines 076, which said `card.controller`.
- A cast from another zone through an alternative cost (flashback) is covered. Both call sites already require `object.controller == player` for one.
- The rejection messages did not change: "actor does not hold priority" is still only for an instant or Flash card. In practice the engine answers `NotYourTurn` first when the actor lacks priority.
- Casts during resolution (cascade, `may_cast`) and the madness `pending_timing_bypass` never read timing (CR 608.2g), so they skip the predicate.
- Land plays and Equip keep `is_sorcery_speed`.

## Leaving

- `retire_object` drops a source's permissions, which covers every move through `commit_zone_move` (a bounce, a death).
- Cast, land play, payment, library choices and pregame move an object by hand and call `remap_abilities`, which also drops permissions keyed to the old id. A script re-registers one for the new zone if it functions there.
- The cost modifier and player modifier lists are not purged in `remap_abilities`. That is a TODO there.

## Lua

`Game.CreateTimingPermission(c)` returns a builder with `:SetFunctionsFrom`, `:SetAffected` (required, no `EnchantedPlayer`), `:SetFilter(Game.Filter({...}))` (optional) and `:Register`. The drain in `run_card_script` stamps the source's live controller. Docs: `docs/lua/player-modifiers.md`, the stub in `docs/lua/stubs/game.lua`, `docs/LUA_API.md`.

## Tests

- `legality.rs` unit tests for `can_cast_now`: own flash, no permission, affected You/Opponent/Any, spell filter and `extra`, no priority, the commander caster.
- `commands/tests.rs`: `retire_object` and `remap_abilities` drop the old source's permission.
- `timing_permissions.rs`: an off-turn sorcery refused and unlisted without a permission, cast and listed with one, Opponent and You affected, the spell filter, flashback from the graveyard, a land still refused, a hand-sourced permission dropped when its source is cast, and a bounced Orrery taking its permission with it.
- `script_timing_permission.rs`: the Lua round trip, `:SetFilter`, the required `:SetAffected`, the rejected `EnchantedPlayer` and plain table, and function zones.

## Proof game

- Vedalken Orrery (id 30) replaces one Wind Drake in the blue-black deck.
- `the_same_seeds_give_a_byte_identical_game` runs seeds 0 to 19, since Attach on entry first opens at seed 19.
- `BEHAVIOUR_REV` is 6 and the seed 9 replay pin moved.

## `token_ceased` gained a zone and owner

- With the Orrery in the deck, seed 3 of `a_proof_game_folds_to_each_player_s_snapshot` bounced a Soldier token to its owner's hand, where it ceased to exist (CR 704.5d). `token_ceased` carried only the id, and an opponent never received it, so no Seat could fold the hand count.
- `GameEventKind::TokenCeased` has `zone` and `owner`, and the wire event follows the `left_game` rule: every Seat gets it, and the id goes only to a Seat that could see the object where it was. The fold test decrements the count from `zone` and `owner`.

## Not built

Zone permissions, "until end of turn" grants, conditional flash (CR 601.3d), "as though it were a main phase" and instant-speed equip each have a TODO.
