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
//! Deliberately narrow, matching Landwalk's own precedent: only
//! single-subtype ("quality") matching is modeled. CR 702.22c/m's
//! supertype-quality form ("bands with other Legends," i.e. Legendary) is
//! TODO, unmodeled. CR 702.22j/k's *other* trigger condition - a blocking
//! creature with bands-with-other[quality] alongside another matching-
//! quality blocker flips the combat-damage split chooser independent of any
//! declared band at all - is a separate mechanic (pure blocker-side check
//! at the damage step) and is out of scope for this slice; named as a
//! follow-up, not silently dropped.

use std::collections::HashMap;

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones, two_player_state as base_two_player_state};

fn creature_definition(id: u64, name: &str, subtypes: &[&str], keywords: KeywordSet) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: subtypes.iter().map(|s| Subtype(s.to_string())).collect(),
        oracle_text: String::new(),
        power: Some(PTValue::Fixed(2)),
        toughness: Some(PTValue::Fixed(2)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::ByKey(name.to_string()),
        keywords,
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
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

fn assert_illegal(status: StepStatus) {
    assert!(matches!(status, StepStatus::Error(EngineCommandError::IllegalAction(_))), "expected an illegal action, got {status:?}");
}

fn assert_legal_band(status: StepStatus) {
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "expected a legal band declaration, got {status:?}");
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
