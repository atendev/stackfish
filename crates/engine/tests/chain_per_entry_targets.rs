//! CR 601.2c, 608.2c: per-entry targets for a chain. `Game.Ops.ChainEach({ op, words }, ...)` gives entry i the target words it owns.
//! `:SetTarget` also accepts an array of filter tables, and `DrawCards` draws for the targeted player when the ability declares a target.

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const SPELL: ObjectId = ObjectId(100);
const BEAR: ObjectId = ObjectId(1);
const BEAR2: ObjectId = ObjectId(2);

fn decode(body: &str) -> Result<Option<AbilityEffect>, String> {
    let source =
        format!("local s = {{}}\nfunction s.operation()\n    return {body}\nend\nreturn s");
    ScriptRuntime::new()
        .build_operation(&source)
        .map(|decoded| decoded.expect("declares an operation").0)
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn chain_each_decodes_entries_and_word_counts() {
    let effect =
        decode("Game.Ops.ChainEach({ Game.Ops.DrawCards(2), 0 }, { Game.Ops.DrawCards(2), 1 })")
            .expect("loads");
    assert_eq!(
        effect,
        Some(AbilityEffect::Chain {
            entries: vec![AbilityEffect::DrawCards(2), AbilityEffect::DrawCards(2)],
            words: Some(vec![0, 1])
        })
    );
}

#[test]
fn chain_each_load_errors_name_the_argument() {
    let cases = [
        ("Game.Ops.ChainEach()", "at least one"),
        (
            "Game.Ops.ChainEach(Game.Ops.Destroy)",
            "argument 1 must be a { op, words } table",
        ),
        (
            "Game.Ops.ChainEach({ Game.Ops.Destroy, -1 })",
            "argument 1: words must be a whole number of 0 or more",
        ),
        (
            "Game.Ops.ChainEach({ Game.Ops.Destroy, 1.5 })",
            "argument 1: words must be a whole number of 0 or more",
        ),
        (
            "Game.Ops.ChainEach({ Game.Ops.Destroy })",
            "argument 1: words must be a whole number of 0 or more",
        ),
        (
            "Game.Ops.ChainEach({ Game.Ops.Destroy, 1 }, { Game.Ops.AddMana(\"G\"), 0 })",
            "argument 2 can't be chained",
        ),
        ("Game.Ops.ChainEach({ Game.Ops.Mill, 1 })", "must be called"),
        (
            "Game.Ops.ChainEach({ function() return {} end, 0 })",
            "custom function",
        ),
    ];
    for (body, expected) in cases {
        let error = decode(body).expect_err(body);
        assert!(error.contains(expected), "{body}: {error}");
    }
}

const TWO_WORD_TRIGGER: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.Enters)
    e:SetTarget({
        { type = "object", card_types = { "Creature" }, controller = "opponent" },
        { type = "player", controller = "opponent" },
    })
    e:SetOperation(Game.Ops.ChainEach(
        { Game.Ops.Destroy, 1 },
        { Game.Ops.LoseLife(2), 1 },
        { Game.Ops.DrawCards(1), 0 }))
    e:Register()
end
return s
"#;

fn ability_script(target: &str, operation: &str) -> String {
    format!(
        "local s = {{}}\nfunction s.register_abilities(c)\n    local a = Game.CreateActivatedAbility(c)\n    a:SetTarget({target})\n    a:SetOperation({operation})\n    a:Register()\nend\nreturn s"
    )
}

fn register(script: &str) -> Result<GameState, String> {
    let mut state = base_two_player_state(zones(
        P0,
        P1,
        &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield],
    ));
    let id = ObjectId(5);
    let mut def = card(5, "Source", CardTypeSet::ARTIFACT);
    def.script_ref = ScriptReference::Inline(script.to_string());
    state
        .objects
        .insert(id, card_object(id, def.clone(), P0, ZoneKind::Battlefield));
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(id);
    ScriptRuntime::new()
        .run_card_script(&mut state, &def, id)
        .map_err(|e| format!("{e:?}"))?;
    Ok(state)
}

#[test]
fn set_target_accepts_an_array_of_filters_as_one_word_each() {
    let script = ability_script(
        "{ { type = \"object\", card_types = { \"Creature\" } }, { type = \"player\" } }",
        "Game.Ops.ChainEach({ Game.Ops.Tap, 1 }, { Game.Ops.LoseLife(1), 1 })",
    );
    let state = register(&script).expect("registers");
    let Some(TargetSpec::Multiple(words)) = &state.abilities[0].target else {
        panic!("{:?}", state.abilities[0].target)
    };
    assert_eq!(words.len(), 2);
    assert!(
        words
            .iter()
            .all(|w| w.count == 1 && !w.up_to && !w.at_least)
    );
}

#[test]
fn a_one_entry_array_and_a_bare_table_are_the_same_single_word() {
    for target in ["{ { type = \"player\" } }", "{ type = \"player\" }"] {
        let script = ability_script(target, "Game.Ops.LoseLife(1)");
        let state = register(&script).expect("registers");
        assert!(
            matches!(&state.abilities[0].target, Some(TargetSpec::Primitive(_))),
            "{target}"
        );
    }
}

#[test]
fn a_table_mixing_array_entries_and_keys_is_a_load_error() {
    let script = ability_script(
        "{ { type = \"player\" }, type = \"player\" }",
        "Game.Ops.LoseLife(1)",
    );
    let error = register(&script).expect_err("mixed");
    assert!(error.contains("mixes array entries"), "{error}");
}

#[test]
fn word_counts_that_do_not_match_the_declared_words_fail_at_register() {
    let script = ability_script(
        "{ type = \"player\" }",
        "Game.Ops.ChainEach({ Game.Ops.LoseLife(1), 1 }, { Game.Ops.DrawCards(1), 1 })",
    );
    let error = register(&script).expect_err("2 words owned, 1 declared");
    assert!(
        error.contains("own 2") && error.contains("declares 1"),
        "{error}"
    );
}

fn card(id: u64, name: &str, types: CardTypeSet) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::ByKey(name.to_string()),
        power: types
            .contains(CardTypeSet::CREATURE)
            .then_some(PTValue::Fixed(2)),
        toughness: types
            .contains(CardTypeSet::CREATURE)
            .then_some(PTValue::Fixed(2)),
        ..minimal_card_definition(
            id,
            name,
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            types,
        )
    }
}

fn creature_word() -> TargetRequirement {
    TargetRequirement {
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
    }
}

fn player_word(controller: Option<FilterController>) -> TargetRequirement {
    TargetRequirement {
        filter: TargetFilter::Player { controller },
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    }
}

fn chain_each(entries: Vec<(AbilityEffect, usize)>) -> AbilityEffect {
    let (entries, words) = entries.into_iter().unzip();
    AbilityEffect::Chain {
        entries,
        words: Some(words),
    }
}

fn spell(effect: AbilityEffect, target_requirement: Option<TargetSpec>) -> CardDefinition {
    CardDefinition {
        effect: Some(effect),
        target_requirement,
        ..minimal_card_definition(
            99,
            "Test Spell",
            "unused",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::INSTANT,
        )
    }
}

fn state_with(spell: CardDefinition, library: u64) -> GameState {
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(
            P0,
            P1,
            &[
                ZoneKind::Hand,
                ZoneKind::Library,
                ZoneKind::Battlefield,
                ZoneKind::Graveyard,
                ZoneKind::Exile,
            ],
        ))
    };
    let mut place = |id: ObjectId, def: CardDefinition, owner: PlayerId, zone: ZoneKind| {
        state.objects.insert(id, card_object(id, def, owner, zone));
        let scope = if matches!(zone, ZoneKind::Battlefield | ZoneKind::Exile) {
            None
        } else {
            Some(owner)
        };
        state.zone_mut(zone, scope).unwrap().push_bottom(id);
    };
    place(
        BEAR,
        card(1, "Bear", CardTypeSet::CREATURE),
        P1,
        ZoneKind::Battlefield,
    );
    place(
        BEAR2,
        card(3, "Bear 2", CardTypeSet::CREATURE),
        P1,
        ZoneKind::Battlefield,
    );
    for i in 0..library {
        place(
            ObjectId(200 + i),
            card(200 + i, &format!("P0 card {i}"), CardTypeSet::LAND),
            P0,
            ZoneKind::Library,
        );
        place(
            ObjectId(300 + i),
            card(300 + i, &format!("P1 card {i}"), CardTypeSet::LAND),
            P1,
            ZoneKind::Library,
        );
    }
    place(SPELL, spell, P0, ZoneKind::Hand);
    state
}

fn engine_with(spell: CardDefinition, library: u64) -> Engine {
    Engine::new(state_with(spell, library))
}

fn cast(engine: &mut Engine, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell {
                card: SPELL,
                modes: Vec::new(),
                targets,
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("p0 holds priority and the targets are legal");
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "{status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .unwrap();
    drain(engine);
}

fn pass_both(engine: &mut Engine) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine);
    engine
        .submit_action(PlayerAction {
            player: P1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine)
}

fn life(engine: &Engine, player: PlayerId) -> i32 {
    engine.state().player(player).unwrap().life
}

fn hand_size(engine: &Engine, player: PlayerId) -> usize {
    engine
        .state()
        .zone(ZoneKind::Hand, Some(player))
        .unwrap()
        .contents
        .len()
}

fn tapped(engine: &Engine, id: ObjectId) -> bool {
    engine.state().object(id).unwrap().tapped
}

fn zones_of(engine: &Engine, name: &str) -> Vec<ZoneKind> {
    engine
        .state()
        .objects
        .values()
        .filter(|o| o.definition.name == name)
        .map(|o| o.zone)
        .collect()
}

fn drawers(engine: &Engine) -> Vec<PlayerId> {
    engine
        .state()
        .ledger
        .iter()
        .filter_map(|e| match &e.kind {
            GameEventKind::ZoneChange {
                object,
                from: ZoneKind::Library,
                to: ZoneKind::Hand,
                ..
            } => engine.state().object(*object).map(|o| o.owner),
            _ => None,
        })
        .collect()
}

fn redirect_battlefield_death(source: u64, to: ZoneKind) -> ReplacementEffect {
    ReplacementEffect {
        source: ObjectId(source),
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Battlefield),
                to: Some(ZoneKind::Graveyard),
                affected: EffectFilter {
                    controller: None,
                    card_types: None,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                    self_only: false,
                },
            },
            payload: ReplacementPayload::ReplaceZoneDestination(to),
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

#[test]
fn draw_two_cards_then_target_opponent_draws_two_cards() {
    let effect = chain_each(vec![
        (AbilityEffect::DrawCards(2), 0),
        (AbilityEffect::DrawCards(2), 1),
    ]);
    let mut engine = engine_with(
        spell(
            effect,
            Some(TargetSpec::Primitive(player_word(Some(
                FilterController::Opponent,
            )))),
        ),
        5,
    );
    cast(&mut engine, vec![Target::Player(P1)]);
    let (mine, theirs) = (hand_size(&engine, P0), hand_size(&engine, P1));
    pass_both(&mut engine);

    assert_eq!(
        hand_size(&engine, P0),
        mine + 2,
        "the zero-word entry draws for the controller"
    );
    assert_eq!(
        hand_size(&engine, P1),
        theirs + 2,
        "the one-word entry draws for the targeted opponent"
    );
    assert_eq!(
        drawers(&engine),
        vec![P0, P0, P1, P1],
        "your two cards first, then theirs"
    );
    assert!(engine.state().stack.is_empty());
}

#[test]
fn target_opponent_rejects_the_controller_as_the_target() {
    let effect = chain_each(vec![
        (AbilityEffect::DrawCards(2), 0),
        (AbilityEffect::DrawCards(2), 1),
    ]);
    let mut engine = engine_with(
        spell(
            effect,
            Some(TargetSpec::Primitive(player_word(Some(
                FilterController::Opponent,
            )))),
        ),
        5,
    );
    let result = engine.submit_action(PlayerAction {
        player: P0,
        kind: PlayerActionKind::CastSpell {
            card: SPELL,
            modes: Vec::new(),
            targets: vec![Target::Player(P0)],
            x_value: None,
            alternative_cost: None,
            pay_buyback: false,
        },
    });
    let rejected = result.is_err() || matches!(drain(&mut engine), StepStatus::Error(_));
    assert!(
        rejected,
        "targeting yourself with a \"target opponent\" word must not be accepted"
    );
}

#[test]
fn a_plain_target_player_draws_spell_draws_for_the_target() {
    let mut engine = engine_with(
        spell(
            AbilityEffect::DrawCards(2),
            Some(TargetSpec::Primitive(player_word(None))),
        ),
        5,
    );
    cast(&mut engine, vec![Target::Player(P1)]);
    let (mine, theirs) = (hand_size(&engine, P0), hand_size(&engine, P1));
    pass_both(&mut engine);

    assert_eq!(hand_size(&engine, P0), mine, "the controller doesn't draw");
    assert_eq!(hand_size(&engine, P1), theirs + 2);
}

#[test]
fn draw_cards_without_a_target_still_draws_for_the_controller() {
    let mut engine = engine_with(spell(AbilityEffect::DrawCards(2), None), 5);
    cast(&mut engine, Vec::new());
    let (mine, theirs) = (hand_size(&engine, P0), hand_size(&engine, P1));
    pass_both(&mut engine);

    assert_eq!(hand_size(&engine, P0), mine + 2);
    assert_eq!(hand_size(&engine, P1), theirs);
}

#[test]
fn each_entry_reads_its_own_word_across_two_different_target_kinds() {
    let effect = chain_each(vec![
        (AbilityEffect::Destroy, 1),
        (AbilityEffect::LoseLife(2), 1),
        (AbilityEffect::DrawCards(1), 0),
    ]);
    let spec = TargetSpec::Multiple(vec![creature_word(), player_word(None)]);
    let mut engine = engine_with(spell(effect, Some(spec)), 3);
    cast(&mut engine, vec![Target::Object(BEAR), Target::Player(P1)]);
    let mine = hand_size(&engine, P0);
    pass_both(&mut engine);

    assert_eq!(
        zones_of(&engine, "Bear"),
        vec![ZoneKind::Graveyard],
        "word 1 went to Destroy"
    );
    assert_eq!(
        zones_of(&engine, "Bear 2"),
        vec![ZoneKind::Battlefield],
        "and not to anything else"
    );
    assert_eq!(life(&engine, P1), 18, "word 2 went to LoseLife");
    assert_eq!(life(&engine, P0), 20);
    assert_eq!(
        hand_size(&engine, P0),
        mine + 1,
        "the zero-word entry acted on the controller"
    );
}

#[test]
fn two_entries_on_two_creature_words_each_get_their_own_creature() {
    let effect = chain_each(vec![(AbilityEffect::Tap, 1), (AbilityEffect::Destroy, 1)]);
    let spec = TargetSpec::Multiple(vec![creature_word(), creature_word()]);
    let mut engine = engine_with(spell(effect, Some(spec)), 0);
    cast(
        &mut engine,
        vec![Target::Object(BEAR), Target::Object(BEAR2)],
    );
    pass_both(&mut engine);

    assert!(tapped(&engine, BEAR), "word 1 was tapped");
    assert_eq!(
        zones_of(&engine, "Bear"),
        vec![ZoneKind::Battlefield],
        "and not destroyed"
    );
    assert_eq!(
        zones_of(&engine, "Bear 2"),
        vec![ZoneKind::Graveyard],
        "word 2 was destroyed"
    );
}

#[test]
fn an_illegal_word_makes_only_its_entry_do_nothing() {
    // CR 608.2b: the spell fizzles only when every target is illegal.
    let effect = chain_each(vec![
        (AbilityEffect::Tap, 1),
        (AbilityEffect::Untap, 1),
        (AbilityEffect::GainLife(3), 0),
    ]);
    let spec = TargetSpec::Multiple(vec![creature_word(), creature_word()]);
    let mut engine = engine_with(spell(effect, Some(spec)), 0);
    cast(
        &mut engine,
        vec![Target::Object(BEAR), Target::Object(BEAR2)],
    );
    engine.state_mut().objects.get_mut(&BEAR2).unwrap().zone = ZoneKind::Graveyard;
    pass_both(&mut engine);

    assert!(
        tapped(&engine, BEAR),
        "the entry with a legal word still ran"
    );
    assert_eq!(
        life(&engine, P0),
        23,
        "and so did the zero-word entry after it"
    );
}

#[test]
fn a_chain_fizzles_whole_when_every_word_is_illegal() {
    let effect = chain_each(vec![
        (AbilityEffect::Tap, 1),
        (AbilityEffect::Tap, 1),
        (AbilityEffect::GainLife(3), 0),
    ]);
    let spec = TargetSpec::Multiple(vec![creature_word(), creature_word()]);
    let mut engine = engine_with(spell(effect, Some(spec)), 0);
    cast(
        &mut engine,
        vec![Target::Object(BEAR), Target::Object(BEAR2)],
    );
    engine.state_mut().objects.get_mut(&BEAR).unwrap().zone = ZoneKind::Graveyard;
    engine.state_mut().objects.get_mut(&BEAR2).unwrap().zone = ZoneKind::Graveyard;
    pass_both(&mut engine);

    assert_eq!(life(&engine, P0), 20, "no entry ran");
    assert!(engine.state().stack.is_empty());
}

#[test]
fn an_ordering_pause_in_the_first_entry_resumes_the_second_on_its_own_word() {
    let effect = chain_each(vec![
        (AbilityEffect::Destroy, 1),
        (AbilityEffect::LoseLife(2), 1),
    ]);
    let spec = TargetSpec::Multiple(vec![creature_word(), player_word(None)]);
    let mut engine = engine_with(spell(effect, Some(spec)), 0);
    for (id, to) in [(901, ZoneKind::Exile), (902, ZoneKind::Hand)] {
        let mut def = card(id, "Replacer", CardTypeSet::ENCHANTMENT);
        def.script_ref = ScriptReference::ByKey("replacer".to_string());
        engine.state_mut().objects.insert(
            ObjectId(id),
            card_object(ObjectId(id), def, P0, ZoneKind::Battlefield),
        );
        engine
            .state_mut()
            .zone_mut(ZoneKind::Battlefield, None)
            .unwrap()
            .push_bottom(ObjectId(id));
        engine
            .state_mut()
            .replacement_effects
            .push(redirect_battlefield_death(id, to));
    }
    cast(&mut engine, vec![Target::Object(BEAR), Target::Player(P1)]);
    let status = pass_both(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingReplacementChoice { .. }),
        "CR 616.1: {status:?}"
    );
    assert_eq!(life(&engine, P1), 20, "the second entry waits");

    engine
        .submit_replacement_choice(ReplacementCandidate::Registered(ObjectId(901)))
        .unwrap();
    drain(&mut engine);

    assert_eq!(zones_of(&engine, "Bear"), vec![ZoneKind::Exile]);
    assert_eq!(
        life(&engine, P1),
        18,
        "the resumed entry read the player word, not the creature"
    );
    assert_eq!(life(&engine, P0), 20);
}

#[test]
fn an_activated_ability_with_two_words_runs_each_entry_on_its_own_word() {
    let mut state = state_with(spell(AbilityEffect::GainLife(0), None), 3);
    let script = ability_script(
        "{ { type = \"object\", card_types = { \"Creature\" } }, { type = \"player\" } }",
        "Game.Ops.ChainEach({ Game.Ops.Tap, 1 }, { Game.Ops.LoseLife(2), 1 }, { Game.Ops.DrawCards(1), 0 })",
    );
    let id = ObjectId(5);
    let mut def = card(5, "Source", CardTypeSet::ARTIFACT);
    def.script_ref = ScriptReference::Inline(script);
    let mut object = card_object(id, def.clone(), P0, ZoneKind::Battlefield);
    object.controlled_since_turn = 0;
    state.objects.insert(id, object);
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(id);
    ScriptRuntime::new()
        .run_card_script(&mut state, &def, id)
        .expect("script registers");
    let mut engine = Engine::new(state);
    pass_both(&mut engine);
    let hand = hand_size(&engine, P0);

    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::ActivateAbility {
                source: id,
                ability_index: 0,
                targets: vec![Target::Object(BEAR), Target::Player(P1)],
                chosen_colors: Vec::new(),
                x_value: None,
            },
        })
        .expect("activation is legal");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "{status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .unwrap();
    drain(&mut engine);
    pass_both(&mut engine);

    assert!(tapped(&engine, BEAR));
    assert!(!tapped(&engine, BEAR2));
    assert_eq!(life(&engine, P1), 18);
    assert_eq!(hand_size(&engine, P0), hand + 1);
}

#[test]
fn an_etb_trigger_with_two_words_asks_for_both_and_runs_each_entry_on_its_word() {
    let looter = ObjectId(6);
    let mut def = card(6, "Looter", CardTypeSet::CREATURE);
    def.script_ref = ScriptReference::Inline(TWO_WORD_TRIGGER.to_string());
    let mut state = state_with(spell(AbilityEffect::GainLife(0), None), 3);
    state
        .objects
        .insert(looter, card_object(looter, def, P0, ZoneKind::Hand));
    state
        .zone_mut(ZoneKind::Hand, Some(P0))
        .unwrap()
        .push_bottom(looter);
    let mut engine = Engine::new(state);

    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell {
                card: looter,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("p0 holds priority");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "{status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .unwrap();
    drain(&mut engine);

    let status = pass_both(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingTriggerTargets { .. }),
        "CR 603.3d: {status:?}"
    );
    let hand = hand_size(&engine, P0);
    engine
        .submit_trigger_targets(vec![Target::Object(BEAR), Target::Player(P1)])
        .expect("both words satisfied");
    drain(&mut engine);
    pass_both(&mut engine);

    assert_eq!(
        zones_of(&engine, "Bear"),
        vec![ZoneKind::Graveyard],
        "word 1 destroyed the creature"
    );
    assert_eq!(zones_of(&engine, "Bear 2"), vec![ZoneKind::Battlefield]);
    assert_eq!(life(&engine, P1), 18, "word 2 hit the opponent");
    assert_eq!(life(&engine, P0), 20);
    assert_eq!(
        hand_size(&engine, P0),
        hand + 1,
        "the zero-word entry drew for the controller"
    );
}

#[test]
fn the_documented_spell_script_declares_matching_words() {
    let source = r#"
        local s = {}
        function s.target_requirement()
            return { type = "player", controller = "opponent", count = 1 }
        end
        function s.operation()
            return Game.Ops.ChainEach({ Game.Ops.DrawCards(2), 0 }, { Game.Ops.DrawCards(2), 1 })
        end
        return s
    "#;
    let runtime = ScriptRuntime::new();
    let (effect, _) = runtime.build_operation(source).unwrap().unwrap();
    let requirement = runtime
        .build_target_requirement(source)
        .unwrap()
        .expect("declares a target_requirement");
    let TargetSpec::Primitive(word) = &requirement else {
        panic!("{requirement:?}")
    };
    assert_eq!(
        word.filter,
        TargetFilter::Player {
            controller: Some(FilterController::Opponent)
        }
    );
    assert_eq!(
        effect,
        Some(AbilityEffect::Chain {
            entries: vec![AbilityEffect::DrawCards(2), AbilityEffect::DrawCards(2)],
            words: Some(vec![0, 1])
        })
    );
}
