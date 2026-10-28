---
id: 002
title: What a Decision and an Answer look like
type: grilling
mode: HITL
status: closed
blocked_by: [001]
claimed: false
---

## Question

How do the roughly 35 `ProcessState::Awaiting*` pauses and their `submit_*` methods fold into one Decision type and one Answer type? Is it a facade in the engine that routes to the existing methods, or a refactor that replaces them? Where does the Decision type live (engine or `crates/host`)?

## Notes

- Pause variants: `crates/engine/src/engine/process_state.rs` (`ProcessState`, `StepStatus`).
- Answer entry points: `engine/choice_answers.rs`, `library_choices.rs`, `replacement_resume.rs`, `triggers_pipeline.rs`, `pregame.rs`, `payment.rs`, `turn.rs`, `action.rs`.
- Each mechanic slice today adds a pause plus a method. Decide what a future slice must add so hosts never need to learn a new method.
- Decide how a rejected Answer is reported and whether the Decision stays open.

## Resolution

**Facade.** The engine keeps its 29 `submit_*` methods and its typed pause variants. The Session routes each Answer to the matching method. Hosts never call a method directly.

**Decision type.** The engine builds a Decision from its pause state. Each Decision holds:

- An id from a counter inside the Session. It starts at 1, goes up by one for each new Decision, and is never reused. Replay rebuilds the same ids from the same seed.
- The player who must answer.
- A reply shape, from the list below.
- The legal candidates, as an explicit list, for any shape that picks from a set.
- A mechanic label, such as "Devour", for display only. The host does not branch on it.
- `min` and `max` where the shape needs them.

**Reply shapes.** One Decision type covers every pause. Each shape is one kind of reply:

- Pick from candidates, with `min` and `max`. Covers targets, discard, search, and most choice pauses.
- Yes or no. Covers Madness, Cascade, unless-paid, the mulligan keep or mulligan choice, and `MayChoose`.
- Order. Covers trigger ordering.
- Modes, as indices between `min` and `max`.
- Partition. Covers Scry and Surveil: an ordered list of cards to keep on top, and the rest sent to a named destination.
- Payment. The host builds a `PaymentPlan`, and the engine validates it. This is the one shape that does not use an explicit candidate list.
- Targets. Trigger targets and similar replies. The reply contents depend on ticket 005.
- Action. The priority decision. `PlayerAction` is its reply. Pregame uses the same type.

**Answer type.** A tagged enum with one variant per reply shape. Every Answer carries the id of the Decision it answers. The Session rejects an Answer that names a stale id with `stale_decision`. It rejects an Answer whose variant does not match the open shape with `wrong_answer_shape`.

**Rejected Answers.** Every rejection returns a code and a readable message. The Decision stays open, and no events are emitted, as settled in ticket 001. The codes are `wrong_seat`, `not_open`, `invalid_choice`, `stale_decision`, and `wrong_answer_shape`. The engine's 26 typed error enums map to these codes at the facade. They stay typed internally.

**Choice keywords.** A new mechanic that asks a question mid-resolution adds an engine pause, its resume logic, and its error mapping. If the question fits an existing shape, it adds no host code and no host message. Keywords without a choice, such as flying, add no pause at all.

**Refactor.** No full internal refactor before the protocol ships. Revisit the internal split only when a mechanic needs a reply shape with no fit, or when two pauses differ only by data.

**Deferred to 005.** The Order and Target replies. Today `submit_trigger_order` takes full `PendingTrigger` values, and the Order reply should carry identifiers instead. Ticket 005 decides the identifier type.

Handed to other tickets:

- 003: the contents of a priority Decision, including whether it lists legal actions.
- 005: the identifier type for candidates, plus the Order and Target replies.
- 007: how shapes, codes, the Decision id, and the status values are encoded on the wire.

