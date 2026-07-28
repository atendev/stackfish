//! `ScriptRuntime` — the crate's real script-bridge API surface, called by
//! `engine.rs`/`commands/`/`layers.rs`/`replacement.rs`/`sba.rs`/
//! `triggers.rs`. Split out of the original monolithic `script.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, Table, Thread, ThreadStatus, Value};

use crate::types::ability::{AbilityEffect, ConditionSpec, RegisteredAbility};
use crate::types::card::{CardDefinition, CardTypeSet, KeywordSet, ScriptReference};
use crate::types::cost::CostModifier;
use crate::types::effect::{ContinuousEffect, EffectPrimitive, ReplacementEffect};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::player::HandSizeModifier;
use crate::types::target::TargetSpec;

use super::decode::{
    decode_condition_value, decode_operation_value, decode_script_choice_request, decode_target_requirement_table,
    parse_card_type_name, parse_color_string, parse_keyword_name,
};
use super::encode::{event_to_lua, state_to_lua};
use super::globals::bind_game_globals;
use super::types::{OperationOutcome, ScriptContext, ScriptError, ScriptRegistry};

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

    fn function(&self, handle: ScriptHandle) -> Result<Function<'_>, ScriptError> {
        let registry = self.registry.borrow();
        let key = registry
            .scripts
            .get(&handle)
            .ok_or_else(|| ScriptError::RuntimeError(format!("unknown ScriptHandle {handle:?}")))?;
        self.lua.registry_value(key).map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// Loads `definition.script_ref`'s source, evaluates it to a Lua table
    /// with a callable `register_abilities` field (the
    /// `s.register_abilities(c)` module convention — see `docs/lua/card-script-basics.md`),
    /// and calls it with `source.0`. Whatever the script's `register_abilities`
    /// `:Register()`ed onto the shared registry's pending queues during the
    /// call is drained right after — `pending_triggered` into
    /// `state.abilities`, `pending_static` into `state.continuous_effects`
    /// (stamped with a real `timestamp` here, the only place with `&mut
    /// GameState` to allocate one), `pending_replacement` into
    /// `state.replacement_effects` (no timestamp to stamp - see
    /// `ReplacementEffect`'s own doc comment). None of `Game.
    /// CreateTriggeredAbility`'s/`Game.CreateStaticAbility`'s/`Game.
    /// CreateReplacementEffect`'s builders ever touch `state` itself — only
    /// this drain does.
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
        let register_abilities: Function = module
            .get("register_abilities")
            .map_err(|e| ScriptError::LoadError(format!("card script has no register_abilities: {e}")))?;
        register_abilities
            .call::<_, ()>(source.0)
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;

        let mut registered: Vec<RegisteredAbility> =
            self.registry.borrow_mut().pending_triggered.drain(..).collect();
        for ability in &mut registered {
            // Real id (AbilityId's own doc comment) - the builder itself has
            // no &mut GameState to allocate one from inside a Lua callback,
            // same reason the static-effect timestamp below is stamped here
            // too.
            ability.id = state.allocate_ability_id();
        }
        state.abilities.append(&mut registered);

        let mut static_effects: Vec<ContinuousEffect> =
            self.registry.borrow_mut().pending_static.drain(..).collect();
        for effect in &mut static_effects {
            // Real timestamp (CR 613.7 ordering) - the builder itself has no
            // &mut GameState to allocate one from inside a Lua callback (see
            // StaticAbilityBuilder's own doc comment).
            effect.timestamp = state.allocate_sequence_no();
        }
        state.continuous_effects.append(&mut static_effects);

        let mut replacement_effects: Vec<ReplacementEffect> =
            self.registry.borrow_mut().pending_replacement.drain(..).collect();
        state.replacement_effects.append(&mut replacement_effects);

        let mut cost_modifiers: Vec<CostModifier> =
            self.registry.borrow_mut().pending_cost_modifiers.drain(..).collect();
        for modifier in &mut cost_modifiers {
            // Real controller (CostModifierBuilder's own doc comment) - the
            // builder itself has no &GameState to read this source's live
            // controller from inside a Lua callback.
            modifier.controller = state.object(modifier.source).map(|o| o.controller).expect(
                "CostModifier's source must already exist in GameState by drain time - \
                 Game.CreateCostModifier was called with a bogus source id",
            );
        }
        state.cost_modifiers.append(&mut cost_modifiers);

        let mut hand_size_modifiers: Vec<HandSizeModifier> =
            self.registry.borrow_mut().pending_hand_size_modifiers.drain(..).collect();
        for modifier in &mut hand_size_modifiers {
            // Real controller and timestamp (HandSizeModifierBuilder's own
            // doc comment) - the builder itself has no &GameState to read
            // this source's live controller from, or allocate a real
            // sequence number, inside a Lua callback.
            modifier.controller = state.object(modifier.source).map(|o| o.controller).expect(
                "HandSizeModifier's source must already exist in GameState by drain time - \
                 Game.CreateHandSizeModifier was called with a bogus source id",
            );
            modifier.timestamp = state.allocate_sequence_no();
        }
        state.hand_size_modifiers.append(&mut hand_size_modifiers);
        Ok(())
    }

    /// CR 702.5a/303.4a: a card's own base target requirement (an Aura's
    /// "Enchant [quality]" line, or any other targeted spell's targeting) as
    /// declared by its own script, via the optional `s.target_requirement()`
    /// module convention — a sibling to `s.register_abilities(c)`, not a
    /// replacement. Called once per `CardDefinition` build, not per object
    /// per zone-entry the way `run_card_script` is: unlike an Aura's granted
    /// abilities (which only matter once it's attached to something), its
    /// own Enchant restriction is fixed text needed *before* it's ever cast
    /// — while it's still sitting in hand — so it can't wait for
    /// `run_card_script`'s battlefield/token/copy-entry timing. No
    /// `GameState`/`ObjectId` involved for the same reason: the restriction
    /// doesn't vary by which specific object ends up carrying it.
    ///
    /// `target_requirement()` takes no arguments and returns the same flat
    /// filter table `Game.SelectTargets`/`ActivatedAbilityBuilder::SetTarget`
    /// already accept (`decode_target_filter_table`) — or, for a legality
    /// shape the flat table can't express, a `filter` key holding a
    /// scripted-predicate function (or `{type="player", filter=fn}`), same
    /// as `:SetTarget` accepts (`decode_target_filter_value`, see
    /// `decode_target_requirement_table`'s own doc comment) — plus
    /// `count`/`up_to`/`at_least`/`exclude_source` as sibling keys —
    /// identical shape to the scenario crate's own TOML
    /// `[cards.target_requirement]` table (`TargetRequirementSpec`), just
    /// Lua-authored. A card with no `target_requirement` function at all
    /// (the overwhelming majority) returns `Ok(None)` — same "requires no
    /// targets" default `CardDefinition::target_requirement`'s own doc
    /// comment already documents.
    ///
    /// `&self`, not `&mut self`, and re-evaluates `source` fresh rather than
    /// caching — same reasoning `run_card_script`'s own doc comment gives for
    /// its identical choice. Registering a scripted predicate's function
    /// still only needs `&self` despite mutating the script registry — same
    /// `Rc<RefCell<ScriptRegistry>>` interior mutability every other builder
    /// method already relies on.
    pub fn build_target_requirement(&self, source: &str) -> Result<Option<TargetSpec>, ScriptError> {
        let module: Table =
            self.lua.load(source).eval().map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let function: Option<Function> =
            module.get("target_requirement").map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let Some(function) = function else { return Ok(None) };
        let table: Table =
            function.call(()).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        let mut registry = self.registry.borrow_mut();
        let requirement = decode_target_requirement_table(&self.lua, &mut registry, table)?;
        Ok(Some(TargetSpec::Primitive(requirement)))
    }

    /// CR 608.2m: a plain spell's own resolution effect (Armageddon's
    /// "Destroy all lands," Shock's "deals 2 damage to any target," ...) as
    /// declared by its own script, via the optional `s.operation()` module
    /// convention — another sibling to `s.register_abilities(c)`/
    /// `s.target_requirement()`, not a replacement for either. A plain
    /// spell has no ability wrapper at all, and `register_abilities(c)`
    /// only ever runs once the card becomes a *battlefield* object (see
    /// that function's own doc comment) — an instant/sorcery that resolves
    /// straight to the graveyard never reaches that point, so its own
    /// effect can't be declared there either. Same timing fix
    /// `build_target_requirement` already applies: called once per
    /// `CardDefinition` build, no `GameState`/`ObjectId` needed.
    ///
    /// `operation()` takes no arguments and returns the same `Game.Ops.*`
    /// tag or custom-function shape `TriggeredAbilityBuilder::SetOperation`/
    /// `ActivatedAbilityBuilder::SetOperation` already accept — reuses
    /// `decode_operation_value` unchanged, so no new Lua-facing vocabulary
    /// exists just for spells. A card with no `operation` function at all
    /// (the overwhelming majority — anything that becomes a permanent, or a
    /// targeted/no-effect instant/sorcery not yet built this way) returns
    /// `Ok(None)`, same "this spell's effect can only be carried out via
    /// the script bridge... not yet possible" default `CardDefinition::
    /// effect`'s own doc comment already documents (now closed for the one
    /// case this method covers).
    ///
    /// Deliberately does *not* add a sibling `operation: ScriptHandle` field
    /// to `CardDefinition` to carry the custom-function-fallback handle
    /// (unlike `RegisteredAbility`, which has both `effect` and `operation`
    /// side by side) — `CardDefinition` has no `Default` impl and ~400
    /// existing struct-literal construction sites across the codebase,
    /// a ripple a previous slice explicitly flagged as prohibitive and only
    /// paid once, with sign-off, when unavoidable (`CardDefinition::
    /// mode_choice`). Instead, a custom-function result is packed into the
    /// *existing* `effect: Option<AbilityEffect>` field via the new
    /// `AbilityEffect::Script(ScriptHandle)` variant (see its own doc
    /// comment) — `commands::execute_command`'s `CastSpell` arm unpacks it
    /// back into `StackObject`'s separate `operation`/`effect` fields at
    /// push time, so `ResolveStackTop` never has to know the difference.
    ///
    /// **Caller trap:** a custom-function result's `ScriptHandle` is only
    /// ever valid against *this* `ScriptRuntime` instance's own registry —
    /// it means nothing to a different `ScriptRuntime` (e.g. a fresh one a
    /// just-constructed `Engine` creates internally). A caller building a
    /// `CardDefinition` ahead of time with a throwaway runtime and then
    /// handing it to `Engine::new` will see the custom function silently
    /// never run (`ResolveStackTop`'s own `Err(_) => {}` arm swallows the
    /// resulting "unknown ScriptHandle" error - a pre-existing, separately
    /// tracked gap, not new here). Call this through the same `Engine`
    /// whose game will actually resolve the spell instead — see
    /// `register_script_operation` (scenario crate) for the real pattern:
    /// construct first with `effect: None`, then patch it in via `Engine::
    /// build_operation` afterward.
    pub fn build_operation(
        &self,
        source: &str,
    ) -> Result<Option<(Option<AbilityEffect>, ScriptHandle)>, ScriptError> {
        let module: Table =
            self.lua.load(source).eval().map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let function: Option<Function> =
            module.get("operation").map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let Some(function) = function else { return Ok(None) };
        let value: Value = function.call(()).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        let mut registry = self.registry.borrow_mut();
        let decoded = decode_operation_value(&self.lua, &mut registry, value)?;
        Ok(Some(decoded))
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
        // Backlog item 7: a minimal state snapshot as a 4th arg - additive
        // and non-breaking (Lua ignores extra call args a function doesn't
        // declare), so every existing 3-arg condition script keeps working
        // unchanged. See state_to_lua's own doc comment for what's in it.
        let state_value = state_to_lua(&self.lua, ctx.state)?;
        function
            .call::<_, bool>((ctx.source.0, ctx.controller.0, event_value, state_value))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// CR 603.4: resolves a `Script`-backed intervening-if predicate.
    /// Unlike `invoke_condition`, there's no event to bridge - `
    /// InterveningIfSpec`'s own doc comment already establishes this is "a
    /// pure state predicate, not event-shaped" - so the Lua call shape is
    /// `function(source, controller, snapshot)`, mirroring
    /// `invoke_condition`'s trailing state-snapshot argument without the
    /// event one.
    pub fn invoke_intervening_if(
        &self,
        handle: ScriptHandle,
        state: &GameState,
        source: ObjectId,
        controller: PlayerId,
    ) -> Result<bool, ScriptError> {
        let function = self.function(handle)?;
        let state_value = state_to_lua(&self.lua, state)?;
        function
            .call::<_, bool>((source.0, controller.0, state_value))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// CR 604.3/208.2a: resolves a characteristic-defining power/toughness
    /// callback. `source_code` is evaluated fresh every call — no
    /// `ScriptHandle`/registry entry, same one-shot-eval convention
    /// `run_card_script` already uses for `ScriptReference::Inline` — and is
    /// expected to evaluate to `function(source, controller) -> integer`,
    /// same argument shape `invoke_operation` already uses. `&self`, not
    /// `&mut self`: `Lua::load`/`.eval()`/`.call()` only need `&Lua`, and
    /// this doesn't touch the script registry at all.
    ///
    /// A `state_to_lua` snapshot is passed as a trailing 3rd arg — same
    /// additive, non-breaking convention `invoke_condition`'s trailing
    /// state-snapshot arg already established (Lua ignores extra call args a
    /// function doesn't declare). This is what makes a real "count cards in
    /// your graveyard"-style CDA buildable — closes backlog item 8's
    /// hidden-information-safe `ScriptContext` half for CDA specifically.
    pub fn invoke_cda(
        &self,
        source_code: &str,
        state: &GameState,
        source: ObjectId,
        controller: PlayerId,
    ) -> Result<i32, ScriptError> {
        let function: Function =
            self.lua.load(source_code).eval().map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let state_value = state_to_lua(&self.lua, state)?;
        function
            .call::<_, i32>((source.0, controller.0, state_value))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// `AffectedFilter::Script`'s real body (CR 613's "affects" predicate,
    /// evaluated per candidate object) — expects `function(object, source,
    /// controller, state) -> bool`. A `state_to_lua` snapshot is now passed
    /// as a trailing 4th arg, same convention `invoke_cda` above just picked
    /// up — a script can decide from real (public) board state, not just
    /// object/source/controller identity, closing backlog item 8's
    /// hidden-information-safe `ScriptContext` half for static-ability
    /// "affects" predicates. Called from `layers::applicable_effects_in_layer`,
    /// not from `ContinuousEffect::applies_to` — that method stays
    /// `Primitive`-only so `types/effect.rs` never needs to depend on
    /// `ScriptRuntime`, same separation `layers::resolve_base_pt` already
    /// keeps for CDA.
    ///
    /// CR 303.4m: `enchanted` (whatever `source` is currently attached to,
    /// `None` if nothing) is passed as a trailing 5th arg — nil in Lua when
    /// `None` — so a script can write `object == enchanted` directly instead
    /// of re-deriving it from the `state` snapshot's own per-card
    /// `attached_to` field. Appended *after* `state` rather than inserted
    /// earlier in the list, so an existing script written against the old
    /// 4-arg signature keeps working unmodified — Lua silently ignores a
    /// trailing argument a function doesn't declare a parameter for.
    pub fn invoke_continuous_affects(
        &self,
        handle: ScriptHandle,
        state: &GameState,
        object: ObjectId,
        source: ObjectId,
        controller: PlayerId,
        enchanted: Option<ObjectId>,
    ) -> Result<bool, ScriptError> {
        let function = self.function(handle)?;
        let state_value = state_to_lua(&self.lua, state)?;
        function
            .call::<_, bool>((object.0, source.0, controller.0, state_value, enchanted.map(|id| id.0)))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// `EffectPayload::Script`'s real body — the "Lua asks, Rust decides"
    /// table convention `invoke_operation` already established, generalized
    /// to the seven `EffectPrimitive` shapes a continuous effect can take
    /// instead of `invoke_operation`'s single `gain_life` key. Expects
    /// `function(source, controller, state) -> table` — a `state_to_lua`
    /// snapshot is now passed as a trailing 3rd arg, same convention
    /// `invoke_cda`/`invoke_continuous_affects` above pick up, closing
    /// backlog item 8's hidden-information-safe `ScriptContext` half for
    /// scripted continuous-effect payloads. Checks each recognized key in a
    /// fixed order and returns the first match:
    /// - `set_name = "..."` -> `SetName`
    /// - `set_pt = {power=.., toughness=..}` -> `SetPT`
    /// - `modify_pt = {power=.., toughness=..}` -> `ModifyPT`
    /// - `add_keyword = {"Flying", ...}` -> `AddKeyword`
    /// - `add_card_type = {"Creature", ...}` -> `AddCardType`
    /// - `add_color = "WU"` -> `AddColor`
    /// - `switch_pt = true` -> `SwitchPT`
    /// - `remove_all_abilities = true` -> `RemoveAllAbilities`
    /// - `double_pt = {power=.., toughness=..}` (bools) -> `DoublePT` — no
    ///   `Game.Effects.DoublePT` tag factory exists (no card needs one yet,
    ///   same "no real card" posture several other narrow gaps in this
    ///   engine already accept), so this script-backed shape is currently
    ///   the only way to author one from Lua. Added so a scripted effect can
    ///   participate in `layers::depends_on`'s one real CR 613.8a dependency
    ///   rule (`DoublePT` vs. `ModifyPT`/`DoublePT`) exactly like a
    ///   `Game.Effects.ModifyPT`-authored primitive already does — see that
    ///   function's own doc comment.
    ///
    /// An empty/unrecognized table, or a script that errors, resolves to
    /// `None` — same "skip rather than guess" stance every other
    /// unresolvable value in this engine already takes; the caller
    /// (`layers::resolve_primitive`) doesn't distinguish "no effect this
    /// call" from "script errored," same as `invoke_cda`'s own caller.
    ///
    /// CR 303.4m: `enchanted` is passed as a trailing 4th arg, same
    /// "appended after `state`, existing scripts unaffected" convention
    /// `invoke_continuous_affects` above establishes — lets a dynamic value
    /// computation (e.g. "equal to the enchanted creature's own power")
    /// reference it directly.
    pub fn invoke_continuous_effect(
        &self,
        handle: ScriptHandle,
        state: &GameState,
        source: ObjectId,
        controller: PlayerId,
        enchanted: Option<ObjectId>,
    ) -> Result<Option<EffectPrimitive>, ScriptError> {
        let function = self.function(handle)?;
        let state_value = state_to_lua(&self.lua, state)?;
        let result: Table = function
            .call((source.0, controller.0, state_value, enchanted.map(|id| id.0)))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;

        if let Some(new_name) = result.get::<_, Option<String>>("set_name").map_err(|e| ScriptError::RuntimeError(e.to_string()))? {
            return Ok(Some(EffectPrimitive::SetName(new_name)));
        }
        if let Some(t) = result.get::<_, Option<Table>>("set_pt").map_err(|e| ScriptError::RuntimeError(e.to_string()))? {
            let power: i32 = t.get("power").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            let toughness: i32 = t.get("toughness").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            return Ok(Some(EffectPrimitive::SetPT { power, toughness }));
        }
        if let Some(t) = result.get::<_, Option<Table>>("modify_pt").map_err(|e| ScriptError::RuntimeError(e.to_string()))? {
            let power: i32 = t.get("power").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            let toughness: i32 = t.get("toughness").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            return Ok(Some(EffectPrimitive::ModifyPT { power, toughness }));
        }
        if let Some(t) = result.get::<_, Option<Table>>("add_keyword").map_err(|e| ScriptError::RuntimeError(e.to_string()))? {
            let mut set = KeywordSet::empty();
            for entry in t.sequence_values::<String>() {
                let name = entry.map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
                let Ok(kw) = parse_keyword_name(&name) else { return Ok(None) };
                set |= kw;
            }
            return Ok(Some(EffectPrimitive::AddKeyword(set)));
        }
        if let Some(t) = result.get::<_, Option<Table>>("remove_keyword").map_err(|e| ScriptError::RuntimeError(e.to_string()))? {
            let mut set = KeywordSet::empty();
            for entry in t.sequence_values::<String>() {
                let name = entry.map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
                let Ok(kw) = parse_keyword_name(&name) else { return Ok(None) };
                set |= kw;
            }
            return Ok(Some(EffectPrimitive::RemoveKeyword(set)));
        }
        if let Some(t) = result.get::<_, Option<Table>>("add_card_type").map_err(|e| ScriptError::RuntimeError(e.to_string()))? {
            let mut set = CardTypeSet::empty();
            for entry in t.sequence_values::<String>() {
                let name = entry.map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
                let Ok(ct) = parse_card_type_name(&name) else { return Ok(None) };
                set |= ct;
            }
            return Ok(Some(EffectPrimitive::AddCardType(set)));
        }
        if let Some(letters) = result.get::<_, Option<String>>("add_color").map_err(|e| ScriptError::RuntimeError(e.to_string()))? {
            let Ok(set) = parse_color_string(&letters) else { return Ok(None) };
            return Ok(Some(EffectPrimitive::AddColor(set)));
        }
        if let Some(true) = result.get::<_, Option<bool>>("switch_pt").map_err(|e| ScriptError::RuntimeError(e.to_string()))? {
            return Ok(Some(EffectPrimitive::SwitchPT));
        }
        if let Some(true) = result.get::<_, Option<bool>>("remove_all_abilities").map_err(|e| ScriptError::RuntimeError(e.to_string()))? {
            return Ok(Some(EffectPrimitive::RemoveAllAbilities));
        }
        if let Some(t) = result.get::<_, Option<Table>>("double_pt").map_err(|e| ScriptError::RuntimeError(e.to_string()))? {
            let power: bool = t.get("power").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            let toughness: bool = t.get("toughness").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            return Ok(Some(EffectPrimitive::DoublePT { power, toughness }));
        }
        Ok(None)
    }

    pub fn invoke_cost(&self, handle: ScriptHandle, ctx: &ScriptContext) -> Result<(), ScriptError> {
        // TODO: same calling pattern as invoke_condition above.
        let _ = (handle, ctx);
        todo!()
    }

    /// `TargetFilter::Script`'s real body (CR 601.2c/602.2b's target
    /// legality check, evaluated per already-chosen candidate) — expects
    /// `function(object, source, controller, state) -> bool`, same call
    /// shape `invoke_continuous_affects` established for
    /// `AffectedFilter::Script`. This is a *static* legality predicate
    /// checked once at cast/activation time, not a `Game.SelectTargets`
    /// suspension — no coroutine needed, unlike that unrelated mechanism
    /// (see `ScriptChoiceRequest::SelectTarget`'s own doc comment). Called
    /// from `commands::target_is_legal`.
    pub fn invoke_target_legal(
        &self,
        handle: ScriptHandle,
        state: &GameState,
        object: ObjectId,
        source: ObjectId,
        controller: PlayerId,
    ) -> Result<bool, ScriptError> {
        let function = self.function(handle)?;
        let state_value = state_to_lua(&self.lua, state)?;
        function
            .call::<_, bool>((object.0, source.0, controller.0, state_value))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// `TargetFilter::PlayerScript`'s real body — `invoke_target_legal`'s
    /// player-candidate twin, `function(player, source, controller, state)
    /// -> bool`. Called from `commands::target_is_legal`.
    pub fn invoke_player_target_legal(
        &self,
        handle: ScriptHandle,
        state: &GameState,
        player: PlayerId,
        source: ObjectId,
        controller: PlayerId,
    ) -> Result<bool, ScriptError> {
        let function = self.function(handle)?;
        let state_value = state_to_lua(&self.lua, state)?;
        function
            .call::<_, bool>((player.0, source.0, controller.0, state_value))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// Real body, narrowed design ("Lua asks, Rust decides"): the loaded
    /// function is invoked as a coroutine (`Lua::create_thread` +
    /// `Thread::resume`) rather than a plain call — transparent for any
    /// script that never yields (a fresh thread's first `resume` behaves
    /// exactly like calling the function directly, confirmed against real
    /// mlua in `phase5_coroutine_spike.rs`), and the only way a script can
    /// later suspend on `Game.SelectTargets` (backlog item 9). See
    /// `handle_operation_result` for what happens with what comes back —
    /// it never mutates `state` itself on the "yielded" path, which
    /// sidesteps needing `&mut GameState` reachable from inside a Lua
    /// callback (the mutability gap this function's own TODO used to flag).
    pub fn invoke_operation(
        &self,
        handle: ScriptHandle,
        state: &mut GameState,
        source: ObjectId,
        controller: PlayerId,
    ) -> Result<OperationOutcome, ScriptError> {
        let function = self.function(handle)?;
        let thread =
            self.lua.create_thread(function).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        let result: Value =
            thread.resume((source.0, controller.0)).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        self.handle_operation_result(thread, result, state, controller)
    }

    /// Resumes a `Game.SelectTargets` suspension (`OperationOutcome::
    /// NeedsChoice`'s own `thread` handle) with the player's chosen target —
    /// the real answer to whatever `ScriptChoiceRequest` was yielded.
    /// `Engine::submit_script_choice` is the only intended caller. Same
    /// `handle_operation_result` tail as `invoke_operation` — a script that
    /// yields a *second* time (asks another question) produces another
    /// `NeedsChoice` rather than being assumed to finish after one answer.
    ///
    /// `answer` resumes the coroutine as a tagged table (`{ kind = "object"|
    /// "player", id = n }`), not a bare number: `ObjectId` and `PlayerId`
    /// are both plain integers to Lua, so an object-only answer could get
    /// away with a raw number (backlog item 9's original shape), but once a
    /// player answer is possible the script needs an explicit way to tell
    /// them apart rather than trusting its own memory of which filter it
    /// asked for.
    pub fn resume_operation(
        &self,
        thread_handle: ScriptHandle,
        state: &mut GameState,
        controller: PlayerId,
        answer: Target,
    ) -> Result<OperationOutcome, ScriptError> {
        let thread = self.thread(thread_handle)?;
        let answer_table = self.lua.create_table().map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        match answer {
            Target::Object(id) => {
                answer_table.set("kind", "object").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
                answer_table.set("id", id.0).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            }
            Target::Player(id) => {
                answer_table.set("kind", "player").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
                answer_table.set("id", id.0).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            }
        }
        let result: Value =
            thread.resume(answer_table).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        self.handle_operation_result(thread, result, state, controller)
    }

    /// Resumes a `Game.MayChoose` suspension (backlog item 9a, CR 608.2d)
    /// with a plain `bool` — unlike `resume_operation`'s tagged `{kind, id}`
    /// table, `Game.MayChoose()` just wants `coroutine.yield`'s return value
    /// to be the yes/no answer directly, so this resumes with a raw Lua
    /// boolean instead. Shares `handle_operation_result` as its tail, same
    /// as `resume_operation`. `Engine::submit_script_may_choice` is the only
    /// intended caller.
    pub fn resume_operation_bool(
        &self,
        thread_handle: ScriptHandle,
        state: &mut GameState,
        controller: PlayerId,
        answer: bool,
    ) -> Result<OperationOutcome, ScriptError> {
        let thread = self.thread(thread_handle)?;
        let result: Value =
            thread.resume(answer).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        self.handle_operation_result(thread, result, state, controller)
    }

    /// Shared tail for `invoke_operation`/`resume_operation`: `thread`
    /// either yielded again (a real `Game.SelectTargets` ask, decoded into a
    /// `ScriptChoiceRequest` and stashed via the registry for a later
    /// `resume_operation` call) or finished for good, in which case `result`
    /// is the operation's own intent table — same `gain_life` decode
    /// `invoke_operation` always had, same strictness (a non-table return
    /// from a finished operation is a real error, not silently ignored —
    /// matches the old `function.call::<_, Table>()`'s own typed-return
    /// strictness exactly). TODO(more effect shapes): draw_cards/
    /// deal_damage/add_counters/etc need their own `result.get::<_,
    /// Option<_>>("...")` + apply arm here as real cards need them, same
    /// pre-existing gap this function always had.
    fn handle_operation_result(
        &self,
        thread: Thread,
        result: Value,
        state: &mut GameState,
        controller: PlayerId,
    ) -> Result<OperationOutcome, ScriptError> {
        if thread.status() == ThreadStatus::Resumable {
            let Value::Table(request_table) = result else {
                return Err(ScriptError::RuntimeError(format!(
                    "script yielded a non-table request: {}",
                    result.type_name()
                )));
            };
            let request = decode_script_choice_request(request_table)?;
            let thread_handle = self.stash_thread(thread)?;
            return Ok(OperationOutcome::NeedsChoice { thread: thread_handle, request });
        }
        let Value::Table(result) = result else {
            return Err(ScriptError::RuntimeError(format!(
                "operation must return a table, got {}",
                result.type_name()
            )));
        };
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
        Ok(OperationOutcome::Finished)
    }

    /// Backlog item 9's own `function`/registry-lookup sibling — see that
    /// method's own doc comment for the `RegistryKey` rationale, identical
    /// here except for `suspended_threads` vs `scripts` and the `T` being
    /// retrieved (`Thread` vs `Function`).
    fn thread(&self, handle: ScriptHandle) -> Result<Thread<'_>, ScriptError> {
        let registry = self.registry.borrow();
        let key = registry
            .suspended_threads
            .get(&handle)
            .ok_or_else(|| ScriptError::RuntimeError(format!("unknown suspended thread {handle:?}")))?;
        self.lua.registry_value(key).map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// Stashes a just-yielded `Thread` into the registry under a fresh
    /// handle, mirroring how a loaded `Function` is stashed in
    /// `decode_condition_value`/`decode_operation_value` — a `RegistryKey`,
    /// not the borrowed value itself, since `Thread<'lua>` can't outlive
    /// this call otherwise.
    fn stash_thread(&self, thread: Thread) -> Result<ScriptHandle, ScriptError> {
        let key = self.lua.create_registry_value(thread).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        let mut registry = self.registry.borrow_mut();
        let handle = registry.next_script_handle();
        registry.suspended_threads.insert(handle, key);
        Ok(handle)
    }
}
