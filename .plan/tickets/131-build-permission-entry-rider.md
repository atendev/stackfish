---
id: 131
title: Build permission entry rider
type: task
mode: HITL
status: open
blocked_by: [130, 124]
claimed: false
---

## Question

Build the Battlefield rider a permanent gets when it is cast or played through a Zone permission (The Eighth Doctor), as "Riders beyond leaving the stack" decided.

## Notes

- Origin: the Resolution of [closed/099-riders-beyond-leaving-the-stack.md](../closed/099-riders-beyond-leaving-the-stack.md).
- Waits on "Build permission choice and once-per-turn limits", because the card says "once during each of your turns" and `exile_if_leaves` joins option identity.
- `types/player.rs`: `ZonePermission.exile_if_leaves`. `types/stack.rs`: a `StackObject` field that carries it, read only at `permanent_entry`. A land play reads the permission directly.
- `granted_at` is the later of the granting permanent's timestamp and the entry (CR 613.7a). State that reading where it is computed.
- The ability-removal check scans the layer 6 effects applying to the object. Confirm it never runs the full layer computation or calls back into the replacement pass.
- Keep `// TODO: effects that say an object can't have an ability do not stop a granted rider (CR 613.1f)`. Answer the TODO at `engine/resolve/permanent_entry.rs:39`.
- Lua: `:SetExileIfLeaves()` on the zone permission builder, refused on a Timed grant unless a printed card needs it. Update `docs/LUA_API.md`, `docs/lua/player-modifiers.md` and the stub in `docs/lua/stubs/game.lua`.
- Tests: spell and land entry, a countered spell and a copy getting no row, two permissions staying two options, and an ability-removal effect after entry stopping the rider while one before entry does not. The Eighth Doctor is a scenario fixture.
- Follow the CLAUDE.md implementation loop on a `phase5/` branch.

## Resolution

<Filled in at close.>
