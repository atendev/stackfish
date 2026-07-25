//! Phase 5 vertical slice: `EventFilter::WouldMoveZone` - a general
//! from/to-wildcard sibling of `EventFilter::WouldDie` (hardcoded to
//! Battlefield -> Graveyard). Lets a replacement effect match a zone-change
//! shape `WouldDie` can't express at all - proven here with "if this would
//! be exiled, put it into its owner's graveyard instead," a real capability
//! (grave-recursion decks fighting exile-based hate) `WouldDie` literally
//! cannot express since it's pinned to `to: Graveyard`.
//!
//! `Game.EventFilters.WouldMoveZone(fromName, toName)` is bound in Lua as a
//! function (either name may be `nil` for "any zone") rather than a bare
//! tagged value, since `EventFilterKind::WouldMoveZone` needs to carry its
//! own `from`/`to` - `WouldDie`'s hardcoded shape needs no such parameters.
//!
//! Wildcard `from`/`to` matching is proven directly against `ReplacementEffect
//! ::matches` in `types/effect.rs`'s own test module instead of end to end
//! here - most zone-change producers in this engine (draw, cleanup discard,
//! cost-payment sacrifice/discard) never call `apply_replacements` at all,
//! only SBA deaths and the scripted Destroy/Sacrifice/Exile ops do, and all
//! three are always Battlefield-sourced. See `EventFilter::WouldMoveZone`'s
//! own doc comment for the honest reachability note.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

const EXILE_INSTEAD_GOES_TO_GRAVEYARD: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone("Battlefield", "Exile"))
    e:SetPayload(Game.ReplacementPayloads.ReplaceZoneDestination("Graveyard"))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

const MISMATCHED_WOULD_MOVE_ZONE_PREVENT_DAMAGE: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone("Battlefield", "Exile"))
    e:SetPayload(Game.ReplacementPayloads.PreventDamage)
    e:Register()
end
return s
"#;

fn creature_definition(definition_id: u64, name: &str, script: &str) -> CardDefinition {
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
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// "Test Exile: Exile target creature." - same fixture
/// `phase5_script_ops_exile.rs` uses, duplicated per that file's own
/// no-shared-fixtures-between-test-binaries convention.
fn exile_spell_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 20,
        name: "Test Exile".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Exile target creature.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_exile".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object { excluded_card_types: None, excluded_subtypes: None, card_types: Some(CardTypeSet::CREATURE), subtypes: None, supertypes: None, excluded_supertypes: None, controller: None, zone: None },
            count: 1,
            up_to: false, at_least: false,
            exclude_source: false,
        })),
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Exile),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    }
}

fn cast_exile_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, target: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: vec![Target::Object(target)],
                x_value: None,
                alternative_cost: None,
            },
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
fn set_filter_would_move_zone_registers_with_the_chosen_from_and_to() {
    let mut state = two_player_state();
    let source = ObjectId(0);
    let definition = creature_definition(1, "Ward", EXILE_INSTEAD_GOES_TO_GRAVEYARD);
    state.objects.insert(source, card_object(source, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, source).expect("script self-registers cleanly");

    assert_eq!(state.replacement_effects.len(), 1);
    match &state.replacement_effects[0].spec {
        ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone { from: Some(ZoneKind::Battlefield), to: Some(ZoneKind::Exile), .. },
            payload: ReplacementPayload::ReplaceZoneDestination(ZoneKind::Graveyard),
        } => {}
        other => panic!("expected a WouldMoveZone(Battlefield, Exile)/ReplaceZoneDestination(Graveyard) primitive spec, got {other:?}"),
    }
}

#[test]
fn mismatched_would_move_zone_filter_and_prevent_damage_payload_is_a_real_register_time_error() {
    let mut state = two_player_state();
    let source = ObjectId(0);
    let definition = creature_definition(1, "Broken", MISMATCHED_WOULD_MOVE_ZONE_PREVENT_DAMAGE);
    state.objects.insert(source, card_object(source, definition.clone(), PlayerId(0), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    let result = runtime.run_card_script(&mut state, &definition, source);

    assert!(result.is_err(), "WouldMoveZone + PreventDamage is a real mismatch, must be rejected");
    assert!(state.replacement_effects.is_empty(), "no partial effect should have been committed");
}

#[test]
fn lua_registered_would_move_zone_redirects_an_exile_attempt_to_the_graveyard_instead() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let target = ObjectId(0);
    let ward_definition = creature_definition(1, "Ward", EXILE_INSTEAD_GOES_TO_GRAVEYARD);
    state.objects.insert(target, card_object(target, ward_definition.clone(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(target);

    ScriptRuntime::new().run_card_script(&mut state, &ward_definition, target).expect("script self-registers cleanly");
    assert_eq!(state.replacement_effects.len(), 1);

    let card = ObjectId(1);
    state.objects.insert(card, card_object(card, exile_spell_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_exile_spell(&mut engine, p0, card, target);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    assert!(engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.is_empty(), "the creature left the battlefield");
    assert!(
        engine.state().zone(ZoneKind::Exile, None).unwrap().contents.is_empty(),
        "WouldMoveZone(Battlefield, Exile) + ReplaceZoneDestination(Graveyard) must have kept the \
         creature out of exile entirely"
    );
    // 2, not 1 - the creature (redirected here) plus the Exile spell itself,
    // which lands in its owner's graveyard normally after resolving.
    assert_eq!(
        engine.state().zone(ZoneKind::Graveyard, Some(p0)).unwrap().contents.len(),
        2,
        "the creature must have landed in its owner's graveyard instead"
    );
}
