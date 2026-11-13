---
id: 147
title: Build stun counters
type: task
mode: HITL
status: open
blocked_by: []
claimed: true
---

## Question

Build stun counters (CR 122.1d) as [closed/102-stun-counters-and-tap-hooks.md](../closed/102-stun-counters-and-tap-hooks.md) decided, so a stunned permanent's untap is replaced by removing a stun counter.

## Notes

- Follow the Resolution of 102 for the helper, the stun count read, the TODOs, the tests and the docs.
- One `Engine` method handles every untap attempt. `untap_step` and the untap side of `flip_tapped` call it.
- Add `CounterKind::Stun` to every match: `script/decode.rs`, `script/globals.rs`, `scenario/build.rs` and `host/src/wire/event.rs`.
- This is a Lua API change, so check `docs/lua/stubs/game.lua`.

## Resolution

<Filled in at close: the decision and the reason for it.>
