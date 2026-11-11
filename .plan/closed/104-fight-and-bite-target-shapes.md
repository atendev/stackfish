---
id: 104
title: Fight and bite target shapes
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Which target shapes and damage rules do `Game.Ops.Fight` and `Bite` support beyond a two-entry requirement, a `count = 2` entry and a single `count = 1` entry?

## Notes

- Origin: backlog item 10 and `docs/lua/not-available-yet.md` (Fight and bite).
- Gaps to rule on:
  - "Fights up to one target creature" (`up_to` and `at_least` entries) and "fights each creature".
  - Damage equal to something other than power.
  - Last known information when the source left the battlefield (CR 608.2h).
  - Fight or bite as one mode of a multi-mode spell, which depends on ticket 090.
  - A two-word fight on a triggered or activated ability has no test.
  - A scenario TOML `target_requirement` cannot declare two words, which ticket 095 may change.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

Decided in auto mode: the main thread asked the questions, a grilling subagent answered them, and an auditor checked the result.

- Self-source fight and LKI. CR 701.14b governs: if either creature left the battlefield or stopped being a creature, neither deals damage. The current behaviour is correct, and the Fight TODO that asks for LKI (CR 608.2h) misreads the rule, so the build deletes it.
- Bite dealer and LKI. A targeted dealer that left is an illegal target and deals nothing (CR 608.2b). An untargeted source dealer (the one-word shape) that left deals damage equal to its last known power from `StackObject::source_lki` (CR 608.2h). A source still on the battlefield that is no longer a creature has no power and deals nothing.
- Keywords of a departed damage source. `apply_damage_event` reads the source's current keywords, so a departed source's lifelink, deathtouch, wither and infect never apply. This affects every ability whose source left, so it is a damage-pipeline question and not part of this build. The build adds a TODO at `apply_damage_event` and states the gap in the Bite docs.
- `up_to`. A count-1 `up_to` word is supported in the one-word shape ("it fights up to one target creature", Kogla, the Titan Ape) and as the second word of the two-word shape. Zero chosen means no fight or bite, and the ability still resolves. The generic CR 608.2b target recheck covers an illegal first word, with no special rule. `at_least`, `up_to` on the first word and `up_to` with a count above 1 stay unsupported.
- Unsupported shapes. They fail at load or registration with a clear error instead of doing nothing. One shared Rust check (for example `fight_shape_error(spec, mutual) -> Option<String>`) runs from ability registration, spell load and modal mode building, so ticket 114 reuses it. Tests that assert a silent no-op, such as a count-2 Bite, assert the load error. No card script, proof card or scenario fixture uses Fight or Bite, so no card stops loading.
- "Fights each creature" and "each fights a different one" (Ezuri's Predation). These need a pairing choice and no fixture card uses them. The TODO stays and the item goes to Not yet specified.
- Damage equal to something other than power. Fight is power by definition (CR 701.14a). A non-power amount is a dynamic-amount `DealDamage` question, so Bite takes no parameter. The TODO stays and the item goes to Not yet specified.
- Fight or bite as one mode. Rust modal resolution already handles both. "Build Lua modal spells and triggers" gains a note to add one Fight mode test and one Bite mode test and to fix the "neither op can be one mode" line in `not-available-yet.md`.
- Two-word fight on abilities. The build adds Lua tests for an activated ability and a triggered ability, each with `:SetTarget({creature, creature})` and `Game.Ops.Fight`.
- Scenario TOML. This closes the TOML gap without reopening 095: the script array form declares two words, and the TOML schema does not change.
- Glossary. No `CONTEXT.md` change. Fight and bite are comp-rules terms already defined in the rules and `docs/lua`.
- Build is "Build fight and bite target shapes".
