---
id: 032
title: Host rejection text and payment cost filters
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Give host rejections friendlier message text, and add the payment cost filters that 012 left open.

## Notes

- Origin 010 and 012. Both closed tickets name these two items as left open and say nothing more, and `docs/implementation/18-host-session.md` has no detail either. The requirements report must first pin down what each one means.

## Resolution

Decided with the user, built on `phase5/host-rejection-text` (`2db85c4`).

- Rejection text means `Display` impls, not a reworded rule set or a new wire field. `Rejected`, `ActionError` and the 23 choice error enums the facade routes each give one plain sentence, in `engine/error_text.rs`. `invalid()` sends that text, so a Seat gets "the payment does not cover the cost" in place of Debug output. An `IllegalAction` shows its own reason string. The wire format and error codes are unchanged.
- Payment cost filters are closed with no code change. 012's TODO left the four filtered costs empty because the engine listed no eligible objects, and ticket 030 added `candidates` for exactly that. The filter itself stays off the wire, and a host reads the cost from the card text.
- Left open: some `IllegalAction` strings still use engine terms such as "actor" and method names (a TODO in `error_text.rs` and the backlog).
