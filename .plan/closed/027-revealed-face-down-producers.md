---
id: 027
title: Revealed and face-down producers, and the event-writing rule
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Make the engine emit `Revealed` and `face_down` where the rules call for them, and write down the rule that no free-text event field carries a hidden identity.

## Notes

- Origin 004 and 013.
- The engine never emits `Revealed` or `face_down`.
- The rule "never put a hidden identity in a free-text event field" goes into the engine's event-writing guidelines. It covers `ScriptError`, `StateBasedActionApplied` and `RngDrawn`.

## Resolution

Built on `phase5/revealed-face-down-producers`.

- `Game.Ops.Search` takes `reveal` (default false). When set, `submit_search_choice` logs one `Revealed` from the library before the found cards move (CR 701.23e). Both amplify answers log a `Revealed` from the hand (CR 702.38a). `Engine::reveal_event` builds all three.
- `face_down` gets no producer. Morph, manifest, disguise and face-down exile are design non-goals, so the field carries a TODO (CR 708.2).
- The free-text rule is in `DESIGN.md`, "Writing events", and on the three event fields. Every `ScriptError` message comes from `GameState::script_error_message`, which leaves out the id and Lua detail of a library or hand source. This closed a real leak: at game start a broken script on a library card sent its id and error text to every Seat.
- Reason: search and amplify are the only reveals the engine models today, and free text cannot be filtered per Seat, so it must not carry hidden cards at all.
- Left open: other reveal effects (TODO on `Revealed`), revealed cards staying revealed until their effect completes, and an end-to-end test with a broken script on a hand card.
