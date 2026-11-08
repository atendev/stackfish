---
id: 127
title: Activate only as a sorcery on any ability
type: grilling
mode: HITL
status: open
blocked_by: []
claimed: false
---

## Question

How does an activated ability other than Equip declare "Activate only as a sorcery" (CR 602.5d), so that `activate.rs` and `entries.rs` read one rule and Lua can set it?

## Notes

- Origin: "Timing permission follow-ups". 521 printed cards use the phrase, by a query of `../stackfish-db/stackfish.db`. No engine field or Lua setter exists.
- `types/ability.rs:480` says the check matches `AbilityEffect::Attach` to avoid a field on about 94 `RegisteredAbility` literals, and to make it a field once a second ability needs the timing. This is that second ability.
- Decide the field's name and type, whether Equip moves onto it, and how it reads the `TimingSubject` from the instant-speed equip build.
- Also weigh "Activate only during your turn" (82 hits by a broader pattern) and whether one timing field covers both.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close.>
