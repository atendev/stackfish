# Real-card curated set

A hand-picked 40-card pool from `../stackfish-db/stackfish.db` used by `crates/host/tests/real_card_bot.rs` to smoke-test the host protocol and card-database loader against real cards. The set is deliberately simple: every card uses only plain colored or generic mana, has no hybrid/Phyrexian/snow/`{X}` symbols, and either loads an existing script (basic lands) or resolves as vanilla until a script is added.

## Decks

### White/Blue

| Count | Card | Oracle ID | Notes |
|-------|------|-------------|-------|
| 8 | Plains | `bc71ebf6-2056-41f7-be35-b2e5c34afa99` | Active land script |
| 8 | Island | `b2c6aa39-2d2a-459c-a555-fb48ba993373` | Active land script |
| 4 | Elite Vanguard | `b3b9a87d-cb95-435c-90b6-037406cab32e` | Vanilla 2/1 |
| 4 | Eager Cadet | `39780477-01ba-40c6-8fe6-2178efb10b92` | Vanilla 1/1 |
| 4 | Merfolk of the Pearl Trident | `218d9277-c179-4de3-9c7f-79b5a6d4fa38` | Vanilla 1/1 |
| 4 | Coral Eel | `10706fd1-7847-4316-be8d-59b56143ce45` | Vanilla 2/1 |
| 4 | Unsummon | `837182db-1bf3-4a2c-bd01-1af9d9873561` | Needs script |
| 4 | Counterspell | `cc187110-1148-4090-bbb8-e205694a39f5` | Needs script |
| 4 | Quick Study | `7bff8d4a-1c7d-48b8-b3e3-737dd6f01823` | Needs script |

### Black/Red

| Count | Card | Oracle ID | Notes |
|-------|------|-------------|-------|
| 8 | Swamp | `56719f6a-1a6c-4c0a-8d21-18f7d7350b68` | Active land script |
| 8 | Mountain | `a3fb7228-e76b-4e96-a40e-20b5fed75685` | Active land script |
| 4 | Muck Rats | `bca13a12-6723-4a5e-8f1b-21646a8b3e7e` | Vanilla 1/1 |
| 4 | Walking Corpse | `fea95888-e16a-4209-9cd4-623f7f4d2f67` | Vanilla 2/2 |
| 4 | Mons's Goblin Raiders | `a37159df-f6d7-4db6-85de-0ea77f425993` | Vanilla 1/1 |
| 4 | Swab Goblin | `343ecf2f-0bbc-4840-8fb3-f5f61b26e76f` | Vanilla 2/2 |
| 4 | Lightning Bolt | `4457ed35-7c10-48c8-9776-456485fdf070` | Needs script |
| 4 | Shock | `a9d288b8-cdc1-4e55-a0c9-d6edfc95e65d` | Needs script |
| 4 | Sovereign's Bite | `13718d93-fe46-4a18-a511-88649ef1c9de` | Needs script |

## Selection rules

- `layout = 'normal'` and `has_multiple_faces = 0`.
- No Plane, Phenomenon, Scheme, Vanguard, Conspiracy, Dungeon, or Class type lines.
- Mana cost contains only `{W}`, `{U}`, `{B}`, `{R}`, `{G}`, `{C}`, and generic numbers. No hybrid `{U/R}`, Phyrexian `{2/R}`, snow `{S}`, or `{X}`.
- Creature power/toughness are numeric (no `*`).
- Non-legendary, so the legend rule does not fire.

## Scripts to add next

These cards have simple, engine-supported effects. Add a row to `card_scripts` with `is_active = 1` and the next `version` for that oracle ID.

### Unsummon
```lua
-- Unsummon -- Oracle ID: 837182db-1bf3-4a2c-bd01-1af9d9873561
local s = {}
function s.target_requirement()
    return { type = "object", card_types = { "Creature" }, count = 1 }
end
function s.operation()
    return Game.Ops.ReturnToHand
end
return s
```

### Counterspell
```lua
-- Counterspell -- Oracle ID: cc187110-1148-4090-bbb8-e205694a39f5
local s = {}
function s.target_requirement()
    return { zone = "Stack", count = 1 }
end
function s.operation()
    return Game.Ops.Counter
end
return s
```

### Quick Study
```lua
-- Quick Study -- Oracle ID: 7bff8d4a-1c7d-48b8-b3e3-737dd6f01823
local s = {}
function s.operation()
    return Game.Ops.DrawCards(2)
end
return s
```

### Lightning Bolt
```lua
-- Lightning Bolt -- Oracle ID: 4457ed35-7c10-48c8-9776-456485fdf070
local s = {}
function s.target_requirement()
    return { type = "any", count = 1 }
end
function s.operation()
    return Game.Ops.Damage(3)
end
return s
```

### Shock
```lua
-- Shock -- Oracle ID: a9d288b8-cdc1-4e55-a0c9-d6edfc95e65d
local s = {}
function s.target_requirement()
    return { type = "any", count = 1 }
end
function s.operation()
    return Game.Ops.Damage(2)
end
return s
```

### Sovereign's Bite
```lua
-- Sovereign's Bite -- Oracle ID: 13718d93-fe46-4a18-a511-88649ef1c9de
local s = {}
function s.target_requirement()
    return { type = "player", count = 1 }
end
function s.operation()
    return Game.Ops.LoseLife(3)
end
return s
```

## Inserting a script

Use the existing land scripts as a template for the header comment. Then insert:

```sql
INSERT INTO card_scripts (oracle_id, version, script_source, is_active)
VALUES ('<oracle_id>', <next_version>, '<script_source>', 1);
```

The loader picks the highest `version` with `is_active = 1`.
