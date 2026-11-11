---
id: 148
title: Build fight and bite target shapes
type: task
mode: AFK
status: open
blocked_by: [104]
claimed: false
---

## Question

Build what "Fight and bite target shapes" decided: source-bite last known power, count-1 `up_to` words, load errors for unsupported shapes, the missing ability tests and the docs.

## Notes

- Origin 104. The full decision is in `closed/104-fight-and-bite-target-shapes.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- Delete the Fight TODO about LKI in `types/ability.rs`, since CR 701.14b makes the current behaviour correct. Narrow the Fight `up_to`/`at_least` TODO to `at_least`, `up_to` on the first word and `up_to` above 1, and keep "fights each creature" in it.
- One-word Bite whose source left: damage equal to the power in `StackObject::source_lki` (CR 608.2h). A targeted dealer that left still deals nothing (CR 608.2b). Narrow the Bite TODO to what stays missing.
- Add `// TODO: a departed damage source's last known keywords (lifelink, deathtouch, wither, infect) do not apply` at `apply_damage_event` in `commands/zone_ops.rs`, and say so in the Bite row of `docs/lua/game-constants.md`.
- `up_to` on a count-1 word: the one-word shape and the second word of the two-word shape. Zero chosen means no damage, and the ability still resolves. Test zero chosen for the second word.
- One shared shape check, for example `fight_shape_error(spec, mutual) -> Option<String>`, called from ability registration, spell load and modal mode building. `at_least`, `up_to` on the first word, `up_to` with a count above 1 and a count-2 Bite become load errors. Turn tests that assert a silent no-op into load-error tests. Before 104 closed, no card script, proof card or scenario fixture used Fight or Bite. Only engine tests do, so grep again before the change. `tests/modal_multi_word_modes.rs` builds a Fight mode from an empty `TargetSpec::Multiple` and must assert the error instead.
- Tests: an activated ability and a triggered ability with `:SetTarget({creature, creature})` and `Game.Ops.Fight`, Kogla-style "fights up to one target creature", and a source bite whose source left before resolution.
- Rewrite the `Fight` and `Bite` doc comments in `types/ability.rs` to match: a count-2 Bite and the other unsupported shapes are load errors, and a one-word Bite whose source left uses its last known power.
- Call the shape check where the ops are read in `script/globals.rs` and the load path, and where a mode is built, so `engine/resolve/modal.rs` never sees an unsupported Fight or Bite mode. Re-check the Fight and Bite arms in `engine/resolve/modal.rs` and `commands/legality.rs`.
- Keep the TODOs for "fights each creature" and for damage equal to something other than power.
- Docs: in `game-constants.md`, the Fight row's "Any other target shape does nothing" becomes the load error, and the Bite row's "A dealer that left the battlefield or stopped being a creature deals nothing" splits into the targeted and source cases. Also update `targeting-and-choices.md` and `not-available-yet.md`, and check `docs/DESIGN.md`, `docs/LUA_API.md` and `docs/implementation/11-ops.md`. Mark backlog item 10 in `docs/implementation/99-backlog.md` with what the build closed and what stays in Not yet specified. The op signatures do not change, so `docs/lua/stubs/game.lua` should not need edits. Check it anyway.

## Resolution

<Filled in at close.>
