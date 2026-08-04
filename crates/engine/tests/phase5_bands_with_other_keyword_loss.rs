//! Phase 5 vertical slice: CR 702.22b — "if an effect causes a permanent to
//! lose banding, the permanent loses all 'bands with other' abilities as
//! well." Closes the last named gap in CR 702.22 (banding).
//!
//! The backlog's own prior note on this ("no effect in this engine removes
//! a keyword yet, untestable") was stale — `EffectPrimitive::RemoveKeyword`
//! already exists, real and Lua-exposed (`phase5/keyword-layer-fix-and-
//! remove-keyword`), just never logged back to that line.
//!
//! `layers::effective_characteristics`'s layer 6 fold already accumulates
//! `bands_with_other_subtypes`/`bands_with_other_supertypes` completely
//! independently of `keywords` — so `RemoveKeyword(BANDING)` already
//! cleared the `Banding` keyword itself, but left a creature's separate
//! "bands with other [quality]" grant untouched. Fixed with one check at
//! the end of that fold: if this object's own *printed-or-copied* keywords
//! include Banding (`def.keywords`, CR 702.22b's own precondition — banding
//! must actually have been *lost*, not simply never present) but the
//! *effective* set no longer does, both `bands_with_other_*` accumulators
//! are cleared, mirroring exactly what `RemoveAllAbilities` already does as
//! a side effect of clearing everything.
//!
//! Deliberately gated on *printed* Banding, not just "effective set lacks
//! it": `phase5_bands_with_other.rs`'s own `quality_band_legal_with_no_
//! banding_member_at_all` test proves a creature can legally have "bands
//! with other [quality]" via a self-registered static ability while never
//! printing Banding at all (CR 702.22c's second clause allows this — no
//! real card does it, but it isn't illegal) — that shape must NOT be
//! affected by this slice, since nothing was ever "lost."
//! `bands_with_other_survives_when_the_creature_never_had_banding_at_all`
//! below re-proves that boundary directly, from this slice's own angle.

use std::collections::HashMap;

use stackfish_engine::commands::CommandError as EngineCommandError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
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
        buyback_cost: None,
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn legendary_creature_definition(id: u64, name: &str, keywords: KeywordSet) -> CardDefinition {
    CardDefinition { supertypes: SupertypeSet::LEGENDARY, ..creature_definition(id, name, &[], keywords) }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn bands_with_other(source: ObjectId, quality: &str) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddBandsWithOther(Subtype(quality.to_string()))),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

fn bands_with_other_supertype(source: ObjectId, quality: SupertypeSet) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddBandsWithOtherSupertype(quality)),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

fn remove_keyword(source: ObjectId, keywords: KeywordSet) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter { controller: None, card_types: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None }),
        effect: EffectPayload::Primitive(EffectPrimitive::RemoveKeyword(keywords)),
        timestamp: SequenceNo(1),
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
// effective_characteristics unit level
// ---------------------------------------------------------------------

#[test]
fn losing_banding_via_remove_keyword_also_clears_bands_with_other_subtype() {
    let p0 = PlayerId(0);
    let elf_lord = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(elf_lord, creature_definition(1, "Elf Lord", &["Elf"], KeywordSet::BANDING), p0));
    state.continuous_effects.push(bands_with_other(elf_lord, "Elf"));
    let runtime = ScriptRuntime::new();

    // Before the RemoveKeyword effect lands: both Banding and bands-with-other apply.
    let before = effective_characteristics(&state, &runtime, elf_lord);
    assert!(before.keywords.contains(KeywordSet::BANDING));
    assert_eq!(before.bands_with_other_subtypes, vec![Subtype("Elf".to_string())]);

    state.continuous_effects.push(remove_keyword(elf_lord, KeywordSet::BANDING));
    let after = effective_characteristics(&state, &runtime, elf_lord);
    assert!(!after.keywords.contains(KeywordSet::BANDING), "Banding itself was removed");
    assert!(after.bands_with_other_subtypes.is_empty(), "CR 702.22b: losing Banding also strips bands-with-other");
}

#[test]
fn losing_banding_via_remove_keyword_also_clears_bands_with_other_supertype() {
    let p0 = PlayerId(0);
    let legend_lord = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(legend_lord, legendary_creature_definition(1, "Legend Lord", KeywordSet::BANDING), p0));
    state.continuous_effects.push(bands_with_other_supertype(legend_lord, SupertypeSet::LEGENDARY));
    state.continuous_effects.push(remove_keyword(legend_lord, KeywordSet::BANDING));

    let after = effective_characteristics(&state, &ScriptRuntime::new(), legend_lord);
    assert!(!after.keywords.contains(KeywordSet::BANDING));
    assert!(after.bands_with_other_supertypes.is_empty(), "CR 702.22b applies to the supertype form too");
}

#[test]
fn removing_an_unrelated_keyword_leaves_bands_with_other_untouched() {
    let p0 = PlayerId(0);
    let elf_lord = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(elf_lord, creature_definition(1, "Elf Lord", &["Elf"], KeywordSet::BANDING | KeywordSet::FLYING), p0),
    );
    state.continuous_effects.push(bands_with_other(elf_lord, "Elf"));
    // Strips Flying, not Banding - bands-with-other must survive.
    state.continuous_effects.push(remove_keyword(elf_lord, KeywordSet::FLYING));

    let after = effective_characteristics(&state, &ScriptRuntime::new(), elf_lord);
    assert!(after.keywords.contains(KeywordSet::BANDING), "Banding itself was never touched");
    assert!(!after.keywords.contains(KeywordSet::FLYING));
    assert_eq!(after.bands_with_other_subtypes, vec![Subtype("Elf".to_string())], "unrelated keyword loss must not clear bands-with-other");
}

#[test]
fn bands_with_other_survives_when_the_creature_never_had_banding_at_all() {
    let p0 = PlayerId(0);
    let elf_lord = ObjectId(0);
    let mut state = combat_state();
    // No Banding printed at all - CR 702.22c's second clause allows a bare
    // "bands with other [quality]" grant with no banding present anywhere.
    put_on_battlefield(&mut state, permanent(elf_lord, creature_definition(1, "Elf Lord", &["Elf"], KeywordSet::empty()), p0));
    state.continuous_effects.push(bands_with_other(elf_lord, "Elf"));

    let after = effective_characteristics(&state, &ScriptRuntime::new(), elf_lord);
    assert!(!after.keywords.contains(KeywordSet::BANDING));
    assert_eq!(
        after.bands_with_other_subtypes,
        vec![Subtype("Elf".to_string())],
        "CR 702.22b only fires on an actual loss - never having Banding isn't one"
    );
}

// ---------------------------------------------------------------------
// End to end: DeclareBands
// ---------------------------------------------------------------------

#[test]
fn a_quality_band_that_would_be_legal_is_rejected_once_banding_is_stripped() {
    let p0 = PlayerId(0);
    let elf_lord = ObjectId(0);
    let elf = ObjectId(1);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(elf_lord, creature_definition(1, "Elf Lord", &["Elf"], KeywordSet::BANDING), p0));
    put_on_battlefield(&mut state, permanent(elf, creature_definition(2, "Elf Warrior", &["Elf"], KeywordSet::empty()), p0));
    state.continuous_effects.push(bands_with_other(elf_lord, "Elf"));
    state.continuous_effects.push(remove_keyword(elf_lord, KeywordSet::BANDING));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(elf_lord, Target::Player(PlayerId(1))), (elf, Target::Player(PlayerId(1)))]);
    // elf_lord's own Banding was stripped, so its bands-with-other grant is
    // gone too, and elf itself never had Banding either - no path makes
    // this band legal any more.
    assert_illegal(declare_bands(&mut engine, p0, vec![vec![elf_lord, elf]]));
}

/// Same setup, minus the `RemoveKeyword` effect - proves the prior test's
/// rejection really is caused by this slice, not some other latent bug.
#[test]
fn the_same_band_is_legal_without_the_remove_keyword_effect() {
    let p0 = PlayerId(0);
    let elf_lord = ObjectId(0);
    let elf = ObjectId(1);

    let mut state = combat_state();
    put_on_battlefield(&mut state, permanent(elf_lord, creature_definition(1, "Elf Lord", &["Elf"], KeywordSet::BANDING), p0));
    put_on_battlefield(&mut state, permanent(elf, creature_definition(2, "Elf Warrior", &["Elf"], KeywordSet::empty()), p0));
    state.continuous_effects.push(bands_with_other(elf_lord, "Elf"));

    let mut engine = Engine::new(state);
    declare_attackers(&mut engine, p0, vec![(elf_lord, Target::Player(PlayerId(1))), (elf, Target::Player(PlayerId(1)))]);
    assert_legal_band(declare_bands(&mut engine, p0, vec![vec![elf_lord, elf]]));
}
