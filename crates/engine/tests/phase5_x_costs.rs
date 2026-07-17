//! Phase 5 vertical slice: {X} costs (CR 107.3/601.2b) for
//! `PlayerActionKind::CastSpell`. Builds on `phase5_cast_spell_payment.rs`'s
//! cost/payment slice - the `CastSpell` handler now requires an announced
//! `x_value` to agree with whether the card's mana cost actually has an
//! `{X}` in it, then substitutes that value in as a generic amount
//! (`ManaCost::resolve_x`) before `PaymentPlan::validate` ever sees the
//! cost. See commands.rs's `CastSpell` arm and mana.rs's `resolve_x` doc
//! comment.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::commands::CommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;

fn spell_definition(mana_cost: ManaCost) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test X Spell".to_string(),
        mana_cost,
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_x_spell".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
    GameObject {
        object_id: id,
        copiable_override: None,
        lineage: ObjectLineage { previous: None },
        definition,
        owner,
        controller: owner,
        zone,
        tapped: false,
        flipped: false,
        face_down: false,
        phased_out: false,
        damage_marked: 0,
        deathtouched: false,
        counters: CounterSet::new(),
        attachments: Vec::new(),
        attached_to: None,
        timestamp: SequenceNo(0),
        controlled_since_turn: 0,
        is_token: false,
        is_commander: false,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn state_with_pool(pool: ManaPool) -> GameState {
    let p0 = PlayerId(0);
    GameState {
        players: vec![PlayerState {
            player_id: p0,
            life: 20,
            mana_pool: pool,
            counters: CounterSet::new(),
            emblems: Vec::new(),
            commander_damage_taken: Vec::new(),
            eliminated: false,
            drew_from_empty_library: false,
            mulligans_taken: 0,
            hand_kept: false,
            commander_casts: std::collections::HashMap::new(),
        }],
        turn_order: vec![p0],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::Untap,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::new(),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
        ],
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: 1, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id: 1000,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    }
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        let result = engine.step();
        match result.status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

/// Test case 1: an `{X}{R}` spell cast with `x_value: Some(3)`, paid with
/// 3 generic (covering the resolved X) + 1 red - legal, resolves.
#[test]
fn x_spell_cast_with_announced_value_is_payable() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![ManaSymbol::X, ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))],
    };
    let mut state =
        state_with_pool(ManaPool { red: 1, colorless: 3, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: Some(3),
                alternative_cost: None,
            },
        })
        .expect("X announced matches the cost's {X}");
    let status = drain(&mut engine);
    let _ = match status {
        StepStatus::AwaitingPayment { player, stack_id } if player == p0 => stack_id,
        other => panic!("expected AwaitingPayment, got {other:?}"),
    };

    let units = vec![
        ManaUnit::Colored(Color::Red),
        ManaUnit::Colorless,
        ManaUnit::Colorless,
        ManaUnit::Colorless,
    ];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("3 generic (for X=3) + 1 red covers {X}{R} exactly");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let pool = &engine.state().player(p0).unwrap().mana_pool;
    assert_eq!((pool.red, pool.colorless), (0, 0), "pool fully debited");
}

/// Test case 2: casting an `{X}{R}` spell without announcing X is rejected
/// before anything is mutated (CR 601.2b).
#[test]
fn x_spell_cast_without_announcing_x_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost {
        symbols: vec![ManaSymbol::X, ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))],
    };
    let mut state = state_with_pool(ManaPool { red: 1, colorless: 3, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell { card, modes: Vec::new(), targets: Vec::new(), x_value: None, alternative_cost: None },
        })
        .unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
    assert_eq!(engine.state().object(card).unwrap().zone, ZoneKind::Hand, "card never left hand");
}

/// Test case 3: announcing an X value for a spell whose cost has no `{X}`
/// is rejected too - a nonsensical announcement, not silently ignored.
#[test]
fn announcing_x_for_a_non_x_spell_is_rejected() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] };
    let mut state = state_with_pool(ManaPool { red: 1, ..ManaPool::default() });
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: Some(1),
                alternative_cost: None,
            },
        })
        .unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::Error(CommandError::IllegalAction(_))));
}

/// Test case 4: `{X}` alone with `x_value: Some(0)` is legal, paid with an
/// explicit empty-units `Mana` entry (CR 107.3/118.5a are distinct
/// acknowledgments - see mana.rs's `resolve_x` doc comment).
#[test]
fn x_only_cost_with_zero_value_needs_an_explicit_empty_mana_entry() {
    let p0 = PlayerId(0);
    let card = ObjectId(0);
    let mana_cost = ManaCost { symbols: vec![ManaSymbol::X] };
    let mut state = state_with_pool(ManaPool::default());
    state.objects.insert(card, card_object(card, spell_definition(mana_cost), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: Some(0),
                alternative_cost: None,
            },
        })
        .expect("X=0 is a legal announced value");
    drain(&mut engine);

    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana { units: Vec::new(), phyrexian_life: 0 }],
        })
        .expect("X resolved to {0}, paid with an explicit empty Mana entry");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));
}
