//! Phase 5 vertical slice: CR 702.22b/c's "bands with other [quality]" -
//! `phase5_banding.rs`'s own second band-formation path, left unmodeled
//! there ("needs a parameter, same shape gap `KeywordSet::BANDING`'s own
//! doc comment names for Landwalk").
//!
//! Same shape as Landwalk (`EffectiveCharacteristics::landwalk_subtypes`, a
//! layer 6 `EffectPrimitive`): no printed field on `CardDefinition`, always
//! effect-granted via a self-registered static ability -
//! `EffectPrimitive::AddBandsWithOther(Subtype)`,
//! `EffectiveCharacteristics::bands_with_other_subtypes`. Enforcement lives
//! in `commands::PlayerActionKind::DeclareBands` as a second accepted shape
//! alongside the pre-existing plain-banding one (>=1 member with
//! `bands_with_other_subtypes` containing quality Q, every *other* member's
//! own subtypes include Q) - a band is legal if it matches either shape.
//!
//! CR 702.22j/k's *other* trigger condition is real now too
//! (`phase5/bands-with-other-split-trigger`): independent of any declared
//! band, if the creatures grouped by a block (an attacker's blockers, or a
//! blocker's forced-onto attackers) contain a creature X with "bands with
//! other [quality]" and another creature Y (Y != X) whose own subtypes
//! include that quality, the combat-damage split chooser flips exactly the
//! way a plain `BANDING` creature already flips it -
//! `commands::AssignCombatDamage`'s `has_bands_with_other_pair` helper,
//! checked alongside the pre-existing `BANDING`-keyword check.
//!
//! Deliberately narrow, matching Landwalk's own precedent: only
//! single-subtype ("quality") matching is modeled. CR 702.22c/m's
//! supertype-quality form ("bands with other Legends," i.e. Legendary) is
//! TODO, unmodeled.

use std::collections::HashMap;

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones, two_player_state as base_two_player_state};

fn creature_definition(id: u64, name: &str, subtypes: &[&str], keywords: KeywordSet) -> CardDefinition {
    creature_definition_pt(id, name, subtypes, keywords, 2, 2)
}

fn creature_definition_pt(
    id: u64,
    name: &str,
    subtypes: &[&str],
    keywords: KeywordSet,
    power: i32,
    toughness: i32,
) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: subtypes.iter().map(|s| Subtype(s.to_string())).collect(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords,
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

/// Directly-constructed "bands with other [quality]" grant, standing in for
/// the ETB-registered version - the Lua wiring (`Game.Effects.
/// AddBandsWithOther`) is proven separately, same convention
/// `phase5_landwalk.rs`'s `islandwalk` helper already establishes.
fn bands_with_other(source: ObjectId, quality: &str) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddBandsWithOther(Subtype(quality.to_string()))),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

fn combat_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::DeclareAttackers,
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield]))
    }
}

fn declare_attackers(engine: &mut Engine, player: PlayerId, attackers: Vec<(ObjectId, Target)>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareAttackers { attackers } })
        .expect("player holds priority");
    drain(engine)
}

fn declare_bands(engine: &mut Engine, player: PlayerId, bands: Vec<Vec<ObjectId>>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBands { bands } })
        .expect("player holds priority");
    drain(engine)
}

fn declare_blockers(engine: &mut Engine, player: PlayerId, blocks: Vec<(ObjectId, ObjectId)>) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::DeclareBlockers { blocks } })
        .expect("player holds priority");
    drain(engine)
}

fn assign_combat_damage(
    engine: &mut Engine,
    player: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::AssignCombatDamage { assignments } })
        .expect("turn-based action, no priority required");
    drain(engine)
}

fn pass_to_step(engine: &mut Engine, step: Step) {
    for _ in 0..20 {
        if engine.state().step == step {
            return;
        }
        let active = engine.state().priority_player;
        engine.submit_action(PlayerAction { player: active, kind: PlayerActionKind::PassPriority }).expect("priority window");
        drain(engine);
    }
    panic!("never reached {step:?} - state.step = {:?}", engine.state().step);
}

fn assert_illegal(status: StepStatus) {
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))), "expected an illegal action, got {status:?}");
}

fn assert_legal_band(status: StepStatus) {
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "expected a legal band declaration, got {status:?}");
}

fn assert_accepted(status: StepStatus) {
    assert!(!matches!(status, StepStatus::Error(_)), "expected the split to be accepted, got {status:?}");
}

// ---------------------------------------------------------------------
// CR 702.22c's second path: "bands with other [quality]"
// ---------------------------------------------------------------------

#[test]
fn quality_band_legal_with_no_banding_member_at_all() {
    let p0 = PlayerId(0);
    let elf_lord = ObjectId(0);
    let elf = ObjectId(1);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(elf_lord, creature_definition(1, "Elf Lord", &["Elf"], KeywordSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(elf, creature_definition(2, "Elf Warrior", &["Elf"], KeywordSet::empty()), p0));
    state.continuous_effects.push(bands_with_other(elf_lord, "Elf"));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(elf_lord, Target::Player(PlayerId(1))), (elf, Target::Player(PlayerId(1)))]);
    // Neither creature has plain Banding - only the quality path can make
    // this legal, and it does (elf_lord has bands-with-other Elf, elf is an
    // Elf).
    assert_legal_band(declare_bands(&mut engine, p0, vec![vec![elf_lord, elf]]));
}

#[test]
fn quality_band_rejects_a_bandmate_missing_the_quality() {
    let p0 = PlayerId(0);
    let elf_lord = ObjectId(0);
    let bear = ObjectId(1);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(elf_lord, creature_definition(1, "Elf Lord", &["Elf"], KeywordSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(bear, creature_definition(2, "Bear", &["Bear"], KeywordSet::empty()), p0));
    state.continuous_effects.push(bands_with_other(elf_lord, "Elf"));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(elf_lord, Target::Player(PlayerId(1))), (bear, Target::Player(PlayerId(1)))]);
    // bear isn't an Elf - doesn't satisfy elf_lord's named quality, and
    // neither creature has plain Banding either.
    assert_illegal(declare_bands(&mut engine, p0, vec![vec![elf_lord, bear]]));
}

#[test]
fn quality_band_rejects_when_no_member_has_bands_with_other() {
    let p0 = PlayerId(0);
    let elf_a = ObjectId(0);
    let elf_b = ObjectId(1);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(elf_a, creature_definition(1, "Elf A", &["Elf"], KeywordSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(elf_b, creature_definition(2, "Elf B", &["Elf"], KeywordSet::empty()), p0));
    // No bands_with_other grant on either creature this time.

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(elf_a, Target::Player(PlayerId(1))), (elf_b, Target::Player(PlayerId(1)))]);
    assert_illegal(declare_bands(&mut engine, p0, vec![vec![elf_a, elf_b]]));
}

#[test]
fn quality_band_allows_any_number_of_matching_members() {
    let p0 = PlayerId(0);
    let elf_lord = ObjectId(0);
    let elf_a = ObjectId(1);
    let elf_b = ObjectId(2);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(elf_lord, creature_definition(1, "Elf Lord", &["Elf"], KeywordSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(elf_a, creature_definition(2, "Elf A", &["Elf"], KeywordSet::empty()), p0));
    put_on_battlefield(&mut state, permanent(elf_b, creature_definition(3, "Elf B", &["Elf"], KeywordSet::empty()), p0));
    state.continuous_effects.push(bands_with_other(elf_lord, "Elf"));

    let mut engine = Engine::new(state);
    declare_attackers(
        &mut engine,
        p0,
        vec![
            (elf_lord, Target::Player(PlayerId(1))),
            (elf_a, Target::Player(PlayerId(1))),
            (elf_b, Target::Player(PlayerId(1))),
        ],
    );
    // Unlike the plain-banding path's "at most one non-banding member" cap,
    // the quality path allows any number of quality-matching members.
    assert_legal_band(declare_bands(&mut engine, p0, vec![vec![elf_lord, elf_a, elf_b]]));
}

#[test]
fn plain_banding_path_unaffected_by_this_slice() {
    let p0 = PlayerId(0);
    let banding_creature = ObjectId(0);
    let plain_creature = ObjectId(1);

    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(banding_creature, creature_definition(1, "Bander", &[], KeywordSet::BANDING), p0),
    );
    put_on_battlefield(&mut state, permanent(plain_creature, creature_definition(2, "Plain", &[], KeywordSet::empty()), p0));

    let mut engine = Engine::new(state);
    declare_attackers(
        &mut engine,
        p0,
        vec![(banding_creature, Target::Player(PlayerId(1))), (plain_creature, Target::Player(PlayerId(1)))],
    );
    assert_legal_band(declare_bands(&mut engine, p0, vec![vec![banding_creature, plain_creature]]));
}

// ---------------------------------------------------------------------
// CR 702.22j/k's other trigger: a quality pair among the blockers/attackers
// flips the combat-damage split chooser, with no `BANDING` keyword and no
// declared band involved at all.
// ---------------------------------------------------------------------

#[test]
fn attacker_double_blocked_by_a_bands_with_other_pair_needs_the_defending_players_choice() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let elf_lord = ObjectId(1);
    let elf = ObjectId(2);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition_pt(1, "Attacker", &[], KeywordSet::empty(), 4, 4), p0));
    put_on_battlefield(&mut state, permanent(elf_lord, creature_definition_pt(2, "Elf Lord", &["Elf"], KeywordSet::empty(), 1, 5), p1));
    put_on_battlefield(&mut state, permanent(elf, creature_definition_pt(3, "Elf", &["Elf"], KeywordSet::empty(), 1, 5), p1));
    // Neither blocker has plain Banding - only the quality pair (elf_lord's
    // bands-with-other Elf + elf's own Elf subtype) can flip the chooser.
    state.continuous_effects.push(bands_with_other(elf_lord, "Elf"));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, vec![(elf_lord, attacker), (elf, attacker)]);
    pass_to_step(&mut engine, Step::CombatDamage);

    // The active player no longer gets to choose this split - the
    // attacker's blockers contain a bands-with-other/quality pair.
    assert_illegal(assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(elf_lord, 2), (elf, 2)])]));
    assert_accepted(assign_combat_damage(&mut engine, p1, vec![(attacker, vec![(elf_lord, 2), (elf, 2)])]));
}

#[test]
fn double_blocked_attacker_with_no_quality_pair_still_defaults_to_active_player() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let elf_lord = ObjectId(1);
    let bear = ObjectId(2);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(attacker, creature_definition_pt(1, "Attacker", &[], KeywordSet::empty(), 4, 4), p0));
    put_on_battlefield(&mut state, permanent(elf_lord, creature_definition_pt(2, "Elf Lord", &["Elf"], KeywordSet::empty(), 1, 5), p1));
    put_on_battlefield(&mut state, permanent(bear, creature_definition_pt(3, "Bear", &["Bear"], KeywordSet::empty(), 1, 5), p1));
    // elf_lord's named quality (Elf) matches nothing else blocking this
    // attacker - bear isn't an Elf, so no pair exists and the chooser stays
    // the active player, same as any ordinary multi-block.
    state.continuous_effects.push(bands_with_other(elf_lord, "Elf"));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(attacker, Target::Player(p1))]);
    pass_to_step(&mut engine, Step::DeclareBlockers);
    declare_blockers(&mut engine, p1, vec![(elf_lord, attacker), (bear, attacker)]);
    pass_to_step(&mut engine, Step::CombatDamage);

    assert_illegal(assign_combat_damage(&mut engine, p1, vec![(attacker, vec![(elf_lord, 2), (bear, 2)])]));
    assert_accepted(assign_combat_damage(&mut engine, p0, vec![(attacker, vec![(elf_lord, 2), (bear, 2)])]));
}
