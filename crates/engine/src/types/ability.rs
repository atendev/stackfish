use crate::types::ids::{ObjectId, ScriptHandle};

/// Per ygopro's effect lifecycle (`docs/ygo_pro_reference.md`), generalized
/// for MTG: condition/cost/target/operation are separate script callbacks
/// rather than one monolithic function, so the engine can check (e.g.)
/// condition without committing to cost/target/operation.
pub enum AbilityKind {
    Triggered,
    Activated,
    Static,
}

/// A card's registered ability, as set up by its script's `initial_effect`-
/// equivalent (`Game.RegisterTriggeredAbility` etc — see `script_api.rs`).
/// Lives in `GameState.abilities` as a flat list (same pattern as
/// `continuous_effects`/`replacement_effects`), not nested under
/// `GameObject`, so the engine can scan all registered abilities against an
/// event/timing window without walking every object first.
pub struct RegisteredAbility {
    pub source: ObjectId,
    pub kind: AbilityKind,
    pub condition: Option<ScriptHandle>,
    pub cost: Option<ScriptHandle>,
    pub target: Option<ScriptHandle>,
    pub operation: ScriptHandle,
}
