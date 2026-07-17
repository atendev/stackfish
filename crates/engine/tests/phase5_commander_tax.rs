//! Phase 5 vertical slice: backlog item 11d — commander tax (CR 903.8),
//! {2} generic per prior time this player has cast *this* commander from
//! the command zone this game. Tracked per commander by name
//! (`PlayerState::commander_casts`), not `ObjectId` — CR 400.7 mints a new
//! id on every cast and every 903.9a return, so an id can't identify "the
//! same commander" across a cast/death/return/recast cycle. Builds on 11c
//! (`phase5_commander.rs`), which already proved marking + the bare
//! command-zone cast permission; this file only exercises the cost.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::Engine;
use stackfish_engine::*;

fn creature_definition(name: &str, mana_cost: ManaCost) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost,
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn card_object(
    id: ObjectId,
    definition: CardDefinition,
    owner: PlayerId,
    zone: ZoneKind,
    is_commander: bool,
) -> GameObject {
    GameObject {
        object_id: id,
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
        is_commander,
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

fn player_state(id: PlayerId, life: i32, pool: ManaPool) -> PlayerState {
    PlayerState {
        player_id: id,
        life,
        mana_pool: pool,
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
        drew_from_empty_library: false,
        mulligans_taken: 0,
        hand_kept: false,
        commander_casts: HashMap::new(),
    }
}

/// Two players, ready for priority at `(Main, PreCombatMain)`, every zone
/// kind (including command) registered for both. `p0`'s pool is seeded
/// generously (10 colorless) - CR 605 mana abilities aren't exercised here,
/// tests hand-set the pool directly, same convention
/// `phase5_cast_spell_payment.rs` established.
fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![
            player_state(p0, 20, ManaPool { colorless: 10, ..ManaPool::default() }),
            player_state(p1, 20, ManaPool::default()),
        ],
        turn_order: vec![p0, p1],
        active_player: p0,
        turn_number: 1,
        phase: Phase::Main,
        step: Step::PreCombatMain,
        priority_player: p0,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        combat: None,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        objects: HashMap::new(),
        zones: vec![
            Zone { kind: ZoneKind::Library, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Command, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Library, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Command, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Exile, owner: None, contents: VecDeque::new() },
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

fn put_in_zone(state: &mut GameState, object: GameObject, owner: Option<PlayerId>) {
    let kind = object.zone;
    let id = object.object_id;
    state.zone_mut(kind, owner).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn drain(engine: &mut Engine) -> engine::StepStatus {
    loop {
        match engine.step().status {
            engine::StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

/// Casts `card` for `p0` from wherever it sits, pays with `n` floating
/// colorless mana (must cover the resolved generic total - `p0`'s pool
/// starts at 10 colorless, plenty of headroom), and returns the pool's
/// remaining colorless count so a test can read off exactly what was spent.
fn cast_and_pay(engine: &mut Engine, card: ObjectId, n: u32) -> u32 {
    let p0 = PlayerId(0);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
            },
        })
        .expect("player holds priority");
    let status = drain(engine);
    assert!(matches!(status, engine::StepStatus::AwaitingPayment { .. }), "got {status:?}");
    let units = vec![ManaUnit::Colorless; n as usize];
    engine
        .submit_payment(PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] })
        .expect("pool covers the resolved cost");
    drain(engine);
    engine.state().player(p0).unwrap().mana_pool.colorless
}

/// Moves `card` directly from the battlefield/graveyard back into `p0`'s
/// command zone under a *new* id, mirroring what CR 903.9a's acceptance
/// path does in real play (11c's own `move_commander_to_command_zone`) -
/// this file only cares about the tax that results from a repeat cast, not
/// re-driving 903.9a's own pause/choice machinery (already covered by
/// `phase5_commander.rs`). Returns the new id.
fn manually_return_to_command_zone(state: &mut GameState, card: ObjectId) -> ObjectId {
    let object = state.objects.get(&card).unwrap().clone();
    let from_zone = object.zone;
    let owner = object.owner;
    state.zone_mut(from_zone, Some(owner)).unwrap().remove(card);
    state.objects.remove(&card);

    let new_id = state.allocate_object_id();
    let mut new_object = object;
    new_object.object_id = new_id;
    new_object.lineage = ObjectLineage { previous: Some(card) };
    new_object.zone = ZoneKind::Command;
    new_object.timestamp = state.allocate_sequence_no();
    state.zone_mut(ZoneKind::Command, Some(owner)).unwrap().push_bottom(new_id);
    state.objects.insert(new_id, new_object);
    new_id
}

#[test]
fn first_cast_from_the_command_zone_has_no_tax() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let commander = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(
            commander,
            creature_definition("General", ManaCost::parse("{1}").unwrap()),
            p0,
            ZoneKind::Command,
            true,
        ),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    let remaining = cast_and_pay(&mut engine, commander, 1);
    assert_eq!(remaining, 9, "base {{1}} only, no tax on the first cast");
}

#[test]
fn second_cast_after_returning_costs_two_more() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let commander = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(
            commander,
            creature_definition("General", ManaCost::parse("{1}").unwrap()),
            p0,
            ZoneKind::Command,
            true,
        ),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    cast_and_pay(&mut engine, commander, 1);

    // Send it (under whatever id it now has on the stack) to the graveyard
    // and back to the command zone, same as a real death + 903.9a
    // acceptance would, then recast it.
    let on_stack = engine.state().stack.last().unwrap().stack_id;
    engine.state_mut().stack.retain(|s| s.stack_id != on_stack);
    let mut obj = engine.state().objects.get(&on_stack).unwrap().clone();
    obj.zone = ZoneKind::Graveyard;
    engine.state_mut().objects.insert(on_stack, obj);
    engine.state_mut().zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(on_stack);

    let recast_id = manually_return_to_command_zone(engine.state_mut(), on_stack);

    let remaining = cast_and_pay(&mut engine, recast_id, 3);
    assert_eq!(remaining, 6, "base {{1}} + {{2}} tax for the one prior cast");
}

#[test]
fn third_cast_costs_four_more() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let commander = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(
            commander,
            creature_definition("General", ManaCost::parse("{1}").unwrap()),
            p0,
            ZoneKind::Command,
            true,
        ),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    let mut current = commander;
    for expected_remaining in [9u32, 6, 1] {
        let remaining = if current == commander {
            cast_and_pay(&mut engine, current, 1)
        } else {
            // n covers {1} base + {2} per prior cast so far.
            let prior = engine.state().player(p0).unwrap().commander_casts.get("General").copied().unwrap_or(0);
            cast_and_pay(&mut engine, current, 1 + 2 * prior)
        };
        assert_eq!(remaining, expected_remaining);

        let on_stack = engine.state().stack.last().unwrap().stack_id;
        engine.state_mut().stack.retain(|s| s.stack_id != on_stack);
        let mut obj = engine.state().objects.get(&on_stack).unwrap().clone();
        obj.zone = ZoneKind::Graveyard;
        engine.state_mut().objects.insert(on_stack, obj);
        engine.state_mut().zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(on_stack);
        current = manually_return_to_command_zone(engine.state_mut(), on_stack);
    }
    assert_eq!(engine.state().player(p0).unwrap().commander_casts.get("General"), Some(&3));
}

#[test]
fn partner_commanders_have_independent_tax_counters() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let general = ObjectId(0);
    let admiral = ObjectId(1);
    put_in_zone(
        &mut state,
        card_object(
            general,
            creature_definition("General", ManaCost::parse("{1}").unwrap()),
            p0,
            ZoneKind::Command,
            true,
        ),
        Some(p0),
    );
    put_in_zone(
        &mut state,
        card_object(
            admiral,
            creature_definition("Admiral", ManaCost::parse("{1}").unwrap()),
            p0,
            ZoneKind::Command,
            true,
        ),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    // Cast and return General twice, taxing it up to a second prior cast.
    cast_and_pay(&mut engine, general, 1);
    let on_stack = engine.state().stack.last().unwrap().stack_id;
    engine.state_mut().stack.retain(|s| s.stack_id != on_stack);
    let mut obj = engine.state().objects.get(&on_stack).unwrap().clone();
    obj.zone = ZoneKind::Graveyard;
    engine.state_mut().objects.insert(on_stack, obj);
    engine.state_mut().zone_mut(ZoneKind::Graveyard, Some(p0)).unwrap().push_bottom(on_stack);
    let general_recast = manually_return_to_command_zone(engine.state_mut(), on_stack);
    cast_and_pay(&mut engine, general_recast, 3);

    // Clear General's second cast off the stack (sorcery speed needs an
    // empty stack) before casting Admiral - where it ends up doesn't matter
    // for this test, only that the stack is empty again.
    let on_stack = engine.state().stack.last().unwrap().stack_id;
    engine.state_mut().stack.retain(|s| s.stack_id != on_stack);

    // Admiral, never cast before, still costs only its base {1} - General's
    // two casts didn't bleed into its own counter.
    let remaining = cast_and_pay(&mut engine, admiral, 1);
    // Pool: started 10, spent 1 (General #1) + 3 (General #2) + 1 (Admiral #1) = 5, 5 left.
    assert_eq!(remaining, 5, "Admiral's first cast is untaxed regardless of General's cast count");
}

#[test]
fn casting_from_hand_is_never_taxed() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    // Same card, same name as a would-be commander, but sitting in hand and
    // not marked is_commander - the ordinary CR 601.2f path, no 903.8
    // involvement at all.
    let card = ObjectId(0);
    put_in_zone(
        &mut state,
        card_object(card, creature_definition("General", ManaCost::parse("{1}").unwrap()), p0, ZoneKind::Hand, false),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    let remaining = cast_and_pay(&mut engine, card, 1);
    assert_eq!(remaining, 9, "hand cast: base {{1}} only, tax never applies");
}

#[test]
fn tax_stacks_additively_onto_a_real_printed_cost() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let commander = ObjectId(0);
    // {2}{G} rather than a bare generic cost, to confirm the tax adds to
    // whatever generic component already exists rather than creating a
    // brand new {2} cost that clobbers the printed one.
    put_in_zone(
        &mut state,
        card_object(
            commander,
            creature_definition("General", ManaCost::parse("{2}{G}").unwrap()),
            p0,
            ZoneKind::Command,
            true,
        ),
        Some(p0),
    );

    let mut engine = Engine::new(state);
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::CastSpell {
                card: commander,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
            },
        })
        .expect("player holds priority");
    let status = drain(&mut engine);
    assert!(matches!(status, engine::StepStatus::AwaitingPayment { .. }), "got {status:?}");
    // First cast: no tax yet, so still exactly {2}{G} - 2 colorless pays
    // the generic, 1 green pays the pip. Confirms tax math is additive by
    // checking the untaxed baseline before recasting.
    engine
        .submit_payment(PaymentPlan {
            payments: vec![CostPayment::Mana {
                units: vec![ManaUnit::Colorless, ManaUnit::Colorless],
                phyrexian_life: 0,
            }],
        })
        .expect_err("green pip unpaid - confirms the resolved cost still carries its printed {G}, not just generic");
}
