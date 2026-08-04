//! Phase 5 vertical slice: CR 702.138b/c/d Escape provenance.
//!
//! CR 702.138b: "a spell or permanent 'escaped' if [it] was cast from a
//! graveyard with an escape ability." This slice makes that a real,
//! queryable fact for the first time:
//!
//! - `AlternativeCost::Replacement.escape: bool` (true only for Escape's own
//!   alternative cost, false for Flashback/every other shape - the only
//!   thing that structurally distinguishes them, since both share
//!   `from_zone: Graveyard`).
//! - `StackObject::escaped: bool`, baked in at push time from the chosen
//!   alternative cost.
//! - `GameObject::entered_via_escape: bool`, set by `Engine::
//!   move_resolving_spell` when a spell with `escaped: true` resolves onto
//!   the battlefield.
//! - `InterveningIfPrimitive::Escaped` (CR 603.4's "if it escaped" clause) -
//!   Kroxa/Uro's own real card pattern is an *ordinary* ETB trigger with
//!   this as its intervening-if, not CR 702.138c/d's own literal "escapes
//!   with [counters/ability]" templates.
//! - CR 702.138c ("escapes with N counters") is real too -
//!   `ReplacementPayload::EntersWithCountersEscape`, the same no-player-
//!   choice gated shape `EntersWithCountersBloodthirst` already established,
//!   checked against `entered_via_escape` instead of
//!   `opponent_damaged_this_turn`.
//! - CR 702.138d ("has [ability] if escaped") needed **no new mechanism at
//!   all** - a static ability's own `StaticAbilityBuilder:SetCondition`
//!   (CR 611.2c, `phase5/conditional-continuous-effects`) already accepts
//!   any `InterveningIfPrimitive`, so `Game.InterveningIf.Escaped` plugs in
//!   directly. `layers::applicable_effects_in_layer` already re-evaluates a
//!   `ContinuousEffect`'s own `condition` fresh on every query, off the
//!   *live* `GameObject` (no CR 603.10 departed-ability concern - unlike an
//!   ETB trigger, a static ability's condition is checked while the
//!   permanent is still on the battlefield).
//!
//! All three shapes are proven together on one creature, cast two ways: via
//! Escape (all three apply) and normally from hand (none of them do) -
//! proving `entered_via_escape` is real per-incarnation provenance, not a
//! standing permission on the card.

use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, player_state, two_player_state as base_two_player_state, zones};

const ESCAPE_GATED_EFFECTS: &str = r#"
local s = {}
function s.register_abilities(c)
    local trig = Game.CreateTriggeredAbility(c)
    trig:SetCondition(Game.Conditions.Enters)
    trig:SetInterveningIf(Game.InterveningIf.Escaped)
    trig:SetOperation(Game.Ops.GainLife(1))
    trig:Register()

    local counters = Game.CreateReplacementEffect(c)
    counters:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    counters:SetPayload(Game.ReplacementPayloads.EntersWithCountersEscape("PlusOnePlusOne", 2))
    counters:SetAffected({ self_only = true })
    counters:Register()

    local keyword = Game.CreateStaticAbility(c)
    keyword:SetLayer(Game.Layers.AbilityAdding)
    keyword:SetEffect(Game.Effects.AddKeyword("Flying"))
    keyword:SetAffected({ self_only = true })
    keyword:SetCondition(Game.InterveningIf.Escaped)
    keyword:Register()
end
return s
"#;

/// "Test Escaper": `{2}{B}` creature, Escape—`{1}{B}, Exile three other
/// cards from your graveyard`, carrying all three escape-gated effects
/// above via a real inline script.
fn escaper_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Escaper".to_string(),
        mana_cost: ManaCost {
            symbols: vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(2)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Black)),
            ],
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: "When this permanent enters the battlefield, if it escaped, you gain 1 life. \
                       This permanent escapes with two +1/+1 counters. This permanent escapes with flying."
            .to_string(),
        power: Some(PTValue::Fixed(3)),
        toughness: Some(PTValue::Fixed(3)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(ESCAPE_GATED_EFFECTS.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: vec![AlternativeCost::Replacement {
            description: "Escape—{1}{B}, Exile three other cards from your graveyard.".to_string(),
            cost: CostSpec {
                mana: Some(ManaCost {
                    symbols: vec![
                        ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
                        ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Black)),
                    ],
                }),
                additional: vec![AdditionalCost::ExileFromGraveyard {
                    filter: FilterSpec { card_types: None, supertypes: None, subtypes: None, controller: None, random: false, extra: None },
                    count: 3,
                }],
                alternative: Vec::new(),
            },
            from_zone: ZoneKind::Graveyard,
            exile_on_leaving_stack: false,
            escape: true,
        }],
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn filler_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(1)),
        toughness: Some(PTValue::Fixed(1)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_filler".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state(pool: ManaPool) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![PlayerState { mana_pool: pool, ..player_state(p0) }, player_state(p1)],
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile]))
    }
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, alternative_cost: Option<usize>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost, pay_buyback: false },
        })
        .expect("player controls the card and holds priority");
}

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

/// The most recently entered battlefield permanent's own id - the fresh
/// CR 400.7 id minted by casting, not the original graveyard/hand id.
fn sole_battlefield_object(engine: &Engine) -> ObjectId {
    let bf = engine.state().zone(ZoneKind::Battlefield, None).unwrap();
    assert_eq!(bf.contents.len(), 1, "expected exactly one battlefield permanent");
    *bf.contents.front().unwrap()
}

#[test]
fn escaping_grants_the_life_trigger_counters_and_conditional_keyword() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(1);
    let fodder = [ObjectId(2), ObjectId(3), ObjectId(4)];
    let mut state = two_player_state(ManaPool { black: 1, colorless: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, escaper_definition(), p0, ZoneKind::Graveyard));
    state.zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(card);
    for (i, &id) in fodder.iter().enumerate() {
        state.objects.insert(id, card_object(id, filler_definition(10 + i as u64, &format!("Fodder {i}")), p0, ZoneKind::Graveyard));
        state.zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(id);
    }

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, Some(0));
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));
    engine
        .submit_payment(PaymentPlan {
            payments: vec![
                CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Black), ManaUnit::Colorless], phyrexian_life: 0 },
                CostPayment::Exile(fodder[0]),
                CostPayment::Exile(fodder[1]),
                CostPayment::Exile(fodder[2]),
            ],
        })
        .expect("escape's {1}{B} + exile three other graveyard cards is a legal payment");
    drain(&mut engine);

    // Resolve the escaping creature itself off the stack.
    pass_both(&mut engine, p0, p1);
    let permanent = sole_battlefield_object(&engine);
    assert!(engine.state().object(permanent).unwrap().entered_via_escape, "CR 702.138b: this incarnation escaped");
    assert_eq!(
        engine.state().object(permanent).unwrap().counters.count(&CounterKind::PlusOnePlusOne),
        2,
        "CR 702.138c: escapes with two +1/+1 counters"
    );
    let effective = effective_characteristics(&engine.state(), &ScriptRuntime::new(), permanent);
    assert!(effective.keywords.contains(KeywordSet::FLYING), "CR 702.138d: escapes with flying");

    // The ETB trigger ("if it escaped") is now on the stack - resolve it.
    assert_eq!(engine.state().stack.len(), 1, "the Escaped-gated ETB trigger was collected");
    pass_both(&mut engine, p0, p1);
    assert_eq!(engine.state().player(p0).unwrap().life, 21, "CR 702.138b: the ETB trigger's own intervening-if held");
}

#[test]
fn casting_normally_grants_none_of_the_escape_gated_effects() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool { black: 1, colorless: 2, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, escaper_definition(), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, None);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: vec![ManaUnit::Colored(Color::Black), ManaUnit::Colorless, ManaUnit::Colorless],
                phyrexian_life: 0,
            }],
        })
        .expect("the plain {2}{B} mana cost is a legal payment");
    drain(&mut engine);

    pass_both(&mut engine, p0, p1);
    let permanent = sole_battlefield_object(&engine);
    assert!(!engine.state().object(permanent).unwrap().entered_via_escape, "cast from hand, not escaped");
    assert_eq!(
        engine.state().object(permanent).unwrap().counters.count(&CounterKind::PlusOnePlusOne),
        0,
        "CR 702.138c only applies to an escaped incarnation"
    );
    let effective = effective_characteristics(&engine.state(), &ScriptRuntime::new(), permanent);
    assert!(!effective.keywords.contains(KeywordSet::FLYING), "CR 702.138d only applies to an escaped incarnation");

    // The ETB trigger's own intervening-if never held - collect_triggers
    // filters it out at collection time, so it never even reaches the stack.
    assert!(engine.state().stack.is_empty(), "the Escaped-gated ETB trigger must not have been collected at all");
    assert_eq!(engine.state().player(p0).unwrap().life, 20, "no GainLife - the trigger never fired");
}
