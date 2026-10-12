//! Phase 5 vertical slice: `Search` and `CreateToken` as modes of a
//! multi-mode spell. Both pause for a player choice mid-resolution: Search
//! for the card to find and then, for a Devour creature, a nested sacrifice
//! choice (`SearchResume`); CreateToken for the CR 303.4f attach choice of an
//! Aura token. Each ends in the standard `ReplacementCheck` tail, so the
//! modal continuation (`Engine::modal_continuation`) resumes the remaining
//! modes after the answer.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);

fn card(id: u64, name: &str, types: CardTypeSet, script: Option<&str>) -> CardDefinition {
    CardDefinition {
        script_ref: script.map(|s| ScriptReference::Inline(s.to_string())).unwrap_or(ScriptReference::ByKey(name.to_string())),
        power: types.contains(CardTypeSet::CREATURE).then_some(PTValue::Fixed(2)),
        toughness: types.contains(CardTypeSet::CREATURE).then_some(PTValue::Fixed(2)),
        ..minimal_card_definition(id, name, "unused", ManaCost { symbols: Vec::new() }, types)
    }
}

/// "Test Aura: enchant creature" - the `CopyOf` source for the Aura-token tests.
fn aura(id: u64) -> CardDefinition {
    CardDefinition {
        subtypes: vec![Subtype("Aura".to_string())],
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                mana_value: None,
                card_types: Some(CardTypeSet::CREATURE),
                excluded_card_types: None,
                subtypes: None,
                excluded_subtypes: None,
                supertypes: None,
                excluded_supertypes: None,
                controller: None,
                zone: None,
            },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        })),
        ..card(id, "Test Aura", CardTypeSet::ENCHANTMENT, None)
    }
}

const DEVOUR_TWO: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersWithCountersDevour(2))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

fn filter(card_types: CardTypeSet) -> FilterSpec {
    FilterSpec { card_types: Some(card_types), supertypes: None, subtypes: None, controller: None, random: false, extra: None }
}

fn search(card_types: CardTypeSet, destination: SearchDestination) -> AbilityEffect {
    AbilityEffect::Search { filter: filter(card_types), count: 1, destination, shuffle_after: true }
}

fn treasures(count: u32) -> AbilityEffect {
    AbilityEffect::CreateToken { spec: TokenSpec::Named(NamedToken::Treasure), count }
}

fn mode(effect: AbilityEffect) -> SpellMode {
    SpellMode { target_requirement: None, effect: Some(effect), additional_cost: None, pawprint_weight: 0 }
}

fn engine_with(modes: Vec<SpellMode>) -> Engine {
    let spell = CardDefinition {
        modes,
        mode_choice: ModeChoice::Exactly(2),
        ..minimal_card_definition(99, "Test Spell", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::SORCERY)
    };
    let state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(P0, P1, &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    };
    let mut engine = Engine::new(state);
    add(&mut engine, SPELL.0, spell, P0, ZoneKind::Hand);
    engine
}

fn add(engine: &mut Engine, id: u64, def: CardDefinition, owner: PlayerId, zone: ZoneKind) {
    let id = ObjectId(id);
    engine.state_mut().objects.insert(id, card_object(id, def, owner, zone));
    let scope = if matches!(zone, ZoneKind::Battlefield | ZoneKind::Exile) { None } else { Some(owner) };
    engine.state_mut().zone_mut(zone, scope).unwrap().push_bottom(id);
}

fn cast(engine: &mut Engine) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell { card: SPELL, modes: vec![0, 1], targets: Vec::new(), x_value: None, alternative_cost: None, pay_buyback: false },
        })
        .expect("p0 holds priority");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap();
    drain(engine);
}

fn pass_both(engine: &mut Engine) -> StepStatus {
    engine.submit_action(PlayerAction { player: P0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: P1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn life(engine: &Engine) -> i32 {
    engine.state().player(P0).unwrap().life
}

fn zone_len(engine: &Engine, zone: ZoneKind, owner: Option<PlayerId>) -> usize {
    engine.state().zone(zone, owner).unwrap().contents.len()
}

fn tokens(engine: &Engine) -> usize {
    engine.state().objects.values().filter(|o| o.is_token && o.zone == ZoneKind::Battlefield).count()
}

fn assert_resolved_cleanly(engine: &Engine) {
    assert!(engine.state().stack.is_empty(), "stack drained");
    let markers = engine.state().ledger.iter().filter(|e| matches!(e.kind, GameEventKind::StackObjectResolved { .. })).count();
    assert_eq!(markers, 1, "the stack object resolves once, after its last mode");
    let spell_zones: Vec<_> = engine.state().objects.values().filter(|o| o.definition.name == "Test Spell").map(|o| o.zone).collect();
    assert_eq!(spell_zones, vec![ZoneKind::Graveyard], "the spell ends in the graveyard");
}

// ---- Search ----------------------------------------------------------------

fn three_lands(engine: &mut Engine) {
    for i in 0..3 {
        add(engine, 200 + i, card(200 + i, &format!("Land {i}"), CardTypeSet::LAND, None), P0, ZoneKind::Library);
    }
}

#[test]
fn a_search_mode_pauses_for_the_find_and_the_next_mode_runs_after_it() {
    let mut engine = engine_with(vec![mode(search(CardTypeSet::LAND, SearchDestination::Hand)), mode(AbilityEffect::GainLife(3))]);
    three_lands(&mut engine);
    cast(&mut engine);
    let status = pass_both(&mut engine);
    let StepStatus::AwaitingSearchChoice { candidates, .. } = status else { panic!("expected a search choice, got {status:?}") };
    assert_eq!(life(&engine), 20, "the next mode waits for the search");

    engine.submit_search_choice(vec![candidates[0]]).unwrap();
    drain(&mut engine);

    assert_eq!(life(&engine), 23);
    assert_eq!(zone_len(&engine, ZoneKind::Hand, Some(P0)), 1, "the found land");
    assert_eq!(zone_len(&engine, ZoneKind::Library, Some(P0)), 2);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_synchronous_mode_followed_by_a_search_mode_both_resolve() {
    let mut engine = engine_with(vec![mode(AbilityEffect::GainLife(3)), mode(search(CardTypeSet::LAND, SearchDestination::Hand))]);
    three_lands(&mut engine);
    cast(&mut engine);
    let status = pass_both(&mut engine);
    let StepStatus::AwaitingSearchChoice { candidates, .. } = status else { panic!("expected a search choice, got {status:?}") };
    engine.submit_search_choice(vec![candidates[1]]).unwrap();
    drain(&mut engine);

    assert_eq!(life(&engine), 23);
    assert_eq!(zone_len(&engine, ZoneKind::Hand, Some(P0)), 1);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_search_mode_with_no_matching_card_does_not_pause_and_the_next_mode_runs() {
    let mut engine = engine_with(vec![mode(search(CardTypeSet::CREATURE, SearchDestination::Hand)), mode(AbilityEffect::GainLife(3))]);
    three_lands(&mut engine);
    cast(&mut engine);
    pass_both(&mut engine);

    assert_eq!(life(&engine), 23);
    assert_eq!(zone_len(&engine, ZoneKind::Hand, Some(P0)), 0);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_search_mode_that_fetches_a_devour_creature_pauses_twice_and_the_next_mode_still_runs() {
    let mut engine = engine_with(vec![mode(search(CardTypeSet::CREATURE, SearchDestination::Battlefield { tapped: false, enters_attached: false })), mode(AbilityEffect::GainLife(3))]);
    let beast = ObjectId(10);
    add(&mut engine, beast.0, card(10, "Gluttonous Beast", CardTypeSet::CREATURE, Some(DEVOUR_TWO)), P0, ZoneKind::Library);
    add(&mut engine, 1, card(1, "Bear A", CardTypeSet::CREATURE, Some("")), P0, ZoneKind::Battlefield);
    add(&mut engine, 2, card(2, "Bear B", CardTypeSet::CREATURE, Some("")), P0, ZoneKind::Battlefield);
    cast(&mut engine);
    let status = pass_both(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingSearchChoice { .. }), "{status:?}");
    engine.submit_search_choice(vec![beast]).unwrap();
    let status = drain(&mut engine);
    let StepStatus::AwaitingSearchDevourChoice { object, .. } = status else { panic!("expected the nested devour choice, got {status:?}") };
    assert_eq!(life(&engine), 20, "the next mode waits for both pauses");

    engine.submit_search_devour_choice(vec![ObjectId(1), ObjectId(2)]).unwrap();
    drain(&mut engine);

    assert_eq!(engine.state().object(object).unwrap().counters.count(&CounterKind::PlusOnePlusOne), 4, "2 per creature x 2 sacrificed");
    assert!(engine.state().object(ObjectId(1)).is_none() && engine.state().object(ObjectId(2)).is_none(), "both bears were devoured");
    assert_eq!(life(&engine), 23);
    assert_resolved_cleanly(&engine);
}

// ---- CreateToken -----------------------------------------------------------

#[test]
fn a_create_token_mode_and_a_synchronous_mode_both_resolve() {
    let mut engine = engine_with(vec![mode(treasures(2)), mode(AbilityEffect::GainLife(3))]);
    cast(&mut engine);
    pass_both(&mut engine);

    assert_eq!(tokens(&engine), 2);
    assert_eq!(life(&engine), 23);
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_synchronous_mode_followed_by_a_create_token_mode_both_resolve() {
    let mut engine = engine_with(vec![mode(AbilityEffect::GainLife(3)), mode(treasures(1))]);
    cast(&mut engine);
    pass_both(&mut engine);

    assert_eq!(tokens(&engine), 1);
    assert_eq!(life(&engine), 23);
    assert_resolved_cleanly(&engine);
}

fn two_bears_and_an_aura_to_copy(engine: &mut Engine) {
    add(engine, 1, card(1, "Bear A", CardTypeSet::CREATURE, None), P0, ZoneKind::Battlefield);
    add(engine, 2, card(2, "Bear B", CardTypeSet::CREATURE, None), P0, ZoneKind::Battlefield);
    // A graveyard source is exempt from the unattached-Aura state-based action.
    add(engine, 3, aura(3), P0, ZoneKind::Graveyard);
}

fn aura_token() -> AbilityEffect {
    AbilityEffect::CreateToken { spec: TokenSpec::CopyOf(ObjectId(3), CopyExceptions::default()), count: 1 }
}

#[test]
fn an_aura_token_mode_pauses_for_the_attach_choice_and_the_next_mode_runs_after_it() {
    let mut engine = engine_with(vec![mode(aura_token()), mode(AbilityEffect::GainLife(3))]);
    two_bears_and_an_aura_to_copy(&mut engine);
    cast(&mut engine);
    let status = pass_both(&mut engine);
    let StepStatus::AwaitingTokenAttachChoice { attaching, .. } = status else { panic!("expected an attach choice, got {status:?}") };
    assert_eq!(life(&engine), 20, "the next mode waits for the attach choice");

    engine.submit_token_attach_choice(Target::Object(ObjectId(2))).unwrap();
    drain(&mut engine);

    assert_eq!(engine.state().object(attaching).unwrap().attached_to, Some(ObjectId(2)));
    assert_eq!(life(&engine), 23);
    assert_resolved_cleanly(&engine);
}

// ---- Two pausing modes in a row -----------------------------------------------

#[test]
fn a_search_mode_then_an_aura_token_mode_pause_one_after_the_other() {
    let mut engine = engine_with(vec![mode(search(CardTypeSet::LAND, SearchDestination::Hand)), mode(aura_token())]);
    three_lands(&mut engine);
    two_bears_and_an_aura_to_copy(&mut engine);
    cast(&mut engine);
    let status = pass_both(&mut engine);
    let StepStatus::AwaitingSearchChoice { candidates, .. } = status else { panic!("expected a search choice, got {status:?}") };
    engine.submit_search_choice(vec![candidates[0]]).unwrap();
    let status = drain(&mut engine);
    let StepStatus::AwaitingTokenAttachChoice { attaching, .. } = status else { panic!("expected the second pause, got {status:?}") };
    assert_eq!(zone_len(&engine, ZoneKind::Hand, Some(P0)), 1, "the first mode already finished");

    engine.submit_token_attach_choice(Target::Object(ObjectId(1))).unwrap();
    drain(&mut engine);

    assert_eq!(engine.state().object(attaching).unwrap().attached_to, Some(ObjectId(1)));
    assert_resolved_cleanly(&engine);
}

#[test]
fn a_treasure_mode_then_a_search_mode_commit_the_tokens_before_the_search_pauses() {
    let mut engine = engine_with(vec![mode(treasures(1)), mode(search(CardTypeSet::LAND, SearchDestination::Hand))]);
    three_lands(&mut engine);
    cast(&mut engine);
    let status = pass_both(&mut engine);
    let StepStatus::AwaitingSearchChoice { candidates, .. } = status else { panic!("expected a search choice, got {status:?}") };
    assert_eq!(tokens(&engine), 1, "the first mode is already done while the search waits");

    engine.submit_search_choice(vec![candidates[0]]).unwrap();
    drain(&mut engine);

    assert_eq!(zone_len(&engine, ZoneKind::Hand, Some(P0)), 1);
    assert_resolved_cleanly(&engine);
}
