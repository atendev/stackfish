---
id: 160
title: Build activation sorcery speed
type: task
mode: HITL
status: open
blocked_by: [126]
claimed: false
---

## Question

Build "Activate only as a sorcery" on any activated ability, as "Activate only as a sorcery on any ability" decided.

## Notes

- Read `.plan/closed/127-activate-only-as-a-sorcery.md` for the decisions.
- `types/ability.rs`: add `ActivationRestriction { sorcery_speed: bool }` with `Default`, and `activation` on `RegisteredAbility`. Edit all literal sites (15 in `crates/*/src`, the rest in `crates/*/tests`). The `condition` field comes with 161.
- `script/builders.rs`: `:SetSorcerySpeed()` on `ActivatedAbilityBuilder`. `Register` sets the flag when `effect == Some(AbilityEffect::Attach)`.
- `commands/legality.rs`: `can_activate_now`, called by the `ActivateAbility` arm and the `activate_entries` scan after the zone check. It replaces the `matches!(Attach)` checks at `activate.rs:129` and `entries.rs:304` and reads the `TimingSubject::EquipAbilities` exemption from 126. Rewrite the `AbilityEffect::Attach` doc paragraph.
- Add `// TODO: an EquipAbilities timing permission also covers Fortify CR 702.67a`.
- A granted or copied ability takes the flag from its own definition (CR 602.5c).
- Tests: rejected outside a main phase, with a non-empty stack and on an opponent's turn; the entries scan omits it; Equip and Fortify stay sorcery speed; an instant-speed equip permission still works and does not exempt a non-equip ability; a sorcery-speed mana ability is not offered while paying for an instant-speed cast; a graveyard ability with `:SetFunctionsFrom` is rejected on an opponent's turn; a grant copy keeps its own value (CR 602.5c); a control change keeps the restriction (CR 602.5b); a Lua round trip. CR 602.5d.
- Lua API change: update `docs/lua/index.md`, `docs/LUA_API.md`, `docs/lua/activated-abilities.md` (document the Attach default) and the stub in `docs/lua/stubs/game.lua`.
- `BEHAVIOUR_REV` should not move. Run the replay tests to confirm.
- Follow the CLAUDE.md implementation loop on a `phase5/` branch.

## Resolution

<Filled in at close.>
