//! Cross-boundary types shared by every other `script/` submodule — the
//! Lua<->Rust error type, the borrowed per-invocation context, the
//! coroutine-choice/operation-outcome types, and the shared script registry
//! — split out of the original monolithic `script.rs`.

use std::collections::HashMap;

use mlua::RegistryKey;

use crate::types::ability::RegisteredAbility;
use crate::types::cost::CostModifier;
use crate::types::effect::{ContinuousEffect, ReplacementEffect};
use crate::types::event::GameEvent;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::player::{DrawLimitModifier, HandRevealModifier, HandSizeModifier};
use crate::types::target::TargetFilter;

#[derive(Debug)]
pub enum ScriptError {
    LoadError(String),
    RuntimeError(String),
}

pub(crate) fn to_lua_err(e: ScriptError) -> mlua::Error {
    mlua::Error::RuntimeError(format!("{e:?}"))
}

/// Borrowed view handed to a Lua callback for the duration of one
/// condition/cost/target/operation invocation.
///
/// `state` is the full `&GameState` — but that's fine on the Rust side; this
/// engine is the omniscient rules authority, not a player, so full access
/// here isn't itself the leak DESIGN.md's open question warns about. The
/// leak surface is strictly the Rust->Lua boundary: nothing here ever hands
/// `state` (or a piece of it) to Lua directly. `invoke_condition` and every
/// other state-aware `invoke_*` method instead builds a `state_to_lua`
/// snapshot — a plain table, not a live reference — which is the actual
/// hidden-information-safe boundary (see that function's own doc comment for
/// what it does and doesn't expose). `ScriptContext` itself doesn't need
/// narrowing so long as that invariant holds.
pub struct ScriptContext<'a> {
    pub state: &'a GameState,
    pub source: ObjectId,
    pub controller: PlayerId,
    pub event: Option<&'a GameEvent>,
    pub chosen_targets: Vec<Target>,
}

/// A card script's request for player input mid-resolution (backlog item
/// 9), yielded from a coroutine-invoked operation via `Game.SelectTargets`/
/// `Game.MayChoose`. Decoded from the yielded Lua table by
/// `decode_script_choice_request`.
#[derive(Clone, Debug)]
pub enum ScriptChoiceRequest {
    /// `Game.SelectTargets(filter)` — a single target matching a
    /// `TargetFilter` (no count > 1, same narrowing precedent every other
    /// targeting-adjacent slice already set — see `TargetFilter`'s own doc
    /// comment).
    SelectTarget { filter: TargetFilter },
    /// `Game.MayChoose()` (backlog item 9a, CR 608.2d) — a plain yes/no
    /// choice with no filter to satisfy; any `bool` answer is legal, unlike
    /// `SelectTarget`'s `target_is_legal` check.
    MayChoose,
}

/// What one `ScriptRuntime::invoke_operation`/`resume_operation` call
/// produced. `Finished` is the pre-existing synchronous behavior (the
/// operation ran to completion, any recognized effect key already applied)
/// — a script that never calls `Game.SelectTargets` only ever produces this,
/// so nothing about existing scripts' observable behavior changes.
/// `NeedsChoice` is new: the script yielded a real request and must be
/// resumed (`resume_operation`) with the player's answer before it can
/// continue — `Engine::submit_script_choice` is the only intended caller.
#[derive(Debug)]
pub enum OperationOutcome {
    Finished,
    NeedsChoice { thread: ScriptHandle, request: ScriptChoiceRequest },
}

/// Shared, interior-mutable store for loaded Lua callbacks plus whatever a
/// currently-running card script has self-registered so far — wrapped in
/// `Rc<RefCell<_>>` so a Lua-bound closure (`Game.CreateTriggeredAbility`,
/// and the `TriggeredAbilityBuilder` it returns) can reach it without ever
/// needing `&mut GameState`, or even `&mut ScriptRuntime`, mid-callback.
/// This is the actual fix for the registration-time mutability gap flagged
/// since Phase 2 — a Lua callback only ever touches this small side-channel,
/// never `GameState` itself.
#[derive(Default)]
pub(crate) struct ScriptRegistry {
    /// One entry per loaded callback (a condition/operation/etc function),
    /// keyed by the opaque `ScriptHandle` data types reference it by. A
    /// `RegistryKey` rather than holding the `mlua::Function` directly since
    /// `Function` borrows from `lua` — the registry is `mlua`'s own escape
    /// hatch for stashing a Lua value independent of that borrow.
    pub(crate) scripts: HashMap<ScriptHandle, RegistryKey>,
    pub(crate) next_handle: u64,
    /// Abilities a currently-running `register_abilities` has `:Register()`ed,
    /// waiting to be drained into `GameState.abilities` once the whole card
    /// script call returns (`ScriptRuntime::run_card_script`). Lua never
    /// gets a `GameState` reference at all — this is what `:Register()`
    /// pushes onto instead. Shared by both `TriggeredAbilityBuilder` and
    /// `ActivatedAbilityBuilder` — the drain doesn't care about
    /// `AbilityKind`, so there's no need for a second, kind-specific queue.
    pub(crate) pending_triggered: Vec<RegisteredAbility>,
    /// Same idea as `pending_triggered`, for `Game.CreateStaticAbility`'s
    /// builder (CR 604/611) — drained into `GameState.continuous_effects` by
    /// `run_card_script`. `timestamp` on each queued `ContinuousEffect` is a
    /// placeholder (`SequenceNo(0)`) until drain time, since the builder has
    /// no `&mut GameState` to call `allocate_sequence_no()` from inside a Lua
    /// callback — the drain step stamps the real value.
    pub(crate) pending_static: Vec<ContinuousEffect>,
    /// Same idea as `pending_static`, for `Game.CreateReplacementEffect`'s
    /// builder (CR 614/701.19b) — drained into `GameState.replacement_effects`
    /// by `run_card_script`. Unlike `pending_static`'s `ContinuousEffect`, a
    /// `ReplacementEffect` has no `timestamp` field to stamp at drain time —
    /// nothing about CR 613.7 ordering applies to it.
    pub(crate) pending_replacement: Vec<ReplacementEffect>,
    /// Same idea as `pending_static`, for `Game.CreateCostModifier`'s builder
    /// (CR 601.2f) — drained into `GameState.cost_modifiers` by
    /// `run_card_script`. `controller` on each queued `CostModifier` is a
    /// placeholder (`PlayerId(0)`) until drain time — the builder has no
    /// `&GameState` to read this source's live controller from inside a Lua
    /// callback, same reason `pending_static`'s `timestamp` is a placeholder
    /// too. The drain step resolves the real value there instead.
    pub(crate) pending_cost_modifiers: Vec<CostModifier>,
    /// Same idea as `pending_cost_modifiers`, for `Game.CreateHandSizeModifier`'s
    /// builder (CR 402.2) — drained into `GameState.hand_size_modifiers` by
    /// `run_card_script`. `controller` on each queued `HandSizeModifier` is a
    /// placeholder (`PlayerId(0)`) until drain time, same reason and same
    /// fix as `pending_cost_modifiers`'s own placeholder `controller`.
    pub(crate) pending_hand_size_modifiers: Vec<HandSizeModifier>,
    /// Same idea as `pending_hand_size_modifiers`, for `Game.
    /// CreateHandRevealModifier`'s builder (CR 701.20a/303.4m) — drained
    /// into `GameState.hand_reveal_modifiers` by `run_card_script`. Same
    /// placeholder-`controller`/`timestamp` pattern.
    pub(crate) pending_hand_reveal_modifiers: Vec<HandRevealModifier>,
    /// Same idea as `pending_hand_reveal_modifiers`, for `Game.
    /// CreateDrawLimitModifier`'s builder (CR 121.2b) — drained into
    /// `GameState.draw_limit_modifiers` by `run_card_script`. Same
    /// placeholder-`controller`/`timestamp` pattern.
    pub(crate) pending_draw_limit_modifiers: Vec<DrawLimitModifier>,
    /// Backlog item 9: a coroutine `Thread` suspended mid-`invoke_operation`
    /// at a `Game.SelectTargets` call, keyed by the same opaque
    /// `ScriptHandle` space `scripts` uses (shares `next_handle`, kept in a
    /// separate map since a suspended thread is consumed exactly once by
    /// `resume_operation`, unlike a loaded `Function` handle which is
    /// reusable). Same "`RegistryKey`, not the borrowed value itself"
    /// rationale as `scripts` — confirmed via a standalone spike
    /// (`phase5_coroutine_spike.rs`) that `Thread` round-trips through the
    /// registry the same way `Function` already does.
    pub(crate) suspended_threads: HashMap<ScriptHandle, RegistryKey>,
}

impl ScriptRegistry {
    pub(crate) fn next_script_handle(&mut self) -> ScriptHandle {
        let handle = ScriptHandle(self.next_handle);
        self.next_handle += 1;
        handle
    }
}
