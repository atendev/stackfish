---
id: 026
title: Whole-declaration combat predicates
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Menace is checked only in `validate_declare_blockers`, `legal_blockers` does not know it, and must-attack does not exist. How do menace and must-attack become one predicate shared by the entries and the command validation?

## Notes

- Origin 011. Decided in 024. Branch `phase5/combat-predicates`.
- Decisions taken at the requirements report (user said "go ahead" on the recommendations):
  - Must-attack is a new `KeywordSet` bit.
  - Must-block stays out of scope (TODO).
  - `AttackerEntry` gains `required: bool`, an additive wire field.

## Resolution

Menace and "attacks each combat if able" are one module, `commands/combat/predicate.rs`, called by `validate_declare_attackers`, `validate_declare_blockers`, `validate_pass_priority` and the two entry builders. Must-attack is the `ATTACKS_EACH_COMBAT` keyword bit. `AttackerEntry` has `required`. A menace attacker is listed only when two creatures could block it. Must-block, attack costs and whole-declaration restrictions stay as `TODO(026-followup)`. See `docs/implementation/23-combat-predicates.md`.
