//! CR 303.4m: `HandSizeAffected::EnchantedPlayer` resolves the player a source is attached to (`attached_to_player`), the way `AffectedFilter::EnchantedOnly` resolves an object, and `begin_cleanup_step` enforces it (CR 402.2).
//! `Game.CreateHandSizeModifier` is the Lua builder, with the same queue-and-drain shape as `Game.CreateCostModifier`.
// TODO: a general player-affecting continuous-effect primitive, since each player-level shape has its own bespoke mechanism

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, zones, two_player_state as base_two_player_state};

const ENCHANTED_PLAYER_HAND_SIZE_THREE: &str = r#"
local s = {}
function s.register_abilities(c)
    local m = Game.CreateHandSizeModifier(c)
    m:SetAffected(Game.Affected.EnchantedPlayer)
    m:SetEffect(Game.HandSizeEffects.SetTo(3))
    m:Register()
end
return s
"#;

fn vanilla_card(id: ObjectId, owner: PlayerId) -> GameObject {
    let definition = CardDefinition {
        definition_id: id.0,
        name: "Test Card".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("bear".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    };
    GameObject { controlled_since_turn: 1, ..card_object(id, definition, owner, ZoneKind::Hand) }
}

fn aura_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Enchant Player Hand Size Aura".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: vec![Subtype("Aura".to_string())],
        oracle_text: "Enchant player. Enchanted player's maximum hand size is 3.".to_string(),
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

fn state_at_end_step() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        turn_number: 2,
        phase: Phase::Ending,
        step: Step::End,
        last_active_turn_number: HashMap::from([(p0, 2), (p1, 1)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Library, ZoneKind::Hand, ZoneKind::Graveyard, ZoneKind::Battlefield, ZoneKind::Exile]))
    }
}

fn add_cards_to_hand(state: &mut GameState, player: PlayerId, count: usize) {
    for i in 0..count {
        let id = ObjectId(100 + i as u64);
        state.objects.insert(id, vanilla_card(id, player));
        state.zone_mut(ZoneKind::Hand, Some(player)).unwrap().push_bottom(id);
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn step_until_pause(engine: &mut Engine) -> StepStatus {
    loop {
        let r = engine.step();
        if !matches!(r.status, StepStatus::Progressed) {
            return r.status;
        }
    }
}

fn pass(engine: &mut Engine, player: PlayerId) {
    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).expect("PassPriority should be accepted");
}

fn pass_through_end_step(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    let s = step_until_pause(engine);
    assert!(matches!(s, StepStatus::AwaitingAction { player } if player == p0), "expected p0 end-step priority, got {s:?}");
    pass(engine, p0);
    let s = step_until_pause(engine);
    assert!(matches!(s, StepStatus::AwaitingAction { player } if player == p1), "expected p1 end-step priority, got {s:?}");
    pass(engine, p1);
    step_until_pause(engine)
}

#[test]
fn run_card_script_self_registers_an_enchanted_player_hand_size_modifier() {
    let mut state = state_at_end_step();
    let aura = ObjectId(0);
    let definition = aura_definition(ENCHANTED_PLAYER_HAND_SIZE_THREE);
    state.objects.insert(aura, card_object(aura, definition.clone(), PlayerId(1), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, aura).expect("script self-registers cleanly");

    assert_eq!(state.hand_size_modifiers.len(), 1);
    let modifier = &state.hand_size_modifiers[0];
    assert_eq!(modifier.source, aura);
    assert_eq!(modifier.controller, PlayerId(1), "controller resolved from the Aura's own live controller at drain time");
    assert!(matches!(modifier.affected, HandSizeAffected::EnchantedPlayer));
    assert_eq!(modifier.effect, HandSizeEffect::SetTo(3));
}

#[test]
fn enchanted_players_hand_size_is_capped_at_cleanup() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(1);
    add_cards_to_hand(&mut state, p0, 12);
    let mut aura_obj = card_object(aura, aura_definition(""), p1, ZoneKind::Battlefield);
    aura_obj.attached_to_player = Some(p0);
    put_on_battlefield(&mut state, aura_obj);
    state.hand_size_modifiers.push(HandSizeModifier {
        source: aura,
        controller: p1,
        affected: HandSizeAffected::EnchantedPlayer,
        effect: HandSizeEffect::SetTo(3),
        timestamp: SequenceNo(0),
    });

    let mut engine = Engine::new(state);
    let status = pass_through_end_step(&mut engine, p0, p1);

    assert!(
        matches!(status, StepStatus::AwaitingDiscard { player, hand_size: 12, max_hand_size: 3 } if player == p0),
        "expected AwaitingDiscard against the enchanted player's own capped threshold of 3, got {status:?}"
    );
}

#[test]
fn a_player_not_enchanted_by_the_modifiers_source_is_unaffected() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(1);
    add_cards_to_hand(&mut state, p0, 12);
    // The Aura is attached to p1 and not p0, so the modifier must not leak onto an unenchanted player.
    let mut aura_obj = card_object(aura, aura_definition(""), p1, ZoneKind::Battlefield);
    aura_obj.attached_to_player = Some(p1);
    put_on_battlefield(&mut state, aura_obj);
    state.hand_size_modifiers.push(HandSizeModifier {
        source: aura,
        controller: p1,
        affected: HandSizeAffected::EnchantedPlayer,
        effect: HandSizeEffect::SetTo(3),
        timestamp: SequenceNo(0),
    });

    let mut engine = Engine::new(state);
    let status = pass_through_end_step(&mut engine, p0, p1);

    assert!(
        matches!(status, StepStatus::AwaitingDiscard { player, hand_size: 12, max_hand_size: 7 } if player == p0),
        "p0 isn't enchanted by this Aura - base max hand size of 7 should still apply, got {status:?}"
    );
}

#[test]
fn the_restriction_stops_applying_once_the_aura_is_no_longer_attached_to_that_player() {
    let mut state = state_at_end_step();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(1);
    add_cards_to_hand(&mut state, p0, 12);
    // Same source and modifier, but the Aura fell off before cleanup, and the check re-resolves fresh each time.
    let aura_obj = card_object(aura, aura_definition(""), p1, ZoneKind::Battlefield);
    put_on_battlefield(&mut state, aura_obj);
    state.hand_size_modifiers.push(HandSizeModifier {
        source: aura,
        controller: p1,
        affected: HandSizeAffected::EnchantedPlayer,
        effect: HandSizeEffect::SetTo(3),
        timestamp: SequenceNo(0),
    });

    let mut engine = Engine::new(state);
    let status = pass_through_end_step(&mut engine, p0, p1);

    assert!(
        matches!(status, StepStatus::AwaitingDiscard { player, hand_size: 12, max_hand_size: 7 } if player == p0),
        "the Aura isn't attached to anyone - the modifier should not apply, got {status:?}"
    );
}
