---
id: 89
title: Lua event visibility for conditions
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Which ledger events does a condition see, and in what table shape? Today `event_to_lua` encodes only zone changes, so a condition that wants a damage, life, tap, untap or mill event reads `nil`.

## Notes

- Origin: backlog item 6 and `docs/lua/not-available-yet.md` ("Only zone changes are visible to a condition", "No becomes tapped condition", "Mill and life loss have no trigger hooks"). This unblocks the most card scripts for the least work.
- `GainLife`, lifelink and damage to a player already write `LifeChanged` (`docs/implementation/31-snapshot-fold-events.md`), so `not-available-yet.md` is stale on that point. Ticket 110 fixes the prose.
- Decide the field names per event kind, how hidden information is kept out (milled cards, CR 701.17), and which `Game.Conditions.*` entries ship with the events.
- Use the `grilling` and `domain-modeling` skills. Code to read first: `crates/engine/src/script/encode.rs`, `script/runtime.rs` near `invoke_condition`, `docs/implementation/10-triggers.md`.
- Ticket 102 (stun counters and tap hooks) waits on this one.

## Resolution

Decided in an auto-mode grilling run. A subagent stood in for the user and took every recommendation. The build is the "Bridge condition events into Lua" ticket.

**Which events a condition sees.** `event_to_lua` bridges `ZoneChange`, `DamageDealt`, `LifeChanged`, `Tapped`, `Untapped`, `CounterAdded`, `PlayerCounterAdded`, `AttackerDeclared`, `BlockerDeclared` and a new `CardsMilled`. All other kinds stay `nil`. Bookkeeping events such as priority, RNG, mana and script errors are not bridged, because no card reads them. The build removes the `_` wildcard from `event_to_lua` and lists every `GameEventKind` variant, so a new variant fails to compile until someone decides whether to bridge it.

**Table shape.** Each event is a flat table. `kind` is the snake_case variant name, and the other fields keep their Rust names. Ids are plain integers.

| `kind` | fields |
|---|---|
| `zone_change` | `object`, `from`, `to` (unchanged) |
| `damage_dealt` | `source`, `target`, `amount`, `is_combat_damage` |
| `life_changed` | `player`, `delta` (signed, never 0) |
| `tapped` / `untapped` | `object` |
| `counter_added` | `object`, `counter`, `amount` |
| `player_counter_added` | `player`, `counter`, `amount` |
| `attacker_declared` | `attacker`, `target` |
| `blocker_declared` | `blocker`, `attacker` |
| `cards_milled` | `player`, `cards` (array of ids) |

- `target` uses the existing target-result table `{kind = "object" | "player", id = n}`. `Target::Ability` is unreachable for both events, because damage is never dealt to an ability (CR 120.1) and nothing attacks one (CR 508.1b). The encoder still writes it as `{kind = "ability", id = n}` and does not panic, since a script-facing path must not crash the engine. The build extends the stub's `TargetResult.kind` to `"object"|"player"|"ability"`, with a note that these events never produce `"ability"` today.
- `counter` holds the name strings `parse_counter_kind_name` accepts. A new inverse function produces them, and a round-trip test keeps the two in step. The field is `counter` because `kind` is already the event discriminator.
- There is one `life_changed` kind, and a condition tests the sign of `delta` (CR 119.3). The build adds a zero guard to the `CostPayment::Life` path, so `delta == 0` is never written. GainLife and LoseLife already skip zero.

**Hidden information.** An event table carries only ids, zone and counter names, players, numbers and flags, never card characteristics. `last_known_info` and `lineage.previous` are never encoded. A logged `ZoneChange` carries the id `commit_zone_move` mints at the move (CR 400.7), not the id the card had before. Ids come from a monotonic counter, so the new id tells a condition only when the move happened, which is public, and nothing about deck-list order. The state snapshot cannot look up a hidden card by id. The build checks that every other logged `ZoneChange` producer also logs the post-move id: `commands/cast.rs`, `commands/land.rs`, `engine/payment.rs`, `sba.rs`, `engine/pregame.rs`, `engine/replacement_resume.rs` and `engine/resolve/tokens.rs`. The audit checked these and found they all do, and a test pins that a drawn card's bridged id is its new hand id. `CardsScried` and `CardsSurveiled` stay `nil`, because their `kept_on_top` lists show the order of library cards to conditions any player controls.

**Mill.** Mill writes no event of its own today, and surveil's graveyard moves have the same library-to-graveyard shape. The build adds `CardsMilled { player, cards }`, written once per player per mill action after the action's last card move. That includes moves held up by replacement pauses, and a `ForEachPlayer` mill writes one per player. `cards` holds the new ids of milled cards that ended up in a public zone, wherever that is, so a card exiled instead still counts (CR 701.17c). No event is written when no card moved, for example from an empty library. If at least one card moved, the event is written even when every card went to a hidden zone. Then `cards` is `{}` and `Mills` still fires, because the player milled and a replacement only changed where the cards went. `cards` can therefore be shorter than the number milled, and the docs say so. Surveil never writes one.
- TODO: a milled card replaced into a hidden zone is left out of `cards`.
- TODO: mill as a cost.

**Conditions that ship.** Each is a bare value at `Game.Conditions.<Tag>`, such as `Game.Conditions.BecomesTapped`, and works inside `Game.Conditions.OncePerTurn(...)`. The names follow the existing naming: a bare verb for the controller, `Each*` for any player, and the source's verb for source-scoped tags.
- `BecomesTapped` and `BecomesUntapped`: source-scoped `Tapped` and `Untapped` (CR 603.2e).
- `GainsLife` and `LosesLife`: the controller's `life_changed`, split by the sign of `delta`.
- `Attacks` and `Blocks`: source-scoped `AttackerDeclared` and `BlockerDeclared`.
- `Mills`: the controller's `CardsMilled`.
- Not shipped: `Each*` siblings (TODO) and `DealsCombatDamage`. A custom function reads `event.is_combat_damage` instead, and `DealsDamage` is unchanged.

**Event order.** One action can write several events: a tap and then an attack, or damage, then a life change, then lifelink. Each event is checked on its own (CR 603.2). The docs promise no order between them.

**Performance.** No per-script event filter. The state snapshot costs more than the event table does, and a filter would need a new `:SetCondition` API. The build leaves `// TODO: skip the Lua call for event kinds a condition never reads` at the call site in `triggers.rs`.

**Docs.** The build adds one `---@class` per event table and an `Event` union alias to `docs/lua/stubs/game.lua`. In the same file it extends `TargetResult.kind` to `"object"|"player"|"ability"` and fixes the stale `StateSnapshot` fields. It rewrites the condition section of `card-script-basics.md` around the four-argument `(source, controller, event, state)` signature. It adds the new tags to `game-constants.md` and removes the "Only zone changes are visible" and "No becomes tapped" lines from `not-available-yet.md`. The stale-note sweep ticket handles the rest of that file.

**Tests the build must add.**
- For each bridged kind, a custom condition that reads every field.
- An old three-argument zone-change condition still works and sees `nil` for an unbridged event.
- The counter-name round trip.
- `CardsMilled` lists the exile id when a mill card is exiled instead. Surveil writes no `cards_milled`, and an empty library writes none.
- A drawn card's bridged `object` is its new hand id.
- Encoding a `Target::Ability` gives `{kind = "ability", id = n}`.
- `CardsScried` and `CardsSurveiled` stay `nil`.
- Damage to a player shows `damage_dealt` and `life_changed` as separate events. Infect damage shows `player_counter_added` and no `life_changed`.
- A paid life cost of 0 writes no `life_changed`.
- Each shipped tag, with a negative case: an opponent's life loss does not fire `LosesLife`, and a vigilance attacker fires `Attacks` but not `BecomesTapped`.

**Scope handoff.** This ticket owns the "becomes tapped" condition. The "Stun counters and tap or untap replacement hooks" ticket drops that clause and keeps only stun counters and would-tap or would-untap replacements.
