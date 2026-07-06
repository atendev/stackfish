use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Function, Lua, RegistryKey, Table, UserData, UserDataMethods, Value};

use crate::types::ability::{
    AbilityCost, AbilityEffect, AbilityKind, ConditionPrimitive, ConditionSpec, RegisteredAbility,
};
use crate::types::card::{CardDefinition, ScriptReference};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::zone::ZoneKind;

/// Thin `mlua::UserData` wrapper around `ConditionPrimitive` so
/// `Game.Conditions.Enters`/`Game.Conditions.Always` are real Lua values a
/// condition script can `return` directly — decoded back out via
/// `decode_condition_value`.
#[derive(Clone, Copy)]
struct ConditionTag(ConditionPrimitive);
impl UserData for ConditionTag {}

/// Same idea as `ConditionTag`, for `Game.Ops.*` — e.g. `Game.Ops.GainLife(1)`
/// is a Lua function that returns one of these, decoded back out via
/// `decode_operation_value`.
#[derive(Clone)]
struct OperationTag(AbilityEffect);
impl UserData for OperationTag {}

/// Same idea, for `Game.Costs.*` — e.g. `Game.Costs.Tap` is a plain tagged
/// value (no arguments, unlike `Game.Ops.GainLife(n)`) decoded back out via
/// `decode_cost_value`.
#[derive(Clone, Copy)]
struct CostTag(AbilityCost);
impl UserData for CostTag {}

#[derive(Debug)]
pub enum ScriptError {
    LoadError(String),
    RuntimeError(String),
}

fn to_lua_err(e: ScriptError) -> mlua::Error {
    mlua::Error::RuntimeError(format!("{e:?}"))
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

/// Shared, interior-mutable store for loaded Lua callbacks plus whatever a
/// currently-running card script has self-registered so far — wrapped in
/// `Rc<RefCell<_>>` so a Lua-bound closure (`Game.CreateTriggeredAbility`,
/// and the `TriggeredAbilityBuilder` it returns) can reach it without ever
/// needing `&mut GameState`, or even `&mut ScriptRuntime`, mid-callback.
/// This is the actual fix for the registration-time mutability gap flagged
/// since Phase 2 — a Lua callback only ever touches this small side-channel,
/// never `GameState` itself.
#[derive(Default)]
struct ScriptRegistry {
    /// One entry per loaded callback (a condition/operation/etc function),
    /// keyed by the opaque `ScriptHandle` data types reference it by. A
    /// `RegistryKey` rather than holding the `mlua::Function` directly since
    /// `Function` borrows from `lua` — the registry is `mlua`'s own escape
    /// hatch for stashing a Lua value independent of that borrow.
    scripts: HashMap<ScriptHandle, RegistryKey>,
    next_handle: u64,
    /// Abilities a currently-running `initial_effect` has `:Register()`ed,
    /// waiting to be drained into `GameState.abilities` once the whole card
    /// script call returns (`ScriptRuntime::run_card_script`). Lua never
    /// gets a `GameState` reference at all — this is what `:Register()`
    /// pushes onto instead.
    pending_triggered: Vec<RegisteredAbility>,
}

impl ScriptRegistry {
    fn next_script_handle(&mut self) -> ScriptHandle {
        let handle = ScriptHandle(self.next_handle);
        self.next_handle += 1;
        handle
    }
}

/// Shared by `ScriptRuntime::load_condition` (decoding a freshly-`eval`'d
/// chunk) and `TriggeredAbilityBuilder::SetCondition` (decoding a value
/// already in hand, passed as a Lua call argument) — same two recognized
/// shapes either way: a `Game.Conditions.*` tag, or a custom function. Any
/// other value (including a bare `nil`, e.g. a typo'd `Game.Conditions.Typo`)
/// is a load error, not guessed at — see `RegisteredAbility::condition`'s
/// doc comment for why `nil` isn't silently treated as "always".
fn decode_condition_value(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    value: Value,
) -> Result<ConditionSpec, ScriptError> {
    match value {
        Value::UserData(ud) => {
            let tag: ConditionTag = ud
                .borrow::<ConditionTag>()
                .map_err(|e| ScriptError::LoadError(e.to_string()))?
                .clone();
            Ok(ConditionSpec::Primitive(tag.0))
        }
        Value::Function(function) => {
            let key = lua
                .create_registry_value(function)
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let handle = registry.next_script_handle();
            registry.scripts.insert(handle, key);
            Ok(ConditionSpec::Script(handle))
        }
        Value::Nil => Err(ScriptError::LoadError(
            "condition script returned nil - use `return Game.Conditions.Always` \
             for an explicit unconditional trigger, not an empty/typo'd chunk"
                .to_string(),
        )),
        other => Err(ScriptError::LoadError(format!(
            "condition script must return a Game.Conditions.* value or a function, got {}",
            other.type_name()
        ))),
    }
}

/// Same two-shapes pattern as `decode_condition_value`, for operations.
/// Shared by `ScriptRuntime::load_operation` and
/// `TriggeredAbilityBuilder::SetOperation`.
fn decode_operation_value(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    value: Value,
) -> Result<(Option<AbilityEffect>, ScriptHandle), ScriptError> {
    match value {
        Value::UserData(ud) => {
            let tag: OperationTag = ud
                .borrow::<OperationTag>()
                .map_err(|e| ScriptError::LoadError(e.to_string()))?
                .clone();
            Ok((Some(tag.0), ScriptHandle(0)))
        }
        Value::Function(function) => {
            let key = lua
                .create_registry_value(function)
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let handle = registry.next_script_handle();
            registry.scripts.insert(handle, key);
            Ok((None, handle))
        }
        Value::Nil => Err(ScriptError::LoadError(
            "operation script returned nil - expected a Game.Ops.* value or a function".to_string(),
        )),
        other => Err(ScriptError::LoadError(format!(
            "operation script must return a Game.Ops.* value or a function, got {}",
            other.type_name()
        ))),
    }
}

/// Same shape as `decode_condition_value`/`decode_operation_value`, for
/// `Game.Costs.*` — no `Script` fallback yet (`RegisteredAbility::cost`'s
/// only consumer, `ScriptRuntime::invoke_cost`, is still `todo!()`), so a
/// bare custom function isn't accepted here, unlike the other two decoders.
fn decode_cost_value(value: Value) -> Result<AbilityCost, ScriptError> {
    match value {
        Value::UserData(ud) => {
            let tag: CostTag =
                ud.borrow::<CostTag>().map_err(|e| ScriptError::LoadError(e.to_string()))?.clone();
            Ok(tag.0)
        }
        Value::Nil => Err(ScriptError::LoadError(
            "cost script returned nil - expected a Game.Costs.* value".to_string(),
        )),
        other => Err(ScriptError::LoadError(format!(
            "cost script must return a Game.Costs.* value, got {}",
            other.type_name()
        ))),
    }
}

/// `Game.CreateTriggeredAbility(c)`'s return value — a builder mirroring
/// ygopro's `Effect.CreateEffect(c)` / `e1:SetCondition(...)` /
/// `c:RegisterEffect(e1)` convention (see `docs/ygo_pro_reference.md`).
/// `:SetCondition`/`:SetOperation` accept either shape `decode_condition_value`/
/// `decode_operation_value` recognize. Omitting `:SetCondition` before
/// `:Register()` means unconditional (`condition: None`, same meaning
/// `RegisteredAbility::condition`'s doc already documents). `:Register()`
/// queues the finished `RegisteredAbility` onto the shared registry's
/// `pending_triggered` list rather than touching `GameState` directly.
///
/// `kind` is implied by which constructor built this (`CreateTriggeredAbility`
/// -> always `AbilityKind::Triggered`) rather than a settable field — this is
/// what permanently resolves `script_api.rs::register_triggered_ability`'s
/// old "no `AbilityKind` parameter" gap; `Game.CreateActivatedAbility`/
/// `Game.CreateStaticAbility` are the natural future siblings, not built yet.
///
/// `:SetCost`/`:SetTarget` are deliberately not bound this slice —
/// `ScriptRuntime::invoke_cost`/`invoke_target` are still `todo!()` regardless,
/// so nothing would consume them yet.
struct TriggeredAbilityBuilder {
    source: ObjectId,
    condition: Option<ConditionSpec>,
    operation: Option<(Option<AbilityEffect>, ScriptHandle)>,
    registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for TriggeredAbilityBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("SetCondition", |lua, this, value: Value| {
            let mut registry = this.registry.borrow_mut();
            let spec = decode_condition_value(lua, &mut registry, value).map_err(to_lua_err)?;
            drop(registry);
            this.condition = Some(spec);
            Ok(())
        });
        methods.add_method_mut("SetOperation", |lua, this, value: Value| {
            let mut registry = this.registry.borrow_mut();
            let decoded = decode_operation_value(lua, &mut registry, value).map_err(to_lua_err)?;
            drop(registry);
            this.operation = Some(decoded);
            Ok(())
        });
        methods.add_method_mut("Register", |_, this, ()| {
            let (effect, operation) = this.operation.clone().ok_or_else(|| {
                mlua::Error::RuntimeError(
                    "Register() called with no SetOperation() - a triggered ability needs an \
                     operation"
                        .to_string(),
                )
            })?;
            let ability = RegisteredAbility {
                source: this.source,
                kind: AbilityKind::Triggered,
                condition: this.condition,
                cost: None,
                target: None,
                operation,
                effect,
            };
            this.registry.borrow_mut().pending_triggered.push(ability);
            Ok(())
        });
    }
}

/// Owns the embedded Lua VM and every loaded card script. One `ScriptRuntime`
/// per running engine process, not per-game — scripts load once and are
/// reused across games, per DESIGN.md's "restart to reload, no hot-reload"
/// decision.
pub struct ScriptRuntime {
    lua: Lua,
    registry: Rc<RefCell<ScriptRegistry>>,
}

impl ScriptRuntime {
    pub fn new() -> Self {
        let lua = Lua::new();
        let registry = Rc::new(RefCell::new(ScriptRegistry::default()));
        bind_game_globals(&lua, Rc::clone(&registry)).expect(
            "binding the Game.Conditions/Game.Ops/Game.CreateTriggeredAbility sugar should never fail",
        );
        ScriptRuntime { lua, registry }
    }

    /// Loads `source` (a Lua chunk that must evaluate to a single function
    /// value, e.g. `return function(...) ... end`) and stores it under a
    /// fresh `ScriptHandle`. Narrower than `run_card_script` below — this
    /// loads one standalone callback, not a whole card script that registers
    /// several of its own via `Game.*`. Callers (test/scenario fixtures) use
    /// this to load a condition callback and an operation callback
    /// separately and build the `RegisteredAbility` referencing both handles
    /// directly, rather than a script registering itself.
    pub fn load_function(&mut self, source: &str) -> Result<ScriptHandle, ScriptError> {
        let function: Function =
            self.lua.load(source).eval().map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let key = self
            .lua
            .create_registry_value(function)
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let mut registry = self.registry.borrow_mut();
        let handle = registry.next_script_handle();
        registry.scripts.insert(handle, key);
        Ok(handle)
    }

    /// Loads a condition source string that may be either shape: a full
    /// custom `return function(source, controller, event) ... end` (falls
    /// back to `ConditionSpec::Script`, invoked for real via
    /// `invoke_condition` at trigger-check time) or a one-liner referencing
    /// `Game.Conditions.*` (`ConditionSpec::Primitive`, resolved once here —
    /// zero further Lua calls). See `decode_condition_value` for the shared
    /// decode logic (also used by `TriggeredAbilityBuilder::SetCondition`).
    pub fn load_condition(&mut self, source: &str) -> Result<ConditionSpec, ScriptError> {
        let value: Value =
            self.lua.load(source).eval().map_err(|e| ScriptError::LoadError(e.to_string()))?;
        decode_condition_value(&self.lua, &mut self.registry.borrow_mut(), value)
    }

    /// Same two-shapes-at-load-time pattern as `load_condition`, for
    /// operations. A recognized `Game.Ops.*` value decodes into
    /// `(Some(AbilityEffect), ScriptHandle(0))` — the handle is an unused
    /// placeholder, same convention CR 605 mana abilities already use for
    /// `RegisteredAbility::operation` when `effect` is `Some`. A custom
    /// function falls back to `(None, <real handle>)`, invoked via
    /// `invoke_operation`. See `decode_operation_value` for the shared
    /// decode logic (also used by `TriggeredAbilityBuilder::SetOperation`).
    pub fn load_operation(
        &mut self,
        source: &str,
    ) -> Result<(Option<AbilityEffect>, ScriptHandle), ScriptError> {
        let value: Value =
            self.lua.load(source).eval().map_err(|e| ScriptError::LoadError(e.to_string()))?;
        decode_operation_value(&self.lua, &mut self.registry.borrow_mut(), value)
    }

    /// Loads a cost source string that resolves to a `Game.Costs.*` value
    /// (e.g. `return Game.Costs.Tap`) — same one-liner-eval convention as
    /// `load_condition`/`load_operation`, narrower: only the recognized-
    /// primitive shape exists for costs this slice (see `decode_cost_value`).
    pub fn load_cost(&mut self, source: &str) -> Result<AbilityCost, ScriptError> {
        let value: Value =
            self.lua.load(source).eval().map_err(|e| ScriptError::LoadError(e.to_string()))?;
        decode_cost_value(value)
    }

    fn function(&self, handle: ScriptHandle) -> Result<Function<'_>, ScriptError> {
        let registry = self.registry.borrow();
        let key = registry
            .scripts
            .get(&handle)
            .ok_or_else(|| ScriptError::RuntimeError(format!("unknown ScriptHandle {handle:?}")))?;
        self.lua.registry_value(key).map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// Loads `definition.script_ref`'s source, evaluates it to a Lua table
    /// with a callable `initial_effect` field (the ygopro-style
    /// `s.initial_effect(c)` module convention — see `docs/lua_scripts.md`),
    /// and calls it with `source.0`. Whatever the script's `initial_effect`
    /// `:Register()`ed onto the shared registry's pending-abilities queue
    /// during the call is drained into `state.abilities` right after —
    /// `Game.CreateTriggeredAbility`'s builder never touches `state` itself,
    /// only this drain does.
    ///
    /// Re-parses/re-evaluates the source fresh on every call rather than
    /// caching the compiled chunk per `CardDefinition` — simpler for this
    /// slice; `CardDefinition` has no stable cache key today to key a cache
    /// off of, and re-parsing a short chunk is cheap enough at this scale.
    ///
    /// Only `ScriptReference::Inline` is handled — `ByKey` returns an error
    /// rather than guessing at a resolution convention. Per `DESIGN.md`'s
    /// "no card database tooling" non-goal, resolving a key to source is the
    /// external database project's job; real integration is expected to
    /// always hand this engine an already-resolved `Inline` source.
    pub fn run_card_script(
        &mut self,
        state: &mut GameState,
        definition: &CardDefinition,
        source: ObjectId,
    ) -> Result<(), ScriptError> {
        let source_code = match &definition.script_ref {
            ScriptReference::Inline(source) => source,
            ScriptReference::ByKey(_) => {
                return Err(ScriptError::LoadError(
                    "ScriptReference::ByKey is not resolvable by this engine - the card database \
                     project is expected to hand back Inline source"
                        .to_string(),
                ));
            }
        };

        let module: Table =
            self.lua.load(source_code).eval().map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let initial_effect: Function = module
            .get("initial_effect")
            .map_err(|e| ScriptError::LoadError(format!("card script has no initial_effect: {e}")))?;
        initial_effect
            .call::<_, ()>(source.0)
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;

        let mut registered: Vec<RegisteredAbility> =
            self.registry.borrow_mut().pending_triggered.drain(..).collect();
        state.abilities.append(&mut registered);
        Ok(())
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

/// Builds the `Game.Conditions`/`Game.Ops`/`Game.Costs`/
/// `Game.CreateTriggeredAbility` sugar and binds it into `lua`'s globals.
/// Deliberately not the same thing
/// as binding all of `script_api.rs`'s functions (`Game.AddContinuousEffect`/
/// `Game.ReplaceEvent`/`Game.CastSpell`/`Game.PayCost`/`Game.SelectTargets`
/// stay unbound) — just enough sugar for a condition/operation source
/// string to be a one-liner, and for a card script to self-register a
/// triggered ability via the `TriggeredAbilityBuilder` above.
fn bind_game_globals(lua: &Lua, registry: Rc<RefCell<ScriptRegistry>>) -> mlua::Result<()> {
    let conditions = lua.create_table()?;
    conditions.set("Enters", ConditionTag(ConditionPrimitive::EntersBattlefield))?;
    conditions.set("Always", ConditionTag(ConditionPrimitive::Always))?;

    let ops = lua.create_table()?;
    let gain_life = lua.create_function(|_, amount: i32| Ok(OperationTag(AbilityEffect::GainLife(amount))))?;
    ops.set("GainLife", gain_life)?;

    let costs = lua.create_table()?;
    costs.set("Tap", CostTag(AbilityCost::Tap))?;

    let create_triggered_ability = {
        let registry = Rc::clone(&registry);
        lua.create_function(move |_, source: u64| {
            Ok(TriggeredAbilityBuilder {
                source: ObjectId(source),
                condition: None,
                operation: None,
                registry: Rc::clone(&registry),
            })
        })?
    };

    let game = lua.create_table()?;
    game.set("Conditions", conditions)?;
    game.set("Ops", ops)?;
    game.set("Costs", costs)?;
    game.set("CreateTriggeredAbility", create_triggered_ability)?;
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
