//! CR 701.20a, 701.18c: `HandRevealModifier` for "plays with hand revealed" effects (Telepathy: "Your opponents play with their hands revealed", CR 701.20c), tested through `is_hand_revealed`.
//! It mirrors `HandSizeModifier`, but `is_hand_revealed` ORs every applicable modifier with no latest-timestamp override, since a revealed card may be revealed again (CR 701.20c).
// TODO: enforcement, since `VisibilityPolicy::is_visible_to` has no callers and `state_to_lua` never encodes hand contents

use std::collections::HashMap;

use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

const ENCHANTED_PLAYER_HAND_REVEALED: &str = r#"
local s = {}
function s.register_abilities(c)
    local m = Game.CreateHandRevealModifier(c)
    m:SetAffected(Game.Affected.EnchantedPlayer)
    m:Register()
end
return s
"#;

fn aura_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Enchant Player Hand Reveal Aura".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: vec![Subtype("Aura".to_string())],
        oracle_text: "Enchant player. Enchanted player plays with their hand revealed.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

#[test]
fn run_card_script_self_registers_an_enchanted_player_hand_reveal_modifier() {
    let mut state = two_player_state();
    let aura = ObjectId(0);
    let definition = aura_definition(ENCHANTED_PLAYER_HAND_REVEALED);
    state.objects.insert(aura, card_object(aura, definition.clone(), PlayerId(1), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, aura).expect("script self-registers cleanly");

    assert_eq!(state.hand_reveal_modifiers.len(), 1);
    let modifier = &state.hand_reveal_modifiers[0];
    assert_eq!(modifier.source, aura);
    assert_eq!(modifier.controller, PlayerId(1), "controller resolved from the Aura's own live controller at drain time");
    assert!(matches!(modifier.affected, HandRevealAffected::EnchantedPlayer));
}

#[test]
fn enchanted_players_hand_is_revealed() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(1);
    let mut aura_obj = card_object(aura, aura_definition(""), p1, ZoneKind::Battlefield);
    aura_obj.attached_to_player = Some(p0);
    put_on_battlefield(&mut state, aura_obj);
    state.hand_reveal_modifiers.push(HandRevealModifier {
        source: aura,
        controller: p1,
        affected: HandRevealAffected::EnchantedPlayer,
        timestamp: SequenceNo(0),
    });

    assert!(is_hand_revealed(&state, &state.hand_reveal_modifiers, p0), "the enchanted player's hand should be revealed");
    assert!(!is_hand_revealed(&state, &state.hand_reveal_modifiers, p1), "p1 isn't enchanted - unaffected");
}

#[test]
fn the_reveal_stops_once_the_aura_is_no_longer_attached_to_that_player() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(1);
    // Same source and modifier, but the Aura fell off, and the check re-resolves fresh each time.
    let aura_obj = card_object(aura, aura_definition(""), p1, ZoneKind::Battlefield);
    put_on_battlefield(&mut state, aura_obj);
    state.hand_reveal_modifiers.push(HandRevealModifier {
        source: aura,
        controller: p1,
        affected: HandRevealAffected::EnchantedPlayer,
        timestamp: SequenceNo(0),
    });

    assert!(!is_hand_revealed(&state, &state.hand_reveal_modifiers, p0), "the Aura isn't attached to anyone - not revealed");
}

#[test]
fn filter_controller_forms_resolve_independent_of_enchantment() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(1);
    put_on_battlefield(&mut state, card_object(source, aura_definition(""), p0, ZoneKind::Battlefield));
    state.hand_reveal_modifiers.push(HandRevealModifier {
        source,
        controller: p0,
        affected: HandRevealAffected::Filter(FilterController::Opponent),
        timestamp: SequenceNo(0),
    });

    assert!(!is_hand_revealed(&state, &state.hand_reveal_modifiers, p0), "the modifier's own controller isn't their own opponent");
    assert!(is_hand_revealed(&state, &state.hand_reveal_modifiers, p1), "p1 is p0's opponent - revealed");
}

#[test]
fn two_independent_reveal_sources_never_conflict() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(1);
    let telepathy_like = ObjectId(2);
    let mut aura_obj = card_object(aura, aura_definition(""), p1, ZoneKind::Battlefield);
    aura_obj.attached_to_player = Some(p0);
    put_on_battlefield(&mut state, aura_obj);
    put_on_battlefield(&mut state, card_object(telepathy_like, aura_definition(""), p1, ZoneKind::Battlefield));
    // Two independent sources both reveal p0's hand, and CR 701.20c lets a revealed card be revealed again, so they never conflict.
    state.hand_reveal_modifiers.push(HandRevealModifier {
        source: aura,
        controller: p1,
        affected: HandRevealAffected::EnchantedPlayer,
        timestamp: SequenceNo(0),
    });
    state.hand_reveal_modifiers.push(HandRevealModifier {
        source: telepathy_like,
        controller: p1,
        affected: HandRevealAffected::Filter(FilterController::Opponent),
        timestamp: SequenceNo(1),
    });

    assert!(is_hand_revealed(&state, &state.hand_reveal_modifiers, p0), "revealed via either source");

    // Removing the earlier source leaves the later Filter-based source holding.
    state.hand_reveal_modifiers.retain(|m| m.source != aura);
    assert!(is_hand_revealed(&state, &state.hand_reveal_modifiers, p0), "still revealed via the remaining Filter-based source");
}

// Forces a real retire_object call on the modifier's source.
fn destroy_permanent_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 30,
        name: "Test Destroy Permanent".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Destroy target permanent.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_destroy_permanent".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { mana_value: None, excluded_card_types: None, excluded_subtypes: None, card_types: None, subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: Some(AbilityEffect::Destroy),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

// CR 611.3b
// A static ability's continuous effect applies only while its source is on the battlefield.
#[test]
fn a_hand_reveal_modifier_is_purged_once_its_source_leaves_the_battlefield() {
    use stackfish_engine::engine::{Engine, StepStatus};

    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(0);
    let mut aura_obj = card_object(aura, aura_definition(""), p1, ZoneKind::Battlefield);
    aura_obj.attached_to_player = Some(p0);
    put_on_battlefield(&mut state, aura_obj);
    state.hand_reveal_modifiers.push(HandRevealModifier {
        source: aura,
        controller: p1,
        affected: HandRevealAffected::EnchantedPlayer,
        timestamp: SequenceNo(0),
    });
    let removal = ObjectId(1);
    state.objects.insert(removal, card_object(removal, destroy_permanent_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(removal);

    assert!(is_hand_revealed(&state, &state.hand_reveal_modifiers, p0), "the modifier is live before its source is destroyed");

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell { card: removal, modes: Vec::new(), targets: vec![Target::Object(aura)], x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("p0 holds priority, spell is in hand");
    let status = stackfish_testutil::drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    stackfish_testutil::drain(&mut engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    stackfish_testutil::drain(&mut engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    stackfish_testutil::drain(&mut engine);

    assert!(engine.state().object(aura).is_none(), "the source Aura was destroyed");
    assert!(engine.state().hand_reveal_modifiers.is_empty(), "the modifier must be purged along with its source");
    assert!(!is_hand_revealed(engine.state(), &engine.state().hand_reveal_modifiers, p0), "no longer revealed once the source is gone");
}
