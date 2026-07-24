//! Phase 5 vertical slice: wire `AbilityEffect::CreateToken` into
//! `replacement::apply_enters_battlefield_modifiers` - the 4th of the four
//! real to-Battlefield producers (`PlayLand`, the resolving-spell path in
//! `ResolveStackTop`, `Search`), closing a real, previously-silent gap:
//! `EntersTapped`/`EntersWithCounters`/Bloodthirst/Sunburst all already
//! worked for every other permanent-entry shape but never applied to a
//! token.
//!
//! Two real ways this gets exercised, since a plain `Named`/`Creature`
//! token has no script of its own (`run_card_script` is only ever called
//! for `TokenSpec::CopyOf`):
//! - An externally-registered, non-`self_only` replacement effect (e.g. "tokens
//!   you control enter tapped") already sitting in `state.replacement_effects`,
//!   filtered by controller rather than source.
//! - A `TokenSpec::CopyOf` token whose copied source had a self-registered
//!   ETB modifier - `run_card_script` already re-registers it under the new
//!   token's id; this slice is what actually applies it.
//!
//! Devour/Amplify stay unwired on tokens - both need a genuine mid-ETB
//! player pause `apply_enters_battlefield_modifiers` structurally can't
//! raise, same narrow posture `pending_devour_choice`'s own doc comment
//! already takes for `Search`.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

fn instant_definition(id: u64, name: &str, effect: AbilityEffect) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: Some(effect),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn bear_definition(id: u64, name: &str, script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: id,
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
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// A permanent with an external, non-`self_only` replacement effect: "tokens
/// you control enter the battlefield tapped" - filtered by controller, not
/// by which object is entering.
fn tokens_you_control_enter_tapped(source: ObjectId) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: None,
                to: Some(ZoneKind::Battlefield),
                affected: EffectFilter { controller: Some(FilterController::You), card_types: None, self_only: false },
            },
            payload: ReplacementPayload::EntersTapped,
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

fn tokens_you_control_enter_with_counters(source: ObjectId, amount: i32) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: None,
                to: Some(ZoneKind::Battlefield),
                affected: EffectFilter { controller: Some(FilterController::You), card_types: None, self_only: false },
            },
            payload: ReplacementPayload::EntersWithCounters { kind: CounterKind::PlusOnePlusOne, amount },
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

const SELF_REGISTER_ENTERS_TAPPED: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersTapped)
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

fn two_player_state() -> GameState {
    base_two_player_state(hand_battlefield_graveyard_zones(PlayerId(0), PlayerId(1)))
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None },
        })
        .expect("player holds priority, spell is in hand");
}

fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the cast's (free) cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine)
}

fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

#[test]
fn token_enters_tapped_via_an_externally_granted_controller_wide_effect() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let anthem = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(anthem, bear_definition(1, "Anthem Source", ""), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(tokens_you_control_enter_tapped(anthem));
    state.objects.insert(
        card,
        card_object(card, instant_definition(2, "Investigate", AbilityEffect::CreateToken { spec: TokenSpec::Named(NamedToken::Clue), count: 1 }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    let token_id = bf.contents.iter().copied().find(|&id| id != anthem).expect("the Clue token exists");
    assert!(engine.state().object(token_id).unwrap().tapped, "the token entered tapped via the controller-wide effect");
}

#[test]
fn token_enters_with_counters_via_an_externally_granted_controller_wide_effect() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let anthem = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(anthem, bear_definition(1, "Anthem Source", ""), p0, ZoneKind::Battlefield));
    state.replacement_effects.push(tokens_you_control_enter_with_counters(anthem, 2));
    let spec = TokenSpec::Creature(CreatureTokenSpec {
        subtypes: vec!["Saproling".to_string()],
        colors: ColorSet::empty(),
        power: 1,
        toughness: 1,
        keywords: KeywordSet::empty(),
        name: None,
        additional_types: CardTypeSet::empty(),
        additional_supertypes: SupertypeSet::empty(),
    });
    state.objects.insert(card, card_object(card, instant_definition(2, "Saproling Migration", AbilityEffect::CreateToken { spec, count: 1 }), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    let token_id = bf.contents.iter().copied().find(|&id| id != anthem).expect("the Saproling token exists");
    assert_eq!(engine.state().object(token_id).unwrap().counters.count(&CounterKind::PlusOnePlusOne), 2);
}

#[test]
fn a_copyof_token_applies_its_freshly_reregistered_self_only_effect() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state();
    put_on_battlefield(&mut state, card_object(source, bear_definition(1, "Tapped Bear", SELF_REGISTER_ENTERS_TAPPED), p0, ZoneKind::Battlefield));
    // The source's own self-registration ran when it entered - real
    // battlefield setup for a plain-struct-literal fixture, mirroring how
    // every other test in this codebase seeds `state.replacement_effects`
    // directly rather than re-deriving it from a script run.
    state.replacement_effects.push(ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone { from: None, to: Some(ZoneKind::Battlefield), affected: EffectFilter { controller: None, card_types: None, self_only: true } },
            payload: ReplacementPayload::EntersTapped,
        },
        consumption: ConsumptionPolicy::Repeatable,
    });
    state.objects.insert(
        card,
        card_object(card, instant_definition(2, "Clone Legion", AbilityEffect::CreateToken { spec: TokenSpec::CopyOf(source, CopyExceptions::default()), count: 1 }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    let token_id = bf.contents.iter().copied().find(|&id| id != source).expect("the copy token exists");
    assert!(engine.state().object(token_id).unwrap().tapped, "run_card_script re-registered EntersTapped under the token's own id, and it was applied");
    assert!(!engine.state().object(source).unwrap().tapped, "the original source itself is untouched");
}

#[test]
fn a_plain_token_with_no_matching_effect_is_unaffected() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(0);
    let mut state = two_player_state();
    state.objects.insert(
        card,
        card_object(card, instant_definition(1, "Investigate", AbilityEffect::CreateToken { spec: TokenSpec::Named(NamedToken::Clue), count: 1 }), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    assert_eq!(bf.contents.len(), 1);
    let token = engine.state().object(bf.contents[0]).unwrap();
    assert!(!token.tapped, "no registered effect matches - the token is unaffected");
    assert_eq!(token.counters.count(&CounterKind::PlusOnePlusOne), 0);
}
