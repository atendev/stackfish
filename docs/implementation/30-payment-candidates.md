# 30 Payment candidates and may_cast options

Ticket 030 (Bot coverage). The bot could not pay object costs or accept a madness or cascade cast, because the Decisions did not list what a reply may name. Both Decisions now do.

## Engine

- `ReplyShape::Payment { cost, eligible }`. `eligible` has one list per `cost.additional` entry. `Sacrifice`, `Discard`, `TapPermanents` and `ExileFromGraveyard` list matching ids in ascending order. Every other kind gets an empty list. Both Payment routes fill it: `AwaitingPayment` and `AwaitingUnlessPaidChoice` (for the payer at the front of the queue).
- `AdditionalCost::accepts` (`types/cost.rs`) is the per-object rule. `PaymentPlan::validate` calls it for the four filtered costs, and `AdditionalCost::eligible` lists with it, so the two cannot drift. `every_listed_object_pays_and_no_other_object_does` pins this. Validation behaviour is unchanged: `TapPermanents` checks controller only through its filter, and nothing stops one object paying two different costs.
- `ReplyShape::MayCast { card, options }` with `CastOptions { modes, targets, x }`. `commands::cast_options` builds it from the same `cast_modes_and_targets` as a priority `CastEntry`. `options` is `None` with no card or no legal announcement.
- `x` is true only for a free cast (cascade, or Lua `CastSpell { free = true }`) of a card with `{X}`. `cast.rs` now rejects a free cast whose X is not 0 (CR 107.3b), before any mutation. Madness X stays under the `cast.rs` alternative-cost TODO.

## Host

- The wire `sacrifice`, `discard`, `tap_permanents` and `exile_from_graveyard` costs carry `candidates` (decimal-string ids). `may_cast` carries `options`, skipped when `None`. Non-choosers still see `Opaque`.
- `illegal.rs` adds a payment that swaps one named object for an unlisted id.
- The bot picks the first unused candidates, never the activating source. It casts from `options` 3 times in 4 and falls back to a decline if the cast is rejected.
- `crates/host/tests/cost_proof_game.rs` plays 30 two-player games with a sacrifice outlet, a discard outlet, a tap-two-creatures card and an escape sorcery. Each makes a player lose a different amount of life, so the test can see every cost paid. A typical run sees each one dozens of times with no rejected Answer.

## Open

- No byte-level game offers `may_cast`: madness and cascade have no Lua constructor a fixture card can use (`cascade.rs` explains the `Cast` trigger gap).
- Phyrexian is only partly covered: the bot keeps 1 life and assumes 20 starting life unless `with_starting_life` is set.
