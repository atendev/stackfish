//! Phase 5 vertical slice: `phase5/mass-choice-effects` — Balance itself,
//! built entirely out of the four generic primitives this slice adds
//! (`Game.ChooseN`, `Game.Ops.SacrificeChosen`/`DiscardChosen`,
//! `Game.Ops.ForEachPlayer`) plus the `state` snapshot `invoke_operation` now
//! passes to every operation function - no card-specific engine code at all.
//!
//! "Each player chooses a number of lands they control equal to the number
//! of lands controlled by the player who controls the fewest, then
//! sacrifices the rest. Players discard cards and sacrifice creatures the
//! same way."

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptChoiceRequest;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, player_state, two_player_state as base_two_player_state, zones};

const BALANCE_SCRIPT: &str = r#"
local s = {}

local function type_has(card_types, name)
    for _, t in ipairs(card_types) do
        if t == name then return true end
    end
    return false
end

function s.operation()
    return Game.Ops.ForEachPlayer(function(source, controller, state)
        local lands = {}
        local creatures = {}
        for _, obj in ipairs(state.battlefield) do
            if type_has(obj.card_types, "Land") then
                lands[obj.controller] = (lands[obj.controller] or 0) + 1
            end
            if obj.is_creature then
                creatures[obj.controller] = (creatures[obj.controller] or 0) + 1
            end
        end

        local min_lands, min_creatures, min_hand = nil, nil, nil
        for pid, hand_size in pairs(state.hand_count) do
            local land_count = lands[pid] or 0
            local creature_count = creatures[pid] or 0
            if min_lands == nil or land_count < min_lands then min_lands = land_count end
            if min_creatures == nil or creature_count < min_creatures then min_creatures = creature_count end
            if min_hand == nil or hand_size < min_hand then min_hand = hand_size end
        end

        local my_lands = lands[controller] or 0
        local my_creatures = creatures[controller] or 0
        local my_hand = state.hand_count[controller] or 0

        local sac_ids = {}
        if my_lands > min_lands then
            local chosen = Game.ChooseN({ card_types = { "Land" }, controller = "you" }, my_lands - min_lands)
            for _, v in ipairs(chosen) do table.insert(sac_ids, v) end
        end
        if my_creatures > min_creatures then
            local chosen = Game.ChooseN({ card_types = { "Creature" }, controller = "you" }, my_creatures - min_creatures)
            for _, v in ipairs(chosen) do table.insert(sac_ids, v) end
        end
        local result = Game.Ops.SacrificeChosen(sac_ids)

        if my_hand > min_hand then
            local chosen_cards = Game.ChooseN({ zone = "Hand", controller = "you" }, my_hand - min_hand)
            local discarded = Game.Ops.DiscardChosen(chosen_cards)
            result.discard_chosen = discarded.discard_chosen
        end

        return result
    end)
end

return s
"#;

fn land_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(format!("test_land_{definition_id}")),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn creature_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(format!("test_creature_{definition_id}")),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn filler_card_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition { supertypes: SupertypeSet::empty(), ..land_definition(definition_id, name) }
}

fn balance_definition(definition_id: u64) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: "Balance".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Each player chooses a number of lands they control equal to the number of lands \
                       controlled by the player who controls the fewest, then sacrifices the rest. Players \
                       discard cards and sacrifice creatures the same way."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(BALANCE_SCRIPT.to_string()),
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
        players: vec![player_state(p0), player_state(p1)],
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Library]))
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn put_in_hand(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    let owner = object.owner;
    state.zone_mut(ZoneKind::Hand, Some(owner)).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

/// Same as `phase5_mass_choice_effects.rs`'s own helper of the same name -
/// see that file's doc comment for why this manual step exists (`s.operation()`'s
/// `ScriptHandle` is only valid against the `Engine` that loaded it).
fn register_script_operation(engine: &mut Engine, object: ObjectId, source: &str) {
    let Some((primitive, handle)) =
        engine.build_operation(source).unwrap_or_else(|e| panic!("script operation() failed to load: {e:?}"))
    else {
        return;
    };
    let effect = primitive.or(Some(AbilityEffect::Script(handle)));
    engine.state_mut().objects.get_mut(&object).expect("object exists").definition.effect = effect;
}

fn cast_and_resolve(engine: &mut Engine, p0: PlayerId, p1: PlayerId, card: ObjectId) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority, spell is in hand");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the {{0}} cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine);
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn expect_select_target(status: StepStatus) -> (PlayerId, TargetFilter) {
    match status {
        StepStatus::AwaitingScriptChoice { player, request: ScriptChoiceRequest::SelectTarget { filter, .. } } => {
            (player, filter)
        }
        other => panic!("expected AwaitingScriptChoice/SelectTarget, got {other:?}"),
    }
}

fn battlefield_names(engine: &Engine) -> Vec<String> {
    let mut names: Vec<String> = engine
        .state()
        .zone(ZoneKind::Battlefield, None)
        .unwrap()
        .contents
        .iter()
        .map(|&id| engine.state().object(id).unwrap().definition.name.clone())
        .collect();
    names.sort();
    names
}

/// p0 controls 3 lands + 2 creatures + 3 hand cards; p1 controls 1 land + 0
/// creatures + 1 hand card - p1 is already at the fewest in every category,
/// so p1 never gets asked anything at all, and p0 alone must reduce down to
/// p1's own counts (2 lands sacrificed, 2 creatures sacrificed, 2 cards
/// discarded).
#[test]
fn balance_reduces_the_richer_player_down_to_the_poorer_players_own_counts() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(card, card_object(card, balance_definition(1), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    // p0: 3 lands, 2 creatures, 2 extra hand cards (+ Balance itself = 3).
    put_on_battlefield(&mut state, card_object(ObjectId(1), land_definition(2, "P0 Plains A"), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(ObjectId(2), land_definition(3, "P0 Plains B"), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(ObjectId(3), land_definition(4, "P0 Plains C"), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(ObjectId(4), creature_definition(5, "P0 Bear A"), p0, ZoneKind::Battlefield));
    put_on_battlefield(&mut state, card_object(ObjectId(5), creature_definition(6, "P0 Bear B"), p0, ZoneKind::Battlefield));
    put_in_hand(&mut state, card_object(ObjectId(6), filler_card_definition(7, "P0 Extra A"), p0, ZoneKind::Hand));
    put_in_hand(&mut state, card_object(ObjectId(7), filler_card_definition(8, "P0 Extra B"), p0, ZoneKind::Hand));

    // p1: 1 land, 0 creatures, 1 hand card - already the fewest everywhere.
    put_on_battlefield(&mut state, card_object(ObjectId(8), land_definition(9, "P1 Island"), p1, ZoneKind::Battlefield));
    put_in_hand(&mut state, card_object(ObjectId(9), filler_card_definition(10, "P1 Card"), p1, ZoneKind::Hand));

    let mut engine = Engine::new(state);
    register_script_operation(&mut engine, card, BALANCE_SCRIPT);
    let mut status = cast_and_resolve(&mut engine, p0, p1, card);

    // p0 must choose 2 of 3 lands to sacrifice (3 - min(3,1) = 2), then 2 of
    // 2 creatures (2 - min(2,0) = 2), then 1 of 2 extra hand cards (2 - 1 =
    // 1 - Balance itself doesn't count, it already left p0's hand by the
    // time this resolves, so my_hand is 2, not 3).
    let land_picks = [ObjectId(1), ObjectId(2)];
    for &pick in &land_picks {
        let (player, filter) = expect_select_target(status);
        assert_eq!(player, p0);
        let TargetFilter::Object { card_types, .. } = filter else { panic!("expected an Object filter") };
        assert!(card_types.unwrap().contains(CardTypeSet::LAND));
        engine.submit_script_choice(Target::Object(pick)).expect("p0's own land");
        status = drain(&mut engine);
    }
    let creature_picks = [ObjectId(4), ObjectId(5)];
    for &pick in &creature_picks {
        let (player, filter) = expect_select_target(status);
        assert_eq!(player, p0);
        let TargetFilter::Object { card_types, .. } = filter else { panic!("expected an Object filter") };
        assert!(card_types.unwrap().contains(CardTypeSet::CREATURE));
        engine.submit_script_choice(Target::Object(pick)).expect("p0's own creature");
        status = drain(&mut engine);
    }
    let hand_picks = [ObjectId(6)];
    for &pick in &hand_picks {
        let (player, filter) = expect_select_target(status);
        assert_eq!(player, p0);
        let TargetFilter::Object { zone, .. } = filter else { panic!("expected an Object filter") };
        assert_eq!(zone, Some(ZoneKind::Hand));
        engine.submit_script_choice(Target::Object(pick)).expect("p0's own hand card");
        status = drain(&mut engine);
    }

    // p1 is already at the fewest everywhere - never asked anything, the
    // whole ForEachPlayer loop finishes once p0's last choice resolves.
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "expected the loop to finish straight through p1, got {status:?}");

    let remaining = battlefield_names(&engine);
    assert_eq!(remaining, vec!["P0 Plains C", "P1 Island"], "both P0 bears and both extra P0 plains sacrificed, P1 untouched");
    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.len(), 1, "1 of 2 extra cards discarded");
    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(p1)).unwrap().contents.len(), 1, "p1's own hand untouched");
    // p0's graveyard: 2 lands + 2 creatures + 1 discard + Balance itself = 6.
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(), 6);
    assert_eq!(engine.state().zone(ZoneKind::Graveyard, Some(p1)).unwrap().contents.len(), 0);
}
