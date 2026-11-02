---
id: 030
title: Bot coverage
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Teach the proof-game bot to accept `may_cast` Decisions and to pay the cost kinds it cannot pay yet.

## Notes

- Origin 014.
- The bot declines `may_cast` Decisions.
- It cannot pay sacrifice, discard, tap or graveyard-exile costs, or hybrid, Phyrexian, snow and `{X}` costs.

## Resolution

Built on `phase5/bot-coverage`. Both gaps were protocol gaps, so the fix starts in the engine Decision.

- Payment Decisions list, per additional cost, the ids a sacrifice, discard, tap-permanents or graveyard-exile cost may name. `AdditionalCost::accepts` is the one rule shared with `PaymentPlan::validate`.
- `may_cast` Decisions carry `options` (mode range, target slots, X flag). A free cast now rejects X other than 0 (CR 107.3b).
- The bot pays the four object costs from the candidates and casts from `options` or declines. `cost_proof_game.rs` proves the costs over bytes with no rejected Answer.
- Hybrid, snow, `{C}` and `{X}` were already paid. Phyrexian stays partly open: the bot keeps 1 life and assumes 20 starting life (`bot.rs` TODO).
- No byte-level game offers `may_cast`, because madness and cascade have no Lua constructor. Engine and bot unit tests cover it.

See `docs/implementation/30-payment-candidates.md`.
