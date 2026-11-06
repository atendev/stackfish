---
id: 112
title: Which non-goal cards the loader rejects
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Which cards should `stackfish-carddb` refuse to load, and how? Ticket 105 found 518 planeswalkers, battles and sagas, 38 Class cards and 302 face-down keyword cards that load today and break silently or partly.

## Notes

- Findings: `docs/research/non-goal-card-types.md`, and the Resolution of `closed/105-unsupported-card-types-in-loader.md`.
- Questions to settle:
  - Face-down: reject only the 73 producers (Manifest, Manifest dread, Cloak) and let the 229 morph, megamorph and disguise cards load face up, or reject all 302 as backlog item 12 says.
  - Keyword-less face-down cards (Ixidron and similar) and the 147 face-down exile cards: reject by oracle text or a hand list, or allow for now.
  - Melds (20 besides Urza, Planeswalker) and the other unmodeled layouts the database does not flag as multi-face (mutate, leveler, prototype, case, augment, host): in scope or not.
  - Reject at load with a new `CardLoadError`, or skip with a log for batch callers such as `db-tests`.
  - Keep the `PLANESWALKER` and `BATTLE` type bits and check in `build_definition`, or stop mapping them in `types_line.rs`.
- Use the `grilling` and `domain-modeling` skills. A build ticket graduates once this closes.

## Resolution

<Filled in at close: the decision and the reason for it.>
