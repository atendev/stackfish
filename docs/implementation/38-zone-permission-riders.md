# Zone permission riders

A zone permission can send the spells cast through it to exile when they leave the stack, as Kess, Dissident Mage does. Flashback's exile uses the same field. The decision is in `.plan/closed/085-zone-permission-riders.md`.

## Shape

- `LeaveStackRider { ExileInstead, ExileInsteadOfGraveyard }` in `types/stack.rs`. `ExileInstead` is flashback's wording and covers any exit. `ExileInsteadOfGraveyard` replaces only a graveyard destination.
- `StackObject.leave_stack_rider: Option<LeaveStackRider>` holds the rider for one cast. It is set at push and never for an ability or a copy, so it outlives the permission's source.
- `StackObject::leave_destination(default)` is the only reader. Resolve (`permanent_entry.rs`), fizzle (`resolve/mod.rs`) and counter (`counter.rs`) call it with `Graveyard`. Buyback is checked first on resolve (CR 702.27a), and a permanent spell's battlefield entry never reads it (CR 608.3).
- `ZonePermission.rider` supplies the value. `AlternativeCost::Replacement.exile_on_leaving_stack` stays the Lua-facing bool and maps to `ExileInstead` in `validate_cast_spell`.

## Which permission applied

- `cast_zone_source` in `commands/legality.rs` returns `Hand`, `Command` or `Permission(rider)`, and `can_cast_from_zone` is its `.is_some()`, so the entry scans did not change.
- `cast_permission_rider` in `types/player.rs` picks among matching `Cast` rows: a rider-less row wins, else the first in list order. `ZonePermission` has no id, so list order is the tiebreak.
- A chosen alternative cost checks its own `from_zone` and takes its own rider, so a flashback cast ignores any permission.

## Lua

- `ZonePermissionBuilder:SetRider("ExileInstead" | "ExileInsteadOfGraveyard")`, optional. `build` refuses it without `SetKind("Cast")`, so a Timed grant through `Game.Ops.GrantZonePermission` follows the same rule.
- Docs: `docs/LUA_API.md`, `docs/lua/player-modifiers.md`, `docs/lua/game-constants.md` and the stub in `docs/lua/stubs/game.lua`.

## Tests

- `zone_permissions.rs`: a rider cast resolving, countered and fizzling into exile, a rider-less cast into the graveyard, a permanent spell onto the battlefield and exiled when countered, flashback beside a rider-less permission, a rider outliving its bounced source, the rider-less permission winning, and buyback returning the spell to hand.
- `script_zone_permission.rs`: both rider names reach the row, and an unknown name or a `PlayLand` permission is refused.
- `types/stack.rs`: `leave_destination` for each rider and default.
- `scenarios/permissions/kess_rider_cast.toml`: Kess starts on the battlefield, a graveyard sorcery is exiled after it resolves, a hand-cast sorcery still reaches the graveyard, and a graveyard creature is refused.

## Open

- No "if it would die or leave exile this way" rider, no battlefield-side exile such as unearth, and no rider on a permanent spell's battlefield entry.
- No player choice between matching permissions (CR 601.2), and no once-per-turn limit, so Kess can be used every time.

## Proof game

No proof card. The seed 9 replay pin did not move, so `BEHAVIOUR_REV` stays 8.
