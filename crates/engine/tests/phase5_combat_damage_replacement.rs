//! Phase 5 vertical slice: backlog item 5a — `AssignCombatDamage`'s
//! `DamageDealt` events now flow through the real replacement pipeline
//! (CR 614-616) before the mutation (`damage_marked`/life) actually
//! happens, instead of the mutation happening synchronously inside
//! `execute_command` and the replacement check running "too late" against
//! an already-applied effect. See docs/implementation.md's
//! phase5-fix-replacement-bypasses slice notes for the bug this closes.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones, two_player_state as base_two_player_state};

fn creature_definition(name: &str, power: i32, toughness: i32) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    GameObject { tapped: true, ..card_object(id, definition, owner, ZoneKind::Battlefield) }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn combat_damage_state(combat: CombatState) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::CombatDamage,
        combat: Some(combat),
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield]))
    }
}

fn assign_combat_damage(
    engine: &mut Engine,
    player: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::AssignCombatDamage { assignments } })
        .expect("turn-based action, no priority required");
    drain(engine)
}

#[test]
fn prevent_damage_effect_stops_combat_damage_from_landing() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let shield_source = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false,
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 3), p0));
    put_on_battlefield(&mut state, permanent(shield_source, creature_definition("Ward", 0, 1), p1));
    state.replacement_effects.push(ReplacementEffect {
        source: shield_source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::DamageDealt {
                affected: EffectFilter {  card_types: None, controller: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
            },
            payload: ReplacementPayload::PreventDamage,
        },
        consumption: ConsumptionPolicy::Repeatable,
    });

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    let p1_life = engine.state().player(p1).unwrap().life;
    assert_eq!(p1_life, 20, "the registered PreventDamage effect must have stopped the damage from landing");

    let dealt = engine
        .state()
        .ledger
        .iter()
        .find_map(|e| match &e.kind {
            GameEventKind::DamageDealt { amount, .. } => Some(*amount),
            _ => None,
        })
        .expect("a DamageDealt event must still be logged, just with amount 0");
    assert_eq!(dealt, 0);
}

#[test]
fn redirect_damage_effect_changes_who_actually_loses_life() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let redirect_source = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: Vec::new(),
        declared_defenders: vec![p1],
        bands: Vec::new(), effect_blocked: Vec::new(), damage_assigned_this_step: false,
    });
    put_on_battlefield(&mut state, permanent(attacker, creature_definition("Bear", 3, 3), p0));
    put_on_battlefield(&mut state, permanent(redirect_source, creature_definition("Redirector", 0, 1), p1));
    state.replacement_effects.push(ReplacementEffect {
        source: redirect_source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::DamageDealt {
                affected: EffectFilter {  card_types: None, controller: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
            },
            payload: ReplacementPayload::RedirectDamage(Target::Player(p0)),
        },
        consumption: ConsumptionPolicy::Repeatable,
    });

    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, p0, Vec::new());

    assert_eq!(engine.state().player(p1).unwrap().life, 20, "damage was redirected away from p1");
    assert_eq!(engine.state().player(p0).unwrap().life, 17, "p0 took the redirected 3 damage instead");
}
