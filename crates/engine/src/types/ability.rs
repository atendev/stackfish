use crate::types::event::GameEventKind;
use crate::types::ids::{ObjectId, ScriptHandle};
use crate::types::mana::ManaUnit;
use crate::types::zone::ZoneKind;

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
    /// The `Game.Ops.GainLife(n)` primitive — gains the controller exactly
    /// `n` life, no script invocation needed. Mirrors the hand-written
    /// `{ gain_life = n }` Lua operation table from the prior slice, just
    /// resolved on the Rust side instead of via `ScriptRuntime::invoke_operation`.
    GainLife(i32),
}

/// The one cost shape `Game.Costs.*` covers this slice — a Rust-native fast
/// path so `{T}` (CR 605's mana-ability cost, generalized here to a non-mana
/// activated ability) doesn't need a script callback at all. Mirrors
/// `ConditionPrimitive`/`AbilityEffect`'s "recognized primitive, zero Lua
/// calls" shape; mana cost, sacrifice/discard/life, and any `CostSpec`-style
/// additional cost stay unmodeled (same pre-existing gap
/// `PaymentPlan::validate` already flags for `CostSpec.additional`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AbilityCost {
    Tap,
}

/// The two condition shapes `Game.Conditions.*` covers this slice — a
/// Rust-native fast path so common trigger shapes don't need a full Lua
/// condition function written out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConditionPrimitive {
    /// "Whenever this permanent enters the battlefield" (CR 603 ETB
    /// triggers) — matches a `ZoneChange` event moving `source` to the
    /// battlefield. Mirrors the hand-written
    /// `event.object == source and event.to == "battlefield"` Lua condition
    /// from the prior slice exactly.
    EntersBattlefield,
    /// No filter — always matches. A real variant (not just `None`) so a
    /// Lua author has an explicit, correct spelling for "unconditional";
    /// see `RegisteredAbility::condition`'s doc for why both spellings
    /// exist.
    Always,
}

impl ConditionPrimitive {
    /// Zero Lua VM calls — evaluated directly against the event that's being
    /// checked. `source` is the registered ability's source object (not
    /// necessarily the event's own object).
    pub fn matches(&self, source: ObjectId, event: &GameEventKind) -> bool {
        match self {
            ConditionPrimitive::Always => true,
            ConditionPrimitive::EntersBattlefield => matches!(
                event,
                GameEventKind::ZoneChange { object, to, .. }
                    if *object == source && *to == ZoneKind::Battlefield
            ),
        }
    }
}

/// A condition/operation source string resolves to one of these two shapes
/// at load time (`ScriptRuntime::load_condition`) — a recognized
/// `Game.Conditions.*` tag (`Primitive`, no further Lua calls needed ever
/// again) or a genuine custom Lua function (`Script`, invoked for real via
/// `ScriptRuntime::invoke_condition` each time, same as before this slice).
#[derive(Clone, Copy, Debug)]
pub enum ConditionSpec {
    Primitive(ConditionPrimitive),
    Script(ScriptHandle),
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
    /// `None` means "no filter, always matches" — same meaning as
    /// `Some(ConditionSpec::Primitive(ConditionPrimitive::Always))`. Two
    /// spellings exist on purpose: `None` is what every non-triggered
    /// (`Activated`/`Static`) ability uses, since it never had a condition
    /// concept to begin with; `Always` is a real, explicit choice a Lua
    /// trigger author writes (`return Game.Conditions.Always`) when they
    /// mean "no filter" deliberately — loading rejects a bare Lua `nil`
    /// result as an error rather than silently treating a typo'd
    /// `Game.Conditions.Typo` as unconditional (see
    /// `ScriptRuntime::load_condition`).
    pub condition: Option<ConditionSpec>,
    /// `None` means "no cost primitive recognized" — same script-bridge-
    /// fallback posture as `effect`, though nothing consumes that fallback
    /// yet (`ScriptRuntime::invoke_cost` is still `todo!()`). `Some(Tap)` is
    /// the one shape `ActivateAbility` resolves this slice.
    pub cost: Option<AbilityCost>,
    pub target: Option<ScriptHandle>,
    pub operation: ScriptHandle,
    /// See `AbilityEffect`'s doc comment — `Some` for an effect shape this
    /// engine can resolve without a script call at all (CR 605 mana
    /// abilities' `AddMana`, or a `Game.Ops.*` primitive like `GainLife`
    /// loaded via `ScriptRuntime::load_operation`). `None` means resolution
    /// falls back to invoking `operation` for real through the script
    /// bridge.
    pub effect: Option<AbilityEffect>,
}
