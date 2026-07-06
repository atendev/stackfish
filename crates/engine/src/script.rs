use std::collections::HashMap;

use mlua::{Function, Lua, RegistryKey, Table, UserData, Value};

use crate::types::ability::{AbilityEffect, ConditionPrimitive, ConditionSpec};
use crate::types::card::CardDefinition;
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::zone::ZoneKind;

/// Thin `mlua::UserData` wrapper around `ConditionPrimitive` so
/// `Game.Conditions.Enters`/`Game.Conditions.Always` are real Lua values a
/// condition script can `return` directly — decoded back out in
/// `ScriptRuntime::load_condition`.
#[derive(Clone, Copy)]
struct ConditionTag(ConditionPrimitive);
impl UserData for ConditionTag {}

/// Same idea as `ConditionTag`, for `Game.Ops.*` — e.g. `Game.Ops.GainLife(1)`
/// is a Lua function that returns one of these, decoded back out in
/// `ScriptRuntime::load_operation`.
#[derive(Clone)]
struct OperationTag(AbilityEffect);
impl UserData for OperationTag {}

#[derive(Debug)]
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
    /// One entry per loaded callback (a condition/operation/etc function),
    /// keyed by the opaque `ScriptHandle` data types reference it by. A
    /// `RegistryKey` rather than holding the `mlua::Function` directly since
    /// `Function` borrows from `lua` — the registry is `mlua`'s own escape
    /// hatch for stashing a Lua value independent of that borrow.
    scripts: HashMap<ScriptHandle, RegistryKey>,
    next_handle: u64,
}

impl ScriptRuntime {
    pub fn new() -> Self {
        let lua = Lua::new();
        // Game.Conditions.*/Game.Ops.* sugar tables only this slice - real
        // Game.RegisterTriggeredAbility/etc self-registration (script_api.rs's
        // functions actually bound into `lua`) is still deferred: that needs
        // the registration-time mutability question (a Lua callback needing
        // to reach &mut GameState mid-script) sorted first. TODO: bind the
        // rest of Game.* here once that's designed.
        bind_game_globals(&lua).expect("binding the Game.Conditions/Game.Ops sugar tables should never fail");
        ScriptRuntime { lua, scripts: HashMap::new(), next_handle: 0 }
    }

    fn next_script_handle(&mut self) -> ScriptHandle {
        let handle = ScriptHandle(self.next_handle);
        self.next_handle += 1;
        handle
    }

    /// Loads `source` (a Lua chunk that must evaluate to a single function
    /// value, e.g. `return function(...) ... end`) and stores it under a
    /// fresh `ScriptHandle`. Narrower than `load_card_script` below — this
    /// loads one standalone callback, not a whole card script that registers
    /// several of its own via `Game.*`. `load_card_script` will likely be
    /// implemented in terms of this once that self-registration path exists;
    /// for now, callers (test/scenario fixtures) load a condition callback
    /// and an operation callback separately and build the `RegisteredAbility`
    /// referencing both handles directly, rather than a script registering
    /// itself.
    pub fn load_function(&mut self, source: &str) -> Result<ScriptHandle, ScriptError> {
        let function: Function =
            self.lua.load(source).eval().map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let key = self
            .lua
            .create_registry_value(function)
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let handle = self.next_script_handle();
        self.scripts.insert(handle, key);
        Ok(handle)
    }

    /// Loads a condition source string that may be either shape: a full
    /// custom `return function(source, controller, event) ... end` (falls
    /// back to `ConditionSpec::Script`, invoked for real via
    /// `invoke_condition` at trigger-check time, unchanged from before this
    /// slice) or a one-liner referencing `Game.Conditions.*`
    /// (`ConditionSpec::Primitive`, resolved once here — zero further Lua
    /// calls). Any other returned value (including a bare `nil`, e.g. a
    /// typo'd `Game.Conditions.Typo`) is a load error, not guessed at — see
    /// `RegisteredAbility::condition`'s doc comment for why `nil` isn't
    /// silently treated as "always".
    pub fn load_condition(&mut self, source: &str) -> Result<ConditionSpec, ScriptError> {
        // The eval+match is confined to this nested block so the `Value`'s
        // tie to `self.lua`'s borrow ends here, before `next_script_handle`
        // needs `&mut self` below — otherwise NLL's drop elaboration for
        // `Value` (some variants wrap a live Lua-side ref) extends that
        // borrow to the end of the whole function even though every arm
        // fully consumes it.
        enum Decoded {
            Primitive(ConditionPrimitive),
            Pending(RegistryKey),
        }
        let decoded = {
            let value: Value =
                self.lua.load(source).eval().map_err(|e| ScriptError::LoadError(e.to_string()))?;
            match value {
                Value::UserData(ud) => {
                    let tag: ConditionTag = ud
                        .borrow::<ConditionTag>()
                        .map_err(|e| ScriptError::LoadError(e.to_string()))?
                        .clone();
                    Decoded::Primitive(tag.0)
                }
                Value::Function(function) => {
                    let key = self
                        .lua
                        .create_registry_value(function)
                        .map_err(|e| ScriptError::LoadError(e.to_string()))?;
                    Decoded::Pending(key)
                }
                Value::Nil => {
                    return Err(ScriptError::LoadError(
                        "condition script returned nil - use `return Game.Conditions.Always` \
                         for an explicit unconditional trigger, not an empty/typo'd chunk"
                            .to_string(),
                    ));
                }
                other => {
                    return Err(ScriptError::LoadError(format!(
                        "condition script must return a Game.Conditions.* value or a function, got {}",
                        other.type_name()
                    )));
                }
            }
        };
        match decoded {
            Decoded::Primitive(primitive) => Ok(ConditionSpec::Primitive(primitive)),
            Decoded::Pending(key) => {
                let handle = self.next_script_handle();
                self.scripts.insert(handle, key);
                Ok(ConditionSpec::Script(handle))
            }
        }
    }

    /// Same two-shapes-at-load-time pattern as `load_condition`, for
    /// operations. A recognized `Game.Ops.*` value decodes into
    /// `(Some(AbilityEffect), ScriptHandle(0))` — the handle is an unused
    /// placeholder, same convention CR 605 mana abilities already use for
    /// `RegisteredAbility::operation` when `effect` is `Some`. A custom
    /// function falls back to `(None, <real handle>)`, invoked via
    /// `invoke_operation` exactly as before this slice.
    pub fn load_operation(
        &mut self,
        source: &str,
    ) -> Result<(Option<AbilityEffect>, ScriptHandle), ScriptError> {
        // Same nested-block reasoning as load_condition above.
        enum Decoded {
            Primitive(AbilityEffect),
            Pending(RegistryKey),
        }
        let decoded = {
            let value: Value =
                self.lua.load(source).eval().map_err(|e| ScriptError::LoadError(e.to_string()))?;
            match value {
                Value::UserData(ud) => {
                    let tag: OperationTag = ud
                        .borrow::<OperationTag>()
                        .map_err(|e| ScriptError::LoadError(e.to_string()))?
                        .clone();
                    Decoded::Primitive(tag.0)
                }
                Value::Function(function) => {
                    let key = self
                        .lua
                        .create_registry_value(function)
                        .map_err(|e| ScriptError::LoadError(e.to_string()))?;
                    Decoded::Pending(key)
                }
                Value::Nil => {
                    return Err(ScriptError::LoadError(
                        "operation script returned nil - expected a Game.Ops.* value or a function"
                            .to_string(),
                    ));
                }
                other => {
                    return Err(ScriptError::LoadError(format!(
                        "operation script must return a Game.Ops.* value or a function, got {}",
                        other.type_name()
                    )));
                }
            }
        };
        match decoded {
            Decoded::Primitive(effect) => Ok((Some(effect), ScriptHandle(0))),
            Decoded::Pending(key) => {
                let handle = self.next_script_handle();
                self.scripts.insert(handle, key);
                Ok((None, handle))
            }
        }
    }

    fn function(&self, handle: ScriptHandle) -> Result<Function<'_>, ScriptError> {
        let key = self
            .scripts
            .get(&handle)
            .ok_or_else(|| ScriptError::RuntimeError(format!("unknown ScriptHandle {handle:?}")))?;
        self.lua.registry_value(key).map_err(|e| ScriptError::RuntimeError(e.to_string()))
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
        // the String directly), then run it so its own initial_effect calls
        // Game.RegisterTriggeredAbility/etc itself, rather than load_function's
        // "caller wires the handle into a RegisteredAbility by hand" shape.
        // Blocked on the Game.* binding + registration-time mutability gap
        // load_function's own doc comment (and ScriptRuntime::new's TODO)
        // flags — not just the Lua load call itself.
        todo!()
    }

    /// Real body: builds a minimal Lua argument set from `ctx` and calls the
    /// loaded function, expecting a bool back. Only `GameEventKind::ZoneChange`
    /// is bridged into a Lua-visible shape this slice (`event_to_lua`, below) —
    /// every other event kind reaches the script as `nil`, which is enough
    /// for the one condition shape this slice exercises ("did `source` just
    /// enter the battlefield").
    pub fn invoke_condition(&self, handle: ScriptHandle, ctx: &ScriptContext) -> Result<bool, ScriptError> {
        let function = self.function(handle)?;
        let event_value = event_to_lua(&self.lua, ctx.event)?;
        function
            .call::<_, bool>((ctx.source.0, ctx.controller.0, event_value))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    pub fn invoke_cost(&self, handle: ScriptHandle, ctx: &ScriptContext) -> Result<(), ScriptError> {
        // TODO: same calling pattern as invoke_condition above.
        let _ = (handle, ctx);
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
        let _ = (handle, ctx);
        todo!()
    }

    /// Real body, narrowed design ("Lua asks, Rust decides"): the loaded
    /// function is called with `(source, controller)` and returns a plain
    /// table describing *intent* — it never mutates `state` itself, which
    /// sidesteps needing `&mut GameState` reachable from inside a Lua
    /// callback (the mutability gap this function's own TODO used to flag).
    /// Only the `gain_life` key is decoded this slice; every other effect
    /// shape (draw, damage, counters, ...) needs its own key + apply arm
    /// here once a card actually needs it.
    pub fn invoke_operation(
        &self,
        handle: ScriptHandle,
        state: &mut GameState,
        source: ObjectId,
        controller: PlayerId,
    ) -> Result<(), ScriptError> {
        let function = self.function(handle)?;
        let result: Table = function
            .call((source.0, controller.0))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        // TODO(more effect shapes): gain_life is the only key wired this
        // slice - draw_cards/deal_damage/add_counters/etc need their own
        // `result.get::<_, Option<_>>("...")` + apply arm here as real cards
        // need them.
        let gain_life: Option<i32> =
            result.get("gain_life").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        if let Some(amount) = gain_life {
            if let Some(player) = state.player_mut(controller) {
                player.gain_life(amount);
            }
        }
        Ok(())
    }
}

/// Builds the `Game.Conditions`/`Game.Ops` sugar tables and binds them into
/// `lua`'s globals as `Game.Conditions.*`/`Game.Ops.*`. Deliberately not the
/// same thing as binding `script_api.rs`'s functions (`Game.RegisterTriggeredAbility`
/// etc stay unbound) — just these two small factories so a condition/
/// operation source string can be a one-liner like `return Game.Conditions.Enters`
/// / `return Game.Ops.GainLife(1)` instead of a full hand-written function.
fn bind_game_globals(lua: &Lua) -> mlua::Result<()> {
    let conditions = lua.create_table()?;
    conditions.set("Enters", ConditionTag(ConditionPrimitive::EntersBattlefield))?;
    conditions.set("Always", ConditionTag(ConditionPrimitive::Always))?;

    let ops = lua.create_table()?;
    let gain_life = lua.create_function(|_, amount: i32| Ok(OperationTag(AbilityEffect::GainLife(amount))))?;
    ops.set("GainLife", gain_life)?;

    let game = lua.create_table()?;
    game.set("Conditions", conditions)?;
    game.set("Ops", ops)?;
    lua.globals().set("Game", game)
}

/// Bridges the one event shape this slice's condition scripts need to read —
/// `GameEventKind::ZoneChange` becomes a Lua table with `kind`/`object`/
/// `from`/`to` fields (zone names as lowercase strings). Every other
/// `GameEventKind` variant becomes `Value::Nil` for now.
///
/// TODO: extend as real conditions need more event shapes bridged (damage
/// dealt, life changed, tapped, ...) - this only covers what "did `source`
/// just enter the battlefield" needs to read.
fn event_to_lua<'lua>(lua: &'lua Lua, event: Option<&GameEvent>) -> Result<Value<'lua>, ScriptError> {
    let Some(event) = event else {
        return Ok(Value::Nil);
    };
    match &event.kind {
        GameEventKind::ZoneChange { object, from, to, .. } => {
            let table = lua.create_table().map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            table
                .set("kind", "zone_change")
                .and_then(|_| table.set("object", object.0))
                .and_then(|_| table.set("from", zone_kind_name(*from)))
                .and_then(|_| table.set("to", zone_kind_name(*to)))
                .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            Ok(Value::Table(table))
        }
        _ => Ok(Value::Nil),
    }
}

fn zone_kind_name(kind: ZoneKind) -> &'static str {
    match kind {
        ZoneKind::Library => "library",
        ZoneKind::Hand => "hand",
        ZoneKind::Battlefield => "battlefield",
        ZoneKind::Graveyard => "graveyard",
        ZoneKind::Stack => "stack",
        ZoneKind::Exile => "exile",
        ZoneKind::Command => "command",
    }
}
