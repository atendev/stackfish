# Requirements: Build player hexproof and shroud

Ticket: `.plan/tickets/141-build-player-hexproof-and-shroud.md`. Decision: `.plan/closed/101-ward-costs-and-player-hexproof.md` ("Player hexproof and shroud").

## What success looks like

A card script can say "You have hexproof" (Leyline of Sanctity), "You have shroud" (True Believer), "Each opponent has hexproof" or "Enchanted player has hexproof", and targeting obeys it at cast, activation, the trigger target scan and the resolution recheck.

### Lua surface

```lua
local m = Game.CreatePlayerKeywordModifier(c)
m:SetAffected(Game.Affected.You)   -- .You, .Opponent, .Any or .EnchantedPlayer
m:SetKeyword("Hexproof")           -- "Hexproof" or "Shroud"
m:Register()
```

- `Game.CreatePlayerKeywordModifier(c)` returns a `PlayerKeywordModifierBuilder` with `:SetFunctionsFrom`, `:SetAffected`, `:SetKeyword` and `:Register`, in the shape of `HandSizeModifierBuilder`.
- `:SetAffected` reuses `decode_player_affected`, so an object tag (`Self`, `Enchanted`, `EnchantedPlayerControls`) is a load error.
- `:SetKeyword` takes the same names `AddKeyword` uses. Only `"Hexproof"` and `"Shroud"` are accepted. Any other string is an error that names it and lists the two allowed.
- `:Register()` without `:SetAffected` or `:SetKeyword` is an error, as for the other player modifiers.
- "You and permanents you control have hexproof" is two registrations: this modifier plus a static `AddKeyword("Hexproof")` on `Game.Affected` for your permanents. No combined `Affected` value.

### Rust model

- `types::player::PlayerKeyword { Hexproof, Shroud }`.
- `types::player::PlayerKeywordModifier { source, controller, affected, keyword, timestamp }`, stored in `GameState.player_keyword_modifiers: Vec<PlayerKeywordModifier>`.
- The affected type is shared. `HandRevealAffected`'s doc says to extract a shared type when a third mechanism needs the `Filter`/`EnchantedPlayer` shape, and this is the third. A new `PlayerAffected { Filter(FilterController), EnchantedPlayer }` with one `applies_to(state, source, controller, player)` matcher replaces the two copies of the match. `HandSizeAffected` and `HandRevealAffected` become type aliases of it, so existing code and tests keep their names.
- The drain in `script/runtime.rs` stamps the controller and timestamp as for hand reveal. Both `retain` sites in `commands/attach.rs` purge rows keyed to a leaving source. `loop_watch` fingerprints the new list. Every `GameState` literal gets the new field.

### Enforcement

`player_blocked_by_hexproof_or_shroud(state, script_runtime, player, caster) -> bool`, next to `target_blocked_by_hexproof_or_shroud` in `commands/legality.rs`. `caster` keeps the name `target_is_legal` already uses. It is the controller of the spell or ability that targets: the caster of a spell, the activator of an activated ability, and for a triggered ability its controller when it is put on the stack (CR 603.3d).

- Shroud on the player blocks every caster, the player included (CR 702.18a).
- Hexproof blocks only when `caster != player` (CR 702.11c).
- `You` and `Opponent` resolve against the source's live controller (`layers::effective_controller`), not the drain-time stamp, so Leyline of Sanctity follows a control change (CR 109.5). `EnchantedPlayer` reads the source's `attached_to_player` (CR 303.4m).

Call sites in `target_is_legal_impl`, each gated on `enforce_targetability`:

1. `TargetFilter::Player`, after the controller check.
2. `TargetFilter::PlayerScript`, before the script runs.
3. `TargetFilter::AnyTarget`, the `Target::Player` branch.

`attachment_target_is_legal` passes `enforce_targetability = false`, so the CR 704.5m recheck of an Aura attached to a player and the CR 303.4f entry scan ignore player hexproof and shroud (CR 115.1b). An Aura spell cast at a player is targeted and is blocked.

The player TODO above `target_blocked_by_hexproof_or_shroud` (`crates/engine/src/commands/legality.rs`) is removed. The header line `// TODO: hexproof and shroud on players (CR 702.11c)` in `crates/engine/tests/hexproof_shroud.rs` is removed too.

## Comp rules

- CR 702.11c: hexproof on a player stops spells and abilities that player's opponents control.
- CR 702.18a: shroud on a player stops all spells and abilities, the player's own included.
- CR 702.18b, 702.11h: multiple instances are redundant. Any applicable modifier is enough, so the check is an OR.
- CR 115.1b: an Aura spell targets, an Aura permanent does not.
- CR 601.2c, 602.2b, 603.3d: targets are chosen at cast, activation and when a trigger is put on the stack.
- CR 608.2b, 608.3b: a resolving spell rechecks its target. An Aura spell whose only target became illegal does not resolve and goes to the graveyard.
- CR 303.4m: "enchanted player".
- CR 109.5: "you" is the source's controller.
- CR 113.6b: `:SetFunctionsFrom` for a modifier that works from another zone.
- CR 704.5m: the SBA that removes an Aura attached to an illegal player.
- CR 702.16b: player protection, not modeled here.

## TODOs kept or added

- `// TODO: no team model, so "opponent" means any other player` stays above `target_blocked_by_hexproof_or_shroud` and is repeated above the player function.
- "hexproof from" and "as though it didn't have hexproof" TODOs stay. They belong to "Build hexproof from".
- At the `PlayerScript` arm, the plain comment saying player protection is not modeled becomes `// TODO: protection on players (CR 702.16b)`. The `Player` and `AnyTarget` player branches get the same TODO.
- No new TODO for hexproof on a player through `AddKeyword`: players have no characteristics, so this builder is the one path.

## Tests

New file `crates/engine/tests/player_hexproof_shroud.rs`:

1. Loading: a script registers `Game.CreatePlayerKeywordModifier` and the row lands in `player_keyword_modifiers` with the live controller, the affected value and the keyword.
2. Load errors: an unknown keyword, an object tag in `:SetAffected`, and `:Register()` missing either setter.
3. Hexproof on a player (`You`): an opponent's spell cannot target the player (`target_is_legal` false), and the player's own spell can.
4. Shroud on a player: neither the opponent nor the player can target the player.
5. `Opponent` and `Any` affect the right players. Under `Any` hexproof, each player can still target themselves (CR 702.11c).
6. `EnchantedPlayer`: only the player the source Aura is attached to has it.
7. `AnyTarget`: the player branch is blocked by hexproof for an opponent.
8. `PlayerScript`: blocked, and the script predicate is not consulted.
9. Aura spell at a player: casting an enchant-player Aura at an opponent with hexproof is rejected. The CR 608.2b recheck: the Aura spell targets a player, the player then gains hexproof (modifier pushed onto the state) before resolution, and the Aura goes to the graveyard without attaching.
10. Attached Aura stays: an Aura attached to a player who gains shroud is not removed by the CR 704.5m SBA, because the Aura permanent does not target (CR 115.1b).
11. Source leaves the battlefield: the modifier is purged and targeting works again.
12. Control change: Leyline of Sanctity under another player's control protects the new controller (CR 109.5).
13. Activated ability: an opponent's Lua activated ability with a player target cannot target a hexproof player, and the activation is rejected (CR 602.2b).
14. Triggered ability: an opponent's trigger with a player target skips the hexproof player in its candidate scan, so it picks the other legal player or is removed when there is none (CR 603.3d).
15. Redundancy: two sources give the same player hexproof. Removing one leaves the player protected (CR 702.11h, 702.18b).

Docs: `docs/DESIGN.md` (where it lists player modifiers), `docs/lua/stubs/game.lua` (builder class and `Game.CreatePlayerKeywordModifier`), `docs/lua/player-modifiers.md` (new section and the `EnchantedPlayer` row), `docs/lua/game-constants.md` (constructor list), `docs/lua/not-available-yet.md` (drop the player half of the hexproof bullet).
