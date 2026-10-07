//! The `Game.Create*Ability`/`Game.CreateReplacementEffect`/
//! `Game.CreateCostModifier` fluent builder `UserData` types, plus the two
//! layer/effect and event-filter/payload pairing validators their
//! `:Register()` methods call — split out of the original monolithic
//! `script.rs`.

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
use crate::types::target::{TargetFilter, TargetRequirement, TargetSpec};
use crate::types::zone::ZoneKind;

use super::decode::{
    decode_affected_value, decode_condition_value, decode_cost_component_value, decode_effect_filter_table,
    decode_effect_value, decode_filter_table, decode_layer_value, decode_operation_value,
    decode_target_filter_value, parse_single_color_letter,
};
use super::tags::{
    ConsumptionPolicyTag, CostComponent, DrawLimitAffectedTag, DurationTag, EventFilterKind, EventFilterKindTag,
    HandRevealAffectedTag, HandSizeAffectedTag, HandSizeEffectTag, InterveningIfTag, LandPlayAffectedTag,
    RedirectDamageToSourceTag, ReplacementPayloadTag,
};
use super::types::{to_lua_err, ScriptError, ScriptRegistry};

/// CR 613.1's per-layer primitive shape (`Layer`'s own doc comment) — a
/// static ability's `:SetLayer`/`:SetEffect` pairing must agree, or
/// `:Register()` would silently build a `ContinuousEffect` that
/// `layers::effective_characteristics` can never actually apply (it matches
/// on `(layer, primitive)` shape internally). Checked explicitly here so a
/// card script gets a real error instead of a silently-inert effect.
///
/// Only checked for `EffectPayload::Primitive` — a `Script` payload's real
/// shape isn't known until `ScriptRuntime::invoke_continuous_effect` actually
/// calls it (it could return a different table each call), so there's
/// nothing to validate at `:Register()` time. A mismatch between a `Script`
/// effect's declared layer and its decoded shape is instead silently skipped
/// at query time (`layers::resolve_primitive`'s caller) — a real, narrower
/// safety net than the `Primitive` case gets, flagged in
/// docs/implementation.md's continuous-effects-script slice notes rather
/// than silently accepted as equivalent.
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

/// `EventFilter`'s per-shape payload pairing (`types/effect.rs`'s own
/// `ReplacementPayload` doc comment) — same "reject a bad pairing at
/// `:Register()` time" stance `validate_layer_effect_pairing` above already
/// takes for `StaticAbilityBuilder`. `RedirectDamage` pairs only with
/// `DamageDealt`, same as `PreventDamage` — CR 614.9 redirection is a
/// damage-event rewrite, not a zone-change one. `EntersTapped`/
/// `EntersWithCounters` (CR 603.6d/614.1c) additionally require `to` to
/// name `Battlefield` specifically (not just any `WouldMoveZone` shape) —
/// "how a permanent enters" only means something for a move landing on the
/// battlefield; `replacement::apply_enters_battlefield_modifiers` (the
/// function that actually reads these two payloads back out) only ever
/// checks a candidate event shaped exactly that way, so a registered effect
/// with a different `to` would silently never match anything - rejected
/// here instead, at `:Register()` time, same "real error over a silently
/// inert effect" stance every other pairing check in this function takes.
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
/// `Game.CreateStaticAbility` are the natural sibling builders (both built,
/// see below).
///
/// `:SetCost` is deliberately not bound — triggered abilities don't have
/// activation costs (CR 603 has no equivalent to CR 602.2b's cost payment).
/// `:SetTarget(table | function)` (`phase5/trigger-target-lua-exposure`)
/// mirrors `ActivatedAbilityBuilder::SetTarget` exactly — same
/// `decode_target_filter_value` decode, same single-exact-count narrowing
/// (`count: 1, up_to: false`, no CR 601.2c multi-word form) — making CR
/// 603.3d's real target-selection pause (`ProcessState`/`StepStatus::
/// AwaitingTriggerTargets`, `Engine::submit_trigger_targets`, already
/// engine-complete since `phase5/trigger-target-selection`) reachable from an
/// actual card script for the first time, not just direct Rust
/// `RegisteredAbility` construction. Omitting `:SetTarget` means `target:
/// None`, same "no targets required" meaning `RegisteredAbility::target`'s
/// own doc already documents. No mode-declaration method exists on this
/// builder either — CR 603.3c's modal choice is real in the engine
/// (`phase5/trigger-modal-choice`) but has no Lua vocabulary anywhere yet
/// (modal *spells* don't either — see `docs/lua/not-available-yet.md`), a
/// separate, bigger, more novel gap than this slice's narrow `:SetTarget`
/// mirror.
pub(crate) struct TriggeredAbilityBuilder {
    pub(crate) source: ObjectId,
    pub(crate) condition: Option<ConditionSpec>,
    pub(crate) target: Option<TargetFilter>,
    pub(crate) operation: Option<(Option<AbilityEffect>, ScriptHandle)>,
    /// CR 603.4's intervening-if clause — see `InterveningIfSpec`'s own doc
    /// comment. Set via `:SetInterveningIf(Game.InterveningIf.*)`, orthogonal
    /// to `condition` above (the trigger-event check).
    pub(crate) intervening_if: Option<InterveningIfSpec>,
    /// Set via `:SetFunctionsFrom(zoneName)` - see `function_zone_method`.
    pub(crate) function_zone: Option<ZoneKind>,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

/// `:SetFunctionsFrom("Graveyard")` (CR 113.6): declares the one zone this
/// ability functions from, instead of the default. Shared by the triggered
/// and activated builders. An ability declaring a zone is registered only
/// while its source is in that zone, and is dropped when it leaves.
fn function_zone_name(name: &str) -> Result<ZoneKind, mlua::Error> {
    let zone = super::decode::parse_zone_kind_name(name).map_err(to_lua_err)?;
    match zone {
        ZoneKind::Graveyard | ZoneKind::Exile => Ok(zone),
        // TODO: Hand/Stack/Library/Command-zone abilities. Hand abilities
        // (Cycling) are still recognized by cost shape in `commands::activate`,
        // and only graveyard/exile have a script-registration point.
        other => Err(mlua::Error::RuntimeError(format!(
            "SetFunctionsFrom only supports \"Graveyard\" or \"Exile\" today, got {other:?}"
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
                // Backlog item 7: a real Lua predicate, invoked via
                // ScriptRuntime::invoke_intervening_if - same registry-
                // handle mechanism decode_condition_value's own Function arm
                // already uses.
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
            let decoded = decode_target_filter_value(lua, &mut registry, value).map_err(to_lua_err)?;
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
            // CR 603.3d - same single-exact-count wrapping
            // `ActivatedAbilityBuilder::Register` already uses for its own
            // `:SetTarget`; see that builder's own doc comment for why
            // (nothing built yet to consume "up to N" or a multi-word
            // TargetSpec::Multiple from this builder).
            let target = this
                .target
                .clone()
                .map(|filter| TargetSpec::Primitive(TargetRequirement { filter, count: 1, up_to: false, at_least: false, exclude_source: false }));
            let ability = RegisteredAbility {
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
                // Real id allocated at drain time (run_card_script) - the
                // builder itself has no &mut GameState to allocate one from
                // inside a Lua callback, same reason StaticAbilityBuilder's
                // own timestamp starts as a placeholder too.
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

/// `Game.CreateActivatedAbility(c)`'s return value — mirrors
/// `TriggeredAbilityBuilder` for CR 602 activated abilities, generalized off
/// the old single `AbilityCost::Tap` shortcut. `:AddCost` is called once per
/// cost component (`Game.Costs.TapSelf`, `Sacrifice({...})`, `Discard({...})`,
/// `TapPermanents({...}, count)`, `Life(n)`, `Mana("{1}{R}")`) — the
/// `AdditionalCost` shapes accumulate onto `CostSpec.additional`,
/// `Game.Costs.Mana(...)` sets `CostSpec.mana` (parsed via
/// `ManaCost::parse` — see that method's own doc comment for the closed
/// "no Lua-facing mana-cost-string parser exists anywhere yet" gap this
/// resolves). Not a single `:SetCost`, since a real activation can need more
/// than one component at once (e.g. `{1}{R}, Sacrifice a creature:`).
/// `:SetOperation` mirrors `TriggeredAbilityBuilder`'s own setter exactly.
/// `:Register()` queues the finished `RegisteredAbility` onto the same
/// shared `pending_triggered` list `TriggeredAbilityBuilder` uses — that
/// queue drains into `state.abilities` regardless of `AbilityKind` (see
/// `ScriptRegistry::pending_triggered`'s doc comment), so there's no need
/// for an `Activated`-only queue.
///
/// `kind` is implied the same way `TriggeredAbilityBuilder`'s own doc
/// comment already explains.
///
/// `:SetTarget(table | function)` (table shape added for
/// `phase5/equip-ability`, CR 702.6a's "target creature you control";
/// function shape added for `phase5/scripted-target-filter`, CR 601.2c/
/// 602.2b's scripted legality predicate) goes through
/// `decode_target_filter_value` — a table decodes the same flat option-bag
/// `Game.SelectTargets` already uses, e.g. `{type="object",
/// card_types={"Creature"}, controller="you"}`; a bare function becomes
/// `TargetFilter::Script`, e.g. `function(object, source, controller,
/// state) return ... end`. Always a single, exact-count target (`count: 1,
/// up_to: false`) — no "up to N" or multi-target shape, same narrowing
/// precedent `Game.SelectTargets`'s own doc comment already set for the
/// identical reason (nothing built yet to consume more than one).
/// `CardDefinition.mana_cost` (spells) is untouched by `ManaCost::parse`
/// existing now — it's still sourced as a structured `ManaCost` from the
/// external card database, never parsed from a string.
pub(crate) struct ActivatedAbilityBuilder {
    pub(crate) source: ObjectId,
    pub(crate) cost: CostSpec,
    pub(crate) target: Option<TargetFilter>,
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
            let decoded = decode_target_filter_value(lua, &mut registry, value).map_err(to_lua_err)?;
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
            // `None` means "no cost at all" - same empty-CostSpec treatment
            // RegisteredAbility::cost's own doc comment describes; only
            // meaningfully different from Some(empty) in that ActivateAbility
            // doesn't have to construct the empty CostSpec itself.
            let cost = if this.cost.additional.is_empty() && this.cost.mana.is_none() {
                None
            } else {
                Some(this.cost.clone())
            };
            let target = this
                .target
                .clone()
                .map(|filter| TargetSpec::Primitive(TargetRequirement { filter, count: 1, up_to: false, at_least: false, exclude_source: false }));
            let ability = RegisteredAbility {
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
                // Real id allocated at drain time - see the sibling
                // TriggeredAbilityBuilder::Register's own comment.
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

/// `Game.CreateManaAbility(c)`'s return value — CR 605's own dedicated
/// sibling of `ActivatedAbilityBuilder`, not a mode of it, for two reasons:
///
/// 1. **No `:SetTarget` at all** (not "accepted but ignored") — CR 605.1a
///    says a mana ability never has a target, so the method simply isn't on
///    this builder's `UserData` impl; there's nothing to silently store and
///    forget.
/// 2. **`:SetOperation` only accepts `Game.Ops.AddMana`/`AddManaChoice`** —
///    checked at `:Register()` time, not just documented convention. Nothing
///    else (a custom function included) would actually resolve correctly:
///    `commands.rs`'s `ActivateAbility` handler dispatches purely on the
///    ability's resolved `effect` shape being `AddMana`/`AddManaChoice`
///    specifically (that's how it tells a mana ability apart from an
///    ordinary one at all) — any other operation shape would silently fall
///    through to the normal stack-using activated-ability path instead,
///    which is wrong for something registered as a mana ability. Rejecting
///    it here means that mistake surfaces as a load-time error instead of a
///    card that quietly behaves like a different kind of ability than its
///    author intended.
///
/// `:AddCost` (same decoder `ActivatedAbilityBuilder` uses, so
/// `Game.Costs.TapSelf`/`Sacrifice`/`Discard`/`TapPermanents`/`Life`/`Mana`
/// all work here too) is genuinely optional in a way that changes behavior,
/// not just cost content: omitting it entirely reproduces the original
/// hardcoded-tap, no-payment-pause shape TOML's `mana_ability` field has
/// always had (free, immediate — no `Engine::submit_payment` call needed).
/// Calling `:AddCost` even once (e.g. just `Game.Costs.TapSelf`) opts into a
/// real payment pause instead, same as any other ability's cost (CR
/// 118.5a's "even a {0}/free cost needs an explicit acknowledgment" stance,
/// applied here for the first time to a mana ability) — see
/// `commands::PaymentResolution::ManaAbility` for how that pause still never
/// reaches the stack (CR 605.3b) even though it now has a real cost.
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
            // Same "None means no cost at all" convention
            // ActivatedAbilityBuilder::Register uses - here it additionally
            // decides which of the two mana-ability resolution shapes this
            // becomes (see this struct's own doc comment).
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

/// `Game.CreateStaticAbility(c)`'s return value — mirrors
/// `TriggeredAbilityBuilder` for CR 604/611 static abilities: `:SetLayer`
/// accepts a `Game.Layers.*` tag only (no `Script` fallback — a `Layer` has
/// no such concept, see `decode_layer_value`'s own doc comment). `:SetEffect`/
/// `:SetAffected` each accept either shape `decode_effect_value`/
/// `decode_affected_value` recognize — a `Game.Effects.*` tag/plain filter
/// table (`EffectPayload::Primitive`/`AffectedFilter::Primitive`, zero
/// further evaluation) or a custom function (`::Script`, invoked later via
/// `ScriptRuntime::invoke_continuous_effect`/`invoke_continuous_affects` —
/// see docs/implementation.md's continuous-effects-script slice notes).
/// `:Register()` validates the layer/effect pairing (`validate_layer_effect_pairing`
/// — skipped for a `Script` effect, see that function's own doc comment)
/// and queues the finished `ContinuousEffect` onto the shared registry's
/// `pending_static` list — `run_card_script` drains it into
/// `state.continuous_effects`, stamping the real `timestamp` there (the only
/// place with `&mut GameState` to allocate one).
///
/// `kind` is implied by which constructor built this the same way
/// `TriggeredAbilityBuilder`'s doc comment already explains for
/// `AbilityKind` — a `ContinuousEffect` has no `kind` field at all (it's
/// always CR 613, not a `RegisteredAbility`), so this is more "which queue
/// does :Register() push onto" than a settable field anywhere.
pub(crate) struct StaticAbilityBuilder {
    pub(crate) source: ObjectId,
    pub(crate) layer: Option<Layer>,
    pub(crate) effect: Option<EffectPayload>,
    pub(crate) affected: AffectedFilter,
    /// CR 611.2c's "applies only as long as X is true" — see
    /// `ContinuousEffect::condition`'s own doc comment. Set via
    /// `:SetCondition(Game.InterveningIf.*)`, same decode shape
    /// `TriggeredAbilityBuilder::SetInterveningIf` already uses.
    pub(crate) condition: Option<InterveningIfSpec>,
    /// CR 611.2a-style limited-duration static effect (e.g. "artifacts you
    /// control gain flying until end of turn") — set via
    /// `:SetDuration(Game.Durations.*)`, same `DurationTag` decode
    /// `Game.Ops.Pump`'s duration argument already uses. Defaults to
    /// `Permanent` (this builder's original, only behavior before this
    /// field existed) when never called.
    pub(crate) duration: EffectDuration,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for StaticAbilityBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
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
        // CR 611.2c - mirrors TriggeredAbilityBuilder::SetInterveningIf's own
        // decode logic exactly (same InterveningIfTag/Function shape).
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
        // `Game.Ops.Pump`'s own duration argument decode (script.rs, `Game.Ops`
        // table setup) - same `AnyUserData`/`DurationTag` shape, exposed here as
        // a settable method instead of a constructor argument since every other
        // `StaticAbilityBuilder` field follows the `:SetX` convention.
        methods.add_method_mut("SetDuration", |_, this, value: mlua::AnyUserData| {
            this.duration = value.borrow::<DurationTag>()?.0.clone();
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
            let continuous_effect = ContinuousEffect {
                source: this.source,
                layer,
                affected: this.affected.clone(),
                effect,
                // Placeholder - run_card_script's drain step stamps the real
                // value (see this struct's own doc comment).
                timestamp: SequenceNo(0),
                duration: this.duration.clone(),
                condition: this.condition.clone(),
            };
            this.registry.borrow_mut().pending_static.push(continuous_effect);
            Ok(())
        });
    }
}

/// `Game.CreateReplacementEffect(c)`'s return value (CR 614/615/701.19b) —
/// the "natural future sibling to `Game.CreateStaticAbility`" `ReplacementSpec`'s
/// own doc comment named. Deliberately narrower than that sibling: a
/// `ReplacementEffect` isn't a CR 613 layer effect (it doesn't touch
/// `layers::effective_characteristics` at all — it rewrites an *event*, a
/// wholly separate mechanism, `state.replacement_effects`), so there's no
/// `:SetLayer`/`:SetEffect` pairing to validate; `:SetFilter`/`:SetPayload`
/// serve the equivalent role, checked by
/// `validate_replacement_filter_payload_pairing`. `:SetAffected` mirrors
/// `StaticAbilityBuilder::SetAffected`'s own table shape, but decodes
/// straight to `EffectFilter` (not `AffectedFilter`) - `EventFilter` has no
/// `Script`-affected shape yet (see `ReplacementSpec`'s own doc comment), so
/// a custom-function argument is rejected with a real error rather than
/// silently discarded.
///
/// Defaults (`WouldDie`/`Regenerate`/`Repeatable`) when `:SetFilter`/
/// `:SetPayload`/`:SetConsumption` are never called reproduce this builder's
/// exact original CR 701.19b-only behavior - same backward-compatible-
/// default convention `StaticAbilityBuilder::SetDuration` already
/// established, so `phase5_static_regeneration.rs`'s existing tests (which
/// only ever call `:SetAffected` before `:Register()`) keep working
/// unmodified.
///
/// `ReplacementPayload::RedirectDamage(Target)` is bound narrowly:
/// `Game.ReplacementPayloads.RedirectDamageToSource` (CR 614.9, "redirect to
/// this effect's own source") is a real, real-card-shaped case (Pariah/
/// Pariah's Shield: "damage that would be dealt to you is dealt to
/// [this permanent] instead"). Redirecting to an arbitrary *chosen* target
/// stays unbound — that would need live target selection
/// (`Game.SelectTargets`), which doesn't obviously compose with a builder
/// that's queued and drained later rather than resolved synchronously. See
/// `RedirectDamageToSourceTag`'s own doc comment.
pub(crate) struct ReplacementEffectBuilder {
    pub(crate) source: ObjectId,
    pub(crate) affected: EffectFilter,
    pub(crate) event_kind: EventFilterKind,
    pub(crate) payload: ReplacementPayload,
    pub(crate) consumption: ConsumptionPolicy,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for ReplacementEffectBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("SetAffected", |_, this, table: Table| {
            this.affected = decode_effect_filter_table(&table).map_err(to_lua_err)?;
            Ok(())
        });
        // `Game.EventFilters.*` - which EventFilter shape this effect
        // watches. See EventFilterKindTag's own doc comment for why this
        // isn't the real EventFilter yet (affected is still separate).
        methods.add_method_mut("SetFilter", |_, this, value: mlua::AnyUserData| {
            this.event_kind = value.borrow::<EventFilterKindTag>()?.0;
            Ok(())
        });
        // `Game.ReplacementPayloads.*` - what this effect rewrites the
        // matched event into. `RedirectDamageToSourceTag` is handled here
        // rather than at decode time - it needs `this.source`, only known
        // once the builder already exists (see that tag's own doc comment).
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
            this.registry.borrow_mut().pending_replacement.push(replacement_effect);
            Ok(())
        });
    }
}

/// `Game.CreateCostModifier(c)`'s return value (CR 601.2f) — mirrors
/// `StaticAbilityBuilder`/`ReplacementEffectBuilder` for a spell cost
/// modifier. `:SetFilter(table)` reuses `decode_filter_table` verbatim — the
/// same flat option-bag (`controller`/`card_types`/`supertypes`/`subtypes`/
/// `random`) `Game.Costs.Sacrifice`/`Discard`/`TapPermanents` already decode
/// through, no new filter-table shape to learn. `:SetAmount(n)` sets the flat
/// delta (negative reduces, positive increases — see `CostModifier::amount`'s
/// own doc comment) against the generic component by default; `:SetAmount(n,
/// color)` (CR 118.7b/c/d, `phase5/cost-modifier-colored-pip`) targets a
/// specific colored pip instead — `color` is a letter string (`"W"`/`"U"`/
/// `"B"`/`"R"`/`"G"`) or `"C"` for the colorless component, reusing
/// `parse_single_color_letter` (a `Color`-returning sibling of
/// `parse_color_letter`, which returns a `ColorSet` instead - not reusable
/// here since `CostModifierTarget::Colored` needs exactly one `Color`, not a
/// set).
///
/// `:Register()` queues onto the shared registry's `pending_cost_modifiers`
/// list with a placeholder `controller` (`PlayerId(0)`) — `run_card_script`'s
/// drain step resolves the real value (this effect's source's own live
/// controller, CR 601.2f's "you"/"opponent" perspective the modifier's own
/// `filter` is relative to) once it does, since it has the `&mut GameState`
/// the builder itself never gets inside a Lua callback. Same "placeholder
/// now, patch at drain" pattern `StaticAbilityBuilder`'s `timestamp` and
/// `TriggeredAbilityBuilder`'s `AbilityId` already use, just for a different
/// field kind.
pub(crate) struct CostModifierBuilder {
    pub(crate) source: ObjectId,
    pub(crate) filter: Option<FilterSpec>,
    pub(crate) amount: Option<i32>,
    pub(crate) target: CostModifierTarget,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for CostModifierBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
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
            this.registry.borrow_mut().pending_cost_modifiers.push(CostModifier {
                source: this.source,
                // Placeholder - run_card_script's drain step stamps the real
                // value (see this struct's own doc comment).
                controller: PlayerId(0),
                filter,
                target: this.target,
                amount,
            });
            Ok(())
        });
    }
}

/// `Game.CreateHandSizeModifier(c)`'s return value (CR 402.2) — mirrors
/// `CostModifierBuilder` exactly, same queue-and-drain shape, same
/// placeholder-`controller`-patched-at-drain-time convention. The only real
/// difference is `:SetAffected` takes a `Game.HandSizeAffected.*` tagged
/// value instead of a filter table — `HandSizeAffected` is a small closed
/// enum (`Filter(FilterController)`/`EnchantedPlayer`, CR 303.4m,
/// `phase5/enchanted-player-hand-size`), not an open filter shape with
/// combinable keys the way `EffectFilter`/`FilterSpec` are, so a tagged
/// value (same idea `EventFilterKindTag`/`ReplacementPayloadTag` already
/// use) fits better than a table decoder.
pub(crate) struct HandSizeModifierBuilder {
    pub(crate) source: ObjectId,
    pub(crate) affected: Option<crate::types::player::HandSizeAffected>,
    pub(crate) effect: Option<crate::types::player::HandSizeEffect>,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for HandSizeModifierBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
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
            this.registry.borrow_mut().pending_hand_size_modifiers.push(HandSizeModifier {
                source: this.source,
                // Placeholder - run_card_script's drain step stamps the real
                // value (see this struct's own doc comment).
                controller: PlayerId(0),
                affected,
                effect,
                // Placeholder - run_card_script's drain step stamps the real
                // value, same reason StaticAbilityBuilder's own timestamp is.
                timestamp: SequenceNo(0),
            });
            Ok(())
        });
    }
}

/// `Game.CreateHandRevealModifier(c)`'s return value (CR 701.20a/303.4m) —
/// mirrors `HandSizeModifierBuilder` exactly, minus `:SetEffect` (`Game.
/// HandRevealModifier` has no `effect` field to set - see that struct's own
/// doc comment for why). `phase5/hand-reveal-modifier`: a mechanism-only
/// slice - no real enforcement consumer exists yet for what this builder
/// registers (see `HandRevealModifier`'s own doc comment).
pub(crate) struct HandRevealModifierBuilder {
    pub(crate) source: ObjectId,
    pub(crate) affected: Option<crate::types::player::HandRevealAffected>,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for HandRevealModifierBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
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
            this.registry.borrow_mut().pending_hand_reveal_modifiers.push(HandRevealModifier {
                source: this.source,
                // Placeholder - run_card_script's drain step stamps the real
                // value (see this struct's own doc comment).
                controller: PlayerId(0),
                affected,
                // Placeholder - run_card_script's drain step stamps the real
                // value, same reason StaticAbilityBuilder's own timestamp is.
                timestamp: SequenceNo(0),
            });
            Ok(())
        });
    }
}

/// `Game.CreateDrawLimitModifier(c)`'s return value (CR 121.2b) — mirrors
/// `HandSizeModifierBuilder`'s exact shape (`:SetAffected` + a magnitude
/// setter, `:Register()`), same queue-and-drain, placeholder-`controller`/
/// `timestamp`-patched-at-drain-time convention. `:SetLimit` is this
/// builder's `HandSizeEffect`-equivalent magnitude - a plain `u32` rather
/// than a tagged value, since `DrawLimitModifier::limit` has no polarity to
/// distinguish (unlike `HandSizeEffect::NoMaximum`/`SetTo(n)`).
pub(crate) struct DrawLimitModifierBuilder {
    pub(crate) source: ObjectId,
    pub(crate) affected: Option<crate::types::player::DrawLimitAffected>,
    pub(crate) limit: Option<u32>,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for DrawLimitModifierBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
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
            this.registry.borrow_mut().pending_draw_limit_modifiers.push(DrawLimitModifier {
                source: this.source,
                // Placeholder - run_card_script's drain step stamps the real
                // value (see this struct's own doc comment).
                controller: PlayerId(0),
                affected,
                limit,
                // Placeholder - run_card_script's drain step stamps the real
                // value, same reason StaticAbilityBuilder's own timestamp is.
                timestamp: SequenceNo(0),
            });
            Ok(())
        });
    }
}

/// `Game.CreateLandPlayModifier(c)`'s return value (CR 305.2) — mirrors
/// `DrawLimitModifierBuilder`'s shape (`:SetAffected` + a magnitude setter,
/// `:Register()`), same queue-and-drain, placeholder-`controller` convention.
/// `:SetAdditional` is this builder's `:SetLimit`-equivalent magnitude. No
/// `timestamp` placeholder to patch at drain time - see `LandPlayModifier`'s
/// own doc comment for why this mechanism has none.
pub(crate) struct LandPlayModifierBuilder {
    pub(crate) source: ObjectId,
    pub(crate) affected: Option<crate::types::player::LandPlayAffected>,
    pub(crate) additional: Option<u32>,
    pub(crate) registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for LandPlayModifierBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
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
            this.registry.borrow_mut().pending_land_play_modifiers.push(LandPlayModifier {
                source: this.source,
                // Placeholder - run_card_script's drain step stamps the real
                // value (see this struct's own doc comment).
                controller: PlayerId(0),
                affected,
                additional,
            });
            Ok(())
        });
    }
}
