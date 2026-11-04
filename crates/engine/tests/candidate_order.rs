//! Candidate lists built by walking `GameState::objects` reach a host as Decision candidates.
//! Each one must come out in ascending `ObjectId` order, or one seed could offer different lists.

use std::collections::{HashMap, VecDeque};

use stackfish_engine::decision::{Candidate, ReplyShape};
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::replacement;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{
    card_object, creature_definition, drain, hand_battlefield_graveyard_zones,
    minimal_card_definition, pass_both_and_drain, pay_free_and_drain, put_on_battlefield,
    two_player_state as base_two_player_state,
};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const COUNT: u64 = 24;

const DEVOUR_TWO: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.WouldMoveZone(nil, "Battlefield"))
    e:SetPayload(Game.ReplacementPayloads.EntersWithCountersDevour(2))
    e:SetAffected(Game.Affected.Self)
    e:Register()
end
return s
"#;

/// `COUNT` distinct ids below the 1000 the engine starts minting at, in an order far from sorted.
fn scattered_ids() -> Vec<ObjectId> {
    (0..COUNT)
        .map(|i| ObjectId(100 + (i * 37) % 97 * 3))
        .collect()
}

fn assert_ascending(ids: &[ObjectId]) {
    let mut sorted = ids.to_vec();
    sorted.sort();
    assert_eq!(ids, sorted, "candidates must follow object id order");
}

fn state() -> GameState {
    let mut zones = hand_battlefield_graveyard_zones(P0, P1);
    zones.push(Zone::new(ZoneKind::Library, Some(P0)));
    zones.push(Zone::new(ZoneKind::Library, Some(P1)));
    GameState {
        step: Step::PreCombatMain,
        last_active_turn_number: HashMap::from([(P0, 1), (P1, 1)]),
        ..base_two_player_state(zones)
    }
}

fn subtyped(definition: CardDefinition, subtype: &str) -> CardDefinition {
    CardDefinition {
        subtypes: vec![Subtype(subtype.to_string())],
        ..definition
    }
}

fn self_entry_effect(source: ObjectId, payload: ReplacementPayload) -> ReplacementEffect {
    ReplacementEffect {
        source,
        spec: ReplacementSpec::Primitive {
            filter: EventFilter::WouldMoveZone {
                from: None,
                to: Some(ZoneKind::Battlefield),
                affected: EffectFilter {
                    controller: None,
                    card_types: None,
                    self_only: true,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                },
            },
            payload,
        },
        consumption: ConsumptionPolicy::Repeatable,
    }
}

fn creatures_on_battlefield(state: &mut GameState, controller: PlayerId) {
    for id in scattered_ids() {
        put_on_battlefield(
            state,
            card_object(
                id,
                creature_definition("Bear"),
                controller,
                ZoneKind::Battlefield,
            ),
        );
    }
}

fn pick_ids(engine: &mut Engine) -> Vec<ObjectId> {
    let decision = engine.open_decision().expect("a Decision is open");
    let ReplyShape::Pick { candidates, .. } = decision.shape else {
        panic!("expected a Pick Decision, got {:?}", decision.shape)
    };
    candidates
        .into_iter()
        .map(|c| match c {
            Candidate::Target(Target::Object(id)) => id,
            other => panic!("expected an object candidate, got {other:?}"),
        })
        .collect()
}

fn cast(engine: &mut Engine, card: ObjectId) {
    engine
        .submit_action(PlayerAction {
            player: P0,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets: Vec::new(),
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("P0 holds priority in its main phase with an empty stack");
}

// CR 702.82a
#[test]
fn devour_candidates_come_in_ascending_id_order() {
    let mut state = state();
    let entrant = ObjectId(5);
    put_on_battlefield(
        &mut state,
        card_object(
            entrant,
            creature_definition("Beast"),
            P0,
            ZoneKind::Battlefield,
        ),
    );
    state.replacement_effects.push(self_entry_effect(
        entrant,
        ReplacementPayload::EntersWithCountersDevour {
            per_creature: 2,
            quality: None,
        },
    ));
    creatures_on_battlefield(&mut state, P0);

    let (_, candidates) = replacement::pending_devour_choice(
        &state,
        &ScriptRuntime::new(),
        entrant,
        ZoneKind::Hand,
        P0,
    )
    .expect("P0 controls creatures");
    assert_eq!(candidates.len(), COUNT as usize + 1);
    assert_ascending(&candidates);
}

// CR 702.38a
#[test]
fn amplify_candidates_come_in_ascending_id_order() {
    let mut state = state();
    let entrant = ObjectId(5);
    put_on_battlefield(
        &mut state,
        card_object(
            entrant,
            subtyped(creature_definition("Elf Lord"), "Elf"),
            P0,
            ZoneKind::Battlefield,
        ),
    );
    state.replacement_effects.push(self_entry_effect(
        entrant,
        ReplacementPayload::EntersWithCountersAmplify { per_card: 1 },
    ));
    for id in scattered_ids() {
        state.objects.insert(
            id,
            card_object(
                id,
                subtyped(creature_definition("Elf"), "Elf"),
                P0,
                ZoneKind::Hand,
            ),
        );
        state
            .zone_mut(ZoneKind::Hand, Some(P0))
            .unwrap()
            .push_bottom(id);
    }

    let (_, candidates) = replacement::pending_amplify_choice(
        &state,
        &ScriptRuntime::new(),
        entrant,
        ZoneKind::Hand,
        P0,
    )
    .expect("P0's hand shares the Elf subtype");
    assert_eq!(candidates.len(), COUNT as usize);
    assert_ascending(&candidates);
}

// CR 707.6
#[test]
fn copy_candidates_come_in_ascending_id_order() {
    let mut state = state();
    let entrant = ObjectId(5);
    put_on_battlefield(
        &mut state,
        card_object(
            entrant,
            creature_definition("Clone"),
            P0,
            ZoneKind::Battlefield,
        ),
    );
    state.replacement_effects.push(self_entry_effect(
        entrant,
        ReplacementPayload::EntersAsCopy(CopyExceptions {
            set_power_toughness: None,
            add_keywords: KeywordSet::empty(),
            retain_name: false,
            retain_color: false,
            grant_recurring_copy_trigger: false,
            enters_with_additional_counters: None,
        }),
    ));
    creatures_on_battlefield(&mut state, P1);

    let (_, candidates) =
        replacement::pending_copy_choice(&state, &ScriptRuntime::new(), entrant, ZoneKind::Hand)
            .expect("creatures are on the battlefield");
    assert_eq!(candidates.len(), COUNT as usize + 1);
    assert_ascending(&candidates);
}

// CR 702.82a
#[test]
fn a_devour_decision_lists_its_candidates_in_ascending_id_order() {
    let mut state = state();
    let card = ObjectId(5);
    let devourer = CardDefinition {
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        script_ref: ScriptReference::Inline(DEVOUR_TWO.to_string()),
        ..minimal_card_definition(
            2,
            "Gluttonous Beast",
            "gluttonous_beast",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::CREATURE,
        )
    };
    state
        .objects
        .insert(card, card_object(card, devourer, P0, ZoneKind::Hand));
    state
        .zone_mut(ZoneKind::Hand, Some(P0))
        .unwrap()
        .push_bottom(card);
    creatures_on_battlefield(&mut state, P0);

    let mut engine = Engine::new(state);
    cast(&mut engine, card);
    pay_free_and_drain(&mut engine);
    let status = pass_both_and_drain(&mut engine, P0, P1);
    assert!(
        matches!(status, StepStatus::AwaitingDevourChoice { .. }),
        "expected a Devour pause, got {status:?}"
    );

    let ids = pick_ids(&mut engine);
    assert_eq!(ids.len(), COUNT as usize + 1);
    assert_ascending(&ids);
}

// CR 301.5e
#[test]
fn an_entry_attach_decision_lists_its_candidates_in_ascending_id_order() {
    let mut state = state();
    let sword = ObjectId(10);
    let spell = ObjectId(2);
    state.objects.insert(
        sword,
        card_object(
            sword,
            subtyped(
                minimal_card_definition(
                    3,
                    "Test Sword",
                    "test_sword",
                    ManaCost {
                        symbols: Vec::new(),
                    },
                    CardTypeSet::ARTIFACT,
                ),
                "Equipment",
            ),
            P0,
            ZoneKind::Library,
        ),
    );
    state
        .zone_mut(ZoneKind::Library, Some(P0))
        .unwrap()
        .contents = VecDeque::from([sword]);
    let search = CardDefinition {
        effect: Some(AbilityEffect::Search {
            filter: FilterSpec {
                card_types: None,
                supertypes: None,
                subtypes: None,
                controller: None,
                random: false,
                extra: None,
            },
            count: 1,
            destination: SearchDestination::Battlefield {
                tapped: false,
                enters_attached: true,
            },
            shuffle_after: true,
            reveal: false,
        }),
        ..minimal_card_definition(
            4,
            "Test Search Spell",
            "test_search_attach",
            ManaCost {
                symbols: Vec::new(),
            },
            CardTypeSet::SORCERY,
        )
    };
    state
        .objects
        .insert(spell, card_object(spell, search, P0, ZoneKind::Hand));
    state
        .zone_mut(ZoneKind::Hand, Some(P0))
        .unwrap()
        .push_bottom(spell);
    creatures_on_battlefield(&mut state, P0);

    let mut engine = Engine::new(state);
    cast(&mut engine, spell);
    pay_free_and_drain(&mut engine);
    pass_both_and_drain(&mut engine, P0, P1);
    engine
        .submit_search_choice(vec![sword])
        .expect("the sword is the only card in the library");
    let status = drain(&mut engine);
    assert!(
        matches!(status, StepStatus::AwaitingEntryAttachChoice { .. }),
        "expected an entry attach pause, got {status:?}"
    );

    let ids = pick_ids(&mut engine);
    assert_eq!(ids.len(), COUNT as usize);
    assert_ascending(&ids);
}
