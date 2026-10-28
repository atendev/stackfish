---
id: 011
title: Build B2 Engine::legal_actions()
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Build `Engine::legal_actions()`, which builds the entries for priority, combat and damage assignment Decisions.

## Notes

- Branch: `phase5/legal-actions`. Follow the CLAUDE.md slice workflow: requirements report first, approval before every commit, closing report, user merges.
- Decisions: 003 (priority Decision lists playable entries with announcement options: mode range, target slots with candidates, X flag, alternative costs, buyback, mana ability flag; no affordability check; payment is a separate Decision with a "cannot pay" Answer; combat Decisions list options the same way) and 005 (object identity).
- Check `docs/comp_rules.md` for casting and combat legality.
- Mark any stubbed parts with TODO.

## Resolution

Built on `phase5/legal-actions`, merged to master as 272891e (commits 52fd374 and 1087d52). Not pushed yet.

- `Engine::legal_actions()` lists the priority player's land plays, casts, activations and pass (always last). `legal_attackers()`, `legal_blockers(player)` and `pending_damage_splits()` list the combat options. Builders live in `commands/entries.rs` and `commands/combat/entries.rs`, types in `decision/entry.rs`. No entry checks affordability. See `docs/implementation/17-legal-actions.md`.
- Combat Decisions (the user chose the typed shapes): `open_decision` offers `DeclareAttackers`, `DeclareBlockers` (one defending player at a time) and `AssignDamage` ahead of priority, each with its own `Reply` variant. An empty declaration is allowed.
- `Engine::answer` rejects a priority, attack, block or damage Answer that no entry offers, with `invalid_choice`. This closes the B1 gap where `submit_action` accepted illegal priority actions.
- Deviations from 003: a cast entry has a `normal` flag, and `Reply::Action` covers pass, land, cast and activate only.
- Left for later: whole-declaration rules (menace, must attack, mode combinations) still fail during `step()` and need the engine atomicity item. Target candidates ignore X. `DeclareBands` is not offered. Timing grants from continuous effects are missing.
- Tests: 19 in `tests/phase5_legal_actions.rs`. Workspace: 1913 passed, 0 failed.
