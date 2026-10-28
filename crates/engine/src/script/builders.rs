//! The `Game.Create*Ability`, `Game.CreateReplacementEffect` and
//! `Game.CreateCostModifier` fluent builders, plus the pairing validators
//! their `:Register()` methods call.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Table, UserData, UserDataMethods, Value};

use crate::types::ability::{AbilityEffect, AbilityKind, ConditionSpec, InterveningIfSpec, RegisteredAbility};
use crate::types::card::ModeChoice;
use crate::types::cost::{CostModifier, CostModifierTarget, CostSpec, FilterSpec};
use crate::types::effect::{
    AffectedFilter, ConsumptionPolicy, ContinuousEffect, EffectDuration, EffectFilter, EffectPayload,
    EffectPrimitive, EventFilter, Layer, ReplacementEffect, ReplacementPayload, ReplacementSpec,
};
use crate::types::ids::{AbilityId, ObjectId, PlayerId, ScriptHandle, SequenceNo, Target};
use crate::types::player::{DrawLimitModifier, HandRevealModifier, HandSizeModifier, LandPlayModifier};
use crate::types::target::TargetSpec;
use crate::types::zone::ZoneKind;

use super::decode::{
    decode_affected_value, decode_condition_value, decode_cost_component_value, decode_effect_filter_table,
    decode_effect_value, decode_filter_table, decode_layer_value, decode_operation_value,
    check_chain_words, decode_target_spec_value, parse_single_color_letter,
};
use super::tags::{
    ConsumptionPolicyTag, CostComponent, DrawLimitAffectedTag, DurationTag, EventFilterKind, EventFilterKindTag,
    HandRevealAffectedTag, HandSizeAffectedTag, HandSizeEffectTag, InterveningIfTag, LandPlayAffectedTag,
    RedirectDamageToSourceTag, ReplacementPayloadTag,
};
use super::types::{to_lua_err, ScriptError, ScriptRegistry};

/// CR 613.1: a static ability's `:SetLayer` and `:SetEffect` must agree, or
/// `:Register()` would build a `ContinuousEffect` that
/// `layers::effective_characteristics` can never apply, since it matches on
/// `(layer, primitive)`. Checked here so a card script gets a real error and
/// not an inert effect.
///
/// Only for `EffectPayload::Primitive`. A `Script` payload's shape isn't known
/// until `ScriptRuntime::invoke_continuous_effect` calls it, and a mismatch
/// there is silently skipped at query time (`layers::resolve_primitive`'s
/// caller), a narrower safety net.
fn validate_layer_effect_pairing(layer: Layer, effect: &EffectPayload) -> Result<(), ScriptError> {
    let EffectPayload::Primitive(effect) = effect else { return Ok(()) };
    let paired = matches!(
        (layer, effect),
        (Layer::TextChanging, EffectPrimitive::SetName(_))
            | (Layer::TypeChanging, EffectPrimitive::AddCardType(_))
            | (Layer::TypeChanging, EffectPrimitive::SetCardTypes(_))
            | (Layer::TypeChanging, EffectPrimitive::AddSupertype(_))
            | (Layer::TypeChanging, EffectPrimitive::AddSubtype(_))
            | (Layer::ColorChanging, EffectPrimitive::AddColor(_))
            | (Layer::ColorChanging, EffectPrimitive::SetColors(_))
            | (Layer::AbilityAdding, EffectPrimitive::AddKeyword(_))
            | (Layer::AbilityAdding, EffectPrimitive::RemoveKeyword(_))
            | (Layer::AbilityAdding, EffectPrimitive::IgnoreKeyword(_))
            | (Layer::AbilityAdding, EffectPrimitive::RemoveAllAbilities)
            | (Layer::AbilityAdding, EffectPrimitive::AddProtectionColor(_))
            | (Layer::AbilityAdding, EffectPrimitive::AddProtectionCardType(_))
            | (Layer::AbilityAdding, EffectPrimitive::AddProtectionSubtype(_))
            | (Layer::AbilityAdding, EffectPrimitive::AddProtectionName(_))
            | (Layer::AbilityAdding, EffectPrimitive::AddProtectionEverything)
            | (Layer::AbilityAdding, EffectPrimitive::AddProtectionPlayer(_))
            | (Layer::AbilityAdding, EffectPrimitive::AddLandwalkSubtype(_))
            | (Layer::AbilityAdding, EffectPrimitive::AddBandsWithOther(_))
            | (Layer::AbilityAdding, EffectPrimitive::AddBandsWithOtherSupertype(_))
            | (Layer::PowerToughnessSetting, EffectPrimitive::SetPT { .. })
            | (Layer::PowerToughnessModifying, EffectPrimitive::ModifyPT { .. })
            | (Layer::PowerToughnessSwitching, EffectPrimitive::SwitchPT)
    );
    if paired {
        Ok(())
    } else {
        Err(ScriptError::LoadError(format!(
            "SetLayer({layer:?}) doesn't pair with SetEffect({effect:?}) - each layer only accepts \
             its own effect primitive shape"
        )))
    }
}

/// CR 604.3a: a characteristic-defining ability, as far as a
/// `Game.CreateStaticAbility` builder can express: it defines the object's own
/// colors or subtypes (power and toughness CDAs live on `CardDefinition`'s
/// `PTValue::CharacteristicDefining`), affects only itself, and isn't
/// conditional. Criteria 2 and 4 (printed on the card, not self-granted) hold
/// because a script's own `register_abilities` is the card's printed text.
fn validate_characteristic_defining(
    layer: Layer,
    effect: &EffectPayload,
    affected: &AffectedFilter,
    has_condition: bool,
    function_zone: Option<ZoneKind>,
    duration: &EffectDuration,
) -> Result<(), ScriptError> {
    let err = |why: &str| ScriptError::LoadError(format!("SetCharacteristicDefining: {why} (CR 604.3a)"));
    let defines_color_or_subtype = matches!(
        (layer, effect),
        (Layer::TypeChanging, EffectPayload::Primitive(EffectPrimitive::AddSubtype(_)))
            | (Layer::ColorChanging, EffectPayload::Primitive(EffectPrimitive::AddColor(_)))
            | (Layer::ColorChanging, EffectPayload::Primitive(EffectPrimitive::SetColors(_)))
    );
    if !defines_color_or_subtype {
        return Err(err("only AddSubtype (layer 4) and AddColor/SetColors (layer 5) can define a characteristic"));
    }
    if !matches!(affected, AffectedFilter::Primitive(filter) if filter.self_only) {
        return Err(err("it must affect only its own source - use SetAffected({ self_only = true })"));
    }
    if has_condition {
        return Err(err("it can't set a characteristic only if a condition is met"));
    }
    if function_zone.is_some() {
        return Err(err("it already functions in every zone - don't combine with SetFunctionsFrom"));
    }
    if !matches!(duration, EffectDuration::Permanent) {
        return Err(err("it can't have a limited duration"));
    }
    Ok(())
}

/// The per-shape `EventFilter` and `ReplacementPayload` pairing, rejected at
/// `:Register()` as `validate_layer_effect_pairing` does. `RedirectDamage`
/// pairs only with `DamageDealt`, like `PreventDamage`, since CR 614.9
/// redirection rewrites damage and not a zone change. `EntersTapped` and
/// `EntersWithCounters` (CR 603.6d, 614.1c) also need `to` to be `Battlefield`,
/// because `replacement::apply_enters_battlefield_modifiers` only checks an
/// event shaped that way, so another `to` would never match.
fn validate_replacement_filter_payload_pairing(
    kind: EventFilterKind,
    payload: &ReplacementPayload,
) -> Result<(), ScriptError> {
    let paired = matches!(
        (kind, payload),
        (EventFilterKind::DamageDealt, ReplacementPayload::PreventDamage)
            | (EventFilterKind::DamageDealt, ReplacementPayload::RedirectDamage(_))
            | (EventFilterKind::WouldDie, ReplacementPayload::ReplaceZoneDestination(_))
            | (EventFilterKind::WouldDie, ReplacementPayload::Regenerate)
            | (EventFilterKind::WouldMoveZone { .. }, ReplacementPayload::ReplaceZoneDestination(_))
            | (EventFilterKind::WouldDieEnchanted, ReplacementPayload::UmbraArmor)
            | (EventFilterKind::WouldEnterEnchantedPlayerGraveyard, ReplacementPayload::ReplaceZoneDestination(_))
    ) || matches!(
        (kind, payload),
        (
            EventFilterKind::WouldMoveZone { to: Some(ZoneKind::Battlefield), .. },
            ReplacementPayload::EntersTapped
                | ReplacementPayload::EntersWithCounters { .. }
                | ReplacementPayload::EntersWithCountersBloodthirst { .. }
                | ReplacementPayload::EntersWithCountersSunburst
                | ReplacementPayload::EntersWithCountersDevour { .. }
                | ReplacementPayload::EntersWithCountersAmplify { .. }
                | ReplacementPayload::EntersWithCountersEscape { .. }
                | ReplacementPayload::EntersAsCopy(_)
        )
    );
    if paired {
        Ok(())
    } else {
        Err(ScriptError::LoadError(format!(
            "SetFilter({kind:?}) doesn't pair with SetPayload({payload:?}) - each event filter only \
             accepts its own replacement payload shape(s)"
        )))
    }
}

/// `Game.CreateTriggeredAbility(c)`'s return value, a builder mirroring
/// ygo-pro's `Effect.CreateEffect(c)`, `e1:SetCondition(...)` and
/// `c:RegisterEffect(e1)` (see `docs/ygo_pro_reference.md`). `:SetCondition`
/// and `:SetOperation` accept either shape `decode_condition_value` and
/// `decode_operation_value` recognize. Omitting `:SetCondition` means
/// unconditional (`condition: None`, see `RegisteredAbility::condition`).
/// `:Register()` queues the `RegisteredAbility` onto the shared registry's
/// `pending_triggered` and never touches `GameState`.
///
/// `kind` comes from the constructor (`CreateTriggeredAbility` is always
/// `AbilityKind::Triggered`), not a settable field. `:SetCost` is not bound,
/// since triggered abilities have no activation cost.
///
/// `:SetTarget(table | function)` mirrors `ActivatedAbilityBuilder::SetTarget`:
/// the same `decode_target_filter_value` and a single exact count
/// (`count: 1, up_to: false`, no multi-word form). It makes CR 603.3d's
/// target-selection pause (`AwaitingTriggerTargets`,
/// `Engine::submit_trigger_targets`) reachable from a card script. Omitting it
/// means `target: None`.
// TODO: a mode-declaration method; CR 603.3c's modal choice works in the engine (`AwaitingTriggerModeChoice`) but has no Lua vocabulary, and modal spells have none either (see `docs/lua/not-available-yet.md`)
pub(crate) struct TriggeredAbilityBuilder {
    pub(crate) source: ObjectId,
    pub(crate) condition: Option<ConditionSpec>,
    pub(crate) target: Option<TargetSpec>,
    pub(crate) operation: Option<(Option<AbilityEffect>, ScriptHandle)>,
    /// CR 603.4's intervening-if clause (see `InterveningIfSpec`), set through
    /// `:SetInterveningIf(Game.InterveningIf.*)`, separate from `condition`.
    pub(crate) intervening_if: Option<InterveningIfSpec>,
    /// Set via `:SetFunctionsFrom(zoneName)` - see `function_zone_method`.
    pub(crate) function_zone: Option<ZoneKind>,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

/// `:SetFunctionsFrom("Graveyard")` (CR 113.6): declares the one zone this
/// ability functions from, shared by the triggered and activated builders. An
/// ability declaring a zone is registered only while its source is there and
/// is dropped when it leaves.
fn function_zone_name(name: &str) -> Result<ZoneKind, mlua::Error> {
    let zone = super::decode::parse_zone_kind_name(name).map_err(to_lua_err)?;
    match zone {
        ZoneKind::Hand | ZoneKind::Graveyard | ZoneKind::Exile => Ok(zone),
        // TODO: Stack, Library and Command-zone abilities. Hand abilities written before this (Cycling) are still also recognized by cost shape in `commands::activate`.
        other => Err(mlua::Error::RuntimeError(format!(
            "SetFunctionsFrom only supports \"Hand\", \"Graveyard\" or \"Exile\" today, got {other:?}"
        ))),
    }
}

impl UserData for TriggeredAbilityBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("SetFunctionsFrom", |_, this, name: String| {
            this.function_zone = Some(function_zone_name(&name)?);
            Ok(())
        });
        methods.add_method_mut("SetCondition", |lua, this, value: Value| {
            let mut registry = this.registry.borrow_mut();
            let spec = decode_condition_value(lua, &mut registry, value).map_err(to_lua_err)?;
            drop(registry);
            this.condition = Some(spec);
            Ok(())
        });
        methods.add_method_mut("SetInterveningIf", |lua, this, value: Value| {
            let spec = match value {
                Value::UserData(ud) => {
                    let primitive = ud.borrow::<InterveningIfTag>()?.0.clone();
                    InterveningIfSpec::Primitive(primitive)
                }
                // A real Lua predicate, invoked through `ScriptRuntime::invoke_intervening_if` and registered as in the `Function` arm of `decode_condition_value`.
                Value::Function(function) => {
                    let key = lua.create_registry_value(function)?;
                    let mut registry = this.registry.borrow_mut();
                    let handle = registry.next_script_handle();
                    registry.scripts.insert(handle, key);
                    InterveningIfSpec::Script(handle)
                }
                other => {
                    return Err(mlua::Error::RuntimeError(format!(
                        "SetInterveningIf expects a Game.InterveningIf.* value or a function, got {}",
                        other.type_name()
                    )))
                }
            };
            this.intervening_if = Some(spec);
            Ok(())
        });
        methods.add_method_mut("SetOperation", |lua, this, value: Value| {
            let mut registry = this.registry.borrow_mut();
            let decoded = decode_operation_value(lua, &mut registry, value).map_err(to_lua_err)?;
            drop(registry);
            this.operation = Some(decoded);
            Ok(())
        });
        methods.add_method_mut("SetTarget", |lua, this, value: Value| {
            let mut registry = this.registry.borrow_mut();
            let decoded = decode_target_spec_value(lua, &mut registry, value).map_err(to_lua_err)?;
            drop(registry);
            this.target = Some(decoded);
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
            // CR 603.3d: the same single-exact-count wrapping as `ActivatedAbilityBuilder::Register`'s `:SetTarget`; nothing consumes "up to N" or `TargetSpec::Multiple` from this builder.
            let target = this.target.clone();
            check_chain_words(effect.as_ref(), target.as_ref()).map_err(to_lua_err)?;
            let ability = RegisteredAbility {
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
                // The real id is allocated at drain time (`run_card_script`), since the builder has no `&mut GameState`.
                id: AbilityId(0),
                source: this.source,
                kind: AbilityKind::Triggered,
                condition: this.condition.clone(),
                cost: None,
                target,
                operation,
                effect,
                intervening_if: this.intervening_if.clone(),
            };
            {
                let mut registry = this.registry.borrow_mut();
                if let Some(zone) = this.function_zone {
                    let index = registry.pending_triggered.len();
                    registry.pending_function_zones.push((index, zone));
                }
                registry.pending_triggered.push(ability);
            }
            Ok(())
        });
    }
}

/// `Game.CreateActivatedAbility(c)`'s return value (CR 602), mirroring
/// `TriggeredAbilityBuilder`. `:AddCost` is called once per cost component
/// (`Game.Costs.TapSelf`, `Sacrifice({...})`, `Discard({...})`,
/// `TapPermanents({...}, count)`, `Life(n)`, `Mana("{1}{R}")`): `AdditionalCost`
/// shapes accumulate onto `CostSpec.additional` and `Game.Costs.Mana(...)` sets
/// `CostSpec.mana`, parsed by `ManaCost::parse`. It isn't a single `:SetCost`,
/// since an activation can need several components (`{1}{R}, Sacrifice a
/// creature:`). `:SetOperation` mirrors the triggered builder's. `:Register()`
/// queues onto the same `pending_triggered`, which drains into `state.abilities`
/// regardless of `AbilityKind`.
///
/// `kind` is implied by the constructor, as for `TriggeredAbilityBuilder`.
///
/// `:SetTarget(table | function)` goes through `decode_target_filter_value`. A
/// table decodes the flat option-bag `Game.SelectTargets` uses, such as
/// `{type="object", card_types={"Creature"}, controller="you"}` (Equip,
/// CR 702.6a). A bare function becomes `TargetFilter::Script`, such as
/// `function(object, source, controller, state) return ... end` (CR 601.2c,
/// 602.2b). Always a single exact-count target (`count: 1, up_to: false`),
/// since nothing consumes more.
pub(crate) struct ActivatedAbilityBuilder {
    pub(crate) source: ObjectId,
    pub(crate) cost: CostSpec,
    pub(crate) target: Option<TargetSpec>,
    pub(crate) operation: Option<(Option<AbilityEffect>, ScriptHandle)>,
    /// Set via `:SetFunctionsFrom(zoneName)` - see `function_zone_name`.
    pub(crate) function_zone: Option<ZoneKind>,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for ActivatedAbilityBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("SetFunctionsFrom", |_, this, name: String| {
            this.function_zone = Some(function_zone_name(&name)?);
            Ok(())
        });
        methods.add_method_mut("AddCost", |_, this, value: Value| {
            match decode_cost_component_value(value).map_err(to_lua_err)? {
                CostComponent::Additional(additional) => this.cost.additional.push(additional),
                CostComponent::Mana(mana) => {
                    if this.cost.mana.is_some() {
                        return Err(mlua::Error::RuntimeError(
                            "AddCost(Game.Costs.Mana(...)) called more than once - an activated \
                             ability has only one mana-cost component"
                                .to_string(),
                        ));
                    }
                    this.cost.mana = Some(mana);
                }
            }
            Ok(())
        });
        methods.add_method_mut("SetOperation", |lua, this, value: Value| {
            let mut registry = this.registry.borrow_mut();
            let decoded = decode_operation_value(lua, &mut registry, value).map_err(to_lua_err)?;
            drop(registry);
            this.operation = Some(decoded);
            Ok(())
        });
        methods.add_method_mut("SetTarget", |lua, this, value: Value| {
            let mut registry = this.registry.borrow_mut();
            let decoded = decode_target_spec_value(lua, &mut registry, value).map_err(to_lua_err)?;
            drop(registry);
            this.target = Some(decoded);
            Ok(())
        });
        methods.add_method_mut("Register", |_, this, ()| {
            let (effect, operation) = this.operation.clone().ok_or_else(|| {
                mlua::Error::RuntimeError(
                    "Register() called with no SetOperation() - an activated ability needs an \
                     operation"
                        .to_string(),
                )
            })?;
            // `None` means no cost, treated as an empty `CostSpec` (see `RegisteredAbility::cost`); it spares `ActivateAbility` from building the empty `CostSpec`.
            let cost = if this.cost.additional.is_empty() && this.cost.mana.is_none() {
                None
            } else {
                Some(this.cost.clone())
            };
            let target = this.target.clone();
            check_chain_words(effect.as_ref(), target.as_ref()).map_err(to_lua_err)?;
            let ability = RegisteredAbility {
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
                // The real id is allocated at drain time; see `TriggeredAbilityBuilder::Register`.
                id: AbilityId(0),
                source: this.source,
                kind: AbilityKind::Activated,
                condition: None,
                cost,
                target,
                operation,
                effect,
                intervening_if: None,
            };
            {
                let mut registry = this.registry.borrow_mut();
                if let Some(zone) = this.function_zone {
                    let index = registry.pending_triggered.len();
                    registry.pending_function_zones.push((index, zone));
                }
                registry.pending_triggered.push(ability);
            }
            Ok(())
        });
    }
}

/// `Game.CreateManaAbility(c)`'s return value, CR 605's own sibling of
/// `ActivatedAbilityBuilder` and not a mode of it, for two reasons:
///
/// 1. It has no `:SetTarget`, since a mana ability never has a target
///    (CR 605.1a). The method isn't on the `UserData` impl, so nothing is
///    stored and ignored.
/// 2. `:SetOperation` accepts only `Game.Ops.AddMana` and `AddManaChoice`,
///    checked at `:Register()`. The `ActivateAbility` handler tells a mana
///    ability apart by its resolved `effect` being `AddMana` or
///    `AddManaChoice`, so any other operation would fall through to the
///    stack-using path, which is wrong for a mana ability. Rejecting it makes
///    that a load-time error.
///
/// `:AddCost` (the same decoder as `ActivatedAbilityBuilder`) changes
/// behavior when present, not just the cost. Omitting it keeps the free,
/// immediate, hardcoded-tap shape of TOML's `mana_ability` field, with no
/// `Engine::submit_payment` call. Calling it even once (such as
/// `Game.Costs.TapSelf`) opts into a real payment pause, as for any ability
/// (CR 118.5a). See `commands::PaymentResolution::ManaAbility` for how that
/// pause still never reaches the stack (CR 605.3b).
pub(crate) struct ManaAbilityBuilder {
    pub(crate) source: ObjectId,
    pub(crate) cost: CostSpec,
    pub(crate) operation: Option<(Option<AbilityEffect>, ScriptHandle)>,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for ManaAbilityBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("AddCost", |_, this, value: Value| {
            match decode_cost_component_value(value).map_err(to_lua_err)? {
                CostComponent::Additional(additional) => this.cost.additional.push(additional),
                CostComponent::Mana(mana) => {
                    if this.cost.mana.is_some() {
                        return Err(mlua::Error::RuntimeError(
                            "AddCost(Game.Costs.Mana(...)) called more than once - a mana \
                             ability has only one mana-cost component"
                                .to_string(),
                        ));
                    }
                    this.cost.mana = Some(mana);
                }
            }
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
                    "Register() called with no SetOperation() - a mana ability needs an operation"
                        .to_string(),
                )
            })?;
            if !matches!(effect, Some(AbilityEffect::AddMana(_)) | Some(AbilityEffect::AddManaChoice { .. })) {
                return Err(mlua::Error::RuntimeError(
                    "Game.CreateManaAbility only supports Game.Ops.AddMana/AddManaChoice \
                     operations - use Game.CreateActivatedAbility for anything else"
                        .to_string(),
                ));
            }
            // `None` means no cost, as in `ActivatedAbilityBuilder::Register`, and here it also picks which of the two mana-ability resolution shapes this becomes.
            let cost = if this.cost.additional.is_empty() && this.cost.mana.is_none() {
                None
            } else {
                Some(this.cost.clone())
            };
            let ability = RegisteredAbility {
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
                id: AbilityId(0),
                source: this.source,
                kind: AbilityKind::Activated,
                condition: None,
                cost,
                target: None,
                operation,
                effect,
                intervening_if: None,
            };
            this.registry.borrow_mut().pending_triggered.push(ability);
            Ok(())
        });
    }
}

/// `Game.CreateStaticAbility(c)`'s return value, mirroring
/// `TriggeredAbilityBuilder` for CR 604 and 611. `:SetLayer` accepts a
/// `Game.Layers.*` tag only, with no `Script` fallback (see
/// `decode_layer_value`). `:SetEffect` and `:SetAffected` accept either shape
/// `decode_effect_value` and `decode_affected_value` recognize: a
/// `Game.Effects.*` tag or plain filter table (`Primitive`, no further
/// evaluation), or a custom function (`Script`, invoked later through
/// `ScriptRuntime::invoke_continuous_effect` and `invoke_continuous_affects`).
/// `:Register()` validates the layer and effect pairing
/// (`validate_layer_effect_pairing`, skipped for a `Script` effect) and queues
/// the `ContinuousEffect` onto `pending_static`, which `run_card_script` drains
/// into `state.continuous_effects`, stamping the real `timestamp` there (the
/// only place with `&mut GameState`).
///
/// A `ContinuousEffect` has no `kind` (it is CR 613, not a `RegisteredAbility`),
/// so the constructor really picks which queue `:Register()` pushes onto.
pub(crate) struct StaticAbilityBuilder {
    pub(crate) source: ObjectId,
    pub(crate) layer: Option<Layer>,
    pub(crate) effect: Option<EffectPayload>,
    pub(crate) affected: AffectedFilter,
    /// CR 611.2c: "applies only as long as X is true" (see
    /// `ContinuousEffect::condition`), set through
    /// `:SetCondition(Game.InterveningIf.*)`, decoded as
    /// `TriggeredAbilityBuilder::SetInterveningIf` does.
    pub(crate) condition: Option<InterveningIfSpec>,
    /// CR 611.2a: a limited-duration static effect ("artifacts you control gain
    /// flying until end of turn"), set through `:SetDuration(Game.Durations.*)`
    /// with the `DurationTag` decode of `Game.Ops.Pump`. Defaults to
    /// `Permanent`.
    pub(crate) duration: EffectDuration,
    /// CR 604.3: set through `:SetCharacteristicDefining()` and validated at
    /// `:Register()` against CR 604.3a (see `validate_characteristic_defining`).
    pub(crate) characteristic_defining: bool,
    pub(crate) function_zone: Option<ZoneKind>,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for StaticAbilityBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("SetFunctionsFrom", |_, this, name: String| {
            this.function_zone = Some(function_zone_name(&name)?);
            Ok(())
        });
        methods.add_method_mut("SetLayer", |_, this, value: Value| {
            this.layer = Some(decode_layer_value(value).map_err(to_lua_err)?);
            Ok(())
        });
        methods.add_method_mut("SetEffect", |lua, this, value: Value| {
            let mut registry = this.registry.borrow_mut();
            let payload = decode_effect_value(lua, &mut registry, value).map_err(to_lua_err)?;
            drop(registry);
            this.effect = Some(payload);
            Ok(())
        });
        methods.add_method_mut("SetAffected", |lua, this, value: Value| {
            let mut registry = this.registry.borrow_mut();
            let affected = decode_affected_value(lua, &mut registry, value).map_err(to_lua_err)?;
            drop(registry);
            this.affected = affected;
            Ok(())
        });
        // CR 611.2c: decoded as `TriggeredAbilityBuilder::SetInterveningIf` is (the same `InterveningIfTag` and `Function` shapes).
        methods.add_method_mut("SetCondition", |lua, this, value: Value| {
            let spec = match value {
                Value::UserData(ud) => {
                    let primitive = ud.borrow::<InterveningIfTag>()?.0.clone();
                    InterveningIfSpec::Primitive(primitive)
                }
                Value::Function(function) => {
                    let key = lua.create_registry_value(function)?;
                    let mut registry = this.registry.borrow_mut();
                    let handle = registry.next_script_handle();
                    registry.scripts.insert(handle, key);
                    InterveningIfSpec::Script(handle)
                }
                other => {
                    return Err(mlua::Error::RuntimeError(format!(
                        "SetCondition expects a Game.InterveningIf.* value or a function, got {}",
                        other.type_name()
                    )))
                }
            };
            this.condition = Some(spec);
            Ok(())
        });
        // The decode of `Game.Ops.Pump`'s duration argument (`AnyUserData` and `DurationTag`), as a settable method to match the `:SetX` convention.
        methods.add_method_mut("SetDuration", |_, this, value: mlua::AnyUserData| {
            this.duration = value.borrow::<DurationTag>()?.0.clone();
            Ok(())
        });
        // CR 604.3: a characteristic-defining ability functions in every zone, not only the battlefield.
        methods.add_method_mut("SetCharacteristicDefining", |_, this, ()| {
            this.characteristic_defining = true;
            Ok(())
        });
        methods.add_method_mut("Register", |_, this, ()| {
            let layer = this.layer.ok_or_else(|| {
                mlua::Error::RuntimeError(
                    "Register() called with no SetLayer() - a static ability needs a layer"
                        .to_string(),
                )
            })?;
            let effect = this.effect.clone().ok_or_else(|| {
                mlua::Error::RuntimeError(
                    "Register() called with no SetEffect() - a static ability needs an effect"
                        .to_string(),
                )
            })?;
            validate_layer_effect_pairing(layer, &effect).map_err(to_lua_err)?;
            if this.characteristic_defining {
                validate_characteristic_defining(
                    layer,
                    &effect,
                    &this.affected,
                    this.condition.is_some(),
                    this.function_zone,
                    &this.duration,
                )
                .map_err(to_lua_err)?;
            }
            let continuous_effect = ContinuousEffect {
                source: this.source,
                layer,
                affected: this.affected.clone(),
                effect,
                // Placeholder, stamped by `run_card_script`'s drain step.
                timestamp: SequenceNo(0),
                duration: this.duration.clone(),
                condition: this.condition.clone(),
            };
            this.registry.borrow_mut().pending_static.push((continuous_effect, this.function_zone, this.characteristic_defining));
            Ok(())
        });
    }
}

/// `Game.CreateReplacementEffect(c)`'s return value (CR 614, 615, 701.19b),
/// the sibling of `Game.CreateStaticAbility` that `ReplacementSpec` named.
/// Narrower: a `ReplacementEffect` rewrites an event in
/// `state.replacement_effects` and isn't a CR 613 layer effect, so there is no
/// `:SetLayer`/`:SetEffect` pairing. `:SetFilter` and `:SetPayload` play that
/// role, checked by `validate_replacement_filter_payload_pairing`.
/// `:SetAffected` takes `StaticAbilityBuilder::SetAffected`'s table shape but
/// decodes straight to `EffectFilter`, since `EventFilter` has no
/// `Script`-affected shape (see `ReplacementSpec`), and a custom function is
/// rejected with an error and not silently dropped.
///
/// Never calling `:SetFilter`, `:SetPayload` or `:SetConsumption` gives the
/// defaults `WouldDie`, `Regenerate` and `Repeatable`, the original CR 701.19b
/// behavior, as `StaticAbilityBuilder::SetDuration` defaults.
///
/// `ReplacementPayload::RedirectDamage(Target)` is bound narrowly:
/// `Game.ReplacementPayloads.RedirectDamageToSource` (CR 614.9, Pariah:
/// "damage that would be dealt to you is dealt to [this permanent] instead").
/// A chosen target stays unbound, since live target selection
/// (`Game.SelectTargets`) doesn't compose with a queued, later-drained
/// builder. See `RedirectDamageToSourceTag`.
pub(crate) struct ReplacementEffectBuilder {
    pub(crate) source: ObjectId,
    pub(crate) affected: EffectFilter,
    pub(crate) event_kind: EventFilterKind,
    pub(crate) payload: ReplacementPayload,
    pub(crate) consumption: ConsumptionPolicy,
    /// Set via `:SetFunctionsFrom(zoneName)` - see `function_zone_name`.
    pub(crate) function_zone: Option<ZoneKind>,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for ReplacementEffectBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("SetFunctionsFrom", |_, this, name: String| {
            this.function_zone = Some(function_zone_name(&name)?);
            Ok(())
        });
        methods.add_method_mut("SetAffected", |_, this, table: Table| {
            this.affected = decode_effect_filter_table(&table).map_err(to_lua_err)?;
            Ok(())
        });
        // `Game.EventFilters.*`: which `EventFilter` shape this effect watches. It isn't the real `EventFilter` yet because `affected` is separate; see `EventFilterKindTag`.
        methods.add_method_mut("SetFilter", |_, this, value: mlua::AnyUserData| {
            this.event_kind = value.borrow::<EventFilterKindTag>()?.0;
            Ok(())
        });
        // `Game.ReplacementPayloads.*`: what the matched event is rewritten into. `RedirectDamageToSourceTag` is handled here and not at decode time, because it needs `this.source`.
        methods.add_method_mut("SetPayload", |_, this, value: mlua::AnyUserData| {
            if value.is::<RedirectDamageToSourceTag>() {
                this.payload = ReplacementPayload::RedirectDamage(Target::Object(this.source));
                return Ok(());
            }
            this.payload = value.borrow::<ReplacementPayloadTag>()?.0.clone();
            Ok(())
        });
        // `Game.Consumption.*` - OneShot/Repeatable/LimitedUses(n).
        methods.add_method_mut("SetConsumption", |_, this, value: mlua::AnyUserData| {
            this.consumption = value.borrow::<ConsumptionPolicyTag>()?.0;
            Ok(())
        });
        methods.add_method_mut("Register", |_, this, ()| {
            let filter = match this.event_kind {
                EventFilterKind::DamageDealt => EventFilter::DamageDealt { affected: this.affected.clone() },
                EventFilterKind::WouldDie => EventFilter::WouldDie { affected: this.affected.clone() },
                EventFilterKind::WouldMoveZone { from, to } => {
                    EventFilter::WouldMoveZone { from, to, affected: this.affected.clone() }
                }
                EventFilterKind::WouldDieEnchanted => EventFilter::WouldDieEnchanted,
                EventFilterKind::WouldEnterEnchantedPlayerGraveyard => EventFilter::WouldEnterEnchantedPlayerGraveyard,
            };
            validate_replacement_filter_payload_pairing(this.event_kind, &this.payload).map_err(to_lua_err)?;
            let replacement_effect = ReplacementEffect {
                source: this.source,
                spec: ReplacementSpec::Primitive { filter, payload: this.payload.clone() },
                consumption: this.consumption,
            };
            this.registry.borrow_mut().pending_replacement.push((replacement_effect, this.function_zone));
            Ok(())
        });
    }
}

/// `Game.CreateCostModifier(c)`'s return value (CR 601.2f), mirroring
/// `StaticAbilityBuilder` and `ReplacementEffectBuilder` for a spell cost
/// modifier. `:SetFilter(table)` reuses `decode_filter_table`, the flat
/// option-bag (`controller`, `card_types`, `supertypes`, `subtypes`, `random`)
/// that `Game.Costs.Sacrifice`, `Discard` and `TapPermanents` decode.
/// `:SetAmount(n)` sets the signed delta (see `CostModifier::amount`) against
/// the generic component, and `:SetAmount(n, color)` (CR 118.7b-d) targets a
/// colored pip, with `color` a letter (`"W"`, `"U"`, `"B"`, `"R"`, `"G"`) or
/// `"C"` for colorless, through `parse_single_color_letter` (a
/// `Color`-returning sibling of `parse_color_letter`, which returns a
/// `ColorSet`).
///
/// `:Register()` queues onto `pending_cost_modifiers` with a placeholder
/// `controller` (`PlayerId(0)`). `run_card_script`'s drain step resolves it to
/// the source's live controller, the "you" and "opponent" perspective of the
/// modifier's `filter` (CR 601.2f), because it has the `&mut GameState` a Lua
/// callback never gets. This is the same placeholder-then-patch pattern as
/// `StaticAbilityBuilder`'s `timestamp` and `TriggeredAbilityBuilder`'s
/// `AbilityId`.
pub(crate) struct CostModifierBuilder {
    pub(crate) source: ObjectId,
    pub(crate) filter: Option<FilterSpec>,
    pub(crate) amount: Option<i32>,
    pub(crate) target: CostModifierTarget,
    /// Set via `:SetFunctionsFrom(zoneName)` - see `function_zone_name`.
    pub(crate) function_zone: Option<ZoneKind>,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for CostModifierBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("SetFunctionsFrom", |_, this, name: String| {
            this.function_zone = Some(function_zone_name(&name)?);
            Ok(())
        });
        methods.add_method_mut("SetFilter", |_, this, table: Table| {
            this.filter = Some(decode_filter_table(table).map_err(to_lua_err)?);
            Ok(())
        });
        methods.add_method_mut("SetAmount", |_, this, (amount, color): (i32, Option<String>)| {
            this.amount = Some(amount);
            this.target = match color.as_deref() {
                None => CostModifierTarget::Generic,
                Some("C") => CostModifierTarget::Colorless,
                Some(letter) => CostModifierTarget::Colored(parse_single_color_letter(letter).map_err(to_lua_err)?),
            };
            Ok(())
        });
        methods.add_method_mut("Register", |_, this, ()| {
            let filter = this.filter.clone().ok_or_else(|| {
                mlua::Error::RuntimeError(
                    "Register() called with no SetFilter() - a cost modifier needs a filter"
                        .to_string(),
                )
            })?;
            let amount = this.amount.ok_or_else(|| {
                mlua::Error::RuntimeError(
                    "Register() called with no SetAmount() - a cost modifier needs an amount"
                        .to_string(),
                )
            })?;
            this.registry.borrow_mut().pending_cost_modifiers.push((CostModifier {
                source: this.source,
                // Placeholder, stamped by `run_card_script`'s drain step.
                controller: PlayerId(0),
                filter,
                target: this.target,
                amount,
            }, this.function_zone));
            Ok(())
        });
    }
}

/// `Game.CreateHandSizeModifier(c)`'s return value (CR 402.2), mirroring
/// `CostModifierBuilder`'s queue-and-drain shape and its placeholder
/// `controller`. `:SetAffected` takes a `Game.HandSizeAffected.*` tagged value
/// and not a filter table, because `HandSizeAffected` is a small closed enum
/// (`Filter(FilterController)`, `EnchantedPlayer` for CR 303.4m) and not an
/// open filter with combinable keys.
pub(crate) struct HandSizeModifierBuilder {
    pub(crate) source: ObjectId,
    pub(crate) affected: Option<crate::types::player::HandSizeAffected>,
    pub(crate) effect: Option<crate::types::player::HandSizeEffect>,
    /// Set via `:SetFunctionsFrom(zoneName)` - see `function_zone_name`.
    pub(crate) function_zone: Option<ZoneKind>,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for HandSizeModifierBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("SetFunctionsFrom", |_, this, name: String| {
            this.function_zone = Some(function_zone_name(&name)?);
            Ok(())
        });
        methods.add_method_mut("SetAffected", |_, this, value: mlua::AnyUserData| {
            this.affected = Some(value.borrow::<HandSizeAffectedTag>()?.0);
            Ok(())
        });
        methods.add_method_mut("SetEffect", |_, this, value: mlua::AnyUserData| {
            this.effect = Some(value.borrow::<HandSizeEffectTag>()?.0);
            Ok(())
        });
        methods.add_method_mut("Register", |_, this, ()| {
            let affected = this.affected.ok_or_else(|| {
                mlua::Error::RuntimeError(
                    "Register() called with no SetAffected() - a hand size modifier needs to know \
                     which player(s) it affects"
                        .to_string(),
                )
            })?;
            let effect = this.effect.ok_or_else(|| {
                mlua::Error::RuntimeError(
                    "Register() called with no SetEffect() - a hand size modifier needs an effect"
                        .to_string(),
                )
            })?;
            this.registry.borrow_mut().pending_hand_size_modifiers.push((HandSizeModifier {
                source: this.source,
                // Placeholder, stamped by `run_card_script`'s drain step.
                controller: PlayerId(0),
                affected,
                effect,
                // Placeholder, stamped by `run_card_script`'s drain step.
                timestamp: SequenceNo(0),
            }, this.function_zone));
            Ok(())
        });
    }
}

/// `Game.CreateHandRevealModifier(c)`'s return value (CR 701.20a, 303.4m),
/// mirroring `HandSizeModifierBuilder` minus `:SetEffect`, since
/// `HandRevealModifier` has no `effect` field. A mechanism only, with no
/// enforcement consumer yet; see `HandRevealModifier`.
pub(crate) struct HandRevealModifierBuilder {
    pub(crate) source: ObjectId,
    pub(crate) affected: Option<crate::types::player::HandRevealAffected>,
    /// Set via `:SetFunctionsFrom(zoneName)` - see `function_zone_name`.
    pub(crate) function_zone: Option<ZoneKind>,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for HandRevealModifierBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("SetFunctionsFrom", |_, this, name: String| {
            this.function_zone = Some(function_zone_name(&name)?);
            Ok(())
        });
        methods.add_method_mut("SetAffected", |_, this, value: mlua::AnyUserData| {
            this.affected = Some(value.borrow::<HandRevealAffectedTag>()?.0);
            Ok(())
        });
        methods.add_method_mut("Register", |_, this, ()| {
            let affected = this.affected.ok_or_else(|| {
                mlua::Error::RuntimeError(
                    "Register() called with no SetAffected() - a hand reveal modifier needs to know \
                     which player(s) it affects"
                        .to_string(),
                )
            })?;
            this.registry.borrow_mut().pending_hand_reveal_modifiers.push((HandRevealModifier {
                source: this.source,
                // Placeholder, stamped by `run_card_script`'s drain step.
                controller: PlayerId(0),
                affected,
                // Placeholder, stamped by `run_card_script`'s drain step.
                timestamp: SequenceNo(0),
            }, this.function_zone));
            Ok(())
        });
    }
}

/// `Game.CreateDrawLimitModifier(c)`'s return value (CR 121.2b), mirroring
/// `HandSizeModifierBuilder` (`:SetAffected`, a magnitude setter and
/// `:Register()`) with the same placeholders. `:SetLimit` is a plain `u32`,
/// since `DrawLimitModifier::limit` has no polarity as `HandSizeEffect` does.
pub(crate) struct DrawLimitModifierBuilder {
    pub(crate) source: ObjectId,
    pub(crate) affected: Option<crate::types::player::DrawLimitAffected>,
    pub(crate) limit: Option<u32>,
    /// Set via `:SetFunctionsFrom(zoneName)` - see `function_zone_name`.
    pub(crate) function_zone: Option<ZoneKind>,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for DrawLimitModifierBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("SetFunctionsFrom", |_, this, name: String| {
            this.function_zone = Some(function_zone_name(&name)?);
            Ok(())
        });
        methods.add_method_mut("SetAffected", |_, this, value: mlua::AnyUserData| {
            this.affected = Some(value.borrow::<DrawLimitAffectedTag>()?.0);
            Ok(())
        });
        methods.add_method_mut("SetLimit", |_, this, limit: u32| {
            this.limit = Some(limit);
            Ok(())
        });
        methods.add_method_mut("Register", |_, this, ()| {
            let affected = this.affected.ok_or_else(|| {
                mlua::Error::RuntimeError(
                    "Register() called with no SetAffected() - a draw limit modifier needs to know \
                     which player(s) it affects"
                        .to_string(),
                )
            })?;
            let limit = this.limit.ok_or_else(|| {
                mlua::Error::RuntimeError(
                    "Register() called with no SetLimit() - a draw limit modifier needs a limit"
                        .to_string(),
                )
            })?;
            this.registry.borrow_mut().pending_draw_limit_modifiers.push((DrawLimitModifier {
                source: this.source,
                // Placeholder, stamped by `run_card_script`'s drain step.
                controller: PlayerId(0),
                affected,
                limit,
                // Placeholder, stamped by `run_card_script`'s drain step.
                timestamp: SequenceNo(0),
            }, this.function_zone));
            Ok(())
        });
    }
}

/// `Game.CreateLandPlayModifier(c)`'s return value (CR 305.2), mirroring
/// `DrawLimitModifierBuilder` (`:SetAffected`, a magnitude setter and
/// `:Register()`) with a placeholder `controller`. `:SetAdditional` is the
/// magnitude setter. It has no `timestamp` placeholder, since
/// `LandPlayModifier` has none.
pub(crate) struct LandPlayModifierBuilder {
    pub(crate) source: ObjectId,
    pub(crate) affected: Option<crate::types::player::LandPlayAffected>,
    pub(crate) additional: Option<u32>,
    /// Set via `:SetFunctionsFrom(zoneName)` - see `function_zone_name`.
    pub(crate) function_zone: Option<ZoneKind>,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for LandPlayModifierBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("SetFunctionsFrom", |_, this, name: String| {
            this.function_zone = Some(function_zone_name(&name)?);
            Ok(())
        });
        methods.add_method_mut("SetAffected", |_, this, value: mlua::AnyUserData| {
            this.affected = Some(value.borrow::<LandPlayAffectedTag>()?.0);
            Ok(())
        });
        methods.add_method_mut("SetAdditional", |_, this, additional: u32| {
            this.additional = Some(additional);
            Ok(())
        });
        methods.add_method_mut("Register", |_, this, ()| {
            let affected = this.affected.ok_or_else(|| {
                mlua::Error::RuntimeError(
                    "Register() called with no SetAffected() - a land play modifier needs to know \
                     which player(s) it affects"
                        .to_string(),
                )
            })?;
            let additional = this.additional.ok_or_else(|| {
                mlua::Error::RuntimeError(
                    "Register() called with no SetAdditional() - a land play modifier needs a magnitude"
                        .to_string(),
                )
            })?;
            this.registry.borrow_mut().pending_land_play_modifiers.push((LandPlayModifier {
                source: this.source,
                // Placeholder, stamped by `run_card_script`'s drain step.
                controller: PlayerId(0),
                affected,
                additional,
            }, this.function_zone));
            Ok(())
        });
    }
}
