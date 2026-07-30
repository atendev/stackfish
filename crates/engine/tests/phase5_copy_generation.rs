//! Phase 5 vertical slice: CR 707.9e/g's shared "has another copy effect
//! applied to this object since a given earlier one did?" question -
//! `GameObject::copy_generation`, bumped by both real copy-application entry
//! points (`AbilityEffect::BecomeCopy` in phase5_layer_engine_copy.rs,
//! `Engine::submit_copy_choice`'s `EntersAsCopy` path in
//! phase5_clone_enters_as_copy.rs - both already have their own bump
//! assertions alongside their existing coverage).
//!
//! This file covers the other real half: `GameState::linked_ability_
//! generation`, a side-channel `AbilityId -> u32` tag (not a field on
//! `RegisteredAbility` itself - 100+ construction sites, no `Default` impl)
//! consulted by `triggers::collect_triggers` before a tagged ability is
//! allowed to match at all. CR 707.9g: an ability granted by a copy effect
//! stops triggering once *another* copy effect supersedes it (bumps the same
//! object's `copy_generation` past the value the ability was tagged with).
//!
//! No producer tags an ability this way yet - no `CopyExceptions` shape in
//! this engine grants a brand-new ability as part of copying (only
//! characteristic-modification exceptions exist so far, see that type's own
//! doc comment). These are direct tests of the enforcement mechanism itself,
//! same "real, tested, no producer wired in yet" posture `HandRevealModifier`
//! already established for the same reason.

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
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn linked_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
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

/// CR 707.9g: an ability tagged with the `copy_generation` it was granted
/// under still matches as long as its source's own current `copy_generation`
/// hasn't moved past that value - the ordinary, overwhelmingly common case
/// (an ability granted by the only copy effect that's ever applied).
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

/// CR 707.9g's actual rule: once *another* copy effect applies to the same
/// object (bumping its `copy_generation` past the tagged value), the tagged
/// ability no longer triggers at all - "the ability doesn't trigger."
#[test]
fn a_tagged_ability_stops_matching_once_its_source_is_copied_again() {
    let source = ObjectId(0);
    let mut state = base_state();
    put_on_battlefield(&mut state, card_object(source, creature_definition("Watcher"), PlayerId(0), ZoneKind::Battlefield));
    state.abilities.push(linked_ability(source));
    // Granted when the source was at generation 0 - but the source has since
    // become a copy again (a real BecomeCopy/EntersAsCopy application would
    // bump this; simulated directly here since this test targets
    // collect_triggers's own consult of the tag, not the bump sites
    // themselves, which have their own direct tests elsewhere).
    state.linked_ability_generation.insert(AbilityId(0), 0);
    state.object_mut(source).unwrap().copy_generation = 1;
    let runtime = ScriptRuntime::new();

    let (pending, _) = collect_triggers(&state, &any_event(), &runtime, &[]);
    assert!(pending.is_empty(), "the source's generation (1) has moved past the tagged generation (0) - superseded");
}

/// An untagged ability (the overwhelming majority - nothing in
/// `linked_ability_generation` names it) is completely unaffected by this
/// check regardless of its source's `copy_generation` - the mechanism is
/// opt-in per ability, not a blanket restriction on every copied object's
/// abilities.
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
