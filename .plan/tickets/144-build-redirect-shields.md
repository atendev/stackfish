---
id: 144
title: Build redirect shields
type: task
mode: HITL
status: open
blocked_by: [143]
claimed: false
---

## Question

Add the list of redirect shields that resolved spells and abilities create, with counts and durations, as "Damage redirected to another target" decided.

## Notes

- Read `closed/103-damage-redirect-to-another-target.md`.
- A new `GameState` list. A row has an id, `DamageWatch`, `RedirectRecipient`, a count (`All`, `NextEvent`, `Budget(n)`), `UntilEndOfTurn` and the creating object, read only for the wire candidate.
- A new `ReplacementCandidate` kind keyed by shield id joins `matching` in `apply_replacements_from`, with arms in `apply_one_replacement_or_protection`, `decision/answer.rs`, `decision/open.rs` and an additive `WireCandidate` arm `{ id, source }`. `PROTOCOL_VERSION` stays.
- A `Budget` smaller than the damage splits the event, and the residual inherits `excluded`. `resolve_replacement_check_state` needs a per-event `excluded`. Check every caller of `apply_replacements` that can see a damage event.
- The shield list has its own copy of the CR 614.9 check. A shield with no valid recipient is not used up (CR 609.7b, as a reading).
- Rows drain in `begin_cleanup_step` (CR 514.2) and end with a watched object (CR 400.7), never with their source (CR 611.2a). The list joins the `loop_watch.rs` fingerprint (CR 104.4b). Close the `engine/turn.rs` TODO only if it no longer applies.
- `Game.Ops.RedirectDamage(opts)` is an ordinary `AbilityEffect` with the `engine/resolve/modal.rs` arm `Regenerate` has. Update `docs/lua/stubs/game.lua`, `game-constants.md`, `replacement-effects.md` and `not-available-yet.md`.
- Queue order picks among simultaneous events until the choice ticket closes, with `// TODO: the shielded player chooses among simultaneous damage events (CR 615.7)`. Also add the snapshot TODO and the creating-object label TODO.
- Tests: two shields from one permanent both apply, a split with a residual, A to B and B to A ends, a CR 616.1 choice against another shield and against protection, a shield surviving its source, end-of-turn expiry, a watched object leaving, a gone recipient not using up the shield, and combat damage keeping its flag and source. `BEHAVIOUR_REV` moves only if the replay pin moves.

## Resolution

<Filled in at close.>
