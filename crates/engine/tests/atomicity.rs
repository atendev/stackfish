//! A rejected `PlayerAction` changes nothing: `assert_action_rejected` compares the whole engine before and after.
//! Covers each command arm. The `submit_*` methods are in atomicity_submit.rs.

use std::collections::HashMap;

use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::{ActionError, Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{
    assert_action_rejected, assert_unchanged_after, card_object, drain, minimal_card_definition, two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn empty_cost() -> ManaCost {
    ManaCost { symbols: Vec::new() }
}

fn def(id: u64, name: &str, types: CardTypeSet) -> CardDefinition {
    minimal_card_definition(id, name, name, empty_cost(), types)
}

fn creature(id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        ..def(id, name, CardTypeSet::CREATURE)
    }
}

fn any_target() -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::AnyTarget,
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    })
}

fn main_state(kinds: &[ZoneKind]) -> GameState {
    GameState {
        last_active_turn_number: HashMap::from([(P0, 1), (P1, 0)]),
        ..two_player_state(zones(P0, P1, kinds))
    }
}

fn game() -> GameState {
    main_state(&[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard])
}

fn put(state: &mut GameState, kind: ZoneKind, owner: Option<PlayerId>, object: GameObject) -> ObjectId {
    let id = object.object_id;
    state.zone_mut(kind, owner).unwrap().push_bottom(id);
    state.objects.insert(id, object);
    id
}

fn in_hand(state: &mut GameState, id: u64, definition: CardDefinition) -> ObjectId {
    put(state, ZoneKind::Hand, Some(P0), card_object(ObjectId(id), definition, P0, ZoneKind::Hand))
}

fn on_battlefield(state: &mut GameState, id: u64, definition: CardDefinition, owner: PlayerId) -> ObjectId {
    put(state, ZoneKind::Battlefield, None, card_object(ObjectId(id), definition, owner, ZoneKind::Battlefield))
}

fn act(player: PlayerId, kind: PlayerActionKind) -> PlayerAction {
    PlayerAction { player, kind }
}

fn cast(card: ObjectId) -> PlayerActionKind {
    PlayerActionKind::CastSpell {
        card,
        modes: Vec::new(),
        targets: Vec::new(),
        x_value: None,
        alternative_cost: None,
        pay_buyback: false,
    }
}

fn activate(source: ObjectId) -> PlayerActionKind {
    PlayerActionKind::ActivateAbility {
        source,
        ability_index: 0,
        targets: Vec::new(),
        chosen_colors: Vec::new(),
        x_value: None,
    }
}

fn ability(source: ObjectId, cost: Option<CostSpec>, effect: AbilityEffect) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(source.0),
        source,
        kind: AbilityKind::Activated,
        condition: None,
        cost,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(effect),
        intervening_if: None,
    }
}

fn tap_cost() -> CostSpec {
    CostSpec { mana: None, additional: vec![AdditionalCost::TapSelf], alternative: Vec::new() }
}

fn illegal(rejected: &ActionError) -> bool {
    matches!(rejected, ActionError::Rejected(_) | ActionError::IllegalAction(_))
}

#[test]
fn pass_priority_by_the_wrong_player_is_rejected() {
    let mut engine = Engine::new(game());
    let rejected = assert_action_rejected(&mut engine, "pass by p1", act(P1, PlayerActionKind::PassPriority));
    assert!(matches!(rejected, ActionError::NotYourTurn));
}

#[test]
fn make_choice_by_the_wrong_player_is_rejected() {
    let mut engine = Engine::new(game());
    let rejected =
        assert_action_rejected(&mut engine, "choice by p1", act(P1, PlayerActionKind::MakeChoice { description: String::new(), selection: Vec::new() }));
    assert!(matches!(rejected, ActionError::NotYourTurn));
}

#[test]
fn pregame_actions_are_rejected_by_submit_action() {
    let mut engine = Engine::new(game());
    let rejected = assert_action_rejected(&mut engine, "keep hand", act(P0, PlayerActionKind::KeepHand));
    assert!(matches!(rejected, ActionError::IllegalAction(_)));
}

#[test]
fn an_action_while_the_engine_is_mid_pipeline_is_rejected() {
    let mut state = game();
    let card = in_hand(&mut state, 1, def(1, "Spell", CardTypeSet::INSTANT));
    let mut engine = Engine::new(state);
    engine.submit_action(act(P0, cast(card))).unwrap();
    // The cast is queued, not run, so the engine is no longer waiting for an action.
    let rejected = assert_unchanged_after(&mut engine, "second action while queued", |e| {
        e.submit_action(act(P0, PlayerActionKind::PassPriority))
    });
    assert!(matches!(rejected, Err(ActionError::IllegalAction(_))));
}

fn land_engine() -> (Engine, ObjectId) {
    let mut state = game();
    let land = in_hand(&mut state, 1, def(1, "Forest", CardTypeSet::LAND));
    (Engine::new(state), land)
}

#[test]
fn play_land_outside_a_main_phase_is_rejected() {
    let mut state = game();
    state.phase = Phase::Combat;
    state.step = Step::BeginningOfCombat;
    let land = in_hand(&mut state, 1, def(1, "Forest", CardTypeSet::LAND));
    let mut engine = Engine::new(state);
    let rejected = assert_action_rejected(&mut engine, "land in combat", act(P0, PlayerActionKind::PlayLand { card: land }));
    assert!(illegal(&rejected), "{rejected:?}");
}

#[test]
fn play_land_with_a_nonempty_stack_is_rejected() {
    let mut state = game();
    let land = in_hand(&mut state, 1, def(1, "Forest", CardTypeSet::LAND));
    let spell = in_hand(&mut state, 2, def(2, "Spell", CardTypeSet::INSTANT));
    let mut engine = Engine::new(state);
    engine.submit_action(act(P0, cast(spell))).unwrap();
    drain(&mut engine);
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).unwrap();
    drain(&mut engine);
    assert_eq!(engine.state().stack.len(), 1);
    let rejected = assert_action_rejected(&mut engine, "land on a stack", act(P0, PlayerActionKind::PlayLand { card: land }));
    assert!(illegal(&rejected), "{rejected:?}");
}

#[test]
fn play_land_over_the_limit_is_rejected() {
    let (mut engine, land) = land_engine();
    engine.state_mut().lands_played_this_turn.insert(P0, 1);
    let rejected = assert_action_rejected(&mut engine, "second land", act(P0, PlayerActionKind::PlayLand { card: land }));
    assert!(illegal(&rejected), "{rejected:?}");
}

#[test]
fn play_land_with_a_nonland_is_rejected() {
    let mut state = game();
    let card = in_hand(&mut state, 1, def(1, "Bear", CardTypeSet::CREATURE));
    let mut engine = Engine::new(state);
    let rejected = assert_action_rejected(&mut engine, "creature as land", act(P0, PlayerActionKind::PlayLand { card }));
    assert!(illegal(&rejected), "{rejected:?}");
}

#[test]
fn play_land_that_is_not_in_hand_is_rejected() {
    let mut state = game();
    let land = on_battlefield(&mut state, 1, def(1, "Forest", CardTypeSet::LAND), P0);
    let mut engine = Engine::new(state);
    let rejected = assert_action_rejected(&mut engine, "land from battlefield", act(P0, PlayerActionKind::PlayLand { card: land }));
    assert!(illegal(&rejected), "{rejected:?}");
}

#[test]
fn play_land_that_does_not_exist_is_rejected() {
    let (mut engine, _) = land_engine();
    let rejected = assert_action_rejected(&mut engine, "unknown land", act(P0, PlayerActionKind::PlayLand { card: ObjectId(777) }));
    assert!(matches!(rejected, ActionError::Rejected(Rejected::InvalidTarget)), "{rejected:?}");
}

// A missing battlefield zone must not leave the card gone from the hand.
#[test]
fn play_land_without_a_battlefield_zone_changes_nothing() {
    let mut state = main_state(&[ZoneKind::Hand]);
    let land = in_hand(&mut state, 1, def(1, "Forest", CardTypeSet::LAND));
    let mut engine = Engine::new(state);
    let rejected = assert_action_rejected(&mut engine, "no battlefield", act(P0, PlayerActionKind::PlayLand { card: land }));
    assert!(illegal(&rejected), "{rejected:?}");
}

fn bolt() -> CardDefinition {
    CardDefinition { target_requirement: Some(any_target()), ..def(10, "Bolt", CardTypeSet::INSTANT) }
}

fn cast_engine(definition: CardDefinition) -> (Engine, ObjectId) {
    let mut state = game();
    let card = in_hand(&mut state, 1, definition);
    (Engine::new(state), card)
}

fn cast_with(card: ObjectId, f: impl FnOnce(&mut PlayerActionKind)) -> PlayerAction {
    let mut kind = cast(card);
    f(&mut kind);
    act(P0, kind)
}

#[test]
fn cast_with_too_few_targets_is_rejected() {
    let (mut engine, card) = cast_engine(bolt());
    let rejected = assert_action_rejected(&mut engine, "no target", act(P0, cast(card)));
    assert!(illegal(&rejected), "{rejected:?}");
}

#[test]
fn cast_with_too_many_targets_is_rejected() {
    let (mut engine, card) = cast_engine(bolt());
    let action = cast_with(card, |k| {
        if let PlayerActionKind::CastSpell { targets, .. } = k {
            *targets = vec![Target::Player(P0), Target::Player(P1)];
        }
    });
    let rejected = assert_action_rejected(&mut engine, "two targets", action);
    assert!(illegal(&rejected), "{rejected:?}");
}

#[test]
fn cast_with_an_illegal_target_is_rejected() {
    let (mut engine, card) = cast_engine(bolt());
    let action = cast_with(card, |k| {
        if let PlayerActionKind::CastSpell { targets, .. } = k {
            *targets = vec![Target::Object(ObjectId(555))];
        }
    });
    assert_action_rejected(&mut engine, "missing target object", action);
}

#[test]
fn cast_with_modes_on_a_nonmodal_card_is_rejected() {
    let (mut engine, card) = cast_engine(def(1, "Spell", CardTypeSet::INSTANT));
    let action = cast_with(card, |k| {
        if let PlayerActionKind::CastSpell { modes, .. } = k {
            *modes = vec![0];
        }
    });
    let rejected = assert_action_rejected(&mut engine, "modes on a plain spell", action);
    assert!(illegal(&rejected), "{rejected:?}");
}

fn modal_choose_one() -> CardDefinition {
    CardDefinition {
        modes: vec![
            SpellMode { target_requirement: None, effect: Some(AbilityEffect::GainLife(3)), additional_cost: None, pawprint_weight: 0 },
            SpellMode { target_requirement: None, effect: Some(AbilityEffect::DrawCards(1)), additional_cost: None, pawprint_weight: 0 },
        ],
        mode_choice: ModeChoice::Exactly(1),
        ..def(1, "Charm", CardTypeSet::INSTANT)
    }
}

#[test]
fn cast_with_the_wrong_number_of_modes_is_rejected() {
    for modes in [vec![], vec![0, 1]] {
        let (mut engine, card) = cast_engine(modal_choose_one());
        let action = cast_with(card, |k| {
            if let PlayerActionKind::CastSpell { modes: m, .. } = k {
                *m = modes.clone();
            }
        });
        let rejected = assert_action_rejected(&mut engine, &format!("modes {modes:?}"), action);
        assert!(illegal(&rejected), "{rejected:?}");
    }
}

#[test]
fn cast_with_a_mode_that_does_not_exist_is_rejected() {
    let (mut engine, card) = cast_engine(modal_choose_one());
    let action = cast_with(card, |k| {
        if let PlayerActionKind::CastSpell { modes, .. } = k {
            *modes = vec![9];
        }
    });
    assert_action_rejected(&mut engine, "mode 9", action);
}

#[test]
fn cast_with_a_bad_x_value_is_rejected() {
    let x_spell = CardDefinition {
        mana_cost: ManaCost { symbols: vec![ManaSymbol::X] },
        ..def(1, "Fireball", CardTypeSet::INSTANT)
    };
    let (mut engine, card) = cast_engine(x_spell);
    let rejected = assert_action_rejected(&mut engine, "X cost with no X", act(P0, cast(card)));
    assert!(illegal(&rejected), "{rejected:?}");

    let (mut engine, card) = cast_engine(def(1, "Spell", CardTypeSet::INSTANT));
    let action = cast_with(card, |k| {
        if let PlayerActionKind::CastSpell { x_value, .. } = k {
            *x_value = Some(2);
        }
    });
    let rejected = assert_action_rejected(&mut engine, "X with no X cost", action);
    assert!(illegal(&rejected), "{rejected:?}");
}

#[test]
fn cast_with_the_wrong_timing_is_rejected() {
    let mut state = game();
    state.phase = Phase::Combat;
    state.step = Step::BeginningOfCombat;
    let card = in_hand(&mut state, 1, def(1, "Sorcery", CardTypeSet::SORCERY));
    let mut engine = Engine::new(state);
    let rejected = assert_action_rejected(&mut engine, "sorcery in combat", act(P0, cast(card)));
    assert!(illegal(&rejected), "{rejected:?}");

    let mut state = game();
    let card = in_hand(&mut state, 1, def(1, "Instant", CardTypeSet::INSTANT));
    state.priority_player = P1;
    let mut engine = Engine::new(state);
    let rejected = assert_action_rejected(&mut engine, "instant without priority", act(P0, cast(card)));
    assert!(matches!(rejected, ActionError::NotYourTurn));
}

#[test]
fn cast_a_land_or_a_card_from_the_wrong_zone_is_rejected() {
    let (mut engine, card) = cast_engine(def(1, "Forest", CardTypeSet::LAND));
    assert_action_rejected(&mut engine, "land as spell", act(P0, cast(card)));

    let mut state = game();
    let card = on_battlefield(&mut state, 1, def(1, "Instant", CardTypeSet::INSTANT), P0);
    let mut engine = Engine::new(state);
    assert_action_rejected(&mut engine, "cast from battlefield", act(P0, cast(card)));

    let mut engine = Engine::new(game());
    let rejected = assert_action_rejected(&mut engine, "unknown card", act(P0, cast(ObjectId(777))));
    assert!(matches!(rejected, ActionError::Rejected(Rejected::InvalidTarget)), "{rejected:?}");
}

#[test]
fn cast_with_a_bad_alternative_cost_or_buyback_is_rejected() {
    let (mut engine, card) = cast_engine(def(1, "Spell", CardTypeSet::INSTANT));
    let action = cast_with(card, |k| {
        if let PlayerActionKind::CastSpell { alternative_cost, .. } = k {
            *alternative_cost = Some(3);
        }
    });
    assert_action_rejected(&mut engine, "alternative cost 3", action);

    let (mut engine, card) = cast_engine(def(1, "Spell", CardTypeSet::INSTANT));
    let action = cast_with(card, |k| {
        if let PlayerActionKind::CastSpell { pay_buyback, .. } = k {
            *pay_buyback = true;
        }
    });
    assert_action_rejected(&mut engine, "buyback with no buyback cost", action);
}

// A missing source zone must not advance the id and sequence counters.
#[test]
fn cast_without_the_source_zone_changes_nothing() {
    let mut state = main_state(&[ZoneKind::Battlefield]);
    let card = ObjectId(1);
    state.objects.insert(card, card_object(card, def(1, "Instant", CardTypeSet::INSTANT), P0, ZoneKind::Hand));
    let mut engine = Engine::new(state);
    let rejected = assert_action_rejected(&mut engine, "no hand zone", act(P0, cast(card)));
    assert!(illegal(&rejected), "{rejected:?}");
}

fn mana_ability(effect: AbilityEffect, cost: Option<CostSpec>) -> (Engine, ObjectId) {
    let mut state = game();
    let source = on_battlefield(&mut state, 1, def(1, "Forest", CardTypeSet::LAND), P0);
    state.abilities.push(ability(source, cost, effect));
    (Engine::new(state), source)
}

fn green() -> AbilityEffect {
    AbilityEffect::AddMana(vec![ManaUnit::Colored(Color::Green)])
}

#[test]
fn activate_an_ability_index_that_does_not_exist_is_rejected() {
    let (mut engine, source) = mana_ability(green(), None);
    let action = act(
        P0,
        PlayerActionKind::ActivateAbility {
            source,
            ability_index: 4,
            targets: Vec::new(),
            chosen_colors: Vec::new(),
            x_value: None,
        },
    );
    let rejected = assert_action_rejected(&mut engine, "ability 4", action);
    assert!(illegal(&rejected), "{rejected:?}");
}

#[test]
fn activate_a_mana_ability_with_a_tapped_or_sick_source_is_rejected() {
    let (mut engine, source) = mana_ability(green(), None);
    engine.state_mut().object_mut(source).unwrap().tapped = true;
    let rejected = assert_action_rejected(&mut engine, "tapped land", act(P0, activate(source)));
    assert!(illegal(&rejected), "{rejected:?}");

    let mut state = game();
    state.last_active_turn_number.insert(P0, 1);
    let source = on_battlefield(&mut state, 1, creature(1, "Elf"), P0);
    state.object_mut(source).unwrap().controlled_since_turn = 1;
    state.abilities.push(ability(source, None, green()));
    let mut engine = Engine::new(state);
    let rejected = assert_action_rejected(&mut engine, "summoning sick elf", act(P0, activate(source)));
    assert!(illegal(&rejected), "{rejected:?}");
}

#[test]
fn activate_with_the_wrong_arguments_is_rejected() {
    let (mut engine, source) = mana_ability(green(), None);
    let with_targets = PlayerActionKind::ActivateAbility {
        source,
        ability_index: 0,
        targets: vec![Target::Player(P1)],
        chosen_colors: Vec::new(),
        x_value: None,
    };
    assert_action_rejected(&mut engine, "targets on a mana ability", act(P0, with_targets));
    let with_colors = PlayerActionKind::ActivateAbility {
        source,
        ability_index: 0,
        targets: Vec::new(),
        chosen_colors: vec![Color::Red],
        x_value: None,
    };
    assert_action_rejected(&mut engine, "colors with no choice", act(P0, with_colors));
    let with_x = PlayerActionKind::ActivateAbility {
        source,
        ability_index: 0,
        targets: Vec::new(),
        chosen_colors: Vec::new(),
        x_value: Some(1),
    };
    assert_action_rejected(&mut engine, "X with no X cost", act(P0, with_x));
}

#[test]
fn activate_a_color_choice_with_the_wrong_colors_is_rejected() {
    let choice = AbilityEffect::AddManaChoice { count: 1, choices: ColorSet::GREEN };
    let (mut engine, source) = mana_ability(choice, None);
    for colors in [vec![], vec![Color::Red], vec![Color::Green, Color::Green]] {
        let action = act(
            P0,
            PlayerActionKind::ActivateAbility {
                source,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: colors.clone(),
                x_value: None,
            },
        );
        assert_action_rejected(&mut engine, &format!("colors {colors:?}"), action);
    }
}

#[test]
fn activate_when_not_the_controller_or_not_on_the_battlefield_is_rejected() {
    let (mut engine, source) = mana_ability(green(), None);
    engine.state_mut().object_mut(source).unwrap().controller = P1;
    assert_action_rejected(&mut engine, "p1's land", act(P0, activate(source)));

    let (mut engine, source) = mana_ability(green(), None);
    engine.state_mut().object_mut(source).unwrap().zone = ZoneKind::Graveyard;
    assert_action_rejected(&mut engine, "land in graveyard", act(P0, activate(source)));

    let mut engine = Engine::new(game());
    let rejected = assert_action_rejected(&mut engine, "unknown source", act(P0, activate(ObjectId(777))));
    assert!(matches!(rejected, ActionError::Rejected(Rejected::InvalidTarget)), "{rejected:?}");
}

#[test]
fn activate_a_tap_cost_ability_with_a_tapped_or_sick_source_is_rejected() {
    let (mut engine, source) = mana_ability(AbilityEffect::GainLife(1), Some(tap_cost()));
    engine.state_mut().object_mut(source).unwrap().tapped = true;
    let rejected = assert_action_rejected(&mut engine, "tapped source", act(P0, activate(source)));
    assert!(illegal(&rejected), "{rejected:?}");

    let mut state = game();
    let source = on_battlefield(&mut state, 1, creature(1, "Elf"), P0);
    state.object_mut(source).unwrap().controlled_since_turn = 1;
    state.abilities.push(ability(source, Some(tap_cost()), AbilityEffect::GainLife(1)));
    let mut engine = Engine::new(state);
    let rejected = assert_action_rejected(&mut engine, "sick source", act(P0, activate(source)));
    assert!(illegal(&rejected), "{rejected:?}");
}

#[test]
fn activate_a_targeted_ability_with_a_bad_target_is_rejected() {
    let mut state = game();
    let source = on_battlefield(&mut state, 1, def(1, "Pinger", CardTypeSet::ARTIFACT), P0);
    let mut targeted = ability(source, None, AbilityEffect::Damage(1));
    targeted.target = Some(any_target());
    state.abilities.push(targeted);
    let mut engine = Engine::new(state);
    assert_action_rejected(&mut engine, "no target", act(P0, activate(source)));
    let bad = PlayerActionKind::ActivateAbility {
        source,
        ability_index: 0,
        targets: vec![Target::Object(ObjectId(555))],
        chosen_colors: Vec::new(),
        x_value: None,
    };
    assert_action_rejected(&mut engine, "missing target", act(P0, bad));
}

// A missing player state must not leave the land tapped.
#[test]
fn activate_a_mana_ability_without_player_state_changes_nothing() {
    let ghost = PlayerId(5);
    let mut state = game();
    state.priority_player = ghost;
    let source = on_battlefield(&mut state, 1, def(1, "Forest", CardTypeSet::LAND), ghost);
    state.abilities.push(ability(source, None, green()));
    let mut engine = Engine::new(state);
    let rejected = assert_action_rejected(&mut engine, "ghost activates", act(ghost, activate(source)));
    assert!(illegal(&rejected), "{rejected:?}");
    assert!(!engine.state().object(source).unwrap().tapped);
}

#[test]
fn a_rejected_action_does_not_use_up_a_command_id() {
    let (mut engine, land) = land_engine();
    engine.state_mut().lands_played_this_turn.insert(P0, 1);
    assert_action_rejected(&mut engine, "second land", act(P0, PlayerActionKind::PlayLand { card: land }));
    assert_eq!(engine.snapshot().next_command_id, 0);
    engine.submit_action(act(P0, PlayerActionKind::PassPriority)).unwrap();
    drain(&mut engine);
    assert_eq!(engine.snapshot().next_command_id, 1, "the accepted pass takes id 0, so the next id is 1");
}

fn paying_engine() -> (Engine, ObjectId) {
    let mut state = game();
    let spell = in_hand(
        &mut state,
        1,
        CardDefinition {
            mana_cost: ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Green))] },
            ..def(1, "Growth", CardTypeSet::INSTANT)
        },
    );
    let forest = on_battlefield(&mut state, 2, def(2, "Forest", CardTypeSet::LAND), P0);
    state.object_mut(forest).unwrap().tapped = true;
    state.abilities.push(ability(forest, None, green()));
    state.player_mut(P0).unwrap().mana_pool.green = 1;
    let mut engine = Engine::new(state);
    engine.submit_action(act(P0, cast(spell))).unwrap();
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "{status:?}");
    (engine, forest)
}

fn pay_green() -> PaymentPlan {
    PaymentPlan { payments: vec![CostPayment::Mana { units: vec![ManaUnit::Colored(Color::Green)], phyrexian_life: 0 }] }
}

#[test]
fn a_rejected_nested_mana_ability_leaves_the_payment_intact() {
    let (mut engine, forest) = paying_engine();
    let rejected = assert_action_rejected(&mut engine, "tapped forest mid-payment", act(P0, activate(forest)));
    assert!(matches!(rejected, ActionError::Rejected(Rejected::IllegalAction(_))), "{rejected:?}");
    assert!(engine.pending_payment_cost().is_some(), "still waiting on the payment");

    engine.submit_payment(pay_green()).expect("the suspended payment resumes and accepts a plan");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == P0), "{status:?}");
    assert_eq!(engine.state().stack.len(), 1);
}

#[test]
fn another_action_during_a_payment_is_rejected_without_touching_it() {
    let (mut engine, forest) = paying_engine();
    let rejected = assert_action_rejected(&mut engine, "pass mid-payment", act(P0, PlayerActionKind::PassPriority));
    assert!(matches!(rejected, ActionError::IllegalAction(_)));
    // A mana ability for someone who is not the payer.
    let rejected = assert_action_rejected(&mut engine, "p1 mid-payment", act(P1, activate(forest)));
    assert!(matches!(rejected, ActionError::IllegalAction(_)));
    // A mana ability that does not exist is not a mana ability.
    let missing = PlayerActionKind::ActivateAbility {
        source: forest,
        ability_index: 9,
        targets: Vec::new(),
        chosen_colors: Vec::new(),
        x_value: None,
    };
    let rejected = assert_action_rejected(&mut engine, "missing ability mid-payment", act(P0, missing));
    assert!(matches!(rejected, ActionError::IllegalAction(_)));
    assert!(engine.pending_payment_cost().is_some());
}

fn combat_state(step: Step, combat: Option<CombatState>) -> GameState {
    GameState { phase: Phase::Combat, step, combat, ..game() }
}

fn combat(attackers: Vec<(ObjectId, Target)>, blocks: Vec<(ObjectId, ObjectId)>) -> CombatState {
    CombatState {
        attackers,
        blocks,
        declared_defenders: Vec::new(),
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false,
        pending_damage_splits: HashMap::new(),
    }
}

fn attack_step_engine() -> (Engine, ObjectId) {
    let mut state = combat_state(Step::DeclareAttackers, None);
    let bear = on_battlefield(&mut state, 1, creature(1, "Bear"), P0);
    (Engine::new(state), bear)
}

fn declare(attackers: Vec<(ObjectId, Target)>) -> PlayerAction {
    act(P0, PlayerActionKind::DeclareAttackers { attackers })
}

#[test]
fn declare_attackers_that_cannot_attack_is_rejected() {
    let (mut engine, bear) = attack_step_engine();
    engine.state_mut().object_mut(bear).unwrap().tapped = true;
    let rejected = assert_action_rejected(&mut engine, "tapped attacker", declare(vec![(bear, Target::Player(P1))]));
    assert!(illegal(&rejected), "{rejected:?}");

    let (mut engine, bear) = attack_step_engine();
    engine.state_mut().object_mut(bear).unwrap().controlled_since_turn = 1;
    assert_action_rejected(&mut engine, "summoning sick attacker", declare(vec![(bear, Target::Player(P1))]));

    let (mut engine, bear) = attack_step_engine();
    engine.state_mut().object_mut(bear).unwrap().zone = ZoneKind::Graveyard;
    assert_action_rejected(&mut engine, "attacker off the battlefield", declare(vec![(bear, Target::Player(P1))]));

    let (mut engine, bear) = attack_step_engine();
    engine.state_mut().object_mut(bear).unwrap().controller = P1;
    assert_action_rejected(&mut engine, "someone else's attacker", declare(vec![(bear, Target::Player(P1))]));

    let mut state = combat_state(Step::DeclareAttackers, None);
    let wall = on_battlefield(
        &mut state,
        1,
        CardDefinition { keywords: KeywordSet::DEFENDER, ..creature(1, "Wall") },
        P0,
    );
    let mut engine = Engine::new(state);
    assert_action_rejected(&mut engine, "defender attacking", declare(vec![(wall, Target::Player(P1))]));

    let mut engine = Engine::new(combat_state(Step::DeclareAttackers, None));
    let rejected = assert_action_rejected(&mut engine, "unknown attacker", declare(vec![(ObjectId(777), Target::Player(P1))]));
    assert!(matches!(rejected, ActionError::Rejected(Rejected::InvalidTarget)), "{rejected:?}");
}

#[test]
fn declare_attackers_at_an_illegal_target_is_rejected() {
    let (mut engine, bear) = attack_step_engine();
    assert_action_rejected(&mut engine, "attack yourself", declare(vec![(bear, Target::Player(P0))]));
    assert_action_rejected(&mut engine, "attack nobody", declare(vec![(bear, Target::Player(PlayerId(9)))]));
    assert_action_rejected(&mut engine, "attack a creature", declare(vec![(bear, Target::Object(bear))]));
}

#[test]
fn declare_attackers_with_one_illegal_attacker_taps_none_of_them() {
    let mut state = combat_state(Step::DeclareAttackers, None);
    let first = on_battlefield(&mut state, 1, creature(1, "First"), P0);
    let second = on_battlefield(&mut state, 2, creature(2, "Second"), P0);
    state.object_mut(second).unwrap().tapped = true;
    let mut engine = Engine::new(state);
    assert_action_rejected(
        &mut engine,
        "one good and one tapped",
        declare(vec![(first, Target::Player(P1)), (second, Target::Player(P1))]),
    );
    assert!(!engine.state().object(first).unwrap().tapped);
}

// The same attacker listed twice must be rejected before it is tapped twice.
#[test]
fn declare_the_same_attacker_twice_is_rejected() {
    let (mut engine, bear) = attack_step_engine();
    let rejected =
        assert_action_rejected(&mut engine, "bear twice", declare(vec![(bear, Target::Player(P1)), (bear, Target::Player(P1))]));
    assert!(illegal(&rejected), "{rejected:?}");
}

#[test]
fn declare_attackers_at_the_wrong_time_or_by_the_wrong_player_is_rejected() {
    let (mut engine, bear) = attack_step_engine();
    let rejected = assert_action_rejected(
        &mut engine,
        "p1 declares",
        act(P1, PlayerActionKind::DeclareAttackers { attackers: vec![(bear, Target::Player(P1))] }),
    );
    assert!(illegal(&rejected), "{rejected:?}");

    let mut state = combat_state(Step::BeginningOfCombat, None);
    let bear = on_battlefield(&mut state, 1, creature(1, "Bear"), P0);
    let mut engine = Engine::new(state);
    assert_action_rejected(&mut engine, "wrong step", declare(vec![(bear, Target::Player(P1))]));

    let mut state = combat_state(Step::DeclareAttackers, Some(combat(Vec::new(), Vec::new())));
    let bear = on_battlefield(&mut state, 1, creature(1, "Bear"), P0);
    let mut engine = Engine::new(state);
    assert_action_rejected(&mut engine, "declared twice", declare(vec![(bear, Target::Player(P1))]));
}

#[test]
fn declare_bands_that_are_illegal_is_rejected() {
    let mut state = combat_state(Step::DeclareAttackers, None);
    let a = on_battlefield(&mut state, 1, creature(1, "A"), P0);
    let b = on_battlefield(&mut state, 2, creature(2, "B"), P0);
    state.combat = Some(combat(vec![(a, Target::Player(P1)), (b, Target::Player(P1))], Vec::new()));
    let mut engine = Engine::new(state);
    let bands = |bands: Vec<Vec<ObjectId>>| act(P0, PlayerActionKind::DeclareBands { bands });
    // Neither creature has banding.
    assert_action_rejected(&mut engine, "band with no banding", bands(vec![vec![a, b]]));
    assert_action_rejected(&mut engine, "band of a stranger", bands(vec![vec![ObjectId(777)]]));
    assert_action_rejected(&mut engine, "creature in two bands", bands(vec![vec![a], vec![a]]));
    assert_action_rejected(
        &mut engine,
        "wrong player",
        act(P1, PlayerActionKind::DeclareBands { bands: vec![vec![a]] }),
    );

    let mut engine = Engine::new(combat_state(Step::DeclareAttackers, None));
    assert_action_rejected(&mut engine, "bands before attackers", bands(vec![vec![ObjectId(1)]]));
}

fn block_step_engine(attacker_keywords: KeywordSet) -> (Engine, ObjectId, ObjectId) {
    let mut state = combat_state(Step::DeclareBlockers, None);
    let attacker =
        on_battlefield(&mut state, 1, CardDefinition { keywords: attacker_keywords, ..creature(1, "Attacker") }, P0);
    let blocker = on_battlefield(&mut state, 2, creature(2, "Blocker"), P1);
    state.combat = Some(combat(vec![(attacker, Target::Player(P1))], Vec::new()));
    (Engine::new(state), attacker, blocker)
}

fn block(player: PlayerId, blocks: Vec<(ObjectId, ObjectId)>) -> PlayerAction {
    act(player, PlayerActionKind::DeclareBlockers { blocks })
}

#[test]
fn declare_an_illegal_block_is_rejected() {
    let (mut engine, attacker, blocker) = block_step_engine(KeywordSet::empty());
    engine.state_mut().object_mut(blocker).unwrap().tapped = true;
    assert_action_rejected(&mut engine, "tapped blocker", block(P1, vec![(blocker, attacker)]));

    let (mut engine, attacker, blocker) = block_step_engine(KeywordSet::FLYING);
    assert_action_rejected(&mut engine, "ground blocker on a flier", block(P1, vec![(blocker, attacker)]));

    let (mut engine, attacker, blocker) = block_step_engine(KeywordSet::MENACE);
    assert_action_rejected(&mut engine, "one blocker on menace", block(P1, vec![(blocker, attacker)]));

    let (mut engine, attacker, blocker) = block_step_engine(KeywordSet::empty());
    engine.state_mut().object_mut(blocker).unwrap().controller = P0;
    assert_action_rejected(&mut engine, "blocking with the attacker's creature", block(P1, vec![(blocker, attacker)]));

    let (mut engine, attacker, blocker) = block_step_engine(KeywordSet::empty());
    engine.state_mut().object_mut(blocker).unwrap().zone = ZoneKind::Graveyard;
    assert_action_rejected(&mut engine, "blocker off the battlefield", block(P1, vec![(blocker, attacker)]));

    let (mut engine, _attacker, blocker) = block_step_engine(KeywordSet::empty());
    assert_action_rejected(&mut engine, "blocking a non-attacker", block(P1, vec![(blocker, blocker)]));
}

#[test]
fn declare_blockers_with_one_illegal_block_records_none_of_them() {
    let (mut engine, attacker, blocker) = block_step_engine(KeywordSet::empty());
    let extra = ObjectId(3);
    let object = card_object(extra, creature(3, "Extra"), P1, ZoneKind::Battlefield);
    engine.state_mut().objects.insert(extra, GameObject { tapped: true, ..object });
    engine.state_mut().zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(extra);
    assert_action_rejected(&mut engine, "one good block and one tapped", block(P1, vec![(blocker, attacker), (extra, attacker)]));
    assert!(engine.state().combat.as_ref().unwrap().blocks.is_empty());
}

#[test]
fn declare_blockers_twice_or_by_a_non_defender_is_rejected() {
    let (mut engine, attacker, blocker) = block_step_engine(KeywordSet::empty());
    assert_action_rejected(&mut engine, "attacker declares blockers", block(P0, vec![(blocker, attacker)]));
    assert_action_rejected(
        &mut engine,
        "same blocker twice",
        block(P1, vec![(blocker, attacker), (blocker, attacker)]),
    );
    engine.state_mut().combat.as_mut().unwrap().declared_defenders.push(P1);
    assert_action_rejected(&mut engine, "already declared", block(P1, vec![(blocker, attacker)]));

    let mut engine = Engine::new(combat_state(Step::DeclareBlockers, None));
    assert_action_rejected(&mut engine, "no combat", block(P1, Vec::new()));
    let mut engine = Engine::new(combat_state(Step::DeclareAttackers, Some(combat(Vec::new(), Vec::new()))));
    assert_action_rejected(&mut engine, "wrong step", block(P1, Vec::new()));
}

fn damage_engine() -> (Engine, ObjectId, ObjectId, ObjectId) {
    let mut state = combat_state(Step::CombatDamage, None);
    let attacker = on_battlefield(
        &mut state,
        1,
        CardDefinition { power: Some(PTValue::Fixed(3)), toughness: Some(PTValue::Fixed(3)), ..creature(1, "Ogre") },
        P0,
    );
    let first = on_battlefield(&mut state, 2, creature(2, "First"), P1);
    let second = on_battlefield(&mut state, 3, creature(3, "Second"), P1);
    state.combat = Some(CombatState {
        declared_defenders: vec![P1],
        ..combat(vec![(attacker, Target::Player(P1))], vec![(first, attacker), (second, attacker)])
    });
    (Engine::new(state), attacker, first, second)
}

fn assign(player: PlayerId, assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>) -> PlayerAction {
    act(player, PlayerActionKind::AssignCombatDamage { assignments })
}

#[test]
fn assign_combat_damage_that_does_not_add_up_is_rejected() {
    let (mut engine, attacker, first, second) = damage_engine();
    assert_action_rejected(&mut engine, "too little", assign(P0, vec![(attacker, vec![(first, 1), (second, 1)])]));
    assert_action_rejected(&mut engine, "too much", assign(P0, vec![(attacker, vec![(first, 2), (second, 2)])]));
    assert_action_rejected(&mut engine, "one blocker missing", assign(P0, vec![(attacker, vec![(first, 3)])]));
    assert_action_rejected(&mut engine, "same blocker twice", assign(P0, vec![(attacker, vec![(first, 1), (first, 2)])]));
    assert_action_rejected(&mut engine, "a stranger as blocker", assign(P0, vec![(attacker, vec![(first, 1), (ObjectId(777), 2)])]));
    assert_action_rejected(&mut engine, "nothing assigned", assign(P0, Vec::new()));
    assert_action_rejected(&mut engine, "the wrong creature", assign(P0, vec![(first, vec![(attacker, 2)])]));
}

#[test]
fn assign_combat_damage_by_the_wrong_player_is_rejected() {
    let (mut engine, attacker, first, second) = damage_engine();
    assert_action_rejected(&mut engine, "defender splits", assign(P1, vec![(attacker, vec![(first, 1), (second, 2)])]));
}

#[test]
fn a_valid_split_is_accepted_after_the_rejections() {
    let (mut engine, attacker, first, second) = damage_engine();
    assert_action_rejected(&mut engine, "too little", assign(P0, vec![(attacker, vec![(first, 1), (second, 1)])]));
    engine.submit_action(assign(P0, vec![(attacker, vec![(first, 1), (second, 2)])])).unwrap();
    let status = drain(&mut engine);
    assert!(!matches!(status, StepStatus::Error(_)), "{status:?}");
    assert!(engine.state().combat.as_ref().unwrap().damage_assigned_this_step);
}
