use crate::{CardTypeSet, Subtype, SupertypeSet, ObjectId, SequenceNo, ColorSet, ManaCost};

/// CR 613.1 layers, with sublayers for layer 7 (power/toughness).
pub enum Layer {
    CopyEffects,            // 1
    ControlChangingEffects, // 2
    TextChanging,           // 3
    TypeChanging,           // 4
    ColorChanging,          // 5
    AbilityAdding,          // 6
    PowerToughnessCDA,      // 7a — characteristic-defining abilities
    PowerToughnessSetting,  // 7b
    PowerToughnessModifying,// 7c
    PowerToughnessCounters, // 7d
    PowerToughnessSwitching,// 7e
}

/// Not exhaustive in the original draft — MTG has more duration shapes than
/// "until end of turn / permanent". Adding: `UntilYourNextTurn` (e.g. many
/// fight/pump effects), `UntilEndOfCombat`, `WhileConditionHolds` (e.g. "as
/// long as you control a Forest" — re-evaluated continuously, not a fixed
/// end point), and `Custom` as a catch-all for unusual oracle wording that
/// doesn't fit a structured case yet.
pub enum EffectDuration {
    UntilEndOfTurn,
    UntilYourNextTurn,
    UntilEndOfCombat,
    WhileSourceOnBattlefield,
    UntilSourceLeaves,
    WhileConditionHolds,
    Permanent,
    Custom(String),
}

/// A registered continuous effect. `predicate`/`apply` are descriptions here;
/// concrete representation (closure vs script callback handle) is decided
/// in Phase 2.
pub struct ContinuousEffect {
    pub source: ObjectId,
    pub layer: Layer,
    /// Description of which objects this effect affects, refined at bind time.
    pub affected_predicate: String,
    pub timestamp: SequenceNo,
    pub duration: EffectDuration,
}

/// Consumption policy for a replacement/prevention effect (CR 614, 615).
pub enum ConsumptionPolicy {
    OneShot,
    Repeatable,
    /// e.g. damage prevention shields with a counter budget.
    LimitedUses(u32),
}

pub struct ReplacementEffect {
    pub source: ObjectId,
    /// Description of which event(s) this matches, refined at bind time.
    pub event_pattern: String,
    pub consumption: ConsumptionPolicy,
}

/// The live, in-game characteristics of an object — output shape of the
/// layer engine's query, computed on demand from `CardDefinition` base
/// values plus all applicable `ContinuousEffect`s. Never stored as
/// a mutable field on `GameObject`; recomputed whenever queried so layer
/// reordering/dependency changes are always reflected. Query function lands
/// in Phase 2.
pub struct EffectiveCharacteristics {
    pub name: String,
    pub mana_cost: ManaCost,
    pub colors: ColorSet,
    pub card_types: CardTypeSet,
    pub supertypes: SupertypeSet,
    pub subtypes: Vec<Subtype>,
    /// `None` for non-creatures (mirrors `CardDefinition`).
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    pub loyalty: Option<i32>,
    pub defense: Option<i32>,
}
