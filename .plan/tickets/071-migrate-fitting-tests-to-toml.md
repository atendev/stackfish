---
id: 071
title: Move the targeting and layer tests the scenario schema can express to TOML
type: task
mode: HITL
status: open
blocked_by: [070]
claimed: false
---

## Question

Move the roughly 20 to 30 Rust tests in `targeting_basics`, `targeting_up_to_n`, `targeting_at_least_n` and `layer_engine_basics` whose setup and assertions the scenario schema already covers into TOML scenarios, and delete the Rust copies.

## Notes

- Optional. Found by 060. Waits on 070 so the moved tests keep useful failure messages.
- Move only tests that end in assertions the schema has (life, zone, tapped, counters and the like). Anything checking effective characteristics or specific errors stays in Rust.

## Resolution

<Filled in at close: the decision and the reason for it.>
