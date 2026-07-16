//! Phase 5 vertical slice: `AbilityEffect::Search` (backlog item 14b) —
//! `Game.Ops.Search(filter, count, opts?)` (CR 701.23a/b).
//!
//! Unlike `Scry`/`Surveil` (backlog item 14a, a fixed top-N peek),
//! `ResolveStackTop` scans the controller's *entire* library and pre-filters
//! to ids matching the search's own `FilterSpec` before pausing on
//! `ProcessState::AwaitingSearchChoice` — no visibility-machinery gap blocks
//! this (`GameState`/`Zone` already expose full contents freely; the only
//! project-wide hidden-info gap is the Lua callback boundary, never crossed
//! by this Rust-native primitive). CR 701.23b: choosing fewer than `count`,
//! including zero, is always legal. `Engine::submit_search_choice` moves
//! each found card to the pause's own `destination` (Hand or Battlefield)
//! and shuffles afterward iff `shuffle_after`.
//!
//! Also exercises `FilterSpec`'s new `supertypes`/`subtypes` fields (added
//! alongside this slice — `cost.rs`'s own test module covers `matches()`
//! directly; this file proves the whole pipeline, including the
//! `Game.Ops.Search` Lua binding's filter-table decoding).

use std::collections::{HashMap, VecDeque};

use stackfish_engine::engine::{Engine, SearchChoiceError, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;

fn basic_land_definition(definition_id: u64, name: &str, subtype: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::BASIC,
        card_types: CardTypeSet::LAND,
        subtypes: vec![Subtype(subtype.to_string())],
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(
            "return { initial_effect = function(source) end }".to_string(),
        ),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn nonbasic_land_definition(definition_id: u64, name: &str) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(
            "return { initial_effect = function(source) end }".to_string(),
        ),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn basic_land_filter() -> FilterSpec {
    FilterSpec {
        card_types: Some(CardTypeSet::LAND),
        supertypes: Some(SupertypeSet::BASIC),
        subtypes: None,
        controller: None,
        random: false,
        extra: None,
    }
}

fn search_spell_definition(
    count: u32,
    destination: SearchDestination,
    shuffle_after: bool,
) -> CardDefinition {
    CardDefinition {
        definition_id: 40,
        name: "Test Search Spell".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::SORCERY,
        subtypes: Vec::new(),
        oracle_text: "Search your library for a basic land card.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey("test_search".to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: Some(AbilityEffect::Search { filter: basic_land_filter(), count, destination, shuffle_after }),
        modes: Vec::new(),
    }
}

fn card_object(id: ObjectId, definition: CardDefinition, owner: PlayerId, zone: ZoneKind) -> GameObject {
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
        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
    }
}

/// Same shape as `phase5_scry_surveil`'s own `two_player_state`, plus a
/// Library zone for `p0` seeded with `library` in an arbitrary order (search
/// doesn't care about library order the way Scry/Surveil do).
fn two_player_state(library: Vec<ObjectId>) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        players: vec![
            PlayerState {
                player_id: p0,
                life: 20,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
            },
            PlayerState {
                player_id: p1,
                life: 20,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
            },
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
            Zone { kind: ZoneKind::Hand, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Hand, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p0), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Graveyard, owner: Some(p1), contents: VecDeque::new() },
            Zone { kind: ZoneKind::Library, owner: Some(p0), contents: VecDeque::from(library) },
            Zone { kind: ZoneKind::Library, owner: Some(p1), contents: VecDeque::new() },
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
    }
}

fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
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

/// Both players pass priority (CR 117.4) so the cast spell actually resolves.
fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine.submit_action(PlayerAction { player: p0, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine);
    engine.submit_action(PlayerAction { player: p1, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn library_order(engine: &Engine, player: PlayerId) -> Vec<ObjectId> {
    engine.state().zone(ZoneKind::Library, Some(player)).unwrap().contents.iter().copied().collect()
}

#[test]
fn search_finds_a_basic_land_and_puts_it_in_hand() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let forest = ObjectId(10);
    let mountain = ObjectId(11);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![forest, mountain]);
    state.objects.insert(forest, card_object(forest, basic_land_definition(1, "Forest", "Forest"), p0, ZoneKind::Library));
    state.objects.insert(mountain, card_object(mountain, basic_land_definition(2, "Mountain", "Mountain"), p0, ZoneKind::Library));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(1, SearchDestination::Hand, true), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingSearchChoice { player, candidates, count } = status else {
        panic!("expected a pause for Search, got {status:?}")
    };
    assert_eq!(player, p0);
    assert_eq!(count, 1);
    assert_eq!(candidates.len(), 2, "both basic lands match the filter");

    engine.submit_search_choice(vec![forest]).expect("forest is one of the offered candidates");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let hand: Vec<ObjectId> = engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.iter().copied().collect();
    assert_eq!(hand.len(), 1, "the found forest is now in hand");
    assert!(engine.state().object(forest).is_none(), "old library id retired (CR 400.7)");
}

#[test]
fn search_choosing_zero_is_legal() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let forest = ObjectId(10);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![forest]);
    state.objects.insert(forest, card_object(forest, basic_land_definition(1, "Forest", "Forest"), p0, ZoneKind::Library));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(1, SearchDestination::Hand, false), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    engine.submit_search_choice(Vec::new()).expect("CR 701.23b: choosing zero is always legal");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert!(engine.state().zone(ZoneKind::Hand, Some(p0)).unwrap().contents.is_empty());
    assert_eq!(library_order(&engine, p0), vec![forest], "nothing moved, untouched");
}

#[test]
fn search_rejects_choosing_more_than_count() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let forest = ObjectId(10);
    let mountain = ObjectId(11);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![forest, mountain]);
    state.objects.insert(forest, card_object(forest, basic_land_definition(1, "Forest", "Forest"), p0, ZoneKind::Library));
    state.objects.insert(mountain, card_object(mountain, basic_land_definition(2, "Mountain", "Mountain"), p0, ZoneKind::Library));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(1, SearchDestination::Hand, false), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let err = engine.submit_search_choice(vec![forest, mountain]).unwrap_err();
    assert!(matches!(err, SearchChoiceError::TooMany));
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingSearchChoice { .. }), "a rejected answer leaves the pause open");
}

#[test]
fn search_rejects_a_non_matching_id() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let forest = ObjectId(10);
    let nonbasic = ObjectId(11);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![forest, nonbasic]);
    state.objects.insert(forest, card_object(forest, basic_land_definition(1, "Forest", "Forest"), p0, ZoneKind::Library));
    state.objects.insert(nonbasic, card_object(nonbasic, nonbasic_land_definition(3, "Test Nonbasic"), p0, ZoneKind::Library));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(1, SearchDestination::Hand, false), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, p0, p1);
    let StepStatus::AwaitingSearchChoice { candidates, .. } = &status else {
        panic!("expected a pause, got {status:?}")
    };
    assert_eq!(candidates, &vec![forest], "the nonbasic land was pre-filtered out entirely");

    let err = engine.submit_search_choice(vec![nonbasic]).unwrap_err();
    assert!(matches!(err, SearchChoiceError::NotACandidate));
}

#[test]
fn search_rejects_a_duplicate_chosen_id() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let forest = ObjectId(10);
    let card = ObjectId(2);
    // count: 2 so a 2-element answer doesn't already fail on TooMany before
    // ever reaching the duplicate check below.
    let mut state = two_player_state(vec![forest]);
    state.objects.insert(forest, card_object(forest, basic_land_definition(1, "Forest", "Forest"), p0, ZoneKind::Library));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(2, SearchDestination::Hand, false), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    let err = engine.submit_search_choice(vec![forest, forest]).unwrap_err();
    assert!(matches!(err, SearchChoiceError::NotACandidate), "same id chosen twice is rejected, not silently deduped");
}

#[test]
fn search_to_battlefield_tapped_sets_tapped_and_runs_etb_script() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let forest = ObjectId(10);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![forest]);
    state.objects.insert(forest, card_object(forest, basic_land_definition(1, "Forest", "Forest"), p0, ZoneKind::Library));
    state.objects.insert(
        card,
        card_object(
            card,
            search_spell_definition(1, SearchDestination::Battlefield { tapped: true }, true),
            p0,
            ZoneKind::Hand,
        ),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    engine.submit_search_choice(vec![forest]).expect("forest is the only candidate");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    let battlefield: Vec<ObjectId> =
        engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.iter().copied().collect();
    assert_eq!(battlefield.len(), 1, "the found land entered the battlefield");
    let new_id = battlefield[0];
    assert!(engine.state().object(new_id).unwrap().tapped, "entered tapped, as requested");
    assert_eq!(engine.state().object(new_id).unwrap().controlled_since_turn, engine.state().turn_number);
}

#[test]
fn search_shuffles_after_by_default() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let forest = ObjectId(10);
    let mountain = ObjectId(11);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![forest, mountain]);
    state.objects.insert(forest, card_object(forest, basic_land_definition(1, "Forest", "Forest"), p0, ZoneKind::Library));
    state.objects.insert(mountain, card_object(mountain, basic_land_definition(2, "Mountain", "Mountain"), p0, ZoneKind::Library));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(1, SearchDestination::Hand, true), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    engine.submit_search_choice(vec![forest]).expect("forest is a candidate");
    drain(&mut engine);

    let shuffled = engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::LibraryShuffled { player } if player == p0));
    assert!(shuffled, "shuffle_after defaults to true - CR 701.24a");
}

#[test]
fn search_without_shuffle_after_leaves_remaining_order_untouched() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let forest = ObjectId(10);
    let mountain = ObjectId(11);
    let swamp = ObjectId(12);
    let card = ObjectId(2);
    let mut state = two_player_state(vec![forest, mountain, swamp]);
    state.objects.insert(forest, card_object(forest, basic_land_definition(1, "Forest", "Forest"), p0, ZoneKind::Library));
    state.objects.insert(mountain, card_object(mountain, basic_land_definition(2, "Mountain", "Mountain"), p0, ZoneKind::Library));
    state.objects.insert(swamp, card_object(swamp, basic_land_definition(3, "Swamp", "Swamp"), p0, ZoneKind::Library));
    state.objects.insert(
        card,
        card_object(card, search_spell_definition(1, SearchDestination::Hand, false), p0, ZoneKind::Hand),
    );
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(card);

    let mut engine = Engine::new(state);
    cast_spell(&mut engine, p0, card);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, p0, p1);

    engine.submit_search_choice(vec![mountain]).expect("mountain is a candidate");
    drain(&mut engine);

    assert_eq!(library_order(&engine, p0), vec![forest, swamp], "forest/swamp's relative order untouched, no shuffle");
    let shuffled = engine.state().ledger.iter().any(|e| matches!(e.kind, GameEventKind::LibraryShuffled { .. }));
    assert!(!shuffled, "shuffle_after: false means no shuffle at all");
}

#[test]
fn game_ops_search_lua_binding_decodes_correctly() {
    let mut runtime = ScriptRuntime::new();
    let (effect, handle) = runtime
        .load_operation(
            r#"return Game.Ops.Search(
                { card_types = { "Land" }, supertypes = { "Basic" }, subtypes = { "Forest" } },
                1,
                { destination = "battlefield", tapped = true, shuffle = false }
            )"#,
        )
        .expect("primitive loads");
    let Some(AbilityEffect::Search { filter, count, destination, shuffle_after }) = effect else {
        panic!("expected Some(AbilityEffect::Search {{ .. }}), got {effect:?}")
    };
    assert_eq!(count, 1);
    assert!(matches!(destination, SearchDestination::Battlefield { tapped: true }));
    assert!(!shuffle_after);
    assert_eq!(filter.card_types, Some(CardTypeSet::LAND));
    assert_eq!(filter.supertypes, Some(SupertypeSet::BASIC));
    assert_eq!(filter.subtypes, Some(vec![Subtype("Forest".to_string())]));
    assert_eq!(handle, ScriptHandle(0));
}

#[test]
fn game_ops_search_defaults_to_hand_and_shuffle_true() {
    let mut runtime = ScriptRuntime::new();
    let (effect, _handle) = runtime
        .load_operation(r#"return Game.Ops.Search({ supertypes = { "Basic" } }, 1)"#)
        .expect("primitive loads");
    let Some(AbilityEffect::Search { destination, shuffle_after, .. }) = effect else {
        panic!("expected Some(AbilityEffect::Search {{ .. }}), got {effect:?}")
    };
    assert!(matches!(destination, SearchDestination::Hand), "no opts table - defaults to hand");
    assert!(shuffle_after, "no opts table - defaults to shuffling after (CR 701.24a convention)");
}
