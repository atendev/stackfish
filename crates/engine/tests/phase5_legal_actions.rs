//! Phase 5 vertical slice B2: the entries a Decision lists (`Engine::
//! legal_actions`, `legal_attackers`, `legal_blockers`,
//! `pending_damage_splits`) and the facade checks that use them. Fixtures are
//! built directly, the way `phase5_combat_declarations.rs` does.

use std::collections::HashMap;

use stackfish_engine::decision::{
    Answer, AttackerEntry, BlockerEntry, Entry, EntryTargets, RejectionCode, Reply, ReplyShape,
};
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state, zones};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn definition(name: &str, types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        ..minimal_card_definition(1, name, name, ManaCost { symbols: Vec::new() }, types)
    }
}

fn creature_target() -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
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
    })
}

/// Main phase, `P0` active and holding priority, empty stack.
fn main_state() -> GameState {
    GameState {
        last_active_turn_number: HashMap::from([(P0, 1), (P1, 0)]),
        ..two_player_state(zones(
            P0,
            P1,
            &[ZoneKind::Hand, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Command],
        ))
    }
}

fn combat_state(step: Step) -> GameState {
    GameState { phase: Phase::Combat, step, ..main_state() }
}

fn put(state: &mut GameState, owner: Option<PlayerId>, object: GameObject) {
    let (zone, id) = (object.zone, object.object_id);
    state.zone_mut(zone, owner).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn in_hand(state: &mut GameState, id: u64, owner: PlayerId, definition: CardDefinition) {
    put(state, Some(owner), card_object(ObjectId(id), definition, owner, ZoneKind::Hand));
}

fn on_battlefield(state: &mut GameState, id: u64, owner: PlayerId, definition: CardDefinition) {
    put(state, None, card_object(ObjectId(id), definition, owner, ZoneKind::Battlefield));
}

fn cast_entry(entries: &[Entry], card: u64) -> Option<&stackfish_engine::decision::CastEntry> {
    entries.iter().find_map(|e| match e {
        Entry::Cast(c) if c.card == ObjectId(card) => Some(c),
        _ => None,
    })
}

fn answer(engine: &mut Engine, player: PlayerId, reply: Reply) -> Result<(), stackfish_engine::decision::Rejection> {
    let decision = engine.open_decision().expect("a Decision is open");
    engine.answer(player, Answer { decision: decision.id, reply })
}

fn rejection_code(result: Result<(), stackfish_engine::decision::Rejection>) -> RejectionCode {
    result.expect_err("the Answer should be rejected").code
}

// ---------------------------------------------------------------------------
// Priority entries
// ---------------------------------------------------------------------------

#[test]
fn pass_is_always_the_last_entry() {
    let engine = Engine::new(main_state());
    assert_eq!(engine.legal_actions(), vec![Entry::Pass]);
}

#[test]
fn a_land_is_offered_once_per_turn_in_a_main_phase_with_an_empty_stack() {
    let mut state = main_state();
    in_hand(&mut state, 1, P0, definition("Forest", CardTypeSet::LAND));
    let engine = Engine::new(state);
    assert_eq!(engine.legal_actions(), vec![Entry::PlayLand { card: ObjectId(1) }, Entry::Pass]);

    let mut played = main_state();
    in_hand(&mut played, 1, P0, definition("Forest", CardTypeSet::LAND));
    played.lands_played_this_turn.insert(P0, 1);
    assert_eq!(Engine::new(played).legal_actions(), vec![Entry::Pass]);

    let mut combat = combat_state(Step::DeclareAttackers);
    in_hand(&mut combat, 1, P0, definition("Forest", CardTypeSet::LAND));
    assert_eq!(Engine::new(combat).legal_actions(), vec![Entry::Pass]);
}

#[test]
fn sorcery_speed_cards_need_the_main_phase_but_instants_and_flash_do_not() {
    let flash = CardDefinition { keywords: KeywordSet::FLASH, ..definition("Ambusher", CardTypeSet::CREATURE) };
    let mut state = combat_state(Step::DeclareAttackers);
    in_hand(&mut state, 1, P0, definition("Bear", CardTypeSet::CREATURE));
    in_hand(&mut state, 2, P0, definition("Bolt", CardTypeSet::INSTANT));
    in_hand(&mut state, 3, P0, flash);
    let entries = Engine::new(state).legal_actions();
    assert!(cast_entry(&entries, 1).is_none(), "a creature cannot be cast during combat");
    assert!(cast_entry(&entries, 2).is_some());
    assert!(cast_entry(&entries, 3).is_some());

    let mut main = main_state();
    in_hand(&mut main, 1, P0, definition("Bear", CardTypeSet::CREATURE));
    assert!(cast_entry(&Engine::new(main).legal_actions(), 1).is_some());
}

#[test]
fn only_the_priority_players_cards_are_offered() {
    let mut state = main_state();
    in_hand(&mut state, 1, P0, definition("Bolt", CardTypeSet::INSTANT));
    in_hand(&mut state, 2, P1, definition("Counter", CardTypeSet::INSTANT));
    state.priority_player = P1;
    let entries = Engine::new(state).legal_actions();
    assert!(cast_entry(&entries, 1).is_none());
    assert!(cast_entry(&entries, 2).is_some());
}

#[test]
fn target_slots_list_candidates_and_a_spell_without_any_is_not_offered() {
    let bolt = CardDefinition { target_requirement: Some(creature_target()), ..definition("Doom Blade", CardTypeSet::INSTANT) };

    let mut empty = main_state();
    in_hand(&mut empty, 1, P0, bolt.clone());
    assert!(cast_entry(&Engine::new(empty).legal_actions(), 1).is_none(), "no creature to target");

    let mut state = main_state();
    in_hand(&mut state, 1, P0, bolt);
    on_battlefield(&mut state, 9, P1, definition("Bear", CardTypeSet::CREATURE));
    on_battlefield(&mut state, 5, P0, definition("Elk", CardTypeSet::CREATURE));
    let entries = Engine::new(state).legal_actions();
    let entry = cast_entry(&entries, 1).expect("a creature can be targeted");
    let EntryTargets::Fixed(slots) = &entry.targets else { panic!("not modal") };
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].candidates, vec![Target::Object(ObjectId(5)), Target::Object(ObjectId(9))]);
    assert_eq!((slots[0].min, slots[0].max), (1, Some(1)));
}

#[test]
fn x_buyback_and_alternative_costs_are_flagged() {
    let x_spell = CardDefinition {
        mana_cost: ManaCost { symbols: vec![ManaSymbol::X] },
        buyback_cost: Some(ManaCost { symbols: Vec::new() }),
        ..definition("Blaze", CardTypeSet::SORCERY)
    };
    let flashback = CardDefinition {
        alternative_costs: vec![AlternativeCost::Replacement {
            description: "Flashback".to_string(),
            cost: CostSpec::mana_only(ManaCost { symbols: Vec::new() }),
            from_zone: ZoneKind::Graveyard,
            exile_on_leaving_stack: true,
            escape: false,
        }],
        ..definition("Ancient Grudge", CardTypeSet::SORCERY)
    };
    let mut state = main_state();
    in_hand(&mut state, 1, P0, x_spell);
    put(&mut state, Some(P0), card_object(ObjectId(2), flashback, P0, ZoneKind::Graveyard));
    let entries = Engine::new(state).legal_actions();

    let blaze = cast_entry(&entries, 1).unwrap();
    assert!(blaze.x && blaze.buyback && blaze.normal && blaze.alternative_costs.is_empty());
    let grudge = cast_entry(&entries, 2).expect("flashback grants the graveyard cast");
    assert!(!grudge.normal, "a graveyard card cannot be cast for its normal cost");
    assert_eq!(grudge.alternative_costs, vec![0]);
}

fn modal_card() -> CardDefinition {
    let mode = |target: Option<TargetSpec>| SpellMode {
        target_requirement: target,
        effect: None,
        additional_cost: None,
        pawprint_weight: 1,
    };
    CardDefinition {
        modes: vec![mode(None), mode(Some(creature_target()))],
        mode_choice: ModeChoice::Exactly(1),
        ..definition("Charm", CardTypeSet::INSTANT)
    }
}

#[test]
fn a_modal_spell_lists_the_range_and_slots_per_mode() {
    let mut state = main_state();
    in_hand(&mut state, 1, P0, modal_card());
    let entries = Engine::new(state).legal_actions();
    let entry = cast_entry(&entries, 1).expect("the untargeted mode is always usable");
    let range = entry.modes.as_ref().expect("modal");
    assert_eq!((range.mode_count, range.min, range.max, range.repeatable), (2, 1, 1, false));
    let EntryTargets::PerMode(per_mode) = &entry.targets else { panic!("modal") };
    assert!(per_mode[0].is_empty());
    assert_eq!(per_mode[1].len(), 1);
    assert!(per_mode[1][0].candidates.is_empty(), "no creature in play yet");
}

fn tap_ability(source: u64, effect: AbilityEffect, cost: Option<CostSpec>) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(source),
        source: ObjectId(source),
        kind: AbilityKind::Activated,
        condition: None,
        cost,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(effect),
        intervening_if: None,
    }
}

#[test]
fn a_tap_ability_is_offered_only_while_the_source_is_untapped_and_not_summoning_sick() {
    let tap = CostSpec { mana: None, additional: vec![AdditionalCost::TapSelf], alternative: Vec::new() };
    let mut state = main_state();
    on_battlefield(&mut state, 1, P0, definition("Elf", CardTypeSet::CREATURE));
    state.abilities.push(tap_ability(1, AbilityEffect::AddMana(vec![ManaUnit::Colored(Color::Green)]), Some(tap.clone())));

    let entries = Engine::new(state).legal_actions();
    let Entry::Activate(activate) = &entries[0] else { panic!("expected an Activate entry, got {entries:?}") };
    assert_eq!((activate.source, activate.ability_index), (ObjectId(1), 0));
    assert!(activate.mana_ability);
    assert!(activate.color_choice.is_none());

    let mut sick = main_state();
    on_battlefield(&mut sick, 1, P0, definition("Elf", CardTypeSet::CREATURE));
    sick.objects.get_mut(&ObjectId(1)).unwrap().controlled_since_turn = 1;
    sick.abilities.push(tap_ability(1, AbilityEffect::AddMana(vec![ManaUnit::Colored(Color::Green)]), Some(tap)));
    assert_eq!(Engine::new(sick).legal_actions(), vec![Entry::Pass]);

}

#[test]
fn an_any_color_mana_ability_lists_its_colors() {
    let mut state = main_state();
    on_battlefield(&mut state, 1, P0, definition("Treasure", CardTypeSet::ARTIFACT));
    state.abilities.push(tap_ability(
        1,
        AbilityEffect::AddManaChoice { count: 1, choices: ColorSet::RED | ColorSet::GREEN },
        None,
    ));
    let entries = Engine::new(state).legal_actions();
    let Entry::Activate(activate) = &entries[0] else { panic!("expected an Activate entry") };
    let choice = activate.color_choice.as_ref().expect("a color choice");
    assert_eq!((choice.count, choice.colors.clone()), (1, vec![Color::Red, Color::Green]));
}

#[test]
fn an_ability_index_counts_every_ability_on_its_source() {
    let mut state = main_state();
    on_battlefield(&mut state, 1, P0, definition("Elf", CardTypeSet::CREATURE));
    let mut trigger = tap_ability(1, AbilityEffect::GainLife(1), None);
    trigger.kind = AbilityKind::Triggered;
    state.abilities.push(trigger);
    state.abilities.push(tap_ability(1, AbilityEffect::GainLife(1), None));
    let entries = Engine::new(state).legal_actions();
    let Entry::Activate(activate) = &entries[0] else { panic!("expected an Activate entry") };
    assert_eq!(activate.ability_index, 1, "the triggered ability takes index 0");
}

// ---------------------------------------------------------------------------
// Facade checks on a priority Answer
// ---------------------------------------------------------------------------

#[test]
fn a_priority_answer_that_no_entry_offers_is_rejected_and_changes_nothing() {
    let mut state = main_state();
    in_hand(&mut state, 1, P0, definition("Bear", CardTypeSet::CREATURE));
    in_hand(&mut state, 2, P0, definition("Forest", CardTypeSet::LAND));
    let mut engine = Engine::new(state);
    let cast = |card: u64, x_value: Option<u32>, buyback: bool| {
        Reply::Action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell {
                card: ObjectId(card),
                modes: Vec::new(),
                targets: Vec::new(),
                x_value,
                alternative_cost: None,
                pay_buyback: buyback,
            },
        })
    };
    let first = engine.open_decision().unwrap().id;

    // The land is not a spell, an unknown card is not castable, and the
    // bear has neither an {X} nor a buyback cost.
    assert_eq!(rejection_code(answer(&mut engine, P0, cast(2, None, false))), RejectionCode::InvalidChoice);
    assert_eq!(rejection_code(answer(&mut engine, P0, cast(99, None, false))), RejectionCode::InvalidChoice);
    assert_eq!(rejection_code(answer(&mut engine, P0, cast(1, Some(2), false))), RejectionCode::InvalidChoice);
    assert_eq!(rejection_code(answer(&mut engine, P0, cast(1, None, true))), RejectionCode::InvalidChoice);
    assert_eq!(engine.open_decision().unwrap().id, first, "a rejection keeps the Decision open");

    answer(&mut engine, P0, cast(1, None, false)).expect("the bear is castable");
}

#[test]
fn a_card_in_the_wrong_zone_or_at_the_wrong_time_cannot_be_played() {
    let mut state = combat_state(Step::DeclareAttackers);
    in_hand(&mut state, 1, P0, definition("Forest", CardTypeSet::LAND));
    let mut engine = Engine::new(state);
    let play = Reply::Action(PlayerAction { player: P0, kind: PlayerActionKind::PlayLand { card: ObjectId(1) } });
    assert_eq!(rejection_code(answer(&mut engine, P0, play)), RejectionCode::InvalidChoice);
}

#[test]
fn every_listed_entry_is_accepted_by_the_engine() {
    let build = || {
        let mut state = main_state();
        in_hand(&mut state, 1, P0, definition("Forest", CardTypeSet::LAND));
        in_hand(&mut state, 2, P0, definition("Bear", CardTypeSet::CREATURE));
        in_hand(&mut state, 3, P0, CardDefinition { target_requirement: Some(creature_target()), ..definition("Doom Blade", CardTypeSet::INSTANT) });
        on_battlefield(&mut state, 4, P1, definition("Elk", CardTypeSet::CREATURE));
        on_battlefield(&mut state, 5, P0, definition("Mox", CardTypeSet::ARTIFACT));
        state.abilities.push(tap_ability(5, AbilityEffect::AddMana(vec![ManaUnit::Colored(Color::Red)]), None));
        state
    };
    let entries = Engine::new(build()).legal_actions();
    assert_eq!(entries.len(), 5, "land, two casts, one ability and pass: {entries:?}");
    for index in 0..entries.len() {
        let mut engine = Engine::new(build());
        let entry = engine.legal_actions().remove(index);
        let kind = match entry {
            Entry::Pass => PlayerActionKind::PassPriority,
            Entry::PlayLand { card } => PlayerActionKind::PlayLand { card },
            Entry::Cast(cast) => PlayerActionKind::CastSpell {
                card: cast.card,
                modes: Vec::new(),
                targets: match &cast.targets {
                    EntryTargets::Fixed(slots) => slots.iter().flat_map(|s| s.candidates.first().copied()).collect(),
                    EntryTargets::PerMode(_) => unreachable!("no modal card here"),
                },
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
            Entry::Activate(activate) => PlayerActionKind::ActivateAbility {
                source: activate.source,
                ability_index: activate.ability_index,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        };
        answer(&mut engine, P0, Reply::Action(PlayerAction { player: P0, kind })).expect("the facade accepts it");
        let status = drain(&mut engine);
        assert!(!matches!(status, StepStatus::Error(_)), "entry {index} failed in the engine: {status:?}");
    }
}

// ---------------------------------------------------------------------------
// Combat entries
// ---------------------------------------------------------------------------

#[test]
fn attackers_are_untapped_creatures_without_summoning_sickness_or_defender() {
    let mut state = combat_state(Step::DeclareAttackers);
    on_battlefield(&mut state, 1, P0, definition("Bear", CardTypeSet::CREATURE));
    on_battlefield(&mut state, 2, P0, definition("Tapped", CardTypeSet::CREATURE));
    state.objects.get_mut(&ObjectId(2)).unwrap().tapped = true;
    on_battlefield(&mut state, 3, P0, definition("Fresh", CardTypeSet::CREATURE));
    state.objects.get_mut(&ObjectId(3)).unwrap().controlled_since_turn = 1;
    on_battlefield(
        &mut state,
        4,
        P0,
        CardDefinition { keywords: KeywordSet::DEFENDER, ..definition("Wall", CardTypeSet::CREATURE) },
    );
    on_battlefield(&mut state, 5, P0, definition("Rock", CardTypeSet::ARTIFACT));
    on_battlefield(&mut state, 6, P1, definition("Their Bear", CardTypeSet::CREATURE));
    let engine = Engine::new(state);
    assert_eq!(
        engine.legal_attackers(),
        vec![AttackerEntry { attacker: ObjectId(1), defenders: vec![Target::Player(P1)], required: false }]
    );
}

#[test]
fn a_creature_that_is_not_on_the_battlefield_is_never_an_attacker() {
    let mut state = combat_state(Step::DeclareAttackers);
    on_battlefield(&mut state, 1, P0, definition("Bear", CardTypeSet::CREATURE));
    in_hand(&mut state, 2, P0, definition("In Hand", CardTypeSet::CREATURE));
    put(&mut state, Some(P0), card_object(ObjectId(3), definition("In Graveyard", CardTypeSet::CREATURE), P0, ZoneKind::Graveyard));
    let engine = Engine::new(state);
    assert_eq!(
        engine.legal_attackers(),
        vec![AttackerEntry { attacker: ObjectId(1), defenders: vec![Target::Player(P1)], required: false }],
        "CR 508.1a: only a creature on the battlefield attacks"
    );
}

#[test]
fn a_planeswalker_that_is_not_on_the_battlefield_is_not_a_defender() {
    let mut state = combat_state(Step::DeclareAttackers);
    on_battlefield(&mut state, 1, P0, definition("Bear", CardTypeSet::CREATURE));
    in_hand(&mut state, 2, P1, definition("In Hand", CardTypeSet::PLANESWALKER));
    let engine = Engine::new(state);
    assert_eq!(
        engine.legal_attackers(),
        vec![AttackerEntry { attacker: ObjectId(1), defenders: vec![Target::Player(P1)], required: false }],
        "CR 508.1b: an attacked planeswalker is a permanent"
    );
}

#[test]
fn a_creature_that_is_not_on_the_battlefield_never_blocks() {
    let mut state = combat_state(Step::DeclareBlockers);
    on_battlefield(&mut state, 1, P0, definition("Bear", CardTypeSet::CREATURE));
    in_hand(&mut state, 2, P1, definition("In Hand", CardTypeSet::CREATURE));
    state.combat = Some(CombatState {
        attackers: vec![(ObjectId(1), Target::Player(P1))],
        blocks: Vec::new(),
        declared_defenders: Vec::new(),
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false,
        pending_damage_splits: HashMap::new(),
    });
    let engine = Engine::new(state);
    assert!(engine.legal_blockers(P1).is_empty(), "CR 509.1a: only a creature on the battlefield blocks");
}

#[test]
fn a_flying_attacker_can_only_be_blocked_by_flying_or_reach() {
    let mut state = combat_state(Step::DeclareBlockers);
    on_battlefield(
        &mut state,
        1,
        P0,
        CardDefinition { keywords: KeywordSet::FLYING, ..definition("Drake", CardTypeSet::CREATURE) },
    );
    on_battlefield(&mut state, 2, P0, definition("Bear", CardTypeSet::CREATURE));
    on_battlefield(&mut state, 3, P1, definition("Elk", CardTypeSet::CREATURE));
    on_battlefield(
        &mut state,
        4,
        P1,
        CardDefinition { keywords: KeywordSet::REACH, ..definition("Spider", CardTypeSet::CREATURE) },
    );
    state.combat = Some(CombatState {
        attackers: vec![(ObjectId(1), Target::Player(P1)), (ObjectId(2), Target::Player(P1))],
        blocks: Vec::new(),
        declared_defenders: Vec::new(),
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false,
        pending_damage_splits: HashMap::new(),
    });
    let engine = Engine::new(state);
    assert_eq!(
        engine.legal_blockers(P1),
        vec![
            BlockerEntry { blocker: ObjectId(3), attackers: vec![ObjectId(2)] },
            BlockerEntry { blocker: ObjectId(4), attackers: vec![ObjectId(1), ObjectId(2)] },
        ]
    );
    assert!(engine.legal_blockers(P0).is_empty(), "the attacking player does not block");
}

#[test]
fn the_declare_attackers_decision_comes_before_priority_and_checks_the_reply() {
    let mut state = combat_state(Step::DeclareAttackers);
    on_battlefield(&mut state, 1, P0, definition("Bear", CardTypeSet::CREATURE));
    let mut engine = Engine::new(state);

    let decision = engine.open_decision().unwrap();
    assert_eq!(decision.player, P0);
    assert!(matches!(&decision.shape, ReplyShape::DeclareAttackers { attackers } if attackers.len() == 1));

    // The attacker cannot attack its own controller or attack twice.
    let own = Reply::DeclareAttackers(vec![(ObjectId(1), Target::Player(P0))]);
    assert_eq!(rejection_code(answer(&mut engine, P0, own)), RejectionCode::InvalidChoice);
    let twice = Reply::DeclareAttackers(vec![(ObjectId(1), Target::Player(P1)), (ObjectId(1), Target::Player(P1))]);
    assert_eq!(rejection_code(answer(&mut engine, P0, twice)), RejectionCode::InvalidChoice);
    // A priority action is the wrong kind of reply here.
    let pass = Reply::Action(PlayerAction { player: P0, kind: PlayerActionKind::PassPriority });
    assert_eq!(rejection_code(answer(&mut engine, P0, pass)), RejectionCode::WrongAnswerShape);

    answer(&mut engine, P0, Reply::DeclareAttackers(vec![(ObjectId(1), Target::Player(P1))])).unwrap();
    assert!(!matches!(drain(&mut engine), StepStatus::Error(_)));
    assert_eq!(engine.state().combat.as_ref().unwrap().attackers.len(), 1);
    assert!(matches!(engine.open_decision().unwrap().shape, ReplyShape::Action { .. }));
}

#[test]
fn a_defending_player_declares_blockers_before_priority_and_may_declare_none() {
    let mut state = combat_state(Step::DeclareBlockers);
    on_battlefield(&mut state, 1, P0, definition("Bear", CardTypeSet::CREATURE));
    on_battlefield(&mut state, 3, P1, definition("Elk", CardTypeSet::CREATURE));
    state.combat = Some(CombatState {
        attackers: vec![(ObjectId(1), Target::Player(P1))],
        blocks: Vec::new(),
        declared_defenders: Vec::new(),
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false,
        pending_damage_splits: HashMap::new(),
    });
    let mut engine = Engine::new(state);
    let decision = engine.open_decision().unwrap();
    assert_eq!(decision.player, P1, "the defender answers although P0 holds priority");
    assert!(matches!(&decision.shape, ReplyShape::DeclareBlockers { blockers } if blockers.len() == 1));

    let unlisted = Reply::DeclareBlockers(vec![(ObjectId(1), ObjectId(1))]);
    assert_eq!(rejection_code(answer(&mut engine, P1, unlisted)), RejectionCode::InvalidChoice);
    assert_eq!(rejection_code(answer(&mut engine, P0, Reply::DeclareBlockers(Vec::new()))), RejectionCode::WrongSeat);

    answer(&mut engine, P1, Reply::DeclareBlockers(Vec::new())).unwrap();
    assert!(!matches!(drain(&mut engine), StepStatus::Error(_)));
    assert!(matches!(engine.open_decision().unwrap().shape, ReplyShape::Action { .. }));
}

fn double_blocked_state() -> GameState {
    let mut state = combat_state(Step::CombatDamage);
    let sturdy = |name: &str| CardDefinition {
        toughness: Some(PTValue::Fixed(5)),
        ..definition(name, CardTypeSet::CREATURE)
    };
    on_battlefield(
        &mut state,
        1,
        P0,
        CardDefinition { power: Some(PTValue::Fixed(3)), ..definition("Ogre", CardTypeSet::CREATURE) },
    );
    on_battlefield(&mut state, 2, P1, sturdy("Wall A"));
    on_battlefield(&mut state, 3, P1, sturdy("Wall B"));
    state.combat = Some(CombatState {
        attackers: vec![(ObjectId(1), Target::Player(P1))],
        blocks: vec![(ObjectId(2), ObjectId(1)), (ObjectId(3), ObjectId(1))],
        declared_defenders: vec![P1],
        bands: Vec::new(),
        effect_blocked: Vec::new(),
        damage_assigned_this_step: false,
        pending_damage_splits: HashMap::new(),
    });
    state
}

#[test]
fn a_double_blocked_attacker_asks_its_controller_for_a_split() {
    let mut engine = Engine::new(double_blocked_state());
    let decision = engine.open_decision().unwrap();
    assert_eq!(decision.player, P0);
    let ReplyShape::AssignDamage { splits } = &decision.shape else { panic!("expected AssignDamage") };
    assert_eq!(splits.len(), 1);
    assert_eq!((splits[0].source, splits[0].power), (ObjectId(1), 3));
    assert_eq!(splits[0].recipients, vec![ObjectId(2), ObjectId(3)]);
    assert!(splits[0].spill.is_none());

    // 2 + 2 is more than the Ogre's power, and a split must name both blockers.
    let too_much = Reply::AssignDamage(vec![(ObjectId(1), vec![(ObjectId(2), 2), (ObjectId(3), 2)])]);
    assert_eq!(rejection_code(answer(&mut engine, P0, too_much)), RejectionCode::InvalidChoice);
    let one_blocker = Reply::AssignDamage(vec![(ObjectId(1), vec![(ObjectId(2), 3)])]);
    assert_eq!(rejection_code(answer(&mut engine, P0, one_blocker)), RejectionCode::InvalidChoice);
    assert_eq!(rejection_code(answer(&mut engine, P1, Reply::AssignDamage(Vec::new()))), RejectionCode::WrongSeat);

    let split = Reply::AssignDamage(vec![(ObjectId(1), vec![(ObjectId(2), 2), (ObjectId(3), 1)])]);
    answer(&mut engine, P0, split).unwrap();
    assert!(!matches!(drain(&mut engine), StepStatus::Error(_)));
    assert_eq!(engine.state().object(ObjectId(2)).unwrap().damage_marked, 2);
    assert_eq!(engine.state().object(ObjectId(3)).unwrap().damage_marked, 1);
}

#[test]
fn a_single_blocker_without_trample_needs_no_split_decision() {
    let mut state = double_blocked_state();
    state.combat.as_mut().unwrap().blocks.pop();
    let engine = Engine::new(state);
    assert!(engine.pending_damage_splits().is_none());
}
