use mlua::Lua;

use crate::types::card::CardDefinition;
use crate::types::event::GameEvent;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};

pub enum ScriptError {
    LoadError(String),
    RuntimeError(String),
}

/// Borrowed view handed to a Lua callback for the duration of one
/// condition/cost/target/operation invocation.
///
/// `state` is currently the full `&GameState` — this is exactly the
/// hidden-information leak surface DESIGN.md flags as an open question (a
/// script could enumerate an opponent's hand off a raw state reference).
/// Needs a visibility-filtered view before this is real; kept as the full
/// state here only to get the call shape down for this pass.
pub struct ScriptContext<'a> {
    pub state: &'a GameState,
    pub source: ObjectId,
    pub controller: PlayerId,
    pub event: Option<&'a GameEvent>,
    pub chosen_targets: Vec<Target>,
}

/// Owns the embedded Lua VM and every loaded card script. One `ScriptRuntime`
/// per running engine process, not per-game — scripts load once and are
/// reused across games, per DESIGN.md's "restart to reload, no hot-reload"
/// decision.
pub struct ScriptRuntime {
    lua: Lua,
}

impl ScriptRuntime {
    pub fn new() -> Self {
        // TODO: ScriptRuntime { lua: Lua::new() }, then register the Game.*
        // API table (script_api.rs functions) into the Lua globals before
        // returning - card scripts loaded afterward assume Game.* exists.
        // The actual mlua binding code (wrapping each script_api.rs fn as a
        // Lua-callable closure via lua.globals().set(...)) isn't written
        // anywhere yet - this constructor is where it'd be wired in.
        todo!()
    }

    /// Loads `definition.script_ref`'s source into the VM and runs its
    /// setup (the ygopro-style `initial_effect(c)` equivalent — registers
    /// the card's abilities/continuous effects/replacement effects via the
    /// `Game.*` API in `script_api.rs`). Once per `CardDefinition`, not once
    /// per `GameObject` instance of that card.
    pub fn load_card_script(&mut self, definition: &CardDefinition) -> Result<ScriptHandle, ScriptError> {
        // TODO: match definition.script_ref (ByKey -> fetch source from
        // wherever ByKey scripts resolve to, not designed yet since that's
        // the external card-database project's territory; Inline -> use
        // the String directly), load via self.lua.load(source).exec(),
        // mint a new ScriptHandle, store whatever's needed to invoke this
        // script's callbacks later (likely an mlua::RegistryKey per
        // callback, keyed by handle), return Err(ScriptError::LoadError)
        // on a Lua syntax/load error.
        todo!()
    }

    pub fn invoke_condition(&self, handle: ScriptHandle, ctx: &ScriptContext) -> Result<bool, ScriptError> {
        // TODO: look up the condition function for handle, call it with
        // ctx's fields converted to Lua values (needs UserData/conversion
        // impls for ObjectId/PlayerId/GameEvent/Target that don't exist
        // yet), convert the Lua return to bool, wrap Lua runtime errors as
        // ScriptError::RuntimeError.
        todo!()
    }

    pub fn invoke_cost(&self, handle: ScriptHandle, ctx: &ScriptContext) -> Result<(), ScriptError> {
        // TODO: same calling pattern as invoke_condition above.
        todo!()
    }

    /// Real target selection generally requires suspending mid-script to
    /// ask the player (same caveat as `script_api::select_targets`) — this
    /// signature is almost certainly wrong as a synchronous `-> Vec<Target>`
    /// and will need Lua coroutine support (`Lua::create_thread` plus
    /// yield/resume spanning `Engine::step()` calls) once that's designed.
    /// Kept synchronous for this first pass to get the call shape down.
    pub fn invoke_target(&self, handle: ScriptHandle, ctx: &ScriptContext) -> Result<Vec<Target>, ScriptError> {
        // TODO: blocked on the coroutine design noted above and in
        // DESIGN.md's open questions - don't implement synchronously, that
        // would lock in the wrong shape. Needs Lua coroutine support
        // designed first.
        todo!()
    }

    pub fn invoke_operation(&self, handle: ScriptHandle, ctx: &mut ScriptContext) -> Result<(), ScriptError> {
        // TODO: same calling pattern as invoke_condition, but the operation
        // callback can itself call back into Game.* (script_api.rs) which
        // needs &mut GameState - ctx.state is currently &GameState (shared
        // ref) here, so this also needs the mutability story sorted out,
        // not just the Lua call mechanics.
        todo!()
    }
}
