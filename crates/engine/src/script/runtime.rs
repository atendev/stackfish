//! `ScriptRuntime`, the script-bridge API called by `engine.rs`, `commands/`,
//! `layers.rs`, `replacement.rs`, `sba.rs` and `triggers.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Function, Lua, Table, Thread, ThreadStatus, Value};

use crate::types::ability::{
    AbilityEffect, AbilityKind, ConditionPrimitive, ConditionSpec, RegisteredAbility,
};
use crate::types::card::{CardDefinition, ScriptReference};
use crate::types::cost::{AlternativeCost, CostModifier};
use crate::types::effect::{ContinuousEffect, EffectPrimitive, ReplacementEffect};
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::mana::ManaCost;
use crate::types::player::{
    DrawLimitModifier, HandRevealModifier, HandSizeModifier, LandPlayModifier,
};
use crate::types::target::TargetSpec;
use crate::types::zone::ZoneKind;

use super::decode::{
    decode_alternative_cost_table, decode_condition_value, decode_operation_value,
    decode_script_choice_request, decode_target_spec_table,
};
use super::encode::{event_to_lua, state_to_lua};
use super::globals::bind_game_globals;
use super::tags::{EffectPrimitiveTag, ResultItem, result_items};
use super::types::{
    OperationOutcome, ScriptContext, ScriptError, ScriptRegistry, ScriptZoneMoveKind,
};

/// Owns the embedded Lua VM and every loaded card script. One per engine
/// process, not per game: scripts load once and are reused across games, since
/// DESIGN.md decides on restart to reload, with no hot-reload.
pub struct ScriptRuntime {
    lua: Lua,
    registry: Rc<RefCell<ScriptRegistry>>,
}

impl Default for ScriptRuntime {
    fn default() -> Self {
        Self::new()
    }
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

    /// Number of loaded callbacks; `Engine::snapshot` uses it to prove a
    /// rejected input registered nothing.
    pub fn handle_count(&self) -> usize {
        self.registry.borrow().scripts.len()
    }

    /// Loads `source` (a Lua chunk evaluating to one function,
    /// `return function(...) ... end`) under a fresh `ScriptHandle`. Narrower
    /// than `run_card_script`: one standalone callback and not a whole card
    /// script. Test and scenario fixtures use it to load a condition and an
    /// operation separately and build the `RegisteredAbility` from both handles.
    pub fn load_function(&mut self, source: &str) -> Result<ScriptHandle, ScriptError> {
        let function: Function = self
            .lua
            .load(source)
            .eval()
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let key = self
            .lua
            .create_registry_value(function)
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let mut registry = self.registry.borrow_mut();
        let handle = registry.next_script_handle();
        registry.scripts.insert(handle, key);
        Ok(handle)
    }

    /// Loads a condition source string of either shape: a custom
    /// `return function(source, controller, event) ... end`
    /// (`ConditionSpec::Script`, invoked by `invoke_condition` at trigger-check
    /// time) or a one-liner on `Game.Conditions.*` (`ConditionSpec::Primitive`,
    /// resolved once here with no further Lua calls). See
    /// `decode_condition_value`, shared with `TriggeredAbilityBuilder::SetCondition`.
    pub fn load_condition(&mut self, source: &str) -> Result<ConditionSpec, ScriptError> {
        let value: Value = self
            .lua
            .load(source)
            .eval()
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        decode_condition_value(&self.lua, &mut self.registry.borrow_mut(), value)
    }

    /// The two-shapes pattern of `load_condition`, for operations. A
    /// `Game.Ops.*` value decodes to `(Some(AbilityEffect), ScriptHandle(0))`,
    /// with the handle an unused placeholder as for CR 605 mana abilities. A
    /// custom function falls back to `(None, <real handle>)`, invoked by
    /// `invoke_operation`. See `decode_operation_value`, shared with
    /// `TriggeredAbilityBuilder::SetOperation`.
    pub fn load_operation(
        &mut self,
        source: &str,
    ) -> Result<(Option<AbilityEffect>, ScriptHandle), ScriptError> {
        let value: Value = self
            .lua
            .load(source)
            .eval()
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        decode_operation_value(&self.lua, &mut self.registry.borrow_mut(), value)
    }

    fn function(&self, handle: ScriptHandle) -> Result<Function<'_>, ScriptError> {
        let registry = self.registry.borrow();
        let key = registry
            .scripts
            .get(&handle)
            .ok_or_else(|| ScriptError::RuntimeError(format!("unknown ScriptHandle {handle:?}")))?;
        self.lua
            .registry_value(key)
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// Loads `definition.script_ref`'s source, evaluates it to a Lua table and
    /// calls its `register_abilities` field with `source.0` (the
    /// `s.register_abilities(c)` convention in `docs/lua/card-script-basics.md`).
    /// A table without it registers nothing. What the call `:Register()`ed onto
    /// the shared registry's pending queues is drained right after:
    /// `pending_triggered` into `state.abilities`, `pending_static` into
    /// `state.continuous_effects` (stamped with a real `timestamp` here, the
    /// only place with `&mut GameState`), and `pending_replacement` into
    /// `state.replacement_effects` (no timestamp, see `ReplacementEffect`). The
    /// builders never touch `state`; only this drain does.
    ///
    /// It reparses the source on every call and caches no compiled chunk, since
    /// `CardDefinition` has no stable cache key and reparsing a short chunk is
    /// cheap.
    ///
    /// Only `ScriptReference::Inline` is handled. `ByKey` returns an error,
    /// since resolving a key is the external database project's job (a
    /// `DESIGN.md` non-goal), which should hand over an already-resolved
    /// `Inline` source.
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

        let module: Table = self
            .lua
            .load(source_code)
            .eval()
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        // A plain spell's script declares only `operation()` and `target_requirement()`, so a missing `register_abilities` registers nothing. A field that is present but not a function is a load error.
        let register_abilities: Option<Function> =
            module.get("register_abilities").map_err(|e| {
                ScriptError::LoadError(format!(
                    "card script's register_abilities is not a function: {e}"
                ))
            })?;
        if let Some(register_abilities) = register_abilities {
            register_abilities
                .call::<_, ()>(source.0)
                .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        }

        let (queued, function_zones) = {
            let mut registry = self.registry.borrow_mut();
            let queued: Vec<RegisteredAbility> = std::mem::take(&mut registry.pending_triggered);
            let zones: std::collections::HashMap<usize, ZoneKind> =
                registry.pending_function_zones.drain(..).collect();
            (queued, zones)
        };
        // CR 601.2a: on the stack, the only script-declared abilities that exist yet are the ones that trigger off being cast ("when you cast this spell"). Anything else, such as an ETB trigger, functions only once the permanent is on the battlefield, and registering it now would let it fire off the cast's own events. A `Cast` trigger has nothing left to match after the spell leaves the stack, so it isn't registered in any other zone.
        //
        // CR 113.6: an ability that declared `:SetFunctionsFrom(zone)` exists only while its source is in that zone. An undeclared one never functions from the graveyard or exile.
        let source_zone = state.object(source).map(|o| o.zone);
        let is_cast_trigger = |a: &RegisteredAbility| {
            a.kind == AbilityKind::Triggered
                && matches!(
                    a.condition,
                    Some(ConditionSpec::Primitive(ConditionPrimitive::Cast))
                )
        };
        let mut registered: Vec<RegisteredAbility> = Vec::new();
        let mut declared_zones: Vec<Option<ZoneKind>> = Vec::new();
        for (index, ability) in queued.into_iter().enumerate() {
            let declared = function_zones.get(&index).copied();
            let keep = match declared {
                Some(zone) => source_zone == Some(zone),
                None if source_zone == Some(ZoneKind::Stack) => is_cast_trigger(&ability),
                None if matches!(
                    source_zone,
                    Some(ZoneKind::Graveyard | ZoneKind::Exile | ZoneKind::Library)
                ) =>
                {
                    false
                }
                None => !is_cast_trigger(&ability),
            };
            if keep {
                registered.push(ability);
                declared_zones.push(declared);
            }
        }
        for (ability, declared) in registered.iter_mut().zip(declared_zones) {
            // The real id (see `AbilityId`), stamped here for the same reason as the static-effect timestamp below: the builder has no `&mut GameState`.
            ability.id = state.allocate_ability_id();
            state.script_ability_ids.insert(ability.id);
            if let Some(zone) = declared {
                state.ability_function_zones.insert(ability.id, zone);
            }
        }
        state.abilities.append(&mut registered);

        // CR 113.6, 604.2: a static ability, replacement effect or player-level modifier functions only while its source is on the battlefield. This script may also run for a card in hand (a hand-only ability like Cycling) or on the stack (a "when you cast this spell" trigger), where those registrations must not take effect and would be left behind under a dead id once the card moves on. Triggered and activated abilities above are kept in every zone, since they carry their own zone requirement (and `commands::remap_abilities` drops them on the next move).
        // CR 113.6: an ability that declared `:SetFunctionsFrom(zone)` is kept only while its source is in that zone (Anger-style graveyard statics), and an undeclared one only on the battlefield. CR 604.3 characteristic-defining statics (`:SetCharacteristicDefining()`) bypass this filter below.
        // TODO: "functions in any zone" statics that aren't CDAs (a spell's own cost reduction) are still discarded off the battlefield; no such script-registered shape exists yet
        // TODO: a CDA registers only where a script run happens (hand, library, graveyard, exile, battlefield), not in the command zone
        let on_battlefield = source_zone.is_none_or(|z| z == ZoneKind::Battlefield);
        fn take_functioning<T>(
            queue: &mut Vec<(T, Option<ZoneKind>)>,
            source_zone: Option<ZoneKind>,
            on_battlefield: bool,
        ) -> Vec<T> {
            queue
                .drain(..)
                .filter(|(_, declared)| match declared {
                    Some(zone) => source_zone == Some(*zone),
                    None => on_battlefield,
                })
                .map(|(item, _)| item)
                .collect()
        }

        // CR 604.3: a characteristic-defining ability functions in all zones, so it bypasses the zone filter every other static goes through.
        let mut static_effects: Vec<ContinuousEffect> = {
            let mut registry = self.registry.borrow_mut();
            let (cda, other): (Vec<_>, Vec<_>) = registry
                .pending_static
                .drain(..)
                .partition(|(_, _, characteristic_defining)| *characteristic_defining);
            let mut kept: Vec<ContinuousEffect> =
                cda.into_iter().map(|(effect, _, _)| effect).collect();
            let mut zoned: Vec<(ContinuousEffect, Option<ZoneKind>)> = other
                .into_iter()
                .map(|(effect, zone, _)| (effect, zone))
                .collect();
            kept.extend(take_functioning(&mut zoned, source_zone, on_battlefield));
            kept
        };
        for effect in &mut static_effects {
            // The real timestamp (CR 613.7 ordering), since the builder has no `&mut GameState` (see `StaticAbilityBuilder`).
            effect.timestamp = state.allocate_sequence_no();
        }
        state.continuous_effects.append(&mut static_effects);

        let mut replacement_effects: Vec<ReplacementEffect> = take_functioning(
            &mut self.registry.borrow_mut().pending_replacement,
            source_zone,
            on_battlefield,
        );
        state.replacement_effects.append(&mut replacement_effects);

        let mut cost_modifiers: Vec<CostModifier> = take_functioning(
            &mut self.registry.borrow_mut().pending_cost_modifiers,
            source_zone,
            on_battlefield,
        );
        for modifier in &mut cost_modifiers {
            // The real controller (see `CostModifierBuilder`), since the builder has no `&GameState` to read it from.
            modifier.controller = state.object(modifier.source).map(|o| o.controller).expect(
                "CostModifier's source must already exist in GameState by drain time - \
                 Game.CreateCostModifier was called with a bogus source id",
            );
        }
        state.cost_modifiers.append(&mut cost_modifiers);

        let mut hand_size_modifiers: Vec<HandSizeModifier> = take_functioning(
            &mut self.registry.borrow_mut().pending_hand_size_modifiers,
            source_zone,
            on_battlefield,
        );
        for modifier in &mut hand_size_modifiers {
            // The real controller and timestamp (see `HandSizeModifierBuilder`), since the builder has no `&GameState` to read the live controller or allocate a sequence number.
            modifier.controller = state.object(modifier.source).map(|o| o.controller).expect(
                "HandSizeModifier's source must already exist in GameState by drain time - \
                 Game.CreateHandSizeModifier was called with a bogus source id",
            );
            modifier.timestamp = state.allocate_sequence_no();
        }
        state.hand_size_modifiers.append(&mut hand_size_modifiers);

        let mut hand_reveal_modifiers: Vec<HandRevealModifier> = take_functioning(
            &mut self.registry.borrow_mut().pending_hand_reveal_modifiers,
            source_zone,
            on_battlefield,
        );
        for modifier in &mut hand_reveal_modifiers {
            // The real controller and timestamp, as for hand_size_modifiers above.
            modifier.controller = state.object(modifier.source).map(|o| o.controller).expect(
                "HandRevealModifier's source must already exist in GameState by drain time - \
                 Game.CreateHandRevealModifier was called with a bogus source id",
            );
            modifier.timestamp = state.allocate_sequence_no();
        }
        state
            .hand_reveal_modifiers
            .append(&mut hand_reveal_modifiers);

        let mut draw_limit_modifiers: Vec<DrawLimitModifier> = take_functioning(
            &mut self.registry.borrow_mut().pending_draw_limit_modifiers,
            source_zone,
            on_battlefield,
        );
        for modifier in &mut draw_limit_modifiers {
            // The real controller and timestamp, as for hand_size_modifiers above.
            modifier.controller = state.object(modifier.source).map(|o| o.controller).expect(
                "DrawLimitModifier's source must already exist in GameState by drain time - \
                 Game.CreateDrawLimitModifier was called with a bogus source id",
            );
            modifier.timestamp = state.allocate_sequence_no();
        }
        state.draw_limit_modifiers.append(&mut draw_limit_modifiers);

        let mut land_play_modifiers: Vec<LandPlayModifier> = take_functioning(
            &mut self.registry.borrow_mut().pending_land_play_modifiers,
            source_zone,
            on_battlefield,
        );
        for modifier in &mut land_play_modifiers {
            // The real controller, as for draw_limit_modifiers above. There is no timestamp to patch (see `LandPlayModifier`).
            modifier.controller = state.object(modifier.source).map(|o| o.controller).expect(
                "LandPlayModifier's source must already exist in GameState by drain time - \
                 Game.CreateLandPlayModifier was called with a bogus source id",
            );
        }
        state.land_play_modifiers.append(&mut land_play_modifiers);
        Ok(())
    }

    /// CR 702.5a, 303.4a: a card's own base target requirement (an Aura's
    /// "Enchant [quality]" line, or any targeted spell's targeting), declared by
    /// the optional `s.target_requirement()` convention, a sibling of
    /// `s.register_abilities(c)`. Called once per `CardDefinition` build and not
    /// per object per zone entry like `run_card_script`: an Aura's Enchant
    /// restriction is fixed text needed while the card is still in hand, so it
    /// can't wait for the battlefield timing of `run_card_script`. It involves
    /// no `GameState` or `ObjectId`, since the restriction doesn't vary by
    /// object.
    ///
    /// `target_requirement()` takes no arguments and returns the flat filter
    /// table of `Game.SelectTargets` and `ActivatedAbilityBuilder::SetTarget`
    /// (`decode_target_filter_table`). For a legality shape the flat table
    /// can't express, a `filter` key holds a scripted-predicate function or
    /// `{type="player", filter=fn}`, as `:SetTarget` accepts
    /// (`decode_target_filter_value`, `decode_target_requirement_table`), plus
    /// sibling `count`, `up_to`, `at_least` and `exclude_source` keys. That
    /// matches the scenario crate's TOML `[cards.target_requirement]`
    /// (`TargetRequirementSpec`). A plain array of such tables declares one
    /// requirement per distinct "target" word (`TargetSpec::Multiple`, see
    /// `decode_target_spec_table`). A card with no `target_requirement`
    /// function returns `Ok(None)`, meaning no targets (see
    /// `CardDefinition::target_requirement`).
    ///
    /// It takes `&self` and reevaluates `source` each time, as `run_card_script`
    /// does. Registering a scripted predicate's function needs only `&self`
    /// through the `Rc<RefCell<ScriptRegistry>>` interior mutability every
    /// builder relies on.
    pub fn build_target_requirement(
        &self,
        source: &str,
    ) -> Result<Option<TargetSpec>, ScriptError> {
        let module: Table = self
            .lua
            .load(source)
            .eval()
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let function: Option<Function> = module
            .get("target_requirement")
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let Some(function) = function else {
            return Ok(None);
        };
        let table: Table = function
            .call(())
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        let mut registry = self.registry.borrow_mut();
        Ok(Some(decode_target_spec_table(
            &self.lua,
            &mut registry,
            table,
        )?))
    }

    /// CR 608.2m: a plain spell's own resolution effect (Armageddon's "Destroy
    /// all lands", Shock's "deals 2 damage to any target"), declared by the
    /// optional `s.operation()` convention, a sibling of
    /// `s.register_abilities(c)` and `s.target_requirement()`. A plain spell has
    /// no ability wrapper, and `register_abilities(c)` runs only once the card
    /// is a battlefield object, so an instant or sorcery resolving to the
    /// graveyard can't declare its effect there. Like `build_target_requirement`,
    /// it is called once per `CardDefinition` build with no `GameState` or
    /// `ObjectId`.
    ///
    /// `operation()` takes no arguments and returns the `Game.Ops.*` tag or
    /// custom function that `TriggeredAbilityBuilder::SetOperation` and
    /// `ActivatedAbilityBuilder::SetOperation` accept, through
    /// `decode_operation_value`. A card with no `operation` function returns
    /// `Ok(None)`: anything that becomes a permanent, or a targeted or no-effect
    /// instant or sorcery.
    ///
    /// There is no sibling `operation: ScriptHandle` field on `CardDefinition`,
    /// which has no `Default` and ~400 literal sites. A custom-function result
    /// is packed into the existing `effect: Option<AbilityEffect>` as
    /// `AbilityEffect::Script(ScriptHandle)`, and the `CastSpell` arm of
    /// `commands::execute_command` unpacks it into `StackObject`'s `operation`
    /// and `effect` at push time, so `ResolveStackTop` never sees the difference.
    ///
    /// **Caller trap:** a custom-function result's `ScriptHandle` is valid only
    /// against this `ScriptRuntime`'s own registry. A `CardDefinition` built with
    /// a throwaway runtime and handed to `Engine::new` has a custom function
    /// that silently never runs, because `ResolveStackTop`'s `Err(_) => {}` arm
    /// swallows the "unknown ScriptHandle" error. Call this through the
    /// `Engine` whose game resolves the spell: construct with `effect: None`,
    /// then patch it in with `Engine::build_operation`, as
    /// `register_script_operation` in the scenario crate does.
    pub fn build_operation(
        &self,
        source: &str,
    ) -> Result<Option<(Option<AbilityEffect>, ScriptHandle)>, ScriptError> {
        let module: Table = self
            .lua
            .load(source)
            .eval()
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let function: Option<Function> = module
            .get("operation")
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let Some(function) = function else {
            return Ok(None);
        };
        let value: Value = function
            .call(())
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        let mut registry = self.registry.borrow_mut();
        let decoded = decode_operation_value(&self.lua, &mut registry, value)?;
        Ok(Some(decoded))
    }

    /// CR 601.2b: a card's alternative costs (flashback, escape), declared by
    /// the optional `s.alternative_costs()` convention, a sibling of
    /// `s.register_abilities(c)`, `s.target_requirement()` and `s.operation()`.
    /// Called once per `CardDefinition` build with no `GameState` or
    /// `ObjectId`, for the reason `build_target_requirement` gives: the cost is
    /// fixed text needed before the `CardDefinition` is finished.
    ///
    /// `alternative_costs()` takes no arguments and returns an array of entry
    /// tables (`{ description = "...", from_zone = "Graveyard", costs = {
    /// Game.Costs.Mana("{R}"), ... }, exile_on_leaving_stack = bool, escape =
    /// bool }`; see `decode_alternative_cost_table`). More than one entry is
    /// selectable, since `CastSpell::alternative_cost` is an `Option<usize>`
    /// index. A card with none returns `Ok(Vec::new())`.
    pub fn build_alternative_costs(
        &self,
        source: &str,
    ) -> Result<Vec<AlternativeCost>, ScriptError> {
        let module: Table = self
            .lua
            .load(source)
            .eval()
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let function: Option<Function> = module
            .get("alternative_costs")
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let Some(function) = function else {
            return Ok(Vec::new());
        };
        let table: Table = function
            .call(())
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        table
            .sequence_values::<Table>()
            .map(|entry| {
                decode_alternative_cost_table(
                    entry.map_err(|e| ScriptError::LoadError(e.to_string()))?,
                )
            })
            .collect()
    }

    /// CR 702.27a Buyback's additional cost, declared by the optional
    /// `s.buyback_cost()` convention and called once per `CardDefinition` build
    /// like `build_alternative_costs`. It returns a plain mana-cost string
    /// parsed by `ManaCost::parse` directly and not through `Game.Costs.*`,
    /// since `CardDefinition.buyback_cost` is always a flat `ManaCost`. A card
    /// with none returns `Ok(None)`.
    pub fn build_buyback_cost(&self, source: &str) -> Result<Option<ManaCost>, ScriptError> {
        let module: Table = self
            .lua
            .load(source)
            .eval()
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let function: Option<Function> = module
            .get("buyback_cost")
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let Some(function) = function else {
            return Ok(None);
        };
        let value: String = function
            .call(())
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        let mana = ManaCost::parse(&value)
            .map_err(|e| ScriptError::LoadError(format!("buyback_cost(): {}", e.0)))?;
        Ok(Some(mana))
    }

    /// Builds a minimal Lua argument set from `ctx` and calls the loaded
    /// function, expecting a bool. Only `GameEventKind::ZoneChange` is bridged
    /// (`event_to_lua`); every other event reaches the script as `nil`.
    pub fn invoke_condition(
        &self,
        handle: ScriptHandle,
        ctx: &ScriptContext,
    ) -> Result<bool, ScriptError> {
        let function = self.function(handle)?;
        let event_value = event_to_lua(&self.lua, ctx.event)?;
        // The state snapshot is a 4th argument, additive since Lua ignores extra arguments a function doesn't declare, so 3-argument condition scripts keep working. See `state_to_lua`.
        let state_value = state_to_lua(&self.lua, ctx.state)?;
        function
            .call::<_, bool>((ctx.source.0, ctx.controller.0, event_value, state_value))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// CR 603.4: resolves a `Script`-backed intervening-if predicate. It is a
    /// pure state predicate with no event to bridge (see `InterveningIfSpec`),
    /// so the Lua call is `function(source, controller, snapshot)`,
    /// `invoke_condition` without the event argument.
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

    /// CR 604.3, 208.2a: resolves a characteristic-defining power/toughness
    /// callback. `source_code` is evaluated fresh on every call, with no
    /// `ScriptHandle` or registry entry, as `run_card_script` does for
    /// `ScriptReference::Inline`, and must evaluate to
    /// `function(source, controller) -> integer`. It takes `&self` because
    /// `Lua::load`, `.eval()` and `.call()` need only `&Lua` and nothing here
    /// touches the registry.
    ///
    /// A `state_to_lua` snapshot is a trailing 3rd argument, additive as in
    /// `invoke_condition`. That makes a "count cards in your graveyard" CDA
    /// buildable.
    pub fn invoke_cda(
        &self,
        source_code: &str,
        state: &GameState,
        source: ObjectId,
        controller: PlayerId,
    ) -> Result<i32, ScriptError> {
        let function: Function = self
            .lua
            .load(source_code)
            .eval()
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let state_value = state_to_lua(&self.lua, state)?;
        function
            .call::<_, i32>((source.0, controller.0, state_value))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// `AffectedFilter::Script`'s body (CR 613's "affects" predicate, evaluated
    /// per candidate), expecting `function(object, source, controller, state)
    /// -> bool`. A `state_to_lua` snapshot is the trailing 4th argument, as for
    /// `invoke_cda`, so a script can decide from public board state and not just
    /// identities. Called from `layers::applicable_effects_in_layer` and not
    /// `ContinuousEffect::applies_to`, which stays `Primitive`-only so
    /// `types/effect.rs` never depends on `ScriptRuntime` (as
    /// `layers::resolve_base_pt` does for CDAs).
    ///
    /// CR 303.4m: `enchanted` (what `source` is attached to, `None` if nothing)
    /// is a trailing 5th argument, nil in Lua for `None`, so a script can write
    /// `object == enchanted` directly. It comes after `state` so scripts written
    /// for the 4-argument signature keep working, since Lua ignores a trailing
    /// argument a function doesn't declare.
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
            .call::<_, bool>((
                object.0,
                source.0,
                controller.0,
                state_value,
                enchanted.map(|id| id.0),
            ))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// `EffectPayload::Script`'s body. Expects
    /// `function(source, controller, state, enchanted) -> Game.Effects.* | nil`,
    /// with a `state_to_lua` snapshot as the 3rd argument as in `invoke_cda`.
    /// `nil` is no effect this call. Any other value, or a script error, is an
    /// `Err`.
    ///
    /// CR 303.4m: `enchanted` is a trailing 4th argument, so a dynamic value
    /// ("equal to the enchanted creature's power") can reference it.
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
        let result: Value = function
            .call((
                source.0,
                controller.0,
                state_value,
                enchanted.map(|id| id.0),
            ))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        match result {
            Value::Nil => Ok(None),
            Value::UserData(ud) if ud.is::<EffectPrimitiveTag>() => {
                let tag = ud
                    .borrow::<EffectPrimitiveTag>()
                    .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
                Ok(Some(tag.0.clone()))
            }
            other => Err(ScriptError::RuntimeError(format!(
                "a continuous effect must return a Game.Effects.* value or nil, got {}",
                other.type_name()
            ))),
        }
    }

    pub fn invoke_cost(
        &self,
        handle: ScriptHandle,
        ctx: &ScriptContext,
    ) -> Result<(), ScriptError> {
        // TODO: same calling pattern as invoke_condition above.
        let _ = (handle, ctx);
        todo!()
    }

    /// `TargetFilter::Script`'s body (CR 601.2c, 602.2b target legality, per
    /// already-chosen candidate), expecting `function(object, source,
    /// controller, state) -> bool`, the call shape of
    /// `invoke_continuous_affects`. A static legality predicate checked once at
    /// cast or activation, not a `Game.SelectTargets` suspension (see
    /// `ScriptChoiceRequest::SelectTarget`), so it needs no coroutine. Called
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

    /// `TargetFilter::PlayerScript`'s body, the player twin of
    /// `invoke_target_legal`: `function(player, source, controller, state) ->
    /// bool`. Called from `commands::target_is_legal`.
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

    /// Invokes the loaded function as a coroutine (`Lua::create_thread` and
    /// `Thread::resume`) and not a plain call. That is transparent for a script
    /// that never yields (a fresh thread's first `resume` behaves like a direct
    /// call, confirmed in `coroutine_spike.rs`), and it is the only way a
    /// script can later suspend on `Game.SelectTargets`. See
    /// `handle_operation_result` for what comes back. It never mutates `state`
    /// on the yielded path, which avoids needing `&mut GameState` inside a Lua
    /// callback.
    pub fn invoke_operation(
        &self,
        handle: ScriptHandle,
        state: &mut GameState,
        source: ObjectId,
        controller: PlayerId,
    ) -> Result<OperationOutcome, ScriptError> {
        let function = self.function(handle)?;
        let thread = self
            .lua
            .create_thread(function)
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        // A third `state` argument: the read-only snapshot condition, CDA and continuous-effect scripts get (`state_to_lua`). A `ForEachPlayer` body needs it to compute something like Balance's "fewest lands any player controls" before calling `Game.ChooseN`. Backward compatible, since Lua ignores an extra argument a function doesn't declare.
        let snapshot = state_to_lua(&self.lua, state)?;
        let result: Value = thread
            .resume((source.0, controller.0, snapshot))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        self.handle_operation_result(thread, result, state, source, controller)
    }

    /// Resumes a `Game.SelectTargets` suspension
    /// (`OperationOutcome::NeedsChoice`'s `thread` handle) with the player's
    /// choice, the answer to the yielded `ScriptChoiceRequest`. Only
    /// `Engine::submit_script_choice` should call it. It has the
    /// `handle_operation_result` tail of `invoke_operation`, so a second yield
    /// produces another `NeedsChoice`.
    ///
    /// `answer` resumes the coroutine as a tagged table
    /// (`{ kind = "object" | "player", id = n }`) and not a bare number, because
    /// `ObjectId` and `PlayerId` are both plain integers to Lua and a script
    /// needs to tell them apart.
    ///
    /// `source` is the original operation's source object. A resumed operation
    /// can still return `deal_damage` or `add_counters` after yielding, which
    /// needs it to build a real `GameEventKind`, and a suspended coroutine
    /// doesn't remember it, so `ProcessState::AwaitingScriptChoice.source`
    /// carries it through the pause.
    pub fn resume_operation(
        &self,
        thread_handle: ScriptHandle,
        state: &mut GameState,
        source: ObjectId,
        controller: PlayerId,
        answer: Target,
    ) -> Result<OperationOutcome, ScriptError> {
        let thread = self.thread(thread_handle)?;
        let answer_table = self
            .lua
            .create_table()
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        match answer {
            Target::Object(id) => {
                answer_table
                    .set("kind", "object")
                    .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
                answer_table
                    .set("id", id.0)
                    .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            }
            Target::Player(id) => {
                answer_table
                    .set("kind", "player")
                    .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
                answer_table
                    .set("id", id.0)
                    .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            }
            Target::Ability(id) => {
                answer_table
                    .set("kind", "ability")
                    .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
                answer_table
                    .set("id", id.0)
                    .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            }
        }
        let result: Value = thread
            .resume(answer_table)
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        self.handle_operation_result(thread, result, state, source, controller)
    }

    /// Resumes a `Game.MayChoose` suspension (CR 608.2d) with a plain `bool`.
    /// `Game.MayChoose()` wants `coroutine.yield`'s return value to be the
    /// yes/no answer, so this resumes with a raw Lua boolean and not
    /// `resume_operation`'s tagged table. It shares the `handle_operation_result`
    /// tail, and only `Engine::submit_script_may_choice` should call it.
    pub fn resume_operation_bool(
        &self,
        thread_handle: ScriptHandle,
        state: &mut GameState,
        source: ObjectId,
        controller: PlayerId,
        answer: bool,
    ) -> Result<OperationOutcome, ScriptError> {
        let thread = self.thread(thread_handle)?;
        let result: Value = thread
            .resume(answer)
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        self.handle_operation_result(thread, result, state, source, controller)
    }

    /// Shared tail of `invoke_operation` and `resume_operation`. `thread` either
    /// yielded again (a `Game.SelectTargets` ask, decoded into a
    /// `ScriptChoiceRequest` and stashed for a later `resume_operation`) or
    /// finished, and then `result` is `nil` or a `Game.Ops.*` value. `GainLife`
    /// mutates `GameState` directly. The other steps are handed up as one
    /// pending `OperationOutcome` kind, and a value holding two kinds is an
    /// error because the outcome carries one request.
    fn handle_operation_result(
        &self,
        thread: Thread,
        result: Value,
        state: &mut GameState,
        source: ObjectId,
        controller: PlayerId,
    ) -> Result<OperationOutcome, ScriptError> {
        if thread.status() == ThreadStatus::Resumable {
            let Value::Table(request_table) = result else {
                return Err(ScriptError::RuntimeError(format!(
                    "script yielded a non-table request: {}",
                    result.type_name()
                )));
            };
            let request = decode_script_choice_request(
                &self.lua,
                &mut self.registry.borrow_mut(),
                request_table,
            )?;
            let thread_handle = self.stash_thread(thread)?;
            return Ok(OperationOutcome::NeedsChoice {
                thread: thread_handle,
                request,
            });
        }
        let items = match result {
            Value::Nil => Vec::new(),
            other => result_items(&other).map_err(|message| {
                ScriptError::RuntimeError(format!(
                    "operation must return a Game.Ops.* value or nil: {message}"
                ))
            })?,
        };
        let mut zone_moves: Vec<(ScriptZoneMoveKind, Vec<ObjectId>)> = Vec::new();
        let mut events = Vec::new();
        let mut draw = None;
        let mut cast = None;
        for item in items {
            match item {
                // `GainLife` mutates `GameState` directly. The pending kinds below need pipelines this layer can't reach.
                ResultItem::GainLife(amount) => {
                    if let Some(player) = state.player_mut(controller) {
                        player.gain_life(amount);
                    }
                }
                ResultItem::DrawCards(amount) => draw = Some(amount),
                ResultItem::ZoneMove(kind, ids) => {
                    match zone_moves.iter_mut().find(|(k, _)| *k == kind) {
                        Some((_, existing)) => existing.extend(ids),
                        None => zone_moves.push((kind, ids)),
                    }
                }
                ResultItem::DealDamage { target, amount } => {
                    events.push(GameEventKind::DamageDealt {
                        source,
                        target,
                        amount,
                        is_combat_damage: false,
                    });
                }
                ResultItem::AddCounters {
                    target,
                    counter,
                    amount,
                } => {
                    // CR 122.1: counters sit only on an object, so a player pick is skipped, as `resolve_counters_effect` does.
                    if let Target::Object(object) = target {
                        events.push(GameEventKind::CounterAdded {
                            object,
                            kind: counter,
                            amount,
                        });
                    }
                }
                ResultItem::CastSpell {
                    card,
                    modes,
                    targets,
                    x_value,
                    free,
                } => {
                    cast = Some(OperationOutcome::PendingCast {
                        card,
                        modes,
                        targets,
                        x_value,
                        free,
                    });
                }
            }
        }
        // TODO: one returned value that mixes a draw, a cast, zone moves and events needs an `OperationOutcome` that carries several requests
        let pending = [
            !zone_moves.is_empty(),
            draw.is_some(),
            !events.is_empty(),
            cast.is_some(),
        ];
        if pending.iter().filter(|present| **present).count() > 1 {
            return Err(ScriptError::RuntimeError(
                "an operation can return only one of: zone moves (SacrificeChosen, DiscardChosen), DrawCards, \
                 events (DealDamageTo, AddCountersTo) or CastSpell"
                    .to_string(),
            ));
        }
        if !zone_moves.is_empty() {
            return Ok(OperationOutcome::PendingZoneMoves(zone_moves));
        }
        if let Some(amount) = draw {
            return Ok(OperationOutcome::PendingDraw { amount });
        }
        if !events.is_empty() {
            return Ok(OperationOutcome::PendingEvents(events));
        }
        if let Some(outcome) = cast {
            return Ok(outcome);
        }
        Ok(OperationOutcome::Finished)
    }

    /// The `Thread` sibling of the registry lookup for functions: the same
    /// `RegistryKey` rationale, except `suspended_threads` and `Thread` replace
    /// `scripts` and `Function`.
    fn thread(&self, handle: ScriptHandle) -> Result<Thread<'_>, ScriptError> {
        let registry = self.registry.borrow();
        let key = registry.suspended_threads.get(&handle).ok_or_else(|| {
            ScriptError::RuntimeError(format!("unknown suspended thread {handle:?}"))
        })?;
        self.lua
            .registry_value(key)
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// Stashes a just-yielded `Thread` in the registry under a fresh handle, as
    /// a loaded `Function` is in `decode_condition_value` and
    /// `decode_operation_value`. It is a `RegistryKey`, since `Thread<'lua>`
    /// can't outlive this call otherwise.
    fn stash_thread(&self, thread: Thread) -> Result<ScriptHandle, ScriptError> {
        let key = self
            .lua
            .create_registry_value(thread)
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        let mut registry = self.registry.borrow_mut();
        let handle = registry.next_script_handle();
        registry.suspended_threads.insert(handle, key);
        Ok(handle)
    }
}
