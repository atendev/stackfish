//! Phase 5 vertical slice: CR 504.1's turn-based draw is a real replacement-
//! checked zone-change proposal now, closing the last piece of the
//! reachability gap `phase5/eventfilter-would-move-zone`'s own UNRESOLVED
//! notes named (`phase5/replaceable-cost-zone-moves` and `phase5/replaceable-
//! cleanup-discard` already closed the other two producers). Reuses
//! `commands::propose_draw` (new) + `commands::commit_zone_move` (already
//! shared by cost payments and cleanup discard) - `Engine::
//! enter_priority_via_replacement_check` is the shared tail every
//! `AdvanceTurnStep` step reaches once its own turn-based action is done,
//! factored out so both the normal path and a CR 616.1 pause/resume
//! (`ReplacementResumeTarget::Draw`) reach it identically.
//!
//! `Game.Ops.DrawCards(n)` (a scripted "draw N cards" ability, inside
//! `ResolveStackTop`'s own giant effect match - a different pause/resume
//! shape from `AdvanceTurnStep`) stays out of scope, named as its own
//! follow-up in this slice's own requirements report.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones, two_player_state as base_two_player_state};

fn land_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::BASIC,
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("forest".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn ward_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 999,
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
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// Two players, starting mid-Beginning-phase (`Phase::Beginning`/
/// `Step::Upkeep`, turn 2 so the draw isn't skipped) so a single `pass_both`
/// reaches `Step::Draw` - mirrors `phase5_draw_combat_end_triggers.rs::
/// state_at_upkeep_turn_2`, plus an Exile zone and a battlefield `ward`
/// permanent for a replacement effect to live on.
fn state_at_upkeep_turn_2() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let ward = ObjectId(999);
    let mut state = GameState {
        turn_number: 2,
        phase: Phase::Beginning,
        step: Step::Upkeep,
        last_active_turn_number: HashMap::from([(p0, 2), (p1, 1)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Exile]))
    };
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(ward);
    state.objects.insert(ward, card_object(ward, ward_definition(""), p0, ZoneKind::Battlefield));
    state
}

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

const DRAW_GOES_TO_EXILE_INSTEAD: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone("Library", "Hand"))
    e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Exile"))
    e:SetAffected({ controller = "you" })
    e:Register()
end
return s
"#;

#[test]
fn a_turn_based_draw_is_replacement_checked_for_real() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(1);
    let mut state = state_at_upkeep_turn_2();
    state.objects.insert(card, card_object(card, land_definition("Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(card);

    let ward = ObjectId(999);
    let ward_def = ward_definition(DRAW_GOES_TO_EXILE_INSTEAD);
    state.object_mut(ward).unwrap().definition = ward_def.clone();
    ScriptRuntime::new().run_card_script(&mut state, &ward_def, ward).expect("ward script self-registers cleanly");
    assert_eq!(state.replacement_effects.len(), 1);

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
    assert_eq!(engine.state().step, Step::Draw);

    assert!(engine.state().zone(ZoneKind::Library, Some(p0)).unwrap().contents.is_empty(), "the card left the library");
    assert!(
        engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.is_empty(),
        "the ward's WouldMoveZone(Library, Hand) + ReplaceZoneDestination(Exile) must have kept \
         the drawn card out of the hand entirely"
    );
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1, "the card must have landed in exile instead of the hand");
}

#[test]
fn a_turn_based_draw_without_a_matching_replacement_lands_in_hand_normally() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(1);
    let mut state = state_at_upkeep_turn_2();
    state.objects.insert(card, card_object(card, land_definition("Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(card);
    // No replacement effect registered - the ward object exists but its
    // script is empty, so nothing's in state.replacement_effects.

    let mut engine = Engine::new(state);
    pass_both(&mut engine, p0, p1);

    assert_eq!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.len(), 1, "no replacement registered - the card lands in hand as normal");
    assert!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.is_empty());
}

#[test]
fn drawing_from_an_empty_library_still_sets_drew_from_empty_library() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    // Library stays empty - no card inserted.
    let state = state_at_upkeep_turn_2();

    let mut engine = Engine::new(state);
    pass_both(&mut engine, p0, p1);

    assert!(engine.state().player(p0).unwrap().drew_from_empty_library, "CR 704.5b flag must still be set with no library and no replacement effect involved");
}

#[test]
fn two_matching_wards_pause_the_draw_for_a_cr_616_1_choice() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(1);
    let mut state = state_at_upkeep_turn_2();
    state.objects.insert(card, card_object(card, land_definition("Forest"), p0, ZoneKind::Library));
    state.zone_mut(ZoneKind::Library, Some(p0)).unwrap().push_bottom(card);

    let ward_a = ObjectId(999);
    let ward_a_def = ward_definition(DRAW_GOES_TO_EXILE_INSTEAD);
    state.object_mut(ward_a).unwrap().definition = ward_a_def.clone();
    ScriptRuntime::new().run_card_script(&mut state, &ward_a_def, ward_a).expect("ward A self-registers cleanly");

    let ward_b = ObjectId(1001);
    state.objects.insert(ward_b, GameObject { object_id: ward_b, ..state.objects[&ward_a].clone() });
    state.replacement_effects.push(ReplacementEffect {
        source: ward_b,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Library),
                to: Some(ZoneKind::Hand),
                affected: EffectFilter {  card_types: None, controller: Some(FilterController::You), self_only: false, subtypes: None, excluded_subtypes: None, },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        },
        consumption: ConsumptionPolicy::Repeatable,
    });
    assert_eq!(state.replacement_effects.len(), 2);

    let mut engine = Engine::new(state);
    let status = pass_both(&mut engine, p0, p1);
    let StepStatus::AwaitingReplacementChoice { player, candidates } = status else {
        panic!("two matching replacement effects on a turn-based draw must pause for a CR 616.1 choice, got {status:?}");
    };
    assert_eq!(player, p0);
    assert_eq!(candidates.len(), 2);
    assert!(engine.state().object(card).is_some(), "nothing committed yet");

    engine.submit_replacement_choice(ReplacementCandidate::Registered(ward_b)).expect("ward_b is a real candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0), "draw should finish resolving after the choice, got {status:?}");

    assert!(engine.state().object(card).is_none());
    assert_eq!(engine.state().zone(ZoneKind::Exile, None).unwrap().contents.len(), 1, "both wards redirect to Exile - either choice ends up there");
}
