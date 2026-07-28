# 04-sba

## Slice: sba-basics (merged)

- Branch `phase5/sba-basics`.
- Scope: CR 704.5a player at 0 life, 704.5f toughness 0, 704.5g lethal marked damage.
- Verify: 10 new tests, 81 total, 0 failed.
- Key changes:
  - `PTValue::as_fixed` real.
  - `sba::check_state_based_actions` real; returns `Vec<GameEvent>`.
  - `apply_player_loss`: flips `PlayerState.eliminated`, emits `PlayerEliminated`.
  - `apply_creature_death`: scans objects, uses `effective_characteristics` for toughness, moves dying creatures to graveyard with new id.
- Unresolved:
  - 704.5b empty-library, 704.5c poison, 704.5h deathtouch, CDA/continuous P/T, regeneration, legend/world, planeswalker loyalty, commander damage, rest of 704.5.

## Slice: sba-player-loss (merged)

- Branch `phase5/sba-player-loss`.
- Scope: 704.5b drew from empty library, 704.5c 10+ poison, CR 903.10a 21+ commander damage.
- Verify: 9 new tests, 431 total, 0 failed.
- Key changes:
  - `PlayerState.drew_from_empty_library: bool` set by `draw_one_card` on empty library.
  - `PlayerState::commander_damage_from` real body.
  - `apply_player_loss` extended with three new causes.
  - Infect combat damage routes to poison counters.
- Deviations:
  - 5 solo TOML fixtures needed seeded library + `ledger_len` bump due to correct 704.5b behavior.
- Unresolved:
  - Commander-damage production (nothing increments `commander_damage_taken` yet).
  - `StepStatus::GameOver` never constructed; eliminated players still take turns/priority.

## Slice: legend-world-rule (merged)

- Branch `phase5/legend-world-rule`.
- Scope: CR 704.5j legend rule, 704.5k world rule.
- Verify: 10 new tests, 441 total, 0 failed.
- Key changes:
  - `PlayerId`/`ObjectId` gained `PartialOrd`/`Ord` for deterministic grouping/sort.
  - `SbaOutcome::NeedsLegendRuleChoice { player, candidates, resolved_so_far }`.
  - `ProcessState::AwaitingLegendRuleChoice` and `Engine::submit_legend_rule_choice`.
  - `find_legend_rule_group`: groups by `(controller, effective.name)` where `supertypes` contains `Legendary`.
  - World rule: `collect_world_rule_losers` returns all but most-recent World permanent (highest timestamp; tie kills all).
  - Both feed `resolve_deaths` so replacements apply.
- Unresolved:
  - Range of influence; legendary/name-changing continuous effects not constructible.

## Slice: player-elimination (merged)

- Branch `phase5/player-elimination`.
- Scope: CR 104/800.4 — game over, skip eliminated players, remove their objects.
- Verify: 8 new tests, 573 total, 0 failed.
- Key changes:
  - `ProcessState::GameOver`; `StepStatus::GameOver { winner }`.
  - `game_over_check`: 1 remaining player = winner (CR 104.2a), 0 = draw (CR 104.4a). Gated to multiplayer (`turn_order.len() >= 2`).
  - `commands::next_active_player`/`non_eliminated_players` skip eliminated players.
  - `sba::remove_players_objects`: drains per-owner zones, filters shared zones/stack, purges via `retire_object`.
- Deviations:
  - `turn_order.len() >= 2` guard added after regression on solo fixtures.
  - 3 pre-existing tests updated to expect `GameOver`.
- Unresolved:
  - `replacement_effects`/`cost_modifiers` not purged on object departure (pre-existing).
  - `StackObject.controller` used as owner proxy.

## Slice: token-cessation (merged)

- Branch `phase5/token-cessation`.
- Scope: CR 111.7/704.5d — tokens cease to exist when in a zone other than battlefield.
- Verify: 5 new tests, 0 failed.
- Key changes:
  - `GameEventKind::TokenCeased { object }`.
  - `sba::collect_ceasing_tokens` scans non-battlefield zones for `is_token`; `commit_token_cessation` removes + purges.
  - Runs at top of `check_state_based_actions` before death collection, so a token dying this pass ceases on the next pass (CR 111.7 timing).
- Unresolved:
  - CR 111.8 token zone lock (later fixed by `replaceable-draw`/`token-zone-lock`).
  - CR 704.5e spell/card copies.

## Fix: token-zone-lock (CR 111.8)

- Added `replacement::apply_token_zone_lock` in `apply_replacements_from` loop.
- A token already off-battlefield proposed to move zones has `to` rewritten back to `from`.
- Unit-tested directly (no end-to-end producer exists).

## Slice: regeneration (merged)

- Branch `phase5/regeneration`.
- Scope: CR 614.8/701.19 — destruction replacement via `ReplacementPayload::Regenerate`.
- Verify: 9 new tests, 678 total, 0 failed.
- Key changes:
  - `GameEventKind::Regenerated { object }`.
  - `ReplacementEffect::matches` gained `is_destroy: bool` param; `Regenerate` only applies to genuine destruction.
  - `sba::DyingCandidate { id, is_destroy }` distinguishes 704.5g (destroy) from 704.5f/704.5k (not destroy).
  - `commit_regeneration`: clears damage/deathtouched, taps, removes from combat.
  - `commit_death_or_regeneration` dispatcher; `is_destroy` threaded through SBA pause/resume chain.
- Deviations:
  - `sba::DyingCandidate` had to be public due to existing public pause types.
- Unresolved:
  - CR 701.19c "can't be regenerated" (later fixed by `cant-be-regenerated`/`static-regeneration`).
  - No Lua builder for shields (later fixed by `static-regeneration`/`replacement-effect-shapes`).
  - Destroy op CR 616.1 ordering gap (later fixed by `destroy-ordering-choice`).

## Slice: cant-be-regenerated (merged)

- Branch `phase5/cant-be-regenerated`.
- Scope: CR 701.19a/c — spell/ability creates a one-shot shield, gated by `CANT_BE_REGENERATED`.
- Verify: 3 new tests, 0 failed.
- Key changes:
  - `KeywordSet::CANT_BE_REGENERATED` (bit 15, last in `u16` before widening).
  - `AbilityEffect::Regenerate`; `ResolveStackTop` arm pushes a one-shot `ReplacementEffect` per legal target.
  - Creation skipped for targets with `CANT_BE_REGENERATED` keyword.

## Slice: static-regeneration (merged)

- Branch `phase5/static-regeneration`.
- Scope: CR 701.19b — static ability can create a repeatable regeneration shield via `Game.CreateReplacementEffect`.
- Verify: 3 new tests + extended unit test, 718 total, 0 failed.
- Key changes:
  - `ReplacementEffect::matches` rechecks `CANT_BE_REGENERATED` fresh every application.
  - `retire_object`/`purge_granted_abilities` now also purge `state.replacement_effects`.
  - `ScriptRegistry.pending_replacement` queue; `ReplacementEffectBuilder` with `:SetAffected`/`:Register()`.
  - Bound as `Game.CreateReplacementEffect`.
  - Builder defaults to `Repeatable`/`WouldDie`/`Regenerate`; later generalized by `replacement-effect-shapes`.

## Slice: max-hand-size (merged)

- Branch `phase5/max-hand-size`.
- Scope: CR 402.2 — "no maximum hand size" / "max hand size is N" effects.
- Verify: 4 new tests, 0 failed.
- Key changes:
  - `HandSizeEffect { NoMaximum, SetTo(u32) }`, `HandSizeModifier { source, controller, affected, effect, timestamp }`.
  - `GameState.hand_size_modifiers: Vec<HandSizeModifier>`.
  - `effective_max_hand_size(modifiers, player, base) -> Option<u32>`.
  - `begin_cleanup_step` uses effective max; `None` skips discard.
  - `AwaitingDiscard` resume arm recomputes.
- Deviations:
  - ~85 `GameState` struct literals needed new field.
- Unresolved:
  - No Lua builder for hand-size modifiers (closed by `phase5/enchanted-player-hand-size`, see below).

## Slice: enchanted-player-hand-size (merged)

- Branch `phase5/enchanted-player-hand-size`.
- Scope: CR 303.4m's "enchanted player" reference for continuous effects — deliberately narrowed, per
  discussion with the user, to a *first cut* proving the resolution pattern against one real,
  already-working consumer rather than inventing a general "continuous effects affecting a player"
  subsystem in one sitting. This gap had been named and deferred across three separate merged slices
  (`phase5/enchanted-reference`, `phase5/aura-enchants-a-player`, `phase5/aura-enters-attached-player`)
  without ever being scoped or estimated — research confirmed there was no existing mechanism to
  extend at all: `layers::effective_characteristics`/`ContinuousEffect`/`AffectedFilter` are 100%
  object-only, and the two existing player-directed modifier mechanisms (`hand_size_modifiers`,
  `cost_modifiers`) are narrow, bespoke, single-purpose flat lists, not a general query.
- Key finding that shaped the plan: `HandSizeModifier`/CR 402.2 already has a real, working, tested
  enforcement path (`Engine::begin_cleanup_step`'s discard check) and was *also* separately missing a
  Lua construction path (`phase5/max-hand-size`'s own "Unresolved" note, above) — reusing it as the
  proof vehicle meant no new enforcement point needed inventing, and closed that separate TODO as a
  side effect.
- Key changes:
  - `HandSizeAffected { Filter(FilterController), EnchantedPlayer }` — `HandSizeModifier.affected`
    widened from a bare `FilterController` to this sibling enum (low ripple: zero real non-test
    producers existed before this slice). `EnchantedPlayer` mirrors `AffectedFilter::EnchantedOnly`'s
    own `source.attached_to == Some(object.object_id)` check exactly, just the player-attach sibling
    field (`attached_to_player`).
  - `effective_max_hand_size` gained a `state: &GameState` parameter to resolve `EnchantedPlayer` (2
    call sites in `turn.rs`, both already had `&self.state` in scope).
  - `Game.CreateHandSizeModifier(c)` — new `HandSizeModifierBuilder`, mirrors `Game.CreateCostModifier`'s
    queue-and-drain shape exactly: `:SetAffected(Game.HandSizeAffected.You/Opponent/Any/EnchantedPlayer)`,
    `:SetEffect(Game.HandSizeEffects.NoMaximum`/`.SetTo(n))`, `:Register()` queues with a placeholder
    `controller`/`timestamp`, both resolved at `run_card_script`'s drain step (identical pattern
    `CostModifierBuilder` already established).
- Verify: 4 new tests (`phase5_hand_size_modifier_enchanted_player.rs`) — script self-registration
  mechanics, an end-to-end cleanup-discard enforcement test, a control case confirming the modifier
  doesn't leak onto a player the source isn't attached to, and a re-resolution test confirming the
  restriction stops applying once the Aura is no longer attached to that player. Full workspace suite
  green.
- Explicit non-goal, named not silently dropped: no general `AffectedFilter`-for-players primitive —
  this proves the *pattern* (resolve via `attached_to_player`) on one concrete mechanism, not a
  reusable query every future player-level effect can plug into. A future "plays with hand revealed"-
  or "can't draw more than one card"-style effect will each need their own bespoke mechanism and
  enforcement point, same as this one needed its own — genuinely two different card-text shapes with
  two different enforcement points, confirmed during scoping research not to share a common query.
