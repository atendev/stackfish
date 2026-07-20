# 11-ops

## Slice: script-ops-damage (merged)

- Branch `phase5/script-ops-damage`.
- Scope: `AbilityEffect::Damage(u32)` / `Game.Ops.Damage(n)`.
- Verify: 5 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::Damage(u32)`.
  - `CardDefinition.effect: Option<AbilityEffect>`; `CastSpell` bakes it onto `StackObject`.
  - `ResolveStackTop` effect match widened to instant/sorcery spells; `Damage` arm pushes `DamageDealt` event into pipeline.
  - Reuses `apply_damage_event` (prevention, Lifelink/Deathtouch/etc.) for free.
- Unresolved:
  - `GainLife` still bypasses pipeline (pre-existing).

## Slice: script-ops-destroy (merged)

- Branch `phase5/script-ops-destroy`.
- Scope: `AbilityEffect::Destroy` / `Game.Ops.Destroy()`.
- Verify: 5 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::Destroy`.
  - `commands::effective_keywords` made `pub(crate)`.
  - `ResolveStackTop` arm checks Indestructible, proposes `ZoneChange`, runs `apply_replacements`, calls `commit_death`.
- Unresolved:
  - CR 616.1 ordering choice for scripted Destroy (fixed later by destroy-ordering-choice).

## Slice: script-ops-counters (merged)

- Branch `phase5/script-ops-counters`.
- Scope: `AbilityEffect::Counters { kind, amount }` / `Game.Ops.Counters(kind, n)`.
- Verify: 6 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::Counters { kind: CounterKind, amount: i32 }`.
  - `ResolveStackTop` pushes `CounterAdded` event; `CommitEvents` applies it.
  - `parse_counter_kind_name` maps known names, custom otherwise.
- Unresolved:
  - Player-target counters.
  - No replacement for counter placement.
  - +1/+1 vs -1/-1 annihilation SBA missing.

## Slice: script-ops-drawcards (merged)

- Branch `phase5/script-ops-drawcards`.
- Scope: `AbilityEffect::DrawCards(u32)` / `Game.Ops.DrawCards(n)`.
- Verify: 5 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::DrawCards(u32)`.
  - `draw_one_card(player)` extracted from `draw_step`; called in loop by `ResolveStackTop`.
- Unresolved:
  - Target player draws deferred.
  - `Game.Ops.DrawCards(n)` itself still direct-mutation (the harder remaining draw gap, not the turn-based draw fixed later).

## Slice: script-ops-exile (merged)

- Branch `phase5/script-ops-exile`.
- Scope: `AbilityEffect::Exile` / `Game.Ops.Exile()`.
- Verify: 5 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::Exile`.
  - `ResolveStackTop` arm inlines Battlefield→Exile move with `zone_mut(Exile, None)` (shared zone).
  - Bypasses indestructibility.

## Slice: script-ops-sacrifice (merged)

- Branch `phase5/script-ops-sacrifice`.
- Scope: `AbilityEffect::Sacrifice` / `Game.Ops.Sacrifice()`.
- Verify: 6 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::Sacrifice`.
  - `ResolveStackTop` arm identical to `Destroy` minus indestructibility check.
  - Reuses `commit_death`.

## Slice: script-ops-create-token (merged)

- Branch `phase5/script-ops-create-token`.
- Scope: `AbilityEffect::CreateToken` / `Game.Tokens.*` / `Game.Ops.CreateToken(spec, count?)`.
- Verify: 13 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::CreateToken { spec: TokenSpec, count }`.
  - `NamedToken::{Clue, Food}`, `CreatureTokenSpec`, `TokenSpec`.
  - `GameObject.is_token: bool` field; minted tokens set true, all other construction sites set false.
  - `build_token_definition`, `register_token_abilities` helpers in `engine.rs`.
  - Clue/Food abilities registered directly in `state.abilities`.
- Deviations:
  - 45+ `GameObject` struct literal sites needed `is_token: false`.
- Unresolved:
  - CR 111.7 token cessation (later fixed by token-cessation).
  - Treasure token blocked on coroutines.
  - FilterSpec identity gap on Clue/Food sacrifice costs.

## Slice: script-ops-shuffle (merged)

- Branch `phase5/script-ops-shuffle`.
- Scope: `AbilityEffect::Shuffle` / `Game.Ops.Shuffle()`.
- Verify: 4 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::Shuffle`.
  - Reused existing private `Engine::shuffle_library`; exposed via `ResolveStackTop`.
- Unresolved:
  - Target player shuffles deferred.

## Slice: scry-surveil (merged)

- Branch `phase5/scry-surveil`.
- Scope: CR 701.22/701.25 Scry/Surveil.
- Verify: 11 new tests, 0 failed.
- Key changes:
  - `AbilityEffect::{Scry(u32), Surveil(u32)}`.
  - `ProcessState::AwaitingScryChoice`/`AwaitingSurveilChoice`; `StepStatus` variants; `Engine::submit_scry_choice`/`submit_surveil_choice`.
  - `is_exact_partition` validation.
  - `GameEventKind::{CardsScried, CardsSurveiled}`.
- Unresolved:
  - "Whenever you scry/surveil" trigger conditions (later fixed by trigger-completeness-cleanup).

## Slice: search-tutor (merged)

- Branch `phase5/search-tutor`.
- Scope: CR 701.23 Search.
- Verify: 2 unit + 10 integration tests, 0 failed.
- Key changes:
  - `FilterSpec` gained `supertypes`/`subtypes` fields (shared benefit for costs too).
  - `AbilityEffect::Search { filter, count, destination, shuffle_after }`.
  - `SearchDestination::{Hand, Battlefield { tapped }}`.
  - `ProcessState::AwaitingSearchChoice`; `Engine::submit_search_choice`.
- Unresolved:
  - Mandatory quantity with no quality filter; reveal; top-N redirection; multiplayer APNAP; graveyard/exile destinations.
