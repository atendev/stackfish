//! Cross-boundary types shared by the `script/` submodules: the error type,
//! the borrowed per-invocation context, choice and outcome types, and the
//! script registry.

use std::collections::HashMap;

use mlua::RegistryKey;

use crate::types::ability::RegisteredAbility;
use crate::types::cost::CostModifier;
use crate::types::effect::{ContinuousEffect, ReplacementEffect};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::player::{DrawLimitModifier, HandRevealModifier, HandSizeModifier, LandPlayModifier};
use crate::types::target::TargetFilter;

#[derive(Debug)]
pub enum ScriptError {
    LoadError(String),
    RuntimeError(String),
}

pub(crate) fn to_lua_err(e: ScriptError) -> mlua::Error {
    mlua::Error::RuntimeError(format!("{e:?}"))
}

/// Borrowed view handed to a Lua callback for one condition, cost, target or
/// operation invocation.
///
/// `state` is the full `&GameState`. That is not a leak, since the engine is
/// the omniscient rules authority: the leak surface is the Rust-to-Lua
/// boundary, and nothing here hands `state` to Lua. Every state-aware
/// `invoke_*` builds a `state_to_lua` snapshot instead, a plain table that is
/// the hidden-information boundary (see that function).
pub struct ScriptContext<'a> {
    pub state: &'a GameState,
    pub source: ObjectId,
    pub controller: PlayerId,
    pub event: Option<&'a GameEvent>,
    pub chosen_targets: Vec<Target>,
}

/// A card script's request for player input mid-resolution, yielded from a
/// coroutine-invoked operation through `Game.SelectTargets` or
/// `Game.MayChoose` and decoded by `decode_script_choice_request`.
#[derive(Clone, Debug)]
pub enum ScriptChoiceRequest {
    /// `Game.SelectTargets(filter)`: one target matching a `TargetFilter`.
    /// `exclude_ids` holds already-picked object ids to reject regardless of
    /// `filter`. It is empty for a bare call, and `Game.ChooseN`'s Lua loop
    /// grows it by one per iteration so repeats can't return a target already
    /// chosen. `Engine::submit_script_choice` checks it directly and not in
    /// `TargetFilter::Object`, which ~50 sites construct and none of the
    /// others need an exclusion set.
    SelectTarget { filter: TargetFilter, exclude_ids: Vec<ObjectId> },
    /// `Game.MayChoose()` (CR 608.2d): a plain yes/no choice, where any `bool`
    /// is legal. `Game.ChooseN`'s `up_to` mode reuses it to ask "one more?"
    /// before each optional pick.
    MayChoose,
}

/// Which zone-move verb a script operation's returned table asked for
/// (`Game.Ops.SacrificeChosen`, `DiscardChosen`), decoded from the
/// `Game.Ops.SacrificeChosen` and `DiscardChosen` steps by
/// `ScriptRuntime::handle_operation_result`. A small local enum and not
/// `ScriptedZoneMoveKind`, because `script` doesn't depend on `engine`
/// internals. The consumers in `engine::resolve::misc_effects` and
/// `each_player` map it with a one-line `match`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScriptZoneMoveKind {
    Sacrifice,
    Discard,
}

/// What one `ScriptRuntime::invoke_operation` or `resume_operation` call
/// produced. For the request variants "outcome describes, engine acts":
/// `ScriptRuntime` can't reach the engine pipelines, so it hands the request
/// up.
/// - `Finished`: the operation ran to completion, with any `GainLife` applied. A script that never calls `Game.SelectTargets` only
///   produces this.
/// - `NeedsChoice`: the script yielded a request and must be resumed with
///   `resume_operation`. Only `Engine::submit_script_choice` should call it.
/// - `PendingZoneMoves`: the `SacrificeChosen` and `DiscardChosen` steps, one
///   entry per distinct verb (at most two today, as a Balance-style
///   body can ask for both). A zone move needs the replacement-checked
///   `Engine::continue_scripted_zone_move`, unlike a direct `GameState`
///   mutation such as `GainLife`.
/// - `PendingCast`: `Game.Ops.CastSpell(card, opts)`
///   (CR 601.2, 702.85a-style). Casting needs `commands::execute_command`'s
///   full pipeline, so `engine::resolve::misc_effects::resolve_misc_or_script_effect`,
///   the only consumer, builds a synthetic `CastSpell` like
///   `Engine::submit_cascade_cast` and `submit_madness_cast`. `free`
///   (CR 702.85a) reuses `GameState::pending_free_cast`, which hardcodes
///   `from_zone: Exile`.
/// - `PendingDraw`: `Game.Ops.DrawCards(n)` returned from a custom function.
///   `Engine::continue_draw_cards` needs `&mut Engine` and can pause for a CR 616.1 ordering choice (a Words of Wind redirect).
///   `player` is the call's `controller`, since a custom function can't make a
///   different player draw.
/// - `PendingEvents`: `Game.Ops.DealDamageTo` and `AddCountersTo`.
///   `resolve_damage_effect` and `resolve_counters_effect` only build `DamageDealt` and `CounterAdded`,
///   and the existing `ReplacementCheck` and `CommitEvents` apply CR 615 and
///   616 prevention and replacement, deathtouch, wither, infect and lifelink.
///   This hands back events and not a request, with no pause, so the caller
///   folds them into its own list. That holds inside `Game.Ops.ForEachPlayer`
///   too, since CR 615 and 616 need no cross-player simultaneity as CR 101.4
///   does.
///
/// TODO: casting for free from a zone other than exile, the same limit as Cascade's free cast
/// TODO: zone-move verbs other than sacrifice and discard in a returned table
#[derive(Debug)]
pub enum OperationOutcome {
    Finished,
    NeedsChoice { thread: ScriptHandle, request: ScriptChoiceRequest },
    PendingZoneMoves(Vec<(ScriptZoneMoveKind, Vec<ObjectId>)>),
    PendingCast { card: ObjectId, modes: Vec<u32>, targets: Vec<Target>, x_value: Option<u32>, free: bool },
    PendingDraw { amount: u32 },
    PendingEvents(Vec<GameEventKind>),
}

/// Shared, interior-mutable store for loaded Lua callbacks and whatever a
/// running card script has self-registered, in `Rc<RefCell<_>>` so a Lua
/// closure (`Game.CreateTriggeredAbility` and its builder) can reach it
/// without `&mut GameState` or `&mut ScriptRuntime` mid-callback. A callback
/// touches only this side channel, never `GameState`.
#[derive(Default)]
pub(crate) struct ScriptRegistry {
    /// One entry per loaded callback, keyed by the opaque `ScriptHandle`. A
    /// `RegistryKey` and not the `mlua::Function`, which borrows from `lua`.
    pub(crate) scripts: HashMap<ScriptHandle, RegistryKey>,
    pub(crate) next_handle: u64,
    /// Abilities a running `register_abilities` has `:Register()`ed, drained
    /// into `GameState.abilities` when the card script call returns
    /// (`ScriptRuntime::run_card_script`). Lua never gets a `GameState`
    /// reference. Shared by `TriggeredAbilityBuilder` and
    /// `ActivatedAbilityBuilder`, since the drain ignores `AbilityKind`.
    pub(crate) pending_triggered: Vec<RegisteredAbility>,
    /// `(index into pending_triggered, zone)` for each ability that declared
    /// `:SetFunctionsFrom(zone)`, parallel to `pending_triggered` because
    /// `GameState::ability_function_zones` holds it. Drained with it.
    pub(crate) pending_function_zones: Vec<(usize, crate::types::zone::ZoneKind)>,
    /// For `Game.CreateStaticAbility`'s builder (CR 604, 611), drained into
    /// `GameState.continuous_effects` by `run_card_script`. Each
    /// `ContinuousEffect.timestamp` is a `SequenceNo(0)` placeholder stamped at
    /// drain, since a Lua callback has no `&mut GameState` to allocate one. The
    /// third element is `:SetCharacteristicDefining()` (CR 604.3), which makes
    /// the effect function in every zone.
    pub(crate) pending_static: Vec<(ContinuousEffect, Option<crate::types::zone::ZoneKind>, bool)>,
    /// For `Game.CreateReplacementEffect`'s builder (CR 614, 701.19b), drained
    /// into `GameState.replacement_effects`. A `ReplacementEffect` has no
    /// `timestamp` to stamp, since CR 613.7 ordering doesn't apply.
    pub(crate) pending_replacement: Vec<(ReplacementEffect, Option<crate::types::zone::ZoneKind>)>,
    /// For `Game.CreateCostModifier`'s builder (CR 601.2f), drained into
    /// `GameState.cost_modifiers`. Each `controller` is a `PlayerId(0)`
    /// placeholder resolved at drain, since the builder has no `&GameState` to
    /// read the source's live controller.
    pub(crate) pending_cost_modifiers: Vec<(CostModifier, Option<crate::types::zone::ZoneKind>)>,
    /// For `Game.CreateHandSizeModifier`'s builder (CR 402.2), drained into
    /// `GameState.hand_size_modifiers`, with a placeholder `controller`
    /// resolved at drain as in `pending_cost_modifiers`.
    pub(crate) pending_hand_size_modifiers: Vec<(HandSizeModifier, Option<crate::types::zone::ZoneKind>)>,
    /// For `Game.CreateHandRevealModifier`'s builder (CR 701.20a, 303.4m),
    /// drained into `GameState.hand_reveal_modifiers`, with the same
    /// placeholders.
    pub(crate) pending_hand_reveal_modifiers: Vec<(HandRevealModifier, Option<crate::types::zone::ZoneKind>)>,
    /// For `Game.CreateDrawLimitModifier`'s builder (CR 121.2b), drained into
    /// `GameState.draw_limit_modifiers`, with the same placeholders.
    pub(crate) pending_draw_limit_modifiers: Vec<(DrawLimitModifier, Option<crate::types::zone::ZoneKind>)>,
    /// For `Game.CreateLandPlayModifier`'s builder (CR 305.2), drained into
    /// `GameState.land_play_modifiers`. A placeholder `controller` only, since
    /// `LandPlayModifier` has no `timestamp`.
    pub(crate) pending_land_play_modifiers: Vec<(LandPlayModifier, Option<crate::types::zone::ZoneKind>)>,
    /// A coroutine `Thread` suspended mid-`invoke_operation` at a
    /// `Game.SelectTargets` call, keyed in the `ScriptHandle` space `scripts`
    /// uses (sharing `next_handle`). A separate map because `resume_operation`
    /// consumes a suspended thread once, unlike a reusable `Function`. A
    /// `RegistryKey` for the same reason as `scripts`.
    pub(crate) suspended_threads: HashMap<ScriptHandle, RegistryKey>,
}

impl ScriptRegistry {
    pub(crate) fn next_script_handle(&mut self) -> ScriptHandle {
        let handle = ScriptHandle(self.next_handle);
        self.next_handle += 1;
        handle
    }
}
