//! CR 702.22b: a permanent that loses banding loses all its "bands with other" abilities.
//! `layers::effective_characteristics` clears both `bands_with_other_*` accumulators when printed Banding is lost, but not when it was never present (CR 702.22c).

use std::collections::HashMap;

use stackfish_engine::commands::Rejected;
use stackfish_engine::engine::ActionError;
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, two_player_state as base_two_player_state, zones};

fn creature_definition(
    id: u64,
    name: &str,
    subtypes: &[&str],
    keywords: KeywordSet,
) -> CardDefinition {
    CardDefinition {
        definition_id: id,
        name: name.to_string(),
        mana_cost: ManaCost {
            symbols: Vec::new(),
        },
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
    CardDefinition {
        supertypes: SupertypeSet::LEGENDARY,
        ..creature_definition(id, name, &[], keywords)
    }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state
        .zone_mut(ZoneKind::Battlefield, None)
        .unwrap()
        .push_bottom(id);
    state.objects.insert(id, object);
}

fn bands_with_other(source: ObjectId, quality: &str) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter {
            controller: None,
            card_types: None,
            self_only: true,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
        }),
        effect: EffectPayload::Primitive(EffectPrimitive::AddBandsWithOther(Subtype(
            quality.to_string(),
        ))),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition: None,
    }
}

fn bands_with_other_supertype(source: ObjectId, quality: SupertypeSet) -> ContinuousEffect {
    ContinuousEffect {
        source,
        layer: Layer::AbilityAdding,
        affected: AffectedFilter::Primitive(EffectFilter {
            controller: None,
            card_types: None,
            self_only: true,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
        }),
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
        affected: AffectedFilter::Primitive(EffectFilter {
            controller: None,
            card_types: None,
            self_only: true,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
        }),
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

fn declare_attackers(
    engine: &mut Engine,
    player: PlayerId,
    attackers: Vec<(ObjectId, Target)>,
) -> Result<StepStatus, ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::DeclareAttackers { attackers },
    })?;
    Ok(drain(engine))
}

fn declare_bands(
    engine: &mut Engine,
    player: PlayerId,
    bands: Vec<Vec<ObjectId>>,
) -> Result<StepStatus, ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::DeclareBands { bands },
    })?;
    Ok(drain(engine))
}

fn assert_illegal(status: Result<StepStatus, ActionError>) {
    assert!(
        matches!(
            status,
            Err(ActionError::Rejected(Rejected::IllegalAction(_)))
        ),
        "expected an illegal action, got {status:?}"
    );
}

fn assert_legal_band(status: Result<StepStatus, ActionError>) {
    let status = status.expect("the engine should accept the action");
    assert!(
        matches!(status, StepStatus::AwaitingAction { .. }),
        "expected a legal band declaration, got {status:?}"
    );
}

#[test]
fn losing_banding_via_remove_keyword_also_clears_bands_with_other_subtype() {
    let p0 = PlayerId(0);
    let elf_lord = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(
            elf_lord,
            creature_definition(1, "Elf Lord", &["Elf"], KeywordSet::BANDING),
            p0,
        ),
    );
    state
        .continuous_effects
        .push(bands_with_other(elf_lord, "Elf"));
    let runtime = ScriptRuntime::new();

    // Before the RemoveKeyword effect lands: both Banding and bands-with-other apply.
    let before = effective_characteristics(&state, &runtime, elf_lord);
    assert!(before.keywords.contains(KeywordSet::BANDING));
    assert_eq!(
        before.bands_with_other_subtypes,
        vec![Subtype("Elf".to_string())]
    );

    state
        .continuous_effects
        .push(remove_keyword(elf_lord, KeywordSet::BANDING));
    let after = effective_characteristics(&state, &runtime, elf_lord);
    assert!(
        !after.keywords.contains(KeywordSet::BANDING),
        "Banding itself was removed"
    );
    assert!(
        after.bands_with_other_subtypes.is_empty(),
        "CR 702.22b: losing Banding also strips bands-with-other"
    );
}

#[test]
fn losing_banding_via_remove_keyword_also_clears_bands_with_other_supertype() {
    let p0 = PlayerId(0);
    let legend_lord = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(
            legend_lord,
            legendary_creature_definition(1, "Legend Lord", KeywordSet::BANDING),
            p0,
        ),
    );
    state.continuous_effects.push(bands_with_other_supertype(
        legend_lord,
        SupertypeSet::LEGENDARY,
    ));
    state
        .continuous_effects
        .push(remove_keyword(legend_lord, KeywordSet::BANDING));

    let after = effective_characteristics(&state, &ScriptRuntime::new(), legend_lord);
    assert!(!after.keywords.contains(KeywordSet::BANDING));
    assert!(
        after.bands_with_other_supertypes.is_empty(),
        "CR 702.22b applies to the supertype form too"
    );
}

#[test]
fn removing_an_unrelated_keyword_leaves_bands_with_other_untouched() {
    let p0 = PlayerId(0);
    let elf_lord = ObjectId(0);
    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(
            elf_lord,
            creature_definition(
                1,
                "Elf Lord",
                &["Elf"],
                KeywordSet::BANDING | KeywordSet::FLYING,
            ),
            p0,
        ),
    );
    state
        .continuous_effects
        .push(bands_with_other(elf_lord, "Elf"));
    // Strips Flying, not Banding - bands-with-other must survive.
    state
        .continuous_effects
        .push(remove_keyword(elf_lord, KeywordSet::FLYING));

    let after = effective_characteristics(&state, &ScriptRuntime::new(), elf_lord);
    assert!(
        after.keywords.contains(KeywordSet::BANDING),
        "Banding itself was never touched"
    );
    assert!(!after.keywords.contains(KeywordSet::FLYING));
    assert_eq!(
        after.bands_with_other_subtypes,
        vec![Subtype("Elf".to_string())],
        "unrelated keyword loss must not clear bands-with-other"
    );
}

#[test]
fn bands_with_other_survives_when_the_creature_never_had_banding_at_all() {
    let p0 = PlayerId(0);
    let elf_lord = ObjectId(0);
    let mut state = combat_state();
    // No Banding printed: CR 702.22c allows a bare "bands with other" grant.
    put_on_battlefield(
        &mut state,
        permanent(
            elf_lord,
            creature_definition(1, "Elf Lord", &["Elf"], KeywordSet::empty()),
            p0,
        ),
    );
    state
        .continuous_effects
        .push(bands_with_other(elf_lord, "Elf"));

    let after = effective_characteristics(&state, &ScriptRuntime::new(), elf_lord);
    assert!(!after.keywords.contains(KeywordSet::BANDING));
    assert_eq!(
        after.bands_with_other_subtypes,
        vec![Subtype("Elf".to_string())],
        "CR 702.22b only fires on an actual loss - never having Banding isn't one"
    );
}

#[test]
fn a_quality_band_that_would_be_legal_is_rejected_once_banding_is_stripped() {
    let p0 = PlayerId(0);
    let elf_lord = ObjectId(0);
    let elf = ObjectId(1);

    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(
            elf_lord,
            creature_definition(1, "Elf Lord", &["Elf"], KeywordSet::BANDING),
            p0,
        ),
    );
    put_on_battlefield(
        &mut state,
        permanent(
            elf,
            creature_definition(2, "Elf Warrior", &["Elf"], KeywordSet::empty()),
            p0,
        ),
    );
    state
        .continuous_effects
        .push(bands_with_other(elf_lord, "Elf"));
    state
        .continuous_effects
        .push(remove_keyword(elf_lord, KeywordSet::BANDING));

    let mut engine = Engine::new(state);
    declare_attackers(
        &mut engine,
        p0,
        vec![
            (elf_lord, Target::Player(PlayerId(1))),
            (elf, Target::Player(PlayerId(1))),
        ],
    )
    .unwrap();
    // elf_lord's Banding was stripped and elf never had it, so no path makes this band legal.
    assert_illegal(declare_bands(&mut engine, p0, vec![vec![elf_lord, elf]]));
}

// Same setup without `RemoveKeyword`, so the rejection above comes from the keyword loss.
#[test]
fn the_same_band_is_legal_without_the_remove_keyword_effect() {
    let p0 = PlayerId(0);
    let elf_lord = ObjectId(0);
    let elf = ObjectId(1);

    let mut state = combat_state();
    put_on_battlefield(
        &mut state,
        permanent(
            elf_lord,
            creature_definition(1, "Elf Lord", &["Elf"], KeywordSet::BANDING),
            p0,
        ),
    );
    put_on_battlefield(
        &mut state,
        permanent(
            elf,
            creature_definition(2, "Elf Warrior", &["Elf"], KeywordSet::empty()),
            p0,
        ),
    );
    state
        .continuous_effects
        .push(bands_with_other(elf_lord, "Elf"));

    let mut engine = Engine::new(state);
    declare_attackers(
        &mut engine,
        p0,
        vec![
            (elf_lord, Target::Player(PlayerId(1))),
            (elf, Target::Player(PlayerId(1))),
        ],
    )
    .unwrap();
    assert_legal_band(declare_bands(&mut engine, p0, vec![vec![elf_lord, elf]]));
}
