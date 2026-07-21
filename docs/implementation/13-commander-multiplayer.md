# 13-commander-multiplayer

## Slice: multiplayer-defending-player (merged)

- Branch `phase5/multiplayer-defending-player`.
- Scope: CR 508.5a — defending player determined per attacker, not hardcoded first non-active.
- Verify: 3 new tests (first 3-player fixture), 565 total, 0 failed.
- Key changes:
  - `CombatState.blockers_declared: bool` -> `declared_defenders: Vec<PlayerId>`.
  - `DeclareBlockers` gate: actor must be a player that an attacker actually targets (player or their planeswalker/battle).
  - `combat.blocks` accumulates across defending players.
- Unresolved:
  - Turn-based actions still don't block priority before performed.
  - Block restrictions/requirements.

## Slice: player-elimination (merged)

- See `04-sba.md` for full details; duplicated here as backlog item 11b.
- Key points:
  - `StepStatus::GameOver` / `ProcessState::GameOver`.
  - Skip eliminated players in `PassPriority` and active-player rotation.
  - Remove eliminated player's objects per CR 800.4a; purge via `retire_object`.
  - Gate on `turn_order.len() >= 2` to protect solo fixtures.

## Slice: stack-owner-and-dead-stubs (implemented, not yet merged)

- Branch `phase5/stack-owner-and-dead-stubs`.
- Scope: backlog items "Stack owner" and "Dead Phase-3 stubs" - two small, unrelated fixes bundled as one cleanup slice.
- Verify: 3 new tests (stack cleanup), full workspace suite green.
- Key changes:
  - `StackObject::owner: PlayerId` added alongside the pre-existing `controller` - a spell's is its card's owner, an ability's is its source permanent's owner (falls back to `controller` only for a departed trigger source with no live object left to read - `PendingTrigger` has no last-known-owner of its own).
  - `sba::remove_players_objects`'s stack cleanup now matches CR 800.4a's actual two-clause text: a spell (represented by a card) leaves the stack by *owner*; an ability (not represented by a card) leaves by *controller*. Previously both used `controller` as a proxy for both.
  - Removed, not implemented (confirmed zero real callers each): `Subtype::new`/`as_str`, `CardDefinition::has_type`/`has_supertype`/`has_subtype` (plus the `CardType`/`Supertype` enums that existed only to serve those methods), `ManaCost::from_symbols`/`from_pool`/`is_empty`/`generic_total`, `ManaPool::empty`/`add`/`total`/`clear`.
- Unresolved:
  - `StackEntryKind::Copy` is still never constructed anywhere (no spell-copy effect exists) - once one does, a copy is "not represented by a card" too and would need the controller-side cleanup clause, not the owner-side one `Spell` gets.

## Slice: commander-marking-and-casting (merged)

- Branch `phase5/commander-marking-and-casting`.
- Scope: CR 903.3 (commander flag), 903.8 (cast from command zone), 903.9a (return from GY/Exile).
- Verify: 10 new tests, 583 total, 0 failed.
- Key changes:
  - `GameObject.is_commander: bool`; preserved across all real zone-change clones.
  - `CastSpell` allows command-zone source if `is_commander && owner == actor`.
  - `PlayerConfig.commanders: Vec<CardDefinition>`; `Engine::new_game` mints commanders into per-player `Command` zone.
  - `SbaOutcome::NeedsCommanderReturnChoice` / `ProcessState::AwaitingCommanderReturnChoice` / `Engine::submit_commander_return_choice`.
  - `GameState.commander_return_offered: HashSet<ObjectId>`.
  - `move_commander_to_command_zone` moves GY/Exile -> command zone under fresh id.
  - Scenario `[[setup]].is_commander` and `[[game.players]].commanders`.
- Deviations:
  - 903.9a included despite original scope, to enable tax/damage testability.
- Unresolved:
  - 903.9b (hand/library redirect) deferred.
  - Zone move skips replacement checking.

## Slice: commander-tax (merged)

- Branch `phase5/commander-tax`.
- Scope: CR 903.8 generic cost +{2} per prior cast from command zone.
- Verify: 6 new tests, 589 total, 0 failed.
- Key changes:
  - `PlayerState.commander_casts: HashMap<String, u32>` keyed by card name (handles Partner).
  - `CastSpell` applies `ManaCost::apply_cost_delta(2 * prior_casts)` after cost modifiers for command-zone casts.
  - Counter increments at zone-move commit.
- Deviations:
  - ~68 `PlayerState` struct literals needed new field.
- Unresolved:
  - `unwind_payment` doesn't roll back `commander_casts` or return to command zone.

## Slice: commander-damage (merged)

- Branch `phase5/commander-damage`.
- Scope: CR 903.10a commander combat damage production.
- Verify: 6 new tests, 595 total, 0 failed.
- Key changes:
  - `GameEventKind::DamageDealt.is_combat_damage: bool`.
  - `AssignCombatDamage` sets `true`; script `Damage` op sets `false`.
  - `apply_damage_event`: when `is_combat_damage && source.is_commander`, accumulates into `commander_damage_taken`.
  - `AssertionSpec::CommanderDamage`.
- Deviations:
  - `commander_damage_taken` stays keyed by `ObjectId` (confirmed scope decision): recast commander gets new id and resets count.
- Unresolved:
  - Planeswalker/battle damage still unmodeled.

## Slice: color-identity (merged)

- Branch `phase5/color-identity`.
- Scope: CR 903.4/903.5c pure query functions.
- Verify: 14 new tests, 609 total, 0 failed.
- Key changes:
  - Filled dead `todo!()` stubs: `AtomicManaSymbol::colors`, `ManaSymbol::colors`, `ManaCost::colors`, `CardDefinition::colors`.
  - New `color_identity.rs`: `color_identity(card)`, `commander_color_identity(commanders)`, `illegal_cards_for_color_identity(identity, cards)`.
  - Strips parenthetical reminder text before scanning oracle text for `{...}` groups.
- Scope decision: pure query only; no `Engine::new_game` enforcement.
- Unresolved:
  - CDA-defined colors, DFC backs, Adventure, 903.4b color choice, deck-size/unique-name enforcement.
