use crate::script::ScriptRuntime;
use crate::types::cost::FilterController;
use crate::types::effect::{ConsumptionPolicy, ReplacementPayload, ReplacementSpec};
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::zone::ZoneKind;

/// What `apply_replacements` produced for one proposed event. Not just a
/// `GameEventKind`: CR 616.1 has the *affected player* choose which
/// replacement applies first when more than one matches the same event —
/// a player decision, not something a pure transform can resolve on its
/// own — so this type makes that case a value the caller (`engine.rs`)
/// must handle rather than something `apply_replacements` could silently
/// resolve "for" the player or panic on.
pub enum ReplacementOutcome {
    /// Zero or exactly one `ReplacementEffect` matched (after however many
    /// rewrite/re-match passes CR 616's stabilization requirement needed) —
    /// this is the event's final form, ready for `CommitEvents`.
    Resolved(GameEventKind),
    /// More than one `ReplacementEffect` matches `event` and `affected_player`
    /// must choose which applies first (CR 616.1) before resolution can
    /// continue. `candidates` identifies the matching effects by their
    /// `ReplacementEffect::source` `ObjectId` (no dedicated id type exists
    /// yet). The engine must surface this as an actual pause — see
    /// `engine::ProcessState::AwaitingReplacementChoice` — not fold it back
    /// into a synchronous transform. `excluded` carries forward every
    /// effect already applied to this same event earlier in this same
    /// resolution (see `apply_replacements`'s own doc comment for why) — the
    /// caller must thread it back into whatever continues this resolution
    /// after the choice is made, not start a fresh empty exclusion set.
    NeedsOrderingChoice {
        affected_player: PlayerId,
        event: GameEventKind,
        candidates: Vec<ObjectId>,
        excluded: Vec<ObjectId>,
        /// Carried forward so a resumed `apply_replacements_from` call (once
        /// the player answers) rechecks the rewritten event with the same
        /// "is this genuinely a destroy" meaning it started with — see
        /// `ReplacementEffect::matches`'s own doc comment.
        is_destroy: bool,
    },
}

/// CR 616.1's "affected player" for a matched-more-than-once event — the
/// permanent's controller if the event's affected slot is an object, the
/// player directly if it's a player. Only ever called against an event
/// `apply_replacements` already confirmed at least one `ReplacementEffect`
/// matched, so the two constructible `EventFilter` shapes (`DamageDealt`,
/// `WouldDie` -> `ZoneChange`) cover every real call; the fallback exists
/// only so this stays a total function rather than something that could
/// panic if a future `Script`-backed filter ever matches a different event
/// shape.
fn affected_player_for(event: &GameEventKind, state: &GameState, script_runtime: &ScriptRuntime) -> PlayerId {
    let fallback = || state.turn_order.first().copied().expect("a game always has players");
    match event {
        GameEventKind::DamageDealt { target, .. } => match target {
            Target::Player(pid) => *pid,
            Target::Object(id) => state
                .object(*id)
                .map(|_| crate::layers::effective_controller(state, script_runtime, *id))
                .unwrap_or_else(fallback),
        },
        GameEventKind::ZoneChange { object, .. } => state
            .object(*object)
            .map(|_| crate::layers::effective_controller(state, script_runtime, *object))
            .unwrap_or_else(fallback),
        _ => fallback(),
    }
}

/// Applies `source`'s `ReplacementEffect` to `event` (assumes `matches`
/// already confirmed it applies) and consumes it per its `ConsumptionPolicy`
/// (CR 614.13-style shields with a counter budget) — `OneShot`/an exhausted
/// `LimitedUses` removes the entry from `state.replacement_effects`
/// entirely, `LimitedUses(n)` with `n > 1` decrements in place, `Repeatable`
/// is untouched. Returns `event` unchanged if `source` doesn't actually name
/// a registered effect (shouldn't happen given the callers below, but this
/// keeps the function total rather than panicking on a stale id).
pub(crate) fn apply_one_replacement(
    state: &mut GameState,
    source: ObjectId,
    event: GameEventKind,
) -> GameEventKind {
    let Some(idx) = state.replacement_effects.iter().position(|r| r.source == source) else {
        return event;
    };
    let rewritten = state.replacement_effects[idx].apply(event);
    match state.replacement_effects[idx].consumption {
        ConsumptionPolicy::OneShot => {
            state.replacement_effects.remove(idx);
        }
        ConsumptionPolicy::LimitedUses(n) => {
            if n <= 1 {
                state.replacement_effects.remove(idx);
            } else {
                state.replacement_effects[idx].consumption = ConsumptionPolicy::LimitedUses(n - 1);
            }
        }
        ConsumptionPolicy::Repeatable => {}
    }
    rewritten
}

/// CR 702.16a: protection prevents all damage from a source matching the
/// protected quality (quality) — color (702.16e), card type (702.16),
/// subtype (702.16), name (702.16a), "everything" (702.16j), or a specific
/// player (702.16k), checked as an OR of all six, since any one alone is
/// sufficient to trigger protection. Unlike a `ReplacementEffect` a card
/// registers, this is a static, always-on characteristic of the object
/// itself (`layers::effective_characteristics()`'s `protection_colors`/
/// `protection_card_types`/`protection_subtypes`/`protection_names`/
/// `protection_everything`/`protection_players`), so it's a hardcoded check
/// here rather than an entry in `state.replacement_effects` — same "checked
/// up front, not modeled as a registered replacement" stance CR 702.12b
/// indestructible already takes ahead of `Destroy`. Idempotent (zeroing an
/// already-zero amount is a no-op), so calling it fresh on every
/// `apply_replacements_from` loop iteration is safe and correctly
/// re-evaluates against a possibly-`RedirectDamage`-changed target, not
/// just the original proposal. A `Target::Player`/unresolvable source or
/// target never has protection to check — returned unchanged.
/// CR 111.8: a token that has already left the battlefield can't move to
/// another zone (or back onto the battlefield) — "if such a token would
/// change zones, it remains in its current zone instead." Hardcoded ahead
/// of the registered-effects match, same stance `apply_protection` already
/// takes for CR 702.16a — this applies to every token unconditionally, not
/// something a card grants. Checked against `state.object(object)`'s own
/// *live* zone (not the proposal's own `from` field, though the two should
/// always agree) — the token's identity/zone don't change until this event
/// actually commits, so this is always the token's true current location.
///
/// Only rewrites `to`, mirroring `ReplacementPayload::ReplaceZoneDestination`'s
/// own shape — the caller still commits a real (no-op) `ZoneChange`, same as
/// every other replacement-rewritten proposal. Doesn't touch the token's
/// own CR 111.7 cessation (`sba::collect_ceasing_tokens`/
/// `commit_token_cessation`, checked separately by state-based actions) —
/// that still fires on the next SBA check regardless of whether this
/// function blocked a second move in the meantime.
///
/// Unreachable via real gameplay today, same as several other CR-accurate
/// but currently-unexercisable checks in this codebase (see e.g. CR
/// 303.4f/h/i/k/m's own note): nothing in this engine can propose a
/// `ZoneChange` whose `from` is anything but `Battlefield` for a token — no
/// "return from graveyard"/"cast from graveyard" path applies to tokens
/// (they aren't cards, so flashback-style alternative costs can't target
/// one), and CR 111.7 removes a token from any non-battlefield zone before
/// most engine code gets a second chance to move it. Proven correct via
/// direct `apply_replacements` construction instead (see this file's own
/// test module) rather than an end-to-end scenario.
fn apply_token_zone_lock(state: &GameState, event: GameEventKind) -> GameEventKind {
    let GameEventKind::ZoneChange { object, from, to, last_known_info } = event else {
        return event;
    };
    let Some(obj) = state.object(object) else {
        return GameEventKind::ZoneChange { object, from, to, last_known_info };
    };
    if obj.is_token && obj.zone != ZoneKind::Battlefield {
        return GameEventKind::ZoneChange { object, from, to: from, last_known_info };
    }
    GameEventKind::ZoneChange { object, from, to, last_known_info }
}

fn apply_protection(state: &GameState, script_runtime: &ScriptRuntime, event: GameEventKind) -> GameEventKind {
    let GameEventKind::DamageDealt { source, target, amount, is_combat_damage } = event else {
        return event;
    };
    let Target::Object(target_id) = target else {
        return GameEventKind::DamageDealt { source, target, amount, is_combat_damage };
    };
    let (Some(source_obj), Some(target_obj)) = (state.object(source), state.object(target_id)) else {
        return GameEventKind::DamageDealt { source, target, amount, is_combat_damage };
    };
    let source_controller = source_obj.controller;
    let target_controller = target_obj.controller;
    let target_characteristics = crate::layers::effective_characteristics(state, script_runtime, target_id);
    let source_characteristics = crate::layers::effective_characteristics(state, script_runtime, source);
    let protected = target_characteristics.protection_everything
        || target_characteristics.protection_colors.intersects(source_characteristics.colors)
        || target_characteristics.protection_card_types.intersects(source_characteristics.card_types)
        || target_characteristics
            .protection_subtypes
            .iter()
            .any(|s| source_characteristics.subtypes.contains(s))
        || target_characteristics.protection_names.iter().any(|n| *n == source_characteristics.name)
        || target_characteristics.protection_players.iter().any(|who| match who {
            FilterController::You => source_controller == target_controller,
            FilterController::Opponent => source_controller != target_controller,
            FilterController::Any => false,
        });
    let amount = if protected { 0 } else { amount };
    GameEventKind::DamageDealt { source, target, amount, is_combat_damage }
}

/// Rewrites/prevents `proposed` per CR 614/615/616, applying every matching
/// `ReplacementEffect` in `state.replacement_effects` before the event is
/// allowed to commit, re-checking after each rewrite in case the rewritten
/// form matches a *different* replacement (CR 616.1's rewrite-stabilization
/// requirement). Thin wrapper over `apply_replacements_from` starting with
/// nothing excluded — the entry point every fresh event (one that hasn't
/// been through any replacement checking yet) uses.
pub fn apply_replacements(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    proposed: GameEventKind,
    is_destroy: bool,
) -> ReplacementOutcome {
    apply_replacements_from(state, script_runtime, proposed, is_destroy, Vec::new())
}

/// Real body of `apply_replacements` — separated out so `Engine::
/// submit_replacement_choice` (engine.rs) can resume an in-progress
/// resolution with the same `excluded` set a paused `NeedsOrderingChoice`
/// already accumulated, rather than starting a fresh one.
///
/// CR 616.1: "an individual replacement effect can apply only once to a
/// given event" — a `Repeatable`/multi-`LimitedUses` effect isn't excluded
/// from being asked again about a *different* event later, but within one
/// event's resolution it must never re-match its own (possibly unchanged)
/// rewritten output, or the CR 616 stabilization loop never terminates
/// (e.g. a damage-prevention effect that zeroes an already-zero amount
/// would otherwise keep "matching" the same event forever). `excluded`
/// tracks exactly that — every effect already applied to `event` earlier in
/// this same resolution, appended to as each single match applies.
pub(crate) fn apply_replacements_from(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    proposed: GameEventKind,
    is_destroy: bool,
    mut excluded: Vec<ObjectId>,
) -> ReplacementOutcome {
    let mut event = proposed;
    loop {
        // CR 702.16a: checked fresh every iteration, ahead of the
        // registered-effects match below - see apply_protection's own doc
        // comment for why this is hardcoded rather than a ReplacementEffect
        // entry.
        event = apply_protection(state, script_runtime, event);
        // CR 111.8: same hardcoded-ahead-of-registered-effects stance, for
        // an already-off-battlefield token's own zone lock.
        event = apply_token_zone_lock(state, event);

        if state.replacement_effects.is_empty() {
            // Genuinely nothing registered to match against - safe to
            // resolve unchanged.
            return ReplacementOutcome::Resolved(event);
        }
        let matching: Vec<ObjectId> = state
            .replacement_effects
            .iter()
            .filter(|r| !excluded.contains(&r.source) && r.matches(&event, is_destroy, state, script_runtime))
            .map(|r| r.source)
            .collect();

        match matching.as_slice() {
            [] => return ReplacementOutcome::Resolved(event),
            [only] => {
                // Exactly one match - apply, exclude it from matching again
                // against this same event, and re-check the rewritten form
                // (CR 616's stabilization requirement) by looping rather
                // than returning immediately.
                event = apply_one_replacement(state, *only, event);
                excluded.push(*only);
            }
            _ => {
                let affected_player = affected_player_for(&event, state, script_runtime);
                return ReplacementOutcome::NeedsOrderingChoice {
                    affected_player,
                    event,
                    candidates: matching,
                    excluded,
                    is_destroy,
                };
            }
        }
    }
}

/// CR 603.6d/614.1c/113.6h: applies every registered `EntersTapped`/
/// `EntersWithCounters` effect matching `object`'s own move from `from` onto
/// the battlefield — the "how a permanent enters" replacement-effect family,
/// covering both a card's own self-registered ability (CR 113.6h: functions
/// as the object is entering, before it's a real battlefield permanent in
/// the trigger-collection sense) and an externally granted one (e.g. "lands
/// your opponents control enter tapped"). Deliberately not routed through
/// `apply_replacements`/`apply_replacements_from`: those rewrite fields on
/// the proposed `GameEventKind` itself, but neither payload has anything on
/// `GameEventKind::ZoneChange` to rewrite (see `ReplacementPayload::
/// EntersTapped`'s own doc comment).
///
/// The two payloads apply differently, deliberately: `EntersTapped` mutates
/// the already-constructed `GameObject` at `object` directly, right here
/// (mirrors `SearchDestination::Battlefield { tapped }`'s own precedent —
/// no dedicated event exists for "this became tapped," so there's nothing
/// to push). `EntersWithCounters` does *not* mutate here — it only proposes
/// a `GameEventKind::CounterAdded`, same "Rust decides by applying the
/// event, not the proposer" split `AbilityEffect::Counters` already
/// established (`ResolveStackTop` proposes, `CommitEvents` is what actually
/// calls `counters.add`). The caller MUST push the returned events onto a
/// path that reaches `CommitEvents` - mutating both here *and* relying on
/// `CommitEvents`'s existing generic `CounterAdded` handler would double
/// -apply the same counters.
///
/// Called only after `object` is already inserted into `state.objects` *and*
/// its own `run_card_script` pass has run (so a self-registered effect is
/// already sitting in `state.replacement_effects` by the time this runs —
/// see `docs/lua_scripts.md`'s "Registration timing" note for why that
/// ordering already holds at every real call site, without needing a
/// propose-before-the-object-exists split the way `commit_zone_move`'s own
/// callers need).
///
/// CR 616.1's ordering choice (`ReplacementOutcome::NeedsOrderingChoice`) is
/// deliberately not invoked here even when multiple effects match the same
/// entry: `EntersTapped` is idempotent and `EntersWithCounters` is additive,
/// so unlike a `ReplaceZoneDestination`-style rewrite, no ordering of
/// multiple matches ever changes the final result — every match just applies.
/// TODO: if a future payload pairs with this same `WouldMoveZone(_,
/// Battlefield)` filter and *isn't* order-independent, this function's
/// "apply every match, no pause" stance stops being correct and needs a real
/// ordering-choice pause of its own.
///
/// Returns the `GameEventKind::CounterAdded` events produced (one per
/// matching `EntersWithCounters` effect) for the caller to append to its own
/// event list, same ledger-visibility convention `AbilityEffect::Counters`
/// already established.
pub fn apply_enters_battlefield_modifiers(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    object: ObjectId,
    from: ZoneKind,
) -> Vec<GameEventKind> {
    // `GameEventKind` isn't `Clone` - rebuilt fresh at each use site below
    // instead of cloned once, same shape every time (`object`/`from` are
    // both `Copy`, `last_known_info` is always `None` - this candidate is
    // never actually committed, only matched against).
    let candidate = || GameEventKind::ZoneChange { object, from, to: ZoneKind::Battlefield, last_known_info: None };
    let matching: Vec<ObjectId> = state
        .replacement_effects
        .iter()
        .filter(|r| {
            matches!(
                &r.spec,
                ReplacementSpec::Primitive {
                    payload: ReplacementPayload::EntersTapped | ReplacementPayload::EntersWithCounters { .. },
                    ..
                }
            )
        })
        .filter(|r| r.matches(&candidate(), false, state, script_runtime))
        .map(|r| r.source)
        .collect();

    let mut counter_events = Vec::new();
    for source in matching {
        let Some(idx) = state.replacement_effects.iter().position(|r| r.source == source) else {
            continue;
        };
        let payload = match &state.replacement_effects[idx].spec {
            ReplacementSpec::Primitive { payload, .. } => payload.clone(),
            ReplacementSpec::Script(_) => continue,
        };
        match payload {
            ReplacementPayload::EntersTapped => {
                if let Some(obj) = state.object_mut(object) {
                    obj.tapped = true;
                }
            }
            // Unlike `EntersTapped` above, this doesn't mutate `object`
            // directly - it only proposes the event, same "Rust decides by
            // applying the event, not the proposer" split `AbilityEffect::
            // Counters` already established (`ResolveStackTop` pushes
            // `CounterAdded`; `CommitEvents` is what actually calls
            // `counters.add`). The caller MUST push this into its own
            // event list on a path that reaches `CommitEvents` - mutating
            // here too would double-apply once that generic handler runs.
            ReplacementPayload::EntersWithCounters { kind, amount } => {
                counter_events.push(GameEventKind::CounterAdded { object, kind, amount });
            }
            _ => continue,
        }
        // CR 614.13/701.19b-style consumption bookkeeping (OneShot/
        // LimitedUses removal, Repeatable untouched) - same helper the
        // general apply_replacements_from loop uses; the rewritten event it
        // returns is discarded, since neither payload here ever rewrites one.
        apply_one_replacement(state, source, candidate());
    }
    counter_events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::card::{CardDefinition, CardTypeSet, KeywordSet, PTValue, ScriptReference, SupertypeSet};
    use crate::types::counter::CounterSet;
    use crate::types::cost::FilterController;
    use crate::types::effect::{EffectFilter, EventFilter, ReplacementEffect, ReplacementPayload, ReplacementSpec};
    use crate::types::game::RngState;
    use crate::types::ids::SequenceNo;
    use crate::types::mana::ManaPool;
    use crate::types::object::{GameObject, VisibilityPolicy};
    use crate::types::player::PlayerState;
    use crate::types::turn::{Phase, Step};
    use crate::types::zone::ZoneKind;
    use std::collections::HashMap;

    fn object(id: ObjectId, controller: PlayerId, card_types: CardTypeSet) -> GameObject {
        GameObject {
            object_id: id,
            lineage: crate::types::ids::ObjectLineage { previous: None },
            definition: CardDefinition {
                definition_id: 1,
                name: "Test Object".to_string(),
                mana_cost: crate::types::mana::ManaCost { symbols: Vec::new() },
                color_indicator: None,
                supertypes: SupertypeSet::empty(),
                card_types,
                subtypes: Vec::new(),
                oracle_text: String::new(),
                power: Some(PTValue::Fixed(1)),
                toughness: Some(PTValue::Fixed(1)),
                loyalty: None,
                defense: None,
                script_ref: ScriptReference::ByKey("test".to_string()),
                keywords: KeywordSet::empty(),
                target_requirement: None,
                alternative_costs: Vec::new(),
                effect: None,
                modes: Vec::new(),
            },
            copiable_override: None,
            owner: controller,
            controller,
            zone: ZoneKind::Battlefield,
            tapped: false,
            flipped: false,
            face_down: false,
            phased_out: false,
            damage_marked: 0,
            deathtouched: false,
            counters: CounterSet::new(),
            attachments: Vec::new(),
            attached_to: None,
            timestamp: SequenceNo(0),
            controlled_since_turn: 0,
            is_commander: false,
            is_token: false,
            visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
        }
    }

    fn state_with(objects: Vec<GameObject>) -> GameState {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        GameState {
            players: vec![
                PlayerState {
                    player_id: p0,
                    life: 20,
                    mana_pool: ManaPool::default(),
                    counters: CounterSet::new(),
                    emblems: Vec::new(),
                    commander_damage_taken: Vec::new(),
                    eliminated: false,
                    drew_from_empty_library: false,
                    mulligans_taken: 0,
                    hand_kept: false,
                    commander_casts: std::collections::HashMap::new(),
                },
                PlayerState {
                    player_id: p1,
                    life: 20,
                    mana_pool: ManaPool::default(),
                    counters: CounterSet::new(),
                    emblems: Vec::new(),
                    commander_damage_taken: Vec::new(),
                    eliminated: false,
                    drew_from_empty_library: false,
                    mulligans_taken: 0,
                    hand_kept: false,
                    commander_casts: std::collections::HashMap::new(),
                },
            ],
            turn_order: vec![p0, p1],
            active_player: p0,
            turn_number: 1,
            phase: Phase::Main,
            step: Step::PreCombatMain,
            priority_player: p0,
            consecutive_passes: 0,
            lands_played_this_turn: HashMap::new(),
            combat: None,
            last_active_turn_number: HashMap::new(),
            objects: objects.into_iter().map(|o| (o.object_id, o)).collect(),
            zones: Vec::new(),
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            cost_modifiers: Vec::new(),
            hand_size_modifiers: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState { seed: 1, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 1000,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
        }
    }

    fn no_filter() -> EffectFilter {
        EffectFilter { card_types: None, controller: None, self_only: false }
    }

    #[test]
    fn resolves_unchanged_with_nothing_registered() {
        let mut state = state_with(Vec::new());
        let event = GameEventKind::DamageDealt { source: ObjectId(0), target: Target::Player(PlayerId(0)), amount: 5, is_combat_damage: false };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("expected Resolved with no replacement effects registered");
        };
        assert!(matches!(resolved, GameEventKind::DamageDealt { amount: 5, .. }));
    }

    #[test]
    fn resolves_unchanged_when_nothing_matches() {
        let effect_source = ObjectId(0);
        let mut state = state_with(vec![object(effect_source, PlayerId(0), CardTypeSet::ARTIFACT)]);
        state.replacement_effects.push(ReplacementEffect {
            source: effect_source,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt {
                    affected: EffectFilter { card_types: Some(CardTypeSet::CREATURE), ..no_filter() },
                },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::Repeatable,
        });
        // Target is an Artifact, not a Creature - the filter doesn't match.
        let event = GameEventKind::DamageDealt { source: ObjectId(1), target: Target::Object(effect_source), amount: 5, is_combat_damage: false };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("expected Resolved - filter doesn't match");
        };
        assert!(matches!(resolved, GameEventKind::DamageDealt { amount: 5, .. }));
    }

    #[test]
    fn one_shot_effect_applies_once_then_is_removed() {
        let effect_source = ObjectId(0);
        let mut state = state_with(vec![object(effect_source, PlayerId(0), CardTypeSet::ARTIFACT)]);
        state.replacement_effects.push(ReplacementEffect {
            source: effect_source,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt { affected: no_filter() },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::OneShot,
        });

        let event = GameEventKind::DamageDealt { source: ObjectId(1), target: Target::Player(PlayerId(0)), amount: 5, is_combat_damage: false };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("expected a single match to resolve");
        };
        assert!(matches!(resolved, GameEventKind::DamageDealt { amount: 0, .. }));
        assert!(state.replacement_effects.is_empty(), "OneShot must be consumed after applying");
    }

    #[test]
    fn limited_uses_decrements_then_expires() {
        let effect_source = ObjectId(0);
        let mut state = state_with(vec![object(effect_source, PlayerId(0), CardTypeSet::ARTIFACT)]);
        state.replacement_effects.push(ReplacementEffect {
            source: effect_source,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt { affected: no_filter() },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::LimitedUses(2),
        });

        let event = || GameEventKind::DamageDealt { source: ObjectId(1), target: Target::Player(PlayerId(0)), amount: 5, is_combat_damage: false };
        apply_replacements(&mut state, &ScriptRuntime::new(), event(), false);
        assert_eq!(
            state.replacement_effects.first().map(|r| r.consumption),
            Some(ConsumptionPolicy::LimitedUses(1)),
            "first use decrements 2 -> 1, doesn't remove"
        );

        apply_replacements(&mut state, &ScriptRuntime::new(), event(), false);
        assert!(state.replacement_effects.is_empty(), "second use exhausts LimitedUses(1) and removes it");
    }

    #[test]
    fn repeatable_effect_is_never_consumed() {
        let effect_source = ObjectId(0);
        let mut state = state_with(vec![object(effect_source, PlayerId(0), CardTypeSet::ARTIFACT)]);
        state.replacement_effects.push(ReplacementEffect {
            source: effect_source,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt { affected: no_filter() },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::Repeatable,
        });

        let event = || GameEventKind::DamageDealt { source: ObjectId(1), target: Target::Player(PlayerId(0)), amount: 5, is_combat_damage: false };
        apply_replacements(&mut state, &ScriptRuntime::new(), event(), false);
        apply_replacements(&mut state, &ScriptRuntime::new(), event(), false);
        assert_eq!(state.replacement_effects.len(), 1, "Repeatable is never removed or decremented");
    }

    #[test]
    fn two_or_more_matches_pause_for_an_ordering_choice() {
        let a = ObjectId(0);
        let b = ObjectId(1);
        let mut state = state_with(vec![
            object(a, PlayerId(0), CardTypeSet::ARTIFACT),
            object(b, PlayerId(0), CardTypeSet::ARTIFACT),
        ]);
        for source in [a, b] {
            state.replacement_effects.push(ReplacementEffect {
                source,
                spec: ReplacementSpec::Primitive {
                    filter: EventFilter::DamageDealt { affected: no_filter() },
                    payload: ReplacementPayload::PreventDamage,
                },
                consumption: ConsumptionPolicy::Repeatable,
            });
        }

        let event = GameEventKind::DamageDealt { source: ObjectId(2), target: Target::Player(PlayerId(0)), amount: 5, is_combat_damage: false };
        let ReplacementOutcome::NeedsOrderingChoice { affected_player, candidates, .. } =
            apply_replacements(&mut state, &ScriptRuntime::new(), event, false)
        else {
            panic!("expected a pause - two effects match the same event");
        };
        assert_eq!(affected_player, PlayerId(0), "damage to a player - that player is affected");
        assert_eq!(candidates.len(), 2);
        assert!(candidates.contains(&a) && candidates.contains(&b));
        // Neither effect was consumed - the choice hasn't been made yet.
        assert_eq!(state.replacement_effects.len(), 2);
    }

    #[test]
    fn stabilizes_across_two_different_replacements() {
        // Effect A: damage dealt to p1 gets redirected to p0.
        // Effect B: damage dealt to p0 is prevented entirely.
        // The original event only matches A - applying A produces a new
        // event that only then matches B (CR 616's stabilization
        // requirement: the rewritten form must be re-checked).
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let source_a = ObjectId(0);
        let source_b = ObjectId(1);
        let mut state =
            state_with(vec![object(source_a, p1, CardTypeSet::ARTIFACT), object(source_b, p0, CardTypeSet::ARTIFACT)]);
        state.replacement_effects.push(ReplacementEffect {
            source: source_a,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt {
                    affected: EffectFilter { controller: Some(FilterController::You), ..no_filter() },
                },
                payload: ReplacementPayload::RedirectDamage(Target::Player(p0)),
            },
            consumption: ConsumptionPolicy::Repeatable,
        });
        state.replacement_effects.push(ReplacementEffect {
            source: source_b,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt {
                    affected: EffectFilter { controller: Some(FilterController::You), ..no_filter() },
                },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::Repeatable,
        });

        let event = GameEventKind::DamageDealt { source: ObjectId(2), target: Target::Player(p1), amount: 5, is_combat_damage: false };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("only one effect matches at any single point - never a real multi-match pause here");
        };
        assert!(
            matches!(resolved, GameEventKind::DamageDealt { target: Target::Player(pid), amount: 0, .. } if pid == p0),
            "redirected to p0 by A, then prevented by B: {resolved:?}"
        );
    }

    // -----------------------------------------------------------------
    // CR 111.8: a token that's already left the battlefield can't move
    // again - no in-game producer can construct this scenario today (see
    // apply_token_zone_lock's own doc comment), so these call
    // apply_replacements directly rather than driving a full Engine.
    // -----------------------------------------------------------------

    #[test]
    fn token_zone_lock_blocks_a_second_move_for_an_already_departed_token() {
        let id = ObjectId(0);
        let token = GameObject { zone: ZoneKind::Graveyard, is_token: true, ..object(id, PlayerId(0), CardTypeSet::CREATURE) };
        let mut state = state_with(vec![token]);
        let event = GameEventKind::ZoneChange { object: id, from: ZoneKind::Graveyard, to: ZoneKind::Exile, last_known_info: None };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("no registered effects, no ordering choice possible");
        };
        assert!(
            matches!(resolved, GameEventKind::ZoneChange { to: ZoneKind::Graveyard, .. }),
            "CR 111.8: the token must stay in its current zone instead of moving to Exile: {resolved:?}"
        );
    }

    #[test]
    fn token_zone_lock_blocks_a_departed_token_from_returning_to_the_battlefield() {
        let id = ObjectId(0);
        let token = GameObject { zone: ZoneKind::Graveyard, is_token: true, ..object(id, PlayerId(0), CardTypeSet::CREATURE) };
        let mut state = state_with(vec![token]);
        let event = GameEventKind::ZoneChange { object: id, from: ZoneKind::Graveyard, to: ZoneKind::Battlefield, last_known_info: None };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("no registered effects, no ordering choice possible");
        };
        assert!(
            matches!(resolved, GameEventKind::ZoneChange { to: ZoneKind::Graveyard, .. }),
            "CR 111.8 explicitly covers \"come back onto the battlefield\" too: {resolved:?}"
        );
    }

    #[test]
    fn token_zone_lock_allows_the_first_move_off_the_battlefield() {
        let id = ObjectId(0);
        let token = GameObject { zone: ZoneKind::Battlefield, is_token: true, ..object(id, PlayerId(0), CardTypeSet::CREATURE) };
        let mut state = state_with(vec![token]);
        let event = GameEventKind::ZoneChange { object: id, from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, last_known_info: None };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("no registered effects, no ordering choice possible");
        };
        assert!(
            matches!(resolved, GameEventKind::ZoneChange { to: ZoneKind::Graveyard, .. }),
            "a token's first move off the battlefield isn't covered by CR 111.8: {resolved:?}"
        );
    }

    #[test]
    fn token_zone_lock_ignores_non_token_objects() {
        let id = ObjectId(0);
        // Not a token (object()'s own default) - CR 111.8 is token-specific.
        let card = GameObject { zone: ZoneKind::Graveyard, ..object(id, PlayerId(0), CardTypeSet::CREATURE) };
        let mut state = state_with(vec![card]);
        let event = GameEventKind::ZoneChange { object: id, from: ZoneKind::Graveyard, to: ZoneKind::Exile, last_known_info: None };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("no registered effects, no ordering choice possible");
        };
        assert!(
            matches!(resolved, GameEventKind::ZoneChange { to: ZoneKind::Exile, .. }),
            "a non-token card already off the battlefield isn't locked in place: {resolved:?}"
        );
    }
}
