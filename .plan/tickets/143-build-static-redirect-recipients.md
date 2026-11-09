---
id: 143
title: Build static redirect recipients
type: task
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Add recipients beyond "this permanent" to static redirects, and the player or permanent damage filters, as "Damage redirected to another target" decided.

## Notes

- Read `closed/103-damage-redirect-to-another-target.md`.
- A `RedirectRecipient` replaces the bare `Target` on `ReplacementPayload::RedirectDamage`: `Fixed(Target)` and the read-at-damage forms (attached creature, affected object's controller, damage source's controller, damage source). The replacement loop resolves it before `apply`.
- A recipient that cannot be resolved makes `matches` return false (CR 614.9). Zero damage never matches (CR 120.8, 614.7a).
- `Game.ReplacementPayloads.RedirectDamageTo(recipient)` and `Game.DamageRecipients.*`. `RedirectDamageToSource` stays.
- `Game.EventFilters.DamageDealtToPlayer` and `DamageDealtToPermanent` pair with the redirect payloads and `PreventDamage`. `DamageDealt` still means either.
- Correct the Pariah doc and script in `docs/lua/replacement-effects.md` and `crates/engine/tests/redirect_damage.rs`. Update `docs/lua/game-constants.md` and the game stub in `docs/lua/stubs/game.lua`.
- Tests: Pariah sends damage to you to the enchanted creature and leaves damage to your creatures alone, a moved Aura follows its new host, Treacherous Link hits the current controller, a redirect to a gone recipient deals the original damage, and a player who left the game (CR 614.9). Confirm whether damage aimed at an eliminated player can reach the check.
- Add `// TODO: static redirects with a source quality or an untapped condition`.

## Resolution

<Filled in at close.>
