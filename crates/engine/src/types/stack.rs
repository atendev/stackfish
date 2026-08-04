use crate::types::ability::{AbilityEffect, InterveningIfSpec};
use crate::types::cost::CostPayment;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::object::GameObject;
use crate::types::target::TargetSpec;

pub enum StackEntryKind {
    Spell,
    ActivatedAbility,
    TriggeredAbility,
    Copy { of: Box<StackEntryKind> },
}

/// A spell, activated ability, triggered ability, or copy thereof on the stack.
pub struct StackObject {
    pub stack_id: ObjectId,
    pub kind: StackEntryKind,
    pub source: ObjectId,
    /// Last-known-information snapshot, captured if `source` may leave the
    /// zone it was in before this resolves (CR 608.2b).
    pub source_lki: Option<GameObject>,
    /// CR 800.4a: the *owner* of whatever this object represents - a
    /// spell's is its card's own `GameObject::owner` (independent of who's
    /// casting it, e.g. a stolen creature's spell still owed to its
    /// original owner - not applicable in this engine yet since nothing
    /// grants "cast a card you don't own," but the field is correct
    /// regardless); an ability's is its source permanent's owner, not its
    /// controller (which may differ under a `GainControl` effect). Distinct
    /// from `controller` below - CR 800.4a removes owned objects (including
    /// spells, which are cards) when their owner leaves the game, but only
    /// removes an *ability* (not represented by a card) when its
    /// *controller* leaves - see `sba::remove_players_objects`'s own stack
    /// cleanup for where that distinction actually matters.
    pub owner: PlayerId,
    pub controller: PlayerId,
    pub chosen_modes: Vec<u32>,
    pub targets: Vec<Target>,
    /// Baked in at push time from whatever produced this `StackObject`
    /// (`CardDefinition::target_requirement` for a spell,
    /// `RegisteredAbility::target` for an ability) — same "primitive
    /// stand-in, decided once at push, not re-derived" contract `effect`
    /// already established. `ResolveStackTop` rechecks `targets` against
    /// this at resolution time (CR 608.2b); it does *not* need the original
    /// `count`, only the `filter`, since CR 601.2c fixes the count at cast
    /// time and it can't change afterward — but the full `TargetSpec` is
    /// carried anyway (not just `TargetFilter`) so the `Script` case has
    /// somewhere to eventually live once that's evaluable.
    pub target_requirement: Option<TargetSpec>,
    pub x_value: Option<u32>,
    /// CR 702.34a: baked in at push time from the chosen `AlternativeCost`
    /// (`false` for a spell cast normally, or an ability — abilities never
    /// set this). When `true`, the zone-destination sites in `engine.rs`
    /// that would otherwise send a resolving/fizzling spell to its owner's
    /// graveyard send it to exile instead.
    pub exile_on_leaving_stack: bool,
    /// CR 702.27a Buyback: baked in at push time from whether the caster
    /// announced (and this card offers) a buyback cost - `false` for a
    /// spell cast normally, or an ability (abilities never set this, same
    /// as `exile_on_leaving_stack`). When `true`, a resolving instant/
    /// sorcery goes to its owner's hand instead of the graveyard
    /// (`engine/resolve/permanent_entry.rs`) - unlike
    /// `exile_on_leaving_stack`, this is *not* checked on the CR 608.2b
    /// fizzle path, since CR 702.27a's own text only covers "as it
    /// resolves," not "or as it leaves the stack otherwise."
    pub bought_back: bool,
    /// CR 702.138b: baked in at push time from the chosen `AlternativeCost::
    /// Replacement::escape` (`false` for a spell cast normally, or an
    /// ability - abilities never set this, same as `exile_on_leaving_stack`/
    /// `bought_back`). Carried onto the entering permanent's own
    /// `GameObject::entered_via_escape` by `Engine::move_resolving_spell`
    /// once this resolves to the battlefield.
    pub escaped: bool,
    pub costs_paid: Vec<CostPayment>,
    /// Which script callback to invoke on resolution. Present in the
    /// original adaptation doc's `StackObject` spec ("resolution callback
    /// reference") but dropped in Phase 1 before the script bridge existed
    /// to give it a concrete type — restored now as a `ScriptHandle`.
    pub operation: ScriptHandle,
    /// Same "primitive stand-in when Some, script fallback when None"
    /// contract as `RegisteredAbility::effect` — baked in at push time
    /// (`PutTriggersApnap`) from the ability that produced this
    /// `StackObject`, so resolution can apply it directly without a Lua
    /// call. `None` falls back to invoking `operation` for real.
    pub effect: Option<AbilityEffect>,
    /// CR 603.4/608.2a: baked in at push time from `RegisteredAbility::
    /// intervening_if` (`PutTriggersApnap`, `None` for a `Spell`/
    /// `ActivatedAbility` — CR 603.4's "if" clause is triggered-ability-only
    /// text) — same "primitive stand-in, decided once at push, not
    /// re-derived" contract `effect`/`target_requirement` already establish.
    /// `ResolveStackTop` rechecks it fresh against current state at
    /// resolution, exactly mirroring `target_requirement`'s own CR 608.2b
    /// recheck.
    pub intervening_if: Option<InterveningIfSpec>,
    /// CR 700.2e: `Some` only for a modal spell whose `ModeChoice` is
    /// `ExactlyChosenByOpponent`/`UpToChosenByOpponent` and whose mode hasn't
    /// been chosen yet — `chosen_modes`/`target_requirement`/`effect` above
    /// are still empty/`None` placeholders at push time. `ResolveStackTop`
    /// checks this (peeking, without popping) before resolving anything and
    /// pauses on `ProcessState::AwaitingModeChoice` instead —
    /// `Engine::submit_mode_choice` patches `chosen_modes`/
    /// `target_requirement`/`effect` in place and clears this back to `None`
    /// once the real other-player choice arrives. `None` for every other
    /// `StackObject` (the overwhelming majority — non-modal cards, and modal
    /// cards using any of `ModeChoice`'s other variants, still resolve
    /// their mode synchronously at cast time exactly as before this slice).
    pub pending_mode_choice: Option<PendingModeChoice>,
}

/// See `StackObject::pending_mode_choice`'s own doc comment.
pub struct PendingModeChoice {
    pub chooser: PlayerId,
    pub min: u32,
    pub max: u32,
}

impl StackObject {
    /// `chosen_modes`/`targets`/`x_value` are taken as params (rather than
    /// always starting empty/`None`) since the first real caller
    /// (`commands::execute_command`'s `CastSpell` arm) already has these
    /// from `PlayerActionKind::CastSpell` by the time it builds a
    /// `StackObject` — deferring them to a separate mutation step would just
    /// mean constructing this in a momentarily-inconsistent state.
    pub fn new(
        stack_id: ObjectId,
        kind: StackEntryKind,
        source: ObjectId,
        owner: PlayerId,
        controller: PlayerId,
        chosen_modes: Vec<u32>,
        targets: Vec<Target>,
        target_requirement: Option<TargetSpec>,
        x_value: Option<u32>,
        exile_on_leaving_stack: bool,
        bought_back: bool,
        escaped: bool,
        operation: ScriptHandle,
        effect: Option<AbilityEffect>,
        intervening_if: Option<InterveningIfSpec>,
        pending_mode_choice: Option<PendingModeChoice>,
    ) -> Self {
        StackObject {
            stack_id,
            kind,
            source,
            // Not captured at construction - only meaningful once `source`
            // might actually leave its zone before this resolves (CR
            // 608.2b), which is a later mutation, not part of building this.
            source_lki: None,
            owner,
            controller,
            chosen_modes,
            targets,
            target_requirement,
            x_value,
            exile_on_leaving_stack,
            bought_back,
            escaped,
            costs_paid: Vec::new(),
            operation,
            effect,
            intervening_if,
            pending_mode_choice,
        }
    }
}
