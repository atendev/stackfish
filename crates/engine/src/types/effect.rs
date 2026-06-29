use crate::{CardTypeSet, Subtype, SupertypeSet, ObjectId, SequenceNo, ColorSet, ManaCost, GameObject, GameEventKind, ScriptHandle};

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

/// A registered continuous effect. `affected_predicate` is a `ScriptHandle`
/// into the card's loaded Lua script (resolved via `ScriptRuntime` — see
/// `script.rs`) — decided in Phase 2's script-bridge pass; Phase 1 had this
/// as a `String` placeholder.
pub struct ContinuousEffect {
    pub source: ObjectId,
    pub layer: Layer,
    pub affected_predicate: ScriptHandle,
    pub timestamp: SequenceNo,
    pub duration: EffectDuration,
}

impl ContinuousEffect {
    /// Whether `object` is in this effect's affected set. Calling
    /// `affected_predicate` requires a `ScriptRuntime` — this signature will
    /// need one threaded through (or to move to `layers.rs` as a free
    /// function taking `&ScriptRuntime`) once that wiring is settled.
    pub fn applies_to(&self, object: &GameObject) -> bool {
        // TODO: invoke ScriptRuntime::invoke_condition (or a dedicated
        // predicate-invocation method) with self.affected_predicate and a
        // ScriptContext built from object/self.source - needs a
        // &ScriptRuntime parameter added to this signature, not available
        // from &self/&GameObject alone. See script.rs's invoke_* methods.
        todo!()
    }
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
    /// `ScriptHandle` into the card's loaded Lua script, same as
    /// `ContinuousEffect::affected_predicate` above.
    pub event_pattern: ScriptHandle,
    pub consumption: ConsumptionPolicy,
}

impl ReplacementEffect {
    pub fn matches(&self, event: &GameEventKind) -> bool {
        // TODO: same shape of gap as ContinuousEffect::applies_to above -
        // needs a &ScriptRuntime to invoke self.event_pattern against
        // event. Called from replacement::apply_replacements (replacement.rs)
        // once that wiring exists.
        todo!()
    }
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
