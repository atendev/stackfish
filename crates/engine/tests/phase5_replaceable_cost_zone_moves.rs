//! Phase 5 vertical slice: `Sacrifice`/`Discard` cost payments are
//! themselves replaceable zone-change proposals (CR 601.2h/616.1) - the real
//! follow-up `phase5/eventfilter-would-move-zone`'s own UNRESOLVED notes
//! named: `commands::commit_payment` used to mutate `Sacrifice`/`Discard`
//! directly, never calling `apply_replacements` at all, so "if a permanent
//! you control would be sacrificed, exile it instead" (or the Discard
//! equivalent) was inert no matter how the replacement effect itself was
//! built.
//!
//! `Engine::submit_payment` now collects `Sacrifice`/`Discard` payments
//! separately from the immediate ones (mana/tap/life) and drives them
//! through `Engine::continue_cost_zone_moves` - the same propose,
//! replacement-check, then commit split `sba::resolve_deaths`/
//! `Engine::continue_scripted_zone_move` already established, reusing this
//! phase's own `EventFilter::WouldMoveZone` to match a from-zone `WouldDie`
//! can't express (Hand, for Discard).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

fn vanilla_definition(name: &str, card_types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(1)),
        toughness: Some(PTValue::Fixed(1)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn ward_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Ward".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone,
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
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

/// One player, main phase, empty stack - a battlefield `source` whose only
/// ability has `cost`, plus sacrifice fodder, a hand card, and a `ward`
/// permanent already carrying a self-registered replacement effect
/// (registered via `run_card_script` before the state is handed to
/// `Engine::new`, same "self-registration happens the same way PlayLand/
/// CastSpell resolution already drives it for real" convention
/// `phase5_replacement_effect_shapes.rs` established).
fn state_with_cost_and_ward(cost: CostSpec, ward_script: &str) -> (GameState, PlayerId, ObjectId, ObjectId, ObjectId) {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let fodder = ObjectId(1);
    let hand_card = ObjectId(2);
    let ward = ObjectId(3);

    let mut objects = HashMap::new();
    objects.insert(source, object(source, vanilla_definition("Source", CardTypeSet::ARTIFACT), p0, ZoneKind::Battlefield));
    objects.insert(fodder, object(fodder, vanilla_definition("Fodder", CardTypeSet::CREATURE), p0, ZoneKind::Battlefield));
    objects.insert(hand_card, object(hand_card, vanilla_definition("Hand Card", CardTypeSet::CREATURE), p0, ZoneKind::Hand));
    let ward_definition = ward_definition(ward_script);
    objects.insert(ward, object(ward, ward_definition.clone(), p0, ZoneKind::Battlefield));

    let mut state = GameState {
        players: vec![PlayerState {
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
        }],
        turn_order: vec![p0],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: { let mut m = HashMap::new(); m.insert(p0, 1); m },
        objects,
        zones: vec![
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::from([source, fodder, ward]) },
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::from([hand_card]) },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Exile, owner: None, contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        hand_size_modifiers: Vec::new(),
        abilities: vec![RegisteredAbility {
            source,
            kind: AbilityKind::Activated,
            condition: None,
            cost: Some(cost),
            target: None,
            operation: ScriptHandle(0),
            effect: Some(AbilityEffect::GainLife(1)),
            intervening_if: None,
        }],
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    };

    ScriptRuntime::new().run_card_script(&mut state, &ward_definition, ward).expect("ward script self-registers cleanly");
    assert_eq!(state.replacement_effects.len(), 1, "the ward's own replacement effect must have registered");

    (state, p0, source, fodder, hand_card)
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn activate_and_await_payment(engine: &mut Engine, player: PlayerId, source: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets: Vec::new(), chosen_colors: Vec::new() },
        })
        .expect("player holds priority");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the ability's cost, got {status:?}");
}

fn last_zone_change(engine: &Engine) -> (ObjectId, ZoneKind, ZoneKind) {
    for event in engine.state().ledger.iter().rev() {
        if let GameEventKind::ZoneChange { object, from, to, .. } = event.kind {
            return (object, from, to);
        }
    }
    panic!("expected a ZoneChange event in the ledger");
}

const SACRIFICED_CREATURE_GOES_TO_EXILE: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone("Battlefield", "Graveyard"))
    e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Exile"))
    e:SetAffected({ card_types = { "Creature" } })
    e:Register()
end
return s
"#;

const DISCARDED_CARD_GOES_TO_EXILE: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone("Hand", "Graveyard"))
    e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Exile"))
    e:SetAffected({ card_types = { "Creature" } })
    e:Register()
end
return s
"#;

#[test]
fn a_sacrifice_cost_payment_is_replacement_checked_for_real() {
    let cost = CostSpec {
        mana: None,
        additional: vec![AdditionalCost::Sacrifice {
            filter: FilterSpec { card_types: Some(CardTypeSet::CREATURE), supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        }],
        alternative: Vec::new(),
    };
    let (state, p0, source, fodder, _hand_card) = state_with_cost_and_ward(cost, SACRIFICED_CREATURE_GOES_TO_EXILE);
    let mut engine = Engine::new(state);

    activate_and_await_payment(&mut engine, p0, source);
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Sacrifice(fodder)] })
        .expect("a legal Sacrifice(fodder) payment pays a Sacrifice{Creature} cost");
    drain(&mut engine);

    assert!(engine.state().object(fodder).is_none(), "the original fodder id is retired");
    let (new_id, from, to) = last_zone_change(&engine);
    assert_eq!(
        (from, to),
        (ZoneKind::Battlefield, ZoneKind::Exile),
        "the ward's WouldMoveZone(Battlefield, Graveyard) + ReplaceZoneDestination(Exile) must have \
         redirected the sacrifice into exile instead of the graveyard"
    );
    assert!(engine.state().object(new_id).is_some(), "fodder now exists in exile under a new id");
    assert!(
        engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.is_empty(),
        "the graveyard must be empty - the sacrifice never actually landed there"
    );
}

#[test]
fn a_discard_cost_payment_is_replacement_checked_for_real() {
    let cost = CostSpec {
        mana: None,
        additional: vec![AdditionalCost::Discard {
            filter: FilterSpec { card_types: Some(CardTypeSet::CREATURE), supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        }],
        alternative: Vec::new(),
    };
    let (state, p0, source, _fodder, hand_card) = state_with_cost_and_ward(cost, DISCARDED_CARD_GOES_TO_EXILE);
    let mut engine = Engine::new(state);

    activate_and_await_payment(&mut engine, p0, source);
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Discard(hand_card)] })
        .expect("a legal Discard(hand_card) payment pays a Discard{Creature} cost");
    drain(&mut engine);

    assert!(engine.state().object(hand_card).is_none(), "the original hand_card id is retired");
    let (new_id, from, to) = last_zone_change(&engine);
    assert_eq!(
        (from, to),
        (ZoneKind::Hand, ZoneKind::Exile),
        "the ward's WouldMoveZone(Hand, Graveyard) + ReplaceZoneDestination(Exile) must have \
         redirected the discard into exile instead of the graveyard - a from-zone WouldDie itself \
         can't express"
    );
    assert!(engine.state().object(new_id).is_some(), "the card now exists in exile under a new id");
    assert!(
        engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.is_empty(),
        "the graveyard must be empty - the discard never actually landed there"
    );
}

#[test]
fn a_sacrifice_cost_payment_without_a_matching_replacement_lands_in_the_graveyard_normally() {
    // Sanity check: the ward's own affected filter is Creature-only, and this
    // cost's fodder still matches - so to prove the *absence* of a match
    // path works too, register a ward that only cares about a different
    // card_types filter (Land) instead.
    let cost = CostSpec {
        mana: None,
        additional: vec![AdditionalCost::Sacrifice {
            filter: FilterSpec { card_types: Some(CardTypeSet::CREATURE), supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        }],
        alternative: Vec::new(),
    };
    const LAND_ONLY_WARD: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone("Battlefield", "Graveyard"))
    e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Exile"))
    e:SetAffected({ card_types = { "Land" } })
    e:Register()
end
return s
"#;
    let (state, _p0, source, fodder, _hand_card) = state_with_cost_and_ward(cost, LAND_ONLY_WARD);
    let mut engine = Engine::new(state);

    activate_and_await_payment(&mut engine, PlayerId(0), source);
    engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Sacrifice(fodder)] }).expect("legal payment");
    drain(&mut engine);

    let (_new_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Battlefield, ZoneKind::Graveyard), "a non-matching ward must not redirect the sacrifice");
}

#[test]
fn two_matching_wards_pause_for_a_cr_616_1_ordering_choice() {
    let cost = CostSpec {
        mana: None,
        additional: vec![AdditionalCost::Sacrifice {
            filter: FilterSpec { card_types: Some(CardTypeSet::CREATURE), supertypes: None, subtypes: None, controller: None, random: false, extra: None },
        }],
        alternative: Vec::new(),
    };
    let (mut state, p0, source, fodder, _hand_card) = state_with_cost_and_ward(cost, SACRIFICED_CREATURE_GOES_TO_EXILE);
    let ward_a_source = ObjectId(3);
    assert_eq!(state.replacement_effects.len(), 1, "state_with_cost_and_ward's own single ward");
    let ward_b_source = ObjectId(999);
    state.objects.insert(
        ward_b_source,
        object(ward_b_source, vanilla_definition("Ward B", CardTypeSet::ENCHANTMENT), p0, ZoneKind::Battlefield),
    );
    state.replacement_effects.push(ReplacementEffect {
        source: ward_b_source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Battlefield),
                to: Some(ZoneKind::Graveyard),
                affected: EffectFilter { card_types: Some(CardTypeSet::CREATURE), controller: None, self_only: false },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption: ConsumptionPolicy::Repeatable,
    });

    let mut engine = Engine::new(state);
    activate_and_await_payment(&mut engine, p0, source);
    engine.submit_payment(PaymentPlan { payments: vec![CostPayment::Sacrifice(fodder)] }).expect("legal payment");
    let status = drain(&mut engine);

    let StepStatus::AwaitingReplacementChoice { player, candidates } = status else {
        panic!("two matching replacement effects on a Sacrifice cost payment must pause for a CR 616.1 choice, got {status:?}");
    };
    assert_eq!(player, p0);
    assert_eq!(candidates.len(), 2);
    assert!(candidates.contains(&ward_a_source));
    assert!(candidates.contains(&ward_b_source));
    // Nothing committed yet - fodder is still on the battlefield.
    assert_eq!(engine.state().object(fodder).unwrap().zone, ZoneKind::Battlefield);

    engine.submit_replacement_choice(ward_b_source).expect("ward_b_source is a real candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "payment should finish resolving after the choice, got {status:?}");

    assert!(engine.state().object(fodder).is_none());
    let (_new_id, from, to) = last_zone_change(&engine);
    assert_eq!((from, to), (ZoneKind::Battlefield, ZoneKind::Exile), "both wards redirect to Exile - either choice ends up there");
}
