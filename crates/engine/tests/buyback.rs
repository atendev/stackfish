//! Phase 5 vertical slice: CR 702.27 Buyback.
//!
//! "Buyback [cost]" means "you may pay an additional [cost] as you cast this
//! spell" and "if the buyback cost was paid, put this spell into its
//! owner's hand instead of into that player's graveyard as it resolves."
//! Unlike Flashback/Escape's `AlternativeCost::Replacement` (which
//! *replaces* the mana cost and is chosen by index), Buyback's cost is paid
//! *in addition to* whatever cost already applies and is a plain yes/no
//! announced at cast time - so it's its own `CardDefinition`/
//! `PlayerActionKind::CastSpell`/`StackObject` field trio, not another
//! `AlternativeCost` variant.
//!
//! The resolution-side redirect (`engine/resolve/permanent_entry.rs`) reuses
//! `move_resolving_spell`, which was already fully generic over the
//! destination zone - proven before this slice by `Engine::unwind_payment`,
//! which already sends a card back to `ZoneKind::Hand` through the same
//! helper for CR 601.6. So the only genuinely new mechanism is announcing
//! and remembering the optional cost itself.
//!
//! CR 702.27a's own wording only covers "as it resolves" - unlike
//! Flashback's 702.34a ("as it resolves or as it leaves the stack
//! otherwise"), a fizzled or countered bought-back spell still goes to the
//! graveyard as normal (confirmed by reading the CR text directly, not
//! assumed) - `bought_back` is deliberately only checked in `resolve_
//! permanent_entry`'s normal-resolution path, not `resolve/mod.rs`'s
//! CR 608.2b fizzle branch.

use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::ActionError;
use stackfish_engine::engine::{Engine, PaymentError, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, player_state, two_player_state as base_two_player_state, zones,
};

/// "Test Buyback Bolt": `{1}{R}` instant, targets a creature, with a
/// buyback cost of a bare `{2}`.
fn bolt_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Buyback Bolt".to_string(),
        mana_cost: ManaCost {
            symbols: vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
            ],
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::INSTANT,
        subtypes: Vec::new(),
        oracle_text: "Test Buyback Bolt deals 3 damage to target creature. Buyback {2}."
            .to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_buyback_bolt".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: Some(TargetSpec::Primitive(TargetRequirement {
            filter: TargetFilter::Object {
                mana_value: None,
                excluded_card_types: None,
                excluded_subtypes: None,
                card_types: Some(CardTypeSet::CREATURE),
                subtypes: None,
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
        alternative_costs: Vec::new(),
        buyback_cost: Some(ManaCost {
            symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Generic(2))],
        }),
        effect: Some(AbilityEffect::Damage(3)),
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// A plain `{1}{R}` instant with no buyback cost at all - for the rejection
/// test.
fn no_buyback_bolt_definition() -> CardDefinition {
    CardDefinition {
        buyback_cost: None,
        ..bolt_definition()
    }
}

fn creature_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 2,
        name: "Test Bear".to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        // Toughness comfortably survives the bolt's 3 damage - keeps the
        // zone-change assertions below unambiguous (the creature never
        // dies, so the only Stack-departure in the ledger is the spell
        // itself, not an SBA-triggered death).
        toughness: Some(PTValue::Fixed(6)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_bear".to_string()),
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
        players: vec![
            PlayerState {
                mana_pool: pool,
                ..player_state(p0)
            },
            player_state(p1),
        ],
        ..base_two_player_state(zones(
            p0,
            p1,
            &[
                ZoneKind::Hand,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
                ZoneKind::Exile,
            ],
        ))
    }
}

fn cast_spell(
    engine: &mut Engine,
    player: PlayerId,
    card: ObjectId,
    targets: Vec<Target>,
    pay_buyback: bool,
) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets,
                x_value: None,
                alternative_cost: None,
                pay_buyback,
            },
        })
        .expect("player holds priority, spell is in hand");
}

fn try_cast_spell(
    engine: &mut Engine,
    player: PlayerId,
    card: ObjectId,
    targets: Vec<Target>,
    pay_buyback: bool,
) -> Result<(), ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::CastSpell {
            card,
            modes: Vec::new(),
            targets,
            x_value: None,
            alternative_cost: None,
            pay_buyback,
        },
    })
}

/// Finds the `ZoneChange` whose origin is the stack - i.e. the resolving
/// spell's own move, not some other zone change (e.g. an SBA-triggered
/// creature death) that might also land in the ledger.
fn stack_departure(engine: &Engine) -> (ObjectId, ZoneKind, ZoneKind) {
    for event in engine.state().ledger.iter().rev() {
        if let GameEventKind::ZoneChange {
            object,
            from: ZoneKind::Stack,
            to,
            ..
        } = event.kind
        {
            return (object, ZoneKind::Stack, to);
        }
    }
    panic!("expected a Stack-origin ZoneChange event in the ledger");
}

fn pass_both(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine);
    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine)
}

/// Casting without paying buyback is entirely unaffected - same mana cost,
/// still lands in the graveyard on resolution.
#[test]
fn cast_without_paying_buyback_resolves_to_graveyard_as_normal() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool {
        red: 1,
        colorless: 1,
        ..ManaPool::default()
    });
    state.objects.insert(
        creature,
        card_object(creature, creature_definition(), p0, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(creature);
    state.objects.insert(
        card,
        card_object(card, bolt_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(creature)], false);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));

    let units = vec![ManaUnit::Colored(Color::Red), ManaUnit::Colorless];
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units,
                phyrexian_life: 0,
            }],
        })
        .expect("{1}{R} paid with 1 red + 1 generic - buyback not paid, so no extra {2} owed");
    drain(&mut engine);
    pass_both(&mut engine, p0, p1);

    let (new_id, from, to) = stack_departure(&engine);
    assert_eq!((from, to), (ZoneKind::Stack, ZoneKind::Graveyard));
    assert!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(p0))
            .unwrap()
            .contents
            .contains(&new_id)
    );
}

/// Paying buyback enforces the combined `{1}{R}` + `{2}` cost, and the spell
/// returns to hand (not the graveyard) once it resolves.
#[test]
fn cast_paying_buyback_returns_to_hand_on_resolution() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool {
        red: 1,
        colorless: 3,
        ..ManaPool::default()
    });
    state.objects.insert(
        creature,
        card_object(creature, creature_definition(), p0, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(creature);
    state.objects.insert(
        card,
        card_object(card, bolt_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(creature)], true);
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }));

    // {1}{R} (mana cost) + {2} (buyback) = {3}{R} total: 1 red + 3 generic.
    let units = vec![
        ManaUnit::Colored(Color::Red),
        ManaUnit::Colorless,
        ManaUnit::Colorless,
        ManaUnit::Colorless,
    ];
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units,
                phyrexian_life: 0,
            }],
        })
        .expect("the combined {3}{R} cost is payable with 1 red + 3 generic");
    drain(&mut engine);
    pass_both(&mut engine, p0, p1);

    assert_eq!(
        engine.state().player(p0).unwrap().life,
        20,
        "no combat/direct damage to p0"
    );
    assert_eq!(
        engine.state().object(creature).map(|o| o.damage_marked),
        Some(3),
        "the spell's own effect still resolved normally"
    );
    let (new_id, from, to) = stack_departure(&engine);
    assert_eq!(
        (from, to),
        (ZoneKind::Stack, ZoneKind::Hand),
        "bought back - returns to hand, not the graveyard"
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Hand, Some(p0))
            .unwrap()
            .contents
            .contains(&new_id)
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(p0))
            .unwrap()
            .contents
            .is_empty()
    );
}

/// Paying an insufficient amount for the combined cost is rejected the same
/// way any other underpayment is - proves the buyback amount is really
/// folded into the enforced total, not just cosmetically tracked.
#[test]
fn underpaying_the_combined_buyback_cost_is_rejected() {
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool {
        red: 1,
        colorless: 3,
        ..ManaPool::default()
    });
    state.objects.insert(
        creature,
        card_object(creature, creature_definition(), p0, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(creature);
    state.objects.insert(
        card,
        card_object(card, bolt_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(creature)], true);
    drain(&mut engine);

    // Only the bare {1}{R} - missing the {2} buyback component.
    let units = vec![ManaUnit::Colored(Color::Red), ManaUnit::Colorless];
    let err = engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units,
                phyrexian_life: 0,
            }],
        })
        .unwrap_err();
    assert!(matches!(err, PaymentError::InvalidPlan));
}

/// Announcing `pay_buyback: true` on a card with no buyback cost is
/// rejected outright, before any state mutation.
#[test]
fn pay_buyback_on_a_card_with_no_buyback_cost_is_rejected() {
    let p0 = PlayerId(0);
    let creature = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool {
        red: 1,
        colorless: 1,
        ..ManaPool::default()
    });
    state.objects.insert(
        creature,
        card_object(creature, creature_definition(), p0, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(creature);
    state.objects.insert(
        card,
        card_object(card, no_buyback_bolt_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    let err =
        try_cast_spell(&mut engine, p0, card, vec![Target::Object(creature)], true).unwrap_err();
    assert!(matches!(
        err,
        ActionError::Rejected(Rejected::IllegalAction(_))
    ));
    assert_eq!(
        engine.state().object(card).unwrap().zone,
        ZoneKind::Hand,
        "card untouched"
    );
    assert!(engine.state().stack.is_empty(), "never pushed");
}

/// CR 702.27a only covers "as it resolves" - a bought-back spell that
/// fizzles (its only target became illegal) still goes to the graveyard,
/// not hand.
#[test]
fn a_fizzled_bought_back_spell_still_goes_to_the_graveyard() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let creature = ObjectId(0);
    let card = ObjectId(1);
    let mut state = two_player_state(ManaPool {
        red: 1,
        colorless: 3,
        ..ManaPool::default()
    });
    state.objects.insert(
        creature,
        card_object(creature, creature_definition(), p1, ZoneKind::Battlefield),
    );
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(creature);
    state.objects.insert(
        card,
        card_object(card, bolt_definition(), p0, ZoneKind::Hand),
    );
    state
        .zone_mut(ZoneKind::Hand, Some(p0))
        .unwrap()
        .push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card, vec![Target::Object(creature)], true);
    drain(&mut engine);
    let units = vec![
        ManaUnit::Colored(Color::Red),
        ManaUnit::Colorless,
        ManaUnit::Colorless,
        ManaUnit::Colorless,
    ];
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units,
                phyrexian_life: 0,
            }],
        })
        .unwrap();
    drain(&mut engine);

    // Target leaves the battlefield before this resolves - same technique
    // flashback_alternative_costs.rs's own fizzle test uses.
    engine
        .state_mut()
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .remove(creature);
    engine.state_mut().objects.remove(&creature);

    let status = pass_both(&mut engine, p0, p1);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let fizzled = engine
        .state()
        .ledger
        .iter()
        .any(|e| matches!(e.kind, GameEventKind::Fizzled { .. }));
    assert!(fizzled, "expected a Fizzled event");

    let (new_id, from, to) = stack_departure(&engine);
    assert_eq!(
        (from, to),
        (ZoneKind::Stack, ZoneKind::Graveyard),
        "buyback doesn't apply to a fizzled spell"
    );
    assert!(
        engine
            .state()
            .zone(ZoneKind::Graveyard, Some(p0))
            .unwrap()
            .contents
            .contains(&new_id)
    );
    assert!(
        !engine
            .state()
            .zone(ZoneKind::Hand, Some(p0))
            .unwrap()
            .contents
            .contains(&new_id)
    );
}
