use stackfish_engine::*;

/// A triggered "when this dies, gain 1 life" ability on `source`, with ability id 0.
/// CR 700.4
pub fn dies_ability(source: ObjectId) -> RegisteredAbility {
    RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id: AbilityId(0),
        source,
        kind: AbilityKind::Triggered,
        condition: Some(ConditionSpec::Primitive(ConditionPrimitive::Dies)),
        cost: None,
        target: None,
        operation: ScriptHandle(0),
        effect: Some(AbilityEffect::GainLife(1)),
        intervening_if: None,
    }
}

/// One required "any target" target.
/// CR 115.4
pub fn any_target_requirement() -> TargetSpec {
    TargetSpec::Primitive(TargetRequirement {
        filter: TargetFilter::AnyTarget,
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    })
}

/// An `EffectFilter` that matches every object.
pub fn no_filter() -> EffectFilter {
    EffectFilter {
        controller: None,
        card_types: None,
        self_only: false,
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
    }
}
