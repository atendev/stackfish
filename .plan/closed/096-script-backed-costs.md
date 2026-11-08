---
id: 96
title: Script-backed costs, or delete invoke_cost
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Does the engine get costs written as Lua functions, or does `ScriptRuntime::invoke_cost` go away? It is a `todo!()` body kept alive by `#[expect(dead_code)]`, and no payment path calls it.

## Notes

- Origin: backlog item 11 and `docs/lua/not-available-yet.md` ("No script-backed costs", "`Game.Costs.Other` doesn't exist"). `Game.Costs.*` covers every structured cost shape the card set needs today.
- Decide whether any real card needs an unstructured cost, and if so what a cost callback may do (check, pay, undo on unwind). Atomicity rules apply: a rejected input changes nothing (ticket 024).
- Deleting is a valid answer. The `todo!()` in `commands/mod.rs` already went away in ticket 035.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

Decided in an auto-mode grilling run. A subagent stood in for the user and took every recommendation. It flagged the ticket's premise as false. The builds are "Delete invoke_cost and the unpayable Other cost" and, for the gap below, the grilling ticket "Cost shapes past the current set".

**No Lua costs.** `ScriptRuntime::invoke_cost` goes away and no callback replaces it, neither a paying one nor a check-only predicate. A Payment Decision needs a candidate list a Seat can answer (030), and a payment has to validate before anything moves (024, CR 118.3, 601.2h). A function gives no candidate list. A mutating one cannot be rejected cleanly, because no command restores state: `Engine::snapshot` only compares. A check-only predicate adds nothing a structured shape lacks. Reopen it only with a concrete card that needs it.

**The premise was false.** `Game.Costs.*` does not cover every cost the card set needs. An oracle-text count over `../stackfish-db/stackfish.db` (34,738 cards) found activation costs with no `Game.Costs.*` form: remove counters 334 cards, exile this card from your graveyard 77, other exile shapes about 190, energy 53, return to hand 44, reveal 27, put a counter 11, put on library or other zones 11, exert 8, mill 7, untap 3. Of 353 spells with additional costs, 86 are "A or B" choices. The answer holds because each is a fixed operation on named objects, which a structured `AdditionalCost` variant can hold. Illuminated Folio's "share a color" is a relation among the chosen objects, and neither a variant's filter nor a per-candidate predicate expresses it.

**Other.** `AdditionalCost::Other`, `CostPayment::Other`, `WireAdditionalCost::Other` and `WirePayment::Other` go too. Nothing constructs `AdditionalCost::Other`, and `PaymentPlan::validate` refuses it, so such a cost can never be paid. `PROTOCOL_VERSION` stays 1. The engine never sends the Decision variant. A Seat that sends `{"kind":"other",...}` inside `AnswerBody.reply` (`WireReply::Payment.plan`, then `WirePlan.payments`) goes from a rejected Answer to `malformed_message`, because `serde_json::from_value` fails at `crates/host/src/session.rs:128`. Both change nothing (024), and no error code is added (007). No test or fixture names the variant.

**Build ticket.** Delete `invoke_cost` with its `#[expect(dead_code)]` and its TODO. Remove the variants at these sites: `crates/engine/src/types/cost.rs` (`AdditionalCost::Other` at line 124, its `accepts` arm at 175, `CostPayment::Other` at 689, the doc comment on `validate` at 752 to 753, the `validate` arm at 978), `crates/engine/src/commands/zone_ops.rs` (the no-op arm and comment at 212 to 216), `crates/host/src/wire/decision.rs` (63, 92) and `crates/host/src/wire/reply.rs` (39, 59). A grep for the four names and `invoke_cost` over `crates/` must come back empty.

The TODO on `invoke_cost` goes with its function. The CLAUDE.md rule to keep every TODO protects a TODO from a pass that rewords comments, and here the code it sits in is deleted because the feature it names is cancelled. The commit body says so. `tools/comment_diff.py` does not apply, because it exists to prove a comment-only pass changed no code, and this change deletes code.

**Docs.** Rewrite `docs/lua/not-available-yet.md` lines 28 and 73 and `docs/lua/game-constants.md` line 255 to say costs are structured data by design, and point to the new ticket for the gap list. State the current position with no history. Drop the `invoke_cost` clause from item 11 of `docs/implementation/99-backlog.md`. `docs/lua/stubs/game.lua` has no entry for either name, so it needs no edit, and the build's auditor confirms that by grep. `docs/implementation/05-script-bridge.md:170` stays as a slice note. `../stackfish-db/API_GAPS.md` is outside this repo.

**New grilling ticket.** "Cost shapes past the current set" holds the counts above, decides which `AdditionalCost` and `CostSpec` shapes to build and in what order, and covers spell "A or B" additional costs, counts tied to X, and whether a set-level constraint (Illuminated Folio) is worth having. It is open and unblocked. Per-shape build tickets come out of it.
