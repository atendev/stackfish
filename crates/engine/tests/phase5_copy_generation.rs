//! CR 707.9g: `GameState::linked_ability_generation` tags an `AbilityId` with a `copy_generation`, and `triggers::collect_triggers` stops a tagged ability from triggering once another copy effect bumps the source past it.
//! `GameObject::copy_generation` itself is bumped by `BecomeCopy` and `EntersAsCopy`, which have their own assertions elsewhere.
// TODO: no `CopyExceptions` shape grants a new ability when copying, so these test the enforcement directly with no producer

use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::triggers::collect_triggers;
use stackfish_engine::*;
use stackfish_testutil::{card_object, two_player_state, zones};

fn creature_definition(name: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(String::new()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn linked_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::Always)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    }
}

fn any_event() -> GameEvent {
    GameEvent {
        event_id: EventId(0),
        sequence_no: SequenceNo(0),
        kind: GameEventKind::CardsScried { player: PlayerId(0), kept_on_top: Vec::new(), put_bottom: Vec::new() },
        caused_by_command: None,
        parent_event: None,
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    two_player_state(zones(p0, p1, &[ZoneKind::Battlefield]))
}

// CR 707.9g
#[test]
fn a_tagged_ability_matches_while_its_source_is_still_at_the_granted_generation() {
    let source = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, card_object(source, creature_definition("Watcher"), PlayerId(0), ZoneKind::Battlefield));
    state.abilities.push(linked_ability(source));
    state.linked_ability_generation.insert(AbilityId(0), 0);
    let runtime = ScriptRuntime::new();

    let (pending, _) = collect_triggers(&state, &any_event(), &runtime, &[]);
    assert_eq!(pending.len(), 1, "granted generation (0) still matches the source's current generation (0)");
}

// CR 707.9g
// Once another copy effect applies, the tagged ability no longer triggers.
#[test]
fn a_tagged_ability_stops_matching_once_its_source_is_copied_again() {
    let source = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, card_object(source, creature_definition("Watcher"), PlayerId(0), ZoneKind::Battlefield));
    state.abilities.push(linked_ability(source));
    // The source is at generation 0 when granted but has since become a copy again, simulated directly because this tests collect_triggers' consult of the tag.
    state.linked_ability_generation.insert(AbilityId(0), 0);
    state.object_mut(source).unwrap().copy_generation = 1;
    let runtime = ScriptRuntime::new();

    let (pending, _) = collect_triggers(&state, &any_event(), &runtime, &[]);
    assert!(pending.is_empty(), "the source's generation (1) has moved past the tagged generation (0) - superseded");
}

// An untagged ability is unaffected regardless of copy_generation, since the mechanism is opt-in per ability.
#[test]
fn an_untagged_ability_matches_regardless_of_its_sources_copy_generation() {
    let source = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, card_object(source, creature_definition("Watcher"), PlayerId(0), ZoneKind::Battlefield));
    state.abilities.push(linked_ability(source));
    state.object_mut(source).unwrap().copy_generation = 5;
    let runtime = ScriptRuntime::new();

    let (pending, _) = collect_triggers(&state, &any_event(), &runtime, &[]);
    assert_eq!(pending.len(), 1, "no tag at all - copy_generation is irrelevant to this ability");
}
