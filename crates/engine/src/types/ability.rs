use crate::types::ids::{ObjectId, ScriptHandle};
use crate::types::mana::ManaUnit;

/// Per ygopro's effect lifecycle (`docs/ygo_pro_reference.md`), generalized
/// for MTG: condition/cost/target/operation are separate script callbacks
/// rather than one monolithic function, so the engine can check (e.g.)
/// condition without committing to cost/target/operation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AbilityKind {
    Triggered,
    Activated,
    Static,
}

/// Structural stand-in for an ability's effect when no script bridge exists
/// to actually run one (every `RegisteredAbility` still needs `operation` —
/// this is narrower: just enough to model CR 605's mana abilities, the one
/// concrete effect shape this engine resolves without a script callback).
/// `None` on `RegisteredAbility::effect` means "this ability's effect can
/// only be carried out via the script bridge" — not yet possible, same gap
/// as `operation` everywhere else.
#[derive(Clone, Debug)]
pub enum AbilityEffect {
    /// CR 605: adds exactly these literal units to the activator's mana
    /// pool. Reuses `ManaUnit` (see `mana.rs`) — same "literal units, not a
    /// cost shape" vocabulary `CostPayment::Mana` already uses, just added
    /// instead of spent.
    AddMana(Vec<ManaUnit>),
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
    /// See `AbilityEffect`'s doc comment — `Some` only for the one effect
    /// shape (CR 605 mana abilities) this engine can resolve without the
    /// script bridge yet.
    pub effect: Option<AbilityEffect>,
}
