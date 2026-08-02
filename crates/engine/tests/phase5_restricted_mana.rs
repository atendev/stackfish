//! Phase 5 vertical slice: CR 106.11 "spend this mana only to..." restricted
//! mana, narrowed to the single most common real shape - a card-type filter
//! on what the mana can be spent to *cast* (Mishra's Workshop: "Add
//! {C}{C}{C}. Spend this mana only to cast artifact spells.").
//!
//! `ManaUnit::RestrictedColored(Color, CardTypeSet)`/`RestrictedColorless
//! (CardTypeSet)` are new variants (not a field on the plain ones, same
//! precedent `SnowColored`/`SnowColorless` already set) that live in
//! `ManaPool::restricted`, a side-channel separate from the plain aggregate
//! fields - those can't tell a restricted unit apart from an ordinary one of
//! the same color. `PaymentPlan::validate` checks a declared restricted unit
//! two ways: it must actually be present in the pool's `restricted` list,
//! and the cost being paid must be eligible - casting a spell (the paying
//! object's own `GameObject` sitting in the `Stack` zone, not an activated
//! ability) of a card type the restriction names.
//!
//! No mana-ability/Lua wiring exercised here (`Game.Ops.AddRestrictedMana`
//! is proven separately) - fixtures hand-seed `ManaPool.restricted` directly,
//! same "fake pre-seeded floating mana" convention `phase5_cast_spell_
//! payment.rs` already established for plain mana.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, PaymentError, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, single_player_state};

fn spell_definition(mana_cost: ManaCost, card_types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Spell".to_string(),
        mana_cost,
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_spell".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn state_with_pool(pool: ManaPool) -> GameState {
    let p0 = PlayerId(0);
    GameState {
        step: Step::Untap,
        last_active_turn_number: HashMap::new(),
        players: vec![PlayerState { mana_pool: pool, ..stackfish_testutil::player_state(p0) }],
        ..single_player_state(
            p0,
            vec![
                Zone::new(ZoneKind::Hand, Some(p0)),
                Zone::new(ZoneKind::Battlefield, None),
                Zone::new(ZoneKind::Graveyard, Some(p0)),
            ],
        )
    }
}

fn cast(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None , pay_buyback: false},
        })
        .expect("player holds priority, spell is in hand, main phase, empty stack");
}

/// One artifact-only unit of restricted colorless mana, standing in for
/// `Game.Ops.AddRestrictedMana("C", "Artifact")`'s real output.
fn artifact_only_colorless() -> ManaUnit {
    ManaUnit::RestrictedColorless(CardTypeSet::ARTIFACT)
}

#[test]
fn restricted_mana_pays_for_a_matching_spell_cast() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(1))] };
    let mut state = state_with_pool(ManaPool { restricted: vec![artifact_only_colorless()], ..ManaPool::default() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::ARTIFACT), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: vec![artifact_only_colorless()], phyrexian_life: 0 }],
        })
        .expect("restricted mana is eligible - casting an artifact spell");
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert!(pool.restricted.is_empty(), "the spent restricted unit is removed from the pool");
}

#[test]
fn restricted_mana_rejected_for_a_non_matching_card_type() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(1))] };
    let mut state = state_with_pool(ManaPool { restricted: vec![artifact_only_colorless()], ..ManaPool::default() });
    // A creature spell, not an artifact spell - the restriction doesn't cover it.
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::CREATURE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: vec![artifact_only_colorless()], phyrexian_life: 0 }],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!(pool.restricted.len(), 1, "rejected plan leaves the restricted unit untouched");
}

#[test]
fn declaring_a_restricted_unit_the_pool_does_not_actually_hold_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(2))] };
    // Only one restricted unit floating - declaring two of them is a lie.
    let mut state = state_with_pool(ManaPool { restricted: vec![artifact_only_colorless()], ..ManaPool::default() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::ARTIFACT), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: vec![artifact_only_colorless(), artifact_only_colorless()],
                phyrexian_life: 0,
            }],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.restricted.len(), 1, "pool untouched");
}

#[test]
fn ordinary_mana_in_the_same_pool_is_unaffected_by_a_restricted_unit() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(1))] };
    // One ordinary colorless, plus a separate restricted colorless that
    // shouldn't be usable (or needed) for this plain-colorless payment.
    let mut state =
        state_with_pool(ManaPool { colorless: 1, restricted: vec![artifact_only_colorless()], ..ManaPool::default() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost, CardTypeSet::CREATURE), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast(&mut engine, p0, card);
    drain(&mut engine);

    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: vec![ManaUnit::Colorless], phyrexian_life: 0 }],
        })
        .expect("the ordinary colorless unit alone covers a plain {1} cost");
    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!(pool.colorless, 0, "the ordinary unit was spent");
    assert_eq!(pool.restricted.len(), 1, "the restricted unit was never touched");
}

/// CR 106.11's restriction only covers *casting* a spell of the named card
/// type - an activated ability's cost is never eligible, even one belonging
/// to an artifact permanent, since the permanent being activated isn't
/// itself "a spell being cast."
#[test]
fn restricted_mana_cannot_fund_an_activated_ability_cost() {
    let p0 = PlayerId(0);
    let source = ObjectId(0);

    let source_definition = CardDefinition {
        definition_id: 1,
        name: "Artifact Source".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ARTIFACT,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    };

    let mut objects = HashMap::new();
    objects.insert(source, card_object(source, source_definition, p0, ZoneKind::Battlefield));

    let cost = CostSpec {
        mana: Some(ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(1))] }),
        additional: Vec::new(),
        alternative: Vec::new(),
    };

    let state = GameState {
        objects,
        zones: vec![
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::from([source]) },
            Zone::new(ZoneKind::Hand, Some(p0)),
            Zone::new(ZoneKind::Graveyard, Some(p0)),
        ],
        players: vec![PlayerState {
            mana_pool: ManaPool { restricted: vec![artifact_only_colorless()], ..ManaPool::default() },
            ..stackfish_testutil::player_state(p0)
        }],
        abilities: vec![RegisteredAbility {
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
            id: AbilityId(0),
            source,
            kind: AbilityKind::Activated,
            condition: None,
            cost: Some(cost),
            target: None,
            operation: ScriptHandle(0),
            effect: Some(AbilityEffect::GainLife(1)),
            intervening_if: None,
        }],
        ..single_player_state(p0, Vec::new())
    };

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::ActivateAbility { source, ability_index: 0, targets: Vec::new(), chosen_colors: Vec::new() },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));

    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: vec![artifact_only_colorless()], phyrexian_life: 0 }],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
    assert_eq!(engine.state().player(p0).unwrap().mana_pool.restricted.len(), 1, "pool untouched");
}
