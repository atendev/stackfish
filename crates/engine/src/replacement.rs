use crate::script::ScriptRuntime;
use crate::types::ability::CopyExceptions;
use crate::types::card::CardTypeSet;
use crate::types::cost::FilterController;
use crate::types::counter::CounterKind;
use crate::types::effect::{ConsumptionPolicy, EventFilter, ReplacementPayload, ReplacementSpec};
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::mana::ColorSet;
use crate::types::zone::ZoneKind;

/// One CR 616.1 ordering-choice candidate. `Registered` is identified by its
/// effect's `source`; `Protection` (CR 702.16e) by the protected object's id,
/// since protection is not in `state.replacement_effects`. Separate variants
/// keep the two apart when they share an object id.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplacementCandidate {
    Registered(ObjectId),
    Protection(ObjectId),
}

/// What `apply_replacements` produced for one proposed event. CR 616.1 has the
/// affected player choose the order, so that case is a value the caller handles.
pub enum ReplacementOutcome {
    /// The event's final form, ready for `CommitEvents`.
    Resolved(GameEventKind),
    /// CR 616.1: `affected_player` must choose which of `candidates` applies first.
    /// `excluded` holds the effects already applied to this event and must be
    /// passed back when resolution resumes.
    NeedsOrderingChoice {
        affected_player: PlayerId,
        event: GameEventKind,
        candidates: Vec<ReplacementCandidate>,
        excluded: Vec<ReplacementCandidate>,
    },
}

/// CR 616.1: the permanent's controller if the affected slot is an object,
/// otherwise the player. The fallback keeps the function total for a shape it
/// cannot construct.
fn affected_player_for(
    event: &GameEventKind,
    state: &GameState,
    script_runtime: &ScriptRuntime,
) -> PlayerId {
    let fallback = || {
        state
            .turn_order
            .first()
            .copied()
            .expect("a game always has players")
    };
    match event {
        GameEventKind::DamageDealt { target, .. } => match target {
            Target::Player(pid) => *pid,
            Target::Object(id) => state
                .object(*id)
                .map(|_| crate::layers::effective_controller(state, script_runtime, *id))
                .unwrap_or_else(fallback),
            // Damage is never dealt to an ability.
            Target::Ability(_) => fallback(),
        },
        GameEventKind::ZoneChange { object, .. } => state
            .object(*object)
            .map(|_| crate::layers::effective_controller(state, script_runtime, *object))
            .unwrap_or_else(fallback),
        _ => fallback(),
    }
}

/// Applies `source`'s `ReplacementEffect` to `event` and consumes it per its
/// `ConsumptionPolicy`. Returns `event` unchanged for a stale `source`.
pub(crate) fn apply_one_replacement(
    state: &mut GameState,
    source: ObjectId,
    event: GameEventKind,
) -> GameEventKind {
    let Some(idx) = state
        .replacement_effects
        .iter()
        .position(|r| r.source == source)
    else {
        return event;
    };
    apply_replacement_at_index(state, idx, event)
}

/// Index-based body of `apply_one_replacement`, for callers where `source` is not unique.
fn apply_replacement_at_index(
    state: &mut GameState,
    idx: usize,
    event: GameEventKind,
) -> GameEventKind {
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

/// Applies the chosen candidate. `Protection` zeroes the damage directly, since
/// it has no registered entry. Eligibility is not rechecked: the caller built
/// `matching`, and `Engine::submit_replacement_choice` validates `chosen`.
pub(crate) fn apply_one_replacement_or_protection(
    state: &mut GameState,
    chosen: ReplacementCandidate,
    event: GameEventKind,
) -> GameEventKind {
    match chosen {
        ReplacementCandidate::Registered(source) => apply_one_replacement(state, source, event),
        ReplacementCandidate::Protection(_) => match event {
            GameEventKind::DamageDealt {
                source,
                target,
                is_combat_damage,
                ..
            } => GameEventKind::DamageDealt {
                source,
                target,
                amount: 0,
                is_combat_damage,
            },
            other => other,
        },
    }
}

/// CR 702.16a: whether protection prevents `event`'s damage, across all six
/// protected qualities. A predicate rather than a registered effect, because
/// protection is a live characteristics query. `apply_replacements_from` folds
/// the result into its candidate list so it joins the ordering choice (CR 616.1e).
///
/// Returns the protected object's id if protection zeroes the damage. It is
/// re-evaluated each loop pass, since a redirect can change the target.
fn protection_prevents(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    event: &GameEventKind,
) -> Option<ObjectId> {
    let GameEventKind::DamageDealt { source, target, .. } = event else {
        return None;
    };
    let Target::Object(target_id) = target else {
        return None;
    };
    let (Some(source_obj), Some(target_obj)) = (state.object(*source), state.object(*target_id))
    else {
        return None;
    };
    let source_controller = source_obj.controller;
    let target_controller = target_obj.controller;
    let target_characteristics =
        crate::layers::effective_characteristics(state, script_runtime, *target_id);
    let source_characteristics =
        crate::layers::effective_characteristics(state, script_runtime, *source);
    let protected = target_characteristics.protection_everything
        || target_characteristics
            .protection_colors
            .intersects(source_characteristics.colors)
        || target_characteristics
            .protection_card_types
            .intersects(source_characteristics.card_types)
        || target_characteristics
            .protection_subtypes
            .iter()
            .any(|s| source_characteristics.subtypes.contains(s))
        || target_characteristics
            .protection_names
            .contains(&source_characteristics.name)
        || target_characteristics
            .protection_players
            .iter()
            .any(|who| match who {
                FilterController::You => source_controller == target_controller,
                FilterController::Opponent => source_controller != target_controller,
                FilterController::Any => false,
            });
    protected.then_some(*target_id)
}

/// CR 111.8: a token that has left the battlefield stays in its current zone.
/// It applies to every token, so it runs ahead of the registered effects. Only
/// `to` is rewritten; the caller still commits a no-op `ZoneChange`.
///
/// Unreachable in play today, so the tests call `apply_replacements` directly.
fn apply_token_zone_lock(state: &GameState, event: GameEventKind) -> GameEventKind {
    let GameEventKind::ZoneChange {
        object,
        from,
        to,
        last_known_info,
    } = event
    else {
        return event;
    };
    let Some(obj) = state.object(object) else {
        return GameEventKind::ZoneChange {
            object,
            from,
            to,
            last_known_info,
        };
    };
    if obj.is_token && obj.zone != ZoneKind::Battlefield {
        return GameEventKind::ZoneChange {
            object,
            from,
            to: from,
            last_known_info,
        };
    }
    GameEventKind::ZoneChange {
        object,
        from,
        to,
        last_known_info,
    }
}

/// Whether `spec` is a battlefield-entry modifier (tapped, the counters family,
/// copy). These skip the CR 616.1 ordering choice: `apply_enters_battlefield_modifiers`
/// applies the order-independent ones directly and the `pending_*_choice`
/// functions handle the rest. Without the exclusion, a creature with both
/// `EntersTapped` and `EntersWithCounters` raised a spurious ordering pause (CR 707.2).
fn is_battlefield_entry_modifier(spec: &ReplacementSpec) -> bool {
    matches!(
        spec,
        ReplacementSpec::Primitive {
            payload: ReplacementPayload::EntersTapped
                | ReplacementPayload::EntersWithCounters { .. }
                | ReplacementPayload::EntersWithCountersBloodthirst { .. }
                | ReplacementPayload::EntersWithCountersSunburst
                | ReplacementPayload::EntersWithCountersDevour { .. }
                | ReplacementPayload::EntersWithCountersAmplify { .. }
                | ReplacementPayload::EntersWithCountersEscape { .. }
                | ReplacementPayload::EntersAsCopy(_),
            ..
        }
    )
}

/// CR 614, 615, 616: rewrites `proposed` by every matching replacement effect,
/// rechecking after each rewrite until it stabilizes. The entry point, with
/// nothing excluded.
pub fn apply_replacements(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    proposed: GameEventKind,
    is_destroy: bool,
) -> ReplacementOutcome {
    apply_replacements_from(state, script_runtime, proposed, is_destroy, Vec::new())
}

/// Body of `apply_replacements`, resumable with the `excluded` set from a paused
/// `NeedsOrderingChoice`.
///
/// CR 616.1: an effect applies only once to a given event, so `excluded` keeps a
/// repeatable effect from rematching its own output and looping forever.
pub(crate) fn apply_replacements_from(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    proposed: GameEventKind,
    is_destroy: bool,
    mut excluded: Vec<ReplacementCandidate>,
) -> ReplacementOutcome {
    let mut event = proposed;
    loop {
        // CR 111.8
        event = apply_token_zone_lock(state, event);

        // CR 702.16a, 616.1: protection joins `matching` as a candidate.
        let protection_candidate = protection_prevents(state, script_runtime, &event)
            .map(ReplacementCandidate::Protection)
            .filter(|c| !excluded.contains(c));

        if state.replacement_effects.is_empty() && protection_candidate.is_none() {
            return ReplacementOutcome::Resolved(event);
        }
        let mut matching: Vec<ReplacementCandidate> = state
            .replacement_effects
            .iter()
            .filter(|r| {
                !excluded.contains(&ReplacementCandidate::Registered(r.source))
                    && !is_battlefield_entry_modifier(&r.spec)
                    && r.matches(&event, is_destroy, state, script_runtime)
            })
            .map(|r| ReplacementCandidate::Registered(r.source))
            .collect();
        if let Some(candidate) = protection_candidate {
            // `Registered(X)` and `Protection(X)` stay distinct even when they share an object id.
            matching.push(candidate);
        }

        match matching.as_slice() {
            [] => return ReplacementOutcome::Resolved(event),
            [only] => {
                // CR 616: apply the only match, then loop to recheck the rewritten form.
                event = apply_one_replacement_or_protection(state, *only, event);
                excluded.push(*only);
            }
            _ => {
                let affected_player = affected_player_for(&event, state, script_runtime);
                return ReplacementOutcome::NeedsOrderingChoice {
                    affected_player,
                    event,
                    candidates: matching,
                    excluded,
                };
            }
        }
    }
}

/// CR 614.1c, 113.6h: applies every registered `EntersTapped` and
/// `EntersWithCounters` effect matching `object`'s move onto the battlefield,
/// whether self-registered or granted. It bypasses `apply_replacements` because
/// neither payload has a field on `ZoneChange` to rewrite.
///
/// `EntersTapped` mutates the object directly. The counter payloads only propose
/// `CounterAdded` events, which the caller must push down a path that reaches
/// `CommitEvents`, or the counters apply twice.
///
/// Call it after `object` is in `state.objects` and its card script has run, so
/// a self-registered effect is present.
///
/// The CR 616.1 ordering choice is skipped because the matched payloads are
/// order-independent: `EntersTapped` is idempotent and counters are additive.
///
/// Returns one `CounterAdded` per matching counter effect.
///
/// `colors_spent` (CR 702.44) is the distinct colors paid for the cast, known
/// only when a spell resolves from the stack. The empty set elsewhere yields no
/// Sunburst counters (CR 702.44b).
// TODO: a payload on the same `WouldMoveZone` battlefield filter that is order-dependent needs its own ordering pause
pub fn apply_enters_battlefield_modifiers(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    object: ObjectId,
    from: ZoneKind,
    colors_spent: ColorSet,
) -> Vec<GameEventKind> {
    // Rebuilt per use because `GameEventKind` is not `Clone`; it is matched, never committed.
    let candidate = || GameEventKind::ZoneChange {
        object,
        from,
        to: ZoneKind::Battlefield,
        last_known_info: None,
    };
    // Indices, not source ids: one source can carry several matching payloads (CR 707.2).
    let matching: Vec<usize> = state
        .replacement_effects
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            matches!(
                &r.spec,
                ReplacementSpec::Primitive {
                    payload: ReplacementPayload::EntersTapped
                        | ReplacementPayload::EntersWithCounters { .. }
                        | ReplacementPayload::EntersWithCountersBloodthirst { .. }
                        | ReplacementPayload::EntersWithCountersSunburst
                        | ReplacementPayload::EntersWithCountersEscape { .. },
                    ..
                }
            )
        })
        .filter(|(_, r)| r.matches(&candidate(), false, state, script_runtime))
        .map(|(i, _)| i)
        .collect();

    let mut counter_events = Vec::new();
    // Reverse order so a removal never invalidates a lower index still to process.
    for idx in matching.into_iter().rev() {
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
            // Only proposes the event; the caller commits it (see the doc comment).
            ReplacementPayload::EntersWithCounters { kind, amount } => {
                counter_events.push(GameEventKind::CounterAdded {
                    object,
                    kind,
                    amount,
                });
            }
            // CR 702.54a: gated on an opponent having been dealt damage this turn.
            ReplacementPayload::EntersWithCountersBloodthirst { kind, amount } => {
                let controller = state.object(object).map(|o| o.controller);
                if controller.is_some_and(|c| opponent_damaged_this_turn(state, c)) {
                    counter_events.push(GameEventKind::CounterAdded {
                        object,
                        kind,
                        amount,
                    });
                }
            }
            // CR 702.138c: gated on `entered_via_escape`, already set on `object`.
            ReplacementPayload::EntersWithCountersEscape { kind, amount } => {
                if state.object(object).is_some_and(|o| o.entered_via_escape) {
                    counter_events.push(GameEventKind::CounterAdded {
                        object,
                        kind,
                        amount,
                    });
                }
            }
            // CR 702.44a: kind and amount are derived here; `ColorSet` has one bit per color.
            ReplacementPayload::EntersWithCountersSunburst => {
                let amount = colors_spent.bits().count_ones() as i32;
                if amount > 0 {
                    let kind = if state
                        .object(object)
                        .is_some_and(|o| o.definition.card_types.contains(CardTypeSet::CREATURE))
                    {
                        CounterKind::PlusOnePlusOne
                    } else {
                        CounterKind::Charge
                    };
                    counter_events.push(GameEventKind::CounterAdded {
                        object,
                        kind,
                        amount,
                    });
                }
            }
            _ => continue,
        }
        // CR 614.13: consumption by index; the returned event is discarded since neither payload rewrites one.
        apply_replacement_at_index(state, idx, candidate());
    }
    counter_events
}

/// CR 702.82a (Devour): the pending Devour choice for `object`, if it carries
/// an `EntersWithCountersDevour` effect. Checked apart from
/// `apply_enters_battlefield_modifiers`, which skips this payload, by callers
/// that can raise a choice pause: `ResolveStackTop` and `Search`'s battlefield
/// entry. `PlayLand` and `CreateToken` never call it, so a Devour effect there
/// grants no counters.
///
/// Returns `Some((per_creature, candidates))`: the creatures `controller`
/// controls, narrowed to `quality` if set (CR 702.82c), `object` included.
/// `None` if no such effect is registered or there is no candidate, since zero
/// is a legal choice that needs no pause (CR 702.82a).
pub fn pending_devour_choice(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    object: ObjectId,
    from: ZoneKind,
    controller: PlayerId,
) -> Option<(i32, Vec<ObjectId>)> {
    let candidate = GameEventKind::ZoneChange {
        object,
        from,
        to: ZoneKind::Battlefield,
        last_known_info: None,
    };
    let (per_creature, quality) = state
        .replacement_effects
        .iter()
        .find_map(|r| match &r.spec {
            ReplacementSpec::Primitive {
                payload:
                    ReplacementPayload::EntersWithCountersDevour {
                        per_creature,
                        quality,
                    },
                ..
            } if r.matches(&candidate, false, state, script_runtime) => {
                Some((*per_creature, quality.clone()))
            }
            _ => None,
        })?;
    let candidates: Vec<ObjectId> = state
        .objects
        .values()
        .filter(|obj| {
            if obj.zone != ZoneKind::Battlefield || obj.controller != controller {
                return false;
            }
            let effective =
                crate::layers::effective_characteristics(state, script_runtime, obj.object_id);
            if !effective.card_types.contains(CardTypeSet::CREATURE) {
                return false;
            }
            // CR 702.82c
            if let Some(quality) = &quality
                && !effective.subtypes.contains(quality)
            {
                return false;
            }
            true
        })
        .map(|obj| obj.object_id)
        .collect();
    if candidates.is_empty() {
        return None;
    }
    Some((per_creature, candidates))
}

/// CR 702.38a (Amplify): the pending Amplify choice for `object`, if it carries
/// an `EntersWithCountersAmplify` effect. Same callers and separation as
/// `pending_devour_choice`.
///
/// Returns `Some((per_card, candidates))`: the cards in `controller`'s hand that
/// share a subtype with `object`, which approximates "creature type". `None`
/// if no effect is registered, `object` has no subtypes, or nothing matches.
pub fn pending_amplify_choice(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    object: ObjectId,
    from: ZoneKind,
    controller: PlayerId,
) -> Option<(i32, Vec<ObjectId>)> {
    let candidate = GameEventKind::ZoneChange {
        object,
        from,
        to: ZoneKind::Battlefield,
        last_known_info: None,
    };
    let per_card = state
        .replacement_effects
        .iter()
        .find_map(|r| match &r.spec {
            ReplacementSpec::Primitive {
                payload: ReplacementPayload::EntersWithCountersAmplify { per_card },
                ..
            } if r.matches(&candidate, false, state, script_runtime) => Some(*per_card),
            _ => None,
        })?;
    let object_subtypes =
        crate::layers::effective_characteristics(state, script_runtime, object).subtypes;
    if object_subtypes.is_empty() {
        return None;
    }
    let candidates: Vec<ObjectId> = state
        .objects
        .values()
        .filter(|obj| {
            obj.zone == ZoneKind::Hand
                && obj.controller == controller
                && crate::layers::effective_characteristics(state, script_runtime, obj.object_id)
                    .subtypes
                    .iter()
                    .any(|s| object_subtypes.contains(s))
        })
        .map(|obj| obj.object_id)
        .collect();
    if candidates.is_empty() {
        return None;
    }
    Some((per_card, candidates))
}

/// CR 707.6, 614.12a (Clone): the pending copy choice for `object`, if it
/// carries an `EntersAsCopy` effect. Same separation as `pending_devour_choice`,
/// with no `controller` since any creature on the battlefield qualifies.
///
/// Returns `Some((exceptions, candidates))`: every creature on the battlefield,
/// `object` included. `None` if no effect is registered or no creature exists.
pub fn pending_copy_choice(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    object: ObjectId,
    from: ZoneKind,
) -> Option<(CopyExceptions, Vec<ObjectId>)> {
    let candidate = GameEventKind::ZoneChange {
        object,
        from,
        to: ZoneKind::Battlefield,
        last_known_info: None,
    };
    let exceptions = state
        .replacement_effects
        .iter()
        .find_map(|r| match &r.spec {
            ReplacementSpec::Primitive {
                payload: ReplacementPayload::EntersAsCopy(exceptions),
                ..
            } if r.matches(&candidate, false, state, script_runtime) => Some(exceptions.clone()),
            _ => None,
        })?;
    let candidates: Vec<ObjectId> = state
        .objects
        .values()
        .filter(|obj| {
            obj.zone == ZoneKind::Battlefield
                && crate::layers::effective_characteristics(state, script_runtime, obj.object_id)
                    .card_types
                    .contains(CardTypeSet::CREATURE)
        })
        .map(|obj| obj.object_id)
        .collect();
    if candidates.is_empty() {
        return None;
    }
    Some((exceptions, candidates))
}

/// CR 702.54a: whether an opponent of `controller` was dealt damage this turn,
/// combat or not. The turn starts after the latest `TurnStarted` in the ledger,
/// or at the ledger's start if there is none. Damage to permanents does not count.
pub fn opponent_damaged_this_turn(state: &GameState, controller: PlayerId) -> bool {
    let turn_start = state
        .ledger
        .iter()
        .rposition(|e| matches!(e.kind, GameEventKind::TurnStarted { .. }));
    let scan_from = turn_start.map(|i| i + 1).unwrap_or(0);
    state.ledger[scan_from..]
        .iter()
        .any(|e| matches!(&e.kind, GameEventKind::DamageDealt { target: Target::Player(p), .. } if *p != controller))
}

/// CR 702.35a: builds the `MadnessExiled` marker for a resolved discard whose
/// exile destination matches a madness redirect registered on the card itself.
/// Call it against the post-replacement, pre-commit `event`, because
/// `retire_object` later purges the card's own replacement entry. Shared by the
/// cleanup-discard and cost-payment discard paths.
// TODO: track which exile-instead-of-discard effect won the CR 616.1 ordering choice when more than one applies
pub fn madness_marker_event(state: &GameState, event: &GameEventKind) -> Option<GameEventKind> {
    let GameEventKind::ZoneChange {
        object,
        from: ZoneKind::Hand,
        to: ZoneKind::Exile,
        last_known_info: Some(lki),
    } = event
    else {
        return None;
    };
    let has_own_madness_redirect = state.replacement_effects.iter().any(|effect| {
        if effect.source != *object {
            return false;
        }
        matches!(
            &effect.spec,
            ReplacementSpec::Primitive {
                filter: EventFilter::WouldMoveZone { from, to, .. },
                payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
            } if from.is_none_or(|f| f == ZoneKind::Hand) && to.is_none_or(|t| t == ZoneKind::Graveyard)
        )
    });
    if !has_own_madness_redirect {
        return None;
    }
    Some(GameEventKind::MadnessExiled {
        object: *object,
        last_known_info: lki.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::card::{
        CardDefinition, CardTypeSet, KeywordSet, ModeChoice, PTValue, ScriptReference, SupertypeSet,
    };
    use crate::types::cost::FilterController;
    use crate::types::counter::CounterSet;
    use crate::types::effect::{
        EffectFilter, EventFilter, ReplacementEffect, ReplacementPayload, ReplacementSpec,
    };
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
                mana_cost: crate::types::mana::ManaCost {
                    symbols: Vec::new(),
                },
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
                buyback_cost: None,
                effect: None,
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
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
            attached_to_player: None,
            timestamp: SequenceNo(0),
            controlled_since_turn: 0,
            is_commander: false,
            is_token: false,
            visibility: VisibilityPolicy {
                public: true,
                visible_to: Vec::new(),
            },
            copy_generation: 0,
            entered_via_escape: false,
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
            hand_reveal_modifiers: Vec::new(),
            draw_limit_modifiers: Vec::new(),
            land_play_modifiers: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState {
                seed: 1,
                draws_consumed: 0,
            },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 1000,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
            linked_ability_generation: std::collections::HashMap::new(),
            script_ability_ids: std::collections::HashSet::new(),
            ability_function_zones: std::collections::HashMap::new(),
            off_battlefield_scripts_run: std::collections::HashSet::new(),
            linked_exiles: Vec::new(),
            pending_timing_bypass: None,
            pending_free_cast: None,
        }
    }

    fn no_filter() -> EffectFilter {
        EffectFilter {
            card_types: None,
            controller: None,
            self_only: false,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
        }
    }

    #[test]
    fn resolves_unchanged_with_nothing_registered() {
        let mut state = state_with(Vec::new());
        let event = GameEventKind::DamageDealt {
            source: ObjectId(0),
            target: Target::Player(PlayerId(0)),
            amount: 5,
            is_combat_damage: false,
        };
        let ReplacementOutcome::Resolved(resolved) =
            apply_replacements(&mut state, &ScriptRuntime::new(), event, false)
        else {
            panic!("expected Resolved with no replacement effects registered");
        };
        assert!(matches!(
            resolved,
            GameEventKind::DamageDealt { amount: 5, .. }
        ));
    }

    #[test]
    fn resolves_unchanged_when_nothing_matches() {
        let effect_source = ObjectId(0);
        let mut state = state_with(vec![object(
            effect_source,
            PlayerId(0),
            CardTypeSet::ARTIFACT,
        )]);
        state.replacement_effects.push(ReplacementEffect {
            source: effect_source,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt {
                    affected: EffectFilter {
                        card_types: Some(CardTypeSet::CREATURE),
                        ..no_filter()
                    },
                },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::Repeatable,
        });
        // The target is an Artifact, so the creature filter does not match.
        let event = GameEventKind::DamageDealt {
            source: ObjectId(1),
            target: Target::Object(effect_source),
            amount: 5,
            is_combat_damage: false,
        };
        let ReplacementOutcome::Resolved(resolved) =
            apply_replacements(&mut state, &ScriptRuntime::new(), event, false)
        else {
            panic!("expected Resolved - filter doesn't match");
        };
        assert!(matches!(
            resolved,
            GameEventKind::DamageDealt { amount: 5, .. }
        ));
    }

    #[test]
    fn one_shot_effect_applies_once_then_is_removed() {
        let effect_source = ObjectId(0);
        let mut state = state_with(vec![object(
            effect_source,
            PlayerId(0),
            CardTypeSet::ARTIFACT,
        )]);
        state.replacement_effects.push(ReplacementEffect {
            source: effect_source,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt {
                    affected: no_filter(),
                },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::OneShot,
        });

        let event = GameEventKind::DamageDealt {
            source: ObjectId(1),
            target: Target::Player(PlayerId(0)),
            amount: 5,
            is_combat_damage: false,
        };
        let ReplacementOutcome::Resolved(resolved) =
            apply_replacements(&mut state, &ScriptRuntime::new(), event, false)
        else {
            panic!("expected a single match to resolve");
        };
        assert!(matches!(
            resolved,
            GameEventKind::DamageDealt { amount: 0, .. }
        ));
        assert!(
            state.replacement_effects.is_empty(),
            "OneShot must be consumed after applying"
        );
    }

    #[test]
    fn limited_uses_decrements_then_expires() {
        let effect_source = ObjectId(0);
        let mut state = state_with(vec![object(
            effect_source,
            PlayerId(0),
            CardTypeSet::ARTIFACT,
        )]);
        state.replacement_effects.push(ReplacementEffect {
            source: effect_source,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt {
                    affected: no_filter(),
                },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::LimitedUses(2),
        });

        let event = || GameEventKind::DamageDealt {
            source: ObjectId(1),
            target: Target::Player(PlayerId(0)),
            amount: 5,
            is_combat_damage: false,
        };
        apply_replacements(&mut state, &ScriptRuntime::new(), event(), false);
        assert_eq!(
            state.replacement_effects.first().map(|r| r.consumption),
            Some(ConsumptionPolicy::LimitedUses(1)),
            "first use decrements 2 -> 1, doesn't remove"
        );

        apply_replacements(&mut state, &ScriptRuntime::new(), event(), false);
        assert!(
            state.replacement_effects.is_empty(),
            "second use exhausts LimitedUses(1) and removes it"
        );
    }

    #[test]
    fn repeatable_effect_is_never_consumed() {
        let effect_source = ObjectId(0);
        let mut state = state_with(vec![object(
            effect_source,
            PlayerId(0),
            CardTypeSet::ARTIFACT,
        )]);
        state.replacement_effects.push(ReplacementEffect {
            source: effect_source,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt {
                    affected: no_filter(),
                },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::Repeatable,
        });

        let event = || GameEventKind::DamageDealt {
            source: ObjectId(1),
            target: Target::Player(PlayerId(0)),
            amount: 5,
            is_combat_damage: false,
        };
        apply_replacements(&mut state, &ScriptRuntime::new(), event(), false);
        apply_replacements(&mut state, &ScriptRuntime::new(), event(), false);
        assert_eq!(
            state.replacement_effects.len(),
            1,
            "Repeatable is never removed or decremented"
        );
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
                    filter: EventFilter::DamageDealt {
                        affected: no_filter(),
                    },
                    payload: ReplacementPayload::PreventDamage,
                },
                consumption: ConsumptionPolicy::Repeatable,
            });
        }

        let event = GameEventKind::DamageDealt {
            source: ObjectId(2),
            target: Target::Player(PlayerId(0)),
            amount: 5,
            is_combat_damage: false,
        };
        let ReplacementOutcome::NeedsOrderingChoice {
            affected_player,
            candidates,
            ..
        } = apply_replacements(&mut state, &ScriptRuntime::new(), event, false)
        else {
            panic!("expected a pause - two effects match the same event");
        };
        assert_eq!(
            affected_player,
            PlayerId(0),
            "damage to a player - that player is affected"
        );
        assert_eq!(candidates.len(), 2);
        assert!(
            candidates.contains(&ReplacementCandidate::Registered(a))
                && candidates.contains(&ReplacementCandidate::Registered(b))
        );
        assert_eq!(state.replacement_effects.len(), 2);
    }

    #[test]
    fn stabilizes_across_two_different_replacements() {
        // CR 616.1: A redirects p1's damage to p0, and only that rewrite matches B.
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let source_a = ObjectId(0);
        let source_b = ObjectId(1);
        let mut state = state_with(vec![
            object(source_a, p1, CardTypeSet::ARTIFACT),
            object(source_b, p0, CardTypeSet::ARTIFACT),
        ]);
        state.replacement_effects.push(ReplacementEffect {
            source: source_a,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt {
                    affected: EffectFilter {
                        controller: Some(FilterController::You),
                        ..no_filter()
                    },
                },
                payload: ReplacementPayload::RedirectDamage(Target::Player(p0)),
            },
            consumption: ConsumptionPolicy::Repeatable,
        });
        state.replacement_effects.push(ReplacementEffect {
            source: source_b,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt {
                    affected: EffectFilter {
                        controller: Some(FilterController::You),
                        ..no_filter()
                    },
                },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::Repeatable,
        });

        let event = GameEventKind::DamageDealt {
            source: ObjectId(2),
            target: Target::Player(p1),
            amount: 5,
            is_combat_damage: false,
        };
        let ReplacementOutcome::Resolved(resolved) =
            apply_replacements(&mut state, &ScriptRuntime::new(), event, false)
        else {
            panic!(
                "only one effect matches at any single point - never a real multi-match pause here"
            );
        };
        assert!(
            matches!(resolved, GameEventKind::DamageDealt { target: Target::Player(pid), amount: 0, .. } if pid == p0),
            "redirected to p0 by A, then prevented by B: {resolved:?}"
        );
    }

    // CR 111.8: no producer builds this state in play, so these call `apply_replacements` directly.

    #[test]
    fn token_zone_lock_blocks_a_second_move_for_an_already_departed_token() {
        let id = ObjectId(0);
        let token = GameObject {
            zone: ZoneKind::Graveyard,
            is_token: true,
            ..object(id, PlayerId(0), CardTypeSet::CREATURE)
        };
        let mut state = state_with(vec![token]);
        let event = GameEventKind::ZoneChange {
            object: id,
            from: ZoneKind::Graveyard,
            to: ZoneKind::Exile,
            last_known_info: None,
        };
        let ReplacementOutcome::Resolved(resolved) =
            apply_replacements(&mut state, &ScriptRuntime::new(), event, false)
        else {
            panic!("no registered effects, no ordering choice possible");
        };
        assert!(
            matches!(
                resolved,
                GameEventKind::ZoneChange {
                    to: ZoneKind::Graveyard,
                    ..
                }
            ),
            "CR 111.8: the token must stay in its current zone instead of moving to Exile: {resolved:?}"
        );
    }

    #[test]
    fn token_zone_lock_blocks_a_departed_token_from_returning_to_the_battlefield() {
        let id = ObjectId(0);
        let token = GameObject {
            zone: ZoneKind::Graveyard,
            is_token: true,
            ..object(id, PlayerId(0), CardTypeSet::CREATURE)
        };
        let mut state = state_with(vec![token]);
        let event = GameEventKind::ZoneChange {
            object: id,
            from: ZoneKind::Graveyard,
            to: ZoneKind::Battlefield,
            last_known_info: None,
        };
        let ReplacementOutcome::Resolved(resolved) =
            apply_replacements(&mut state, &ScriptRuntime::new(), event, false)
        else {
            panic!("no registered effects, no ordering choice possible");
        };
        assert!(
            matches!(
                resolved,
                GameEventKind::ZoneChange {
                    to: ZoneKind::Graveyard,
                    ..
                }
            ),
            "CR 111.8 explicitly covers \"come back onto the battlefield\" too: {resolved:?}"
        );
    }

    #[test]
    fn token_zone_lock_allows_the_first_move_off_the_battlefield() {
        let id = ObjectId(0);
        let token = GameObject {
            zone: ZoneKind::Battlefield,
            is_token: true,
            ..object(id, PlayerId(0), CardTypeSet::CREATURE)
        };
        let mut state = state_with(vec![token]);
        let event = GameEventKind::ZoneChange {
            object: id,
            from: ZoneKind::Battlefield,
            to: ZoneKind::Graveyard,
            last_known_info: None,
        };
        let ReplacementOutcome::Resolved(resolved) =
            apply_replacements(&mut state, &ScriptRuntime::new(), event, false)
        else {
            panic!("no registered effects, no ordering choice possible");
        };
        assert!(
            matches!(
                resolved,
                GameEventKind::ZoneChange {
                    to: ZoneKind::Graveyard,
                    ..
                }
            ),
            "a token's first move off the battlefield isn't covered by CR 111.8: {resolved:?}"
        );
    }

    #[test]
    fn token_zone_lock_ignores_non_token_objects() {
        let id = ObjectId(0);
        // CR 111.8: applies to tokens only.
        let card = GameObject {
            zone: ZoneKind::Graveyard,
            ..object(id, PlayerId(0), CardTypeSet::CREATURE)
        };
        let mut state = state_with(vec![card]);
        let event = GameEventKind::ZoneChange {
            object: id,
            from: ZoneKind::Graveyard,
            to: ZoneKind::Exile,
            last_known_info: None,
        };
        let ReplacementOutcome::Resolved(resolved) =
            apply_replacements(&mut state, &ScriptRuntime::new(), event, false)
        else {
            panic!("no registered effects, no ordering choice possible");
        };
        assert!(
            matches!(
                resolved,
                GameEventKind::ZoneChange {
                    to: ZoneKind::Exile,
                    ..
                }
            ),
            "a non-token card already off the battlefield isn't locked in place: {resolved:?}"
        );
    }
}
