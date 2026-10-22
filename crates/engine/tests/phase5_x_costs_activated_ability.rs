//! Phase 5 vertical slice: {X} costs (CR 107.3a/107.3k/602.2b) for
//! `PlayerActionKind::ActivateAbility` - mirrors `phase5_x_costs.rs`'s
//! `CastSpell` slice exactly, on the activation side. `ActivateAbility` now
//! carries its own `x_value`, checked for agreement against whether this
//! ability's own cost actually has an `{X}` (`activate.rs`'s `cost_has_x`
//! check) before `ManaCost::resolve_x` substitutes it in, same as casting.

use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{ActionError, Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, single_player_state};

fn ability_definition() -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test X Source".to_string(),
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
        script_ref: ScriptReference::ByKey("test_x_source".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

/// One player, main phase, empty stack, a battlefield `source` with a single
/// `Activated` ability whose own cost is `mana_cost` (an `{X}`-bearing cost,
/// same shape `Game.Costs.Mana` produces) and whose effect gains 1 life -
/// the effect itself never reads X, since this slice is about the cost side
/// only (CR 107.3a), not about X-parameterized effect text.
fn state_with_ability_cost(mana_cost: ManaCost, pool: ManaPool) -> (GameState, PlayerId, ObjectId) {
    let p0 = PlayerId(0);
    let source = ObjectId(0);
    let mut state = GameState {
        step: Step::Untap,
        players: vec![PlayerState { mana_pool: pool, ..stackfish_testutil::player_state(p0) }],
        abilities: vec![RegisteredAbility {
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
            id: AbilityId(0),
            source,
            kind: AbilityKind::Activated,
            condition: None,
            cost: Some(CostSpec::mana_only(mana_cost)),
            target: None,
            operation: ScriptHandle(0),
            effect: Some(AbilityEffect::GainLife(1)),
            intervening_if: None,
        }],
        ..single_player_state(p0, vec![Zone::new(ZoneKind::Battlefield, None)])
    };
    state.objects.insert(source, card_object(source, ability_definition(), p0, ZoneKind::Battlefield));
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(source);
    (state, p0, source)
}

fn activate(engine: &mut Engine, player: PlayerId, source: ObjectId, x_value: Option<u32>) -> Result<(), ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::ActivateAbility {
            source,
            ability_index: 0,
            targets: Vec::new(),
            chosen_colors: Vec::new(),
            x_value,
        },
    })
}

/// Test case 1: an `{X}{R}` ability cost activated with `x_value: Some(3)`,
/// paid with 3 generic (covering the resolved X) + 1 red - legal, resolves.
#[test]
fn x_ability_activated_with_announced_value_is_payable() {
    let mana_cost =
        ManaCost { symbols: vec![ManaSymbol::X, ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] };
    let (state, p0, source) =
        state_with_ability_cost(mana_cost, ManaPool { red: 1, colorless: 3, ..ManaPool::default() });
    let mut engine = Engine::new(state);

    activate(&mut engine, p0, source, Some(3)).expect("X announced matches the ability's own {X}");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { player, .. } if player == p0));

    let units =
        vec![ManaUnit::Colored(Color::Red), ManaUnit::Colorless, ManaUnit::Colorless, ManaUnit::Colorless];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("3 generic (for X=3) + 1 red covers {X}{R} exactly");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!((pool.red, pool.colorless), (0, 0), "pool fully debited");
}

/// Test case 2: activating an `{X}{R}` ability without announcing X is
/// rejected before anything is mutated (CR 602.2b/601.2b).
#[test]
fn x_ability_activated_without_announcing_x_is_rejected() {
    let mana_cost =
        ManaCost { symbols: vec![ManaSymbol::X, ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] };
    let (state, p0, source) =
        state_with_ability_cost(mana_cost, ManaPool { red: 1, colorless: 3, ..ManaPool::default() });
    let mut engine = Engine::new(state);

    let err = activate(&mut engine, p0, source, None).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
    assert!(!engine.state().object(source).unwrap().tapped, "nothing paid, nothing mutated");
}

/// Test case 3: announcing an X value for an ability whose cost has no
/// `{X}` is rejected too - a nonsensical announcement, not silently ignored.
#[test]
fn announcing_x_for_a_non_x_ability_is_rejected() {
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] };
    let (state, p0, source) = state_with_ability_cost(mana_cost, ManaPool { red: 1, ..ManaPool::default() });
    let mut engine = Engine::new(state);

    let err = activate(&mut engine, p0, source, Some(1)).unwrap_err();
    assert!(matches!(err, ActionError::Rejected(Rejected::IllegalAction(_))));
}

/// Test case 4: `{X}` alone with `x_value: Some(0)` is legal, paid with an
/// explicit empty-units `Mana` entry (CR 107.3/118.5a are distinct
/// acknowledgments - see mana.rs's `resolve_x` doc comment).
#[test]
fn x_only_ability_cost_with_zero_value_needs_an_explicit_empty_mana_entry() {
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::X] };
    let (state, p0, source) = state_with_ability_cost(mana_cost, ManaPool::default());
    let mut engine = Engine::new(state);

    activate(&mut engine, p0, source, Some(0)).expect("X=0 is a legal announced value");
    drain(&mut engine);

    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units: Vec::new(), phyrexian_life: 0 }] })
        .expect("X resolved to {0}, paid with an explicit empty Mana entry");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}

/// Test case 5 (CR 107.3k): this ability's own X is independent of any
/// other instance of the same ability already on the stack - activating it
/// twice with two different announced X values before either resolves
/// leaves both `StackObject`s carrying their own value, uncontaminated by
/// the other.
#[test]
fn two_activations_of_the_same_x_ability_keep_independent_x_values() {
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::X] };
    let (state, p0, source) =
        state_with_ability_cost(mana_cost, ManaPool { colorless: 5, ..ManaPool::default() });
    let mut engine = Engine::new(state);

    activate(&mut engine, p0, source, Some(2)).expect("first activation, X=2");
    drain(&mut engine);
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: vec![ManaUnit::Colorless, ManaUnit::Colorless],
                phyrexian_life: 0,
            }],
        })
        .expect("2 generic covers the first activation's resolved {2}");
    drain(&mut engine);

    activate(&mut engine, p0, source, Some(3)).expect("second activation, X=3, independent of the first");
    drain(&mut engine);
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: vec![ManaUnit::Colorless, ManaUnit::Colorless, ManaUnit::Colorless],
                phyrexian_life: 0,
            }],
        })
        .expect("3 generic covers the second activation's resolved {3}");
    drain(&mut engine);

    assert_eq!(engine.state().stack.len(), 2, "both activations are on the stack, neither resolved yet");
    let x_values: Vec<Option<u32>> = engine.state().stack.iter().map(|so| so.x_value).collect();
    assert_eq!(x_values, vec![Some(2), Some(3)], "each instance kept its own announced X (CR 107.3k)");
}
