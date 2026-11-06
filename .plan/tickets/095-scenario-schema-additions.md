---
id: 95
title: Scenario schema assertions and fixture fields
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

Which assertions and fixture fields does the TOML scenario schema gain? 28 of the 32 targeting and layer tests stay in Rust because the schema cannot express them.

## Notes

- Origin: the "Scenario assertions" entry in `99-backlog.md`, the alternative-cost and buyback entry in `docs/lua/not-available-yet.md`, and the "no field for a chain" item from `25-chain-operations.md`.
- Gaps to rule on:
  - `expect_error` with a rejection variant.
  - An exact `damage_marked` assertion, and a `CardSpec` resolution effect with no Lua script.
  - Ledger event kinds present or absent, and a stack object's targets.
  - Effective power, toughness and keywords after layers, and a per-effect timestamp on `continuous_effect`.
  - A mid-scenario state edit that makes a chosen target leave play.
  - `[cards.alternative_costs]`, `buyback_cost`, and chain fields.
- Decide which are worth the schema weight and which tests stay in Rust. Ticket 071 already moved the tests the schema could express.
- Read `docs/scenario_format.md`. Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
