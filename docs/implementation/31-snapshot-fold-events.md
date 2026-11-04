# Snapshot fold events

A Seat's first snapshot folded with every event it receives equals its latest snapshot: zones, hand and library counts, tapped, turn, phase, step, priority, life and poison. These are the engine events that make it add up.

## Life

- Every life change writes `LifeChanged`, so a Seat's life is its starting life plus the deltas.
- `commands::apply_damage_event` returns the changes it made: `LifeChanged { delta: -n }` for damage to a player without infect (CR 120.3a), `PlayerCounterAdded { kind: Poison }` for infect damage to a player (CR 702.90b), and `LifeChanged { delta: +n }` for lifelink (CR 702.15b). Zero damage returns nothing.
- `CommitEvents` writes those right after the `DamageDealt`, with `parent_event` set to the damage event. They are already applied, so they skip the replacement check.
- `resolve_gain_life_effect` pushes `LifeChanged` like `lose_life_now`. A zero gain pushes nothing.
- A callback returning `Game.Ops.GainLife` changes `GameState` inside the runtime, so the runtime also writes the `LifeChanged` straight to the ledger. It is not a pending kind, so it still mixes with one, for example `Chain(GainLife, DealDamageTo)`.
- No trigger condition matches `LifeChanged` yet (CR 119.9 TODO in `types/ability.rs`).

## Priority

- `Engine::set_priority` sets `priority_player` and returns a `PrioritySet` only on a change. `action.rs` appends it to the command's events, so it comes after `PriorityPassed`. Resolution, the step entry and the cleanup reopen log it directly. The cleanup reopen logs it after the ledger-growth check, so it cannot open an extra window.
- CR 800.4a: when the leaving player holds priority, `sba::apply_player_loss` passes it to `commands::next_active_player` and writes `PrioritySet`.

## Leaving the game

- `sba::remove_players_objects` returns one `LeftGame { object, zone, owner }` per object it retires, before `PlayerEliminated`. A spell is listed once, from the stack zone. An ability on the stack lists its controller as `owner`.
- The host's `left_game` keeps `object` only for a Seat that could see it where it was. Everyone gets `zone` and `owner`, so hand and library counts still fold.
- TODO (in `sba.rs`): control effects that give the leaving player other players' objects do not end, and objects they still control are not exiled.

## Tests

- Engine: `pass_priority_loop.rs` (full ledgers), `keyword_primitives.rs` (lifelink and infect follow-ons), `mill_lose_life_sacrifice_self.rs` (resolved `GainLife`), `script_operation.rs` (callback `GainLife`), `sba_basics.rs` (a three-player CR 800.4a elimination).
- Host: `seat_visibility.rs` folds priority, life and poison, and plays five proof games to the end. `view.rs` checks `left_game` ids per Seat. `bot.rs` checks that tracked life is the sum of `life_changed`.
- Scenario `ledger_len` counts went up by the `PrioritySet` events, and `BEHAVIOUR_REV` is 3.

## Left open

- The wire `v` stays 1. Damage to a player now also writes `life_changed`, so a host that subtracted `damage_dealt` itself would count it twice.
