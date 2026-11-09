---
id: 145
title: Build a source of your choice
type: task
mode: HITL
status: open
blocked_by: [144]
claimed: false
---

## Question

Let a player choose a damage source at resolution (CR 609.7a) and have a shield watch it, as "Damage redirected to another target" decided.

## Notes

- Read `closed/103-damage-redirect-to-another-target.md`.
- `Game.ChooseDamageSource()` is a script choice with its own candidates: permanents, spells on the stack and face-up command zone objects. It does not run `target_is_legal`. A source need not be able to deal damage (CR 609.7a).
- The chosen object goes on the `DamageWatch` source. The choice happens when the effect is created.
- Candidates reach the Seat through the existing pick Decision. Check that a stack spell and a command zone object can be listed and answered.
- Add `// TODO: a permanent spell chosen as a source covers the permanent it becomes (CR 609.7a)` and `// TODO: an object referred to by a waiting effect as a chosen source (CR 609.7a)`.
- Update `docs/lua/stubs/game.lua`, `docs/lua/targeting-and-choices.md` and `game-constants.md`.
- Tests: Reflect Damage on a permanent and on a spell, a hexproof or protected source staying choosable, and a shield ignoring damage from another source.

## Resolution

<Filled in at close.>
