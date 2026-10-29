//! `mlua::UserData` wrappers that let a Lua script hold an opaque Rust enum
//! value (`Game.Conditions.Enters` is a `ConditionTag`). Each tag's doc says
//! which `decode_*` function in `super::decode` unwraps it.

use mlua::UserData;

use crate::types::ability::{AbilityEffect, ConditionPrimitive, InterveningIfPrimitive, TokenSpec};
use crate::types::cost::AdditionalCost;
use crate::types::cost::FilterController;
use crate::types::counter::CounterKind;
use crate::types::effect::{
    ConsumptionPolicy, EffectDuration, EffectPrimitive, Layer, ReplacementPayload,
};
use crate::types::ids::{ObjectId, ScriptHandle, Target};
use crate::types::mana::ManaCost;
use crate::types::player::HandSizeEffect;
use crate::types::zone::ZoneKind;

use super::decode::ObjectFilterKeys;
use super::types::ScriptZoneMoveKind;

/// Wraps `ConditionPrimitive` so `Game.Conditions.Enters` is a Lua value a
/// condition script can `return`, decoded by `decode_condition_value`.
/// `Clone` only, since `ConditionPrimitive` owns a `FilterSpec`.
#[derive(Clone)]
pub(crate) struct ConditionTag(pub(crate) ConditionPrimitive);
impl UserData for ConditionTag {}

/// Wraps `InterveningIfPrimitive` like `ConditionTag`:
/// `Game.Conditions.AtOrBelowLife(n)` returns one, consumed by
/// `TriggeredAbilityBuilder::SetInterveningIf` (CR 603.4) and
/// `StaticAbilityBuilder::SetCondition` (CR 611.2c). It lives in the same Lua
/// namespace as `ConditionTag`, so each consumer rejects the other kind by
/// name. `Clone` only, since `InterveningIfPrimitive` owns a `FilterSpec`.
#[derive(Clone)]
pub(crate) struct InterveningIfTag(pub(crate) InterveningIfPrimitive);
impl UserData for InterveningIfTag {}

/// `Game.Conditions.OncePerTurn(tag)`'s return value (CR 603.2h), wrapping
/// another `ConditionTag`'s primitive so `decode_condition_value` can tell it
/// from a bare tag and build `ConditionSpec::PrimitiveOncePerTurn`. A wrapper
/// and not a second `ConditionTag` field, so the two Lua shapes differ by type
/// at the `AnyUserData::borrow` check.
#[derive(Clone)]
pub(crate) struct OncePerTurnTag(pub(crate) ConditionPrimitive);
impl UserData for OncePerTurnTag {}

/// Like `ConditionTag`, for `Game.Ops.*`: `Game.Ops.GainLife(1)` returns one,
/// decoded by `decode_operation_value`.
#[derive(Clone)]
pub(crate) struct OperationTag(pub(crate) AbilityEffect);
impl UserData for OperationTag {}

/// One step of a `:SetOperation` callback's return value, built by the
/// `Game.Ops.*` constructors that act on picks the script gathered itself.
/// `handle_operation_result` turns the steps into an `OperationOutcome`.
#[derive(Clone)]
pub(crate) enum ResultItem {
    GainLife(i32),
    DrawCards(u32),
    ZoneMove(ScriptZoneMoveKind, Vec<ObjectId>),
    CastSpell {
        card: ObjectId,
        modes: Vec<u32>,
        targets: Vec<Target>,
        x_value: Option<u32>,
        free: bool,
    },
    DealDamage {
        target: Target,
        amount: u32,
    },
    AddCounters {
        target: Target,
        counter: CounterKind,
        amount: i32,
    },
}

/// A `:SetOperation` callback's return value: `Game.Ops.SacrificeChosen`,
/// `DiscardChosen`, `CastSpell`, `DealDamageTo`, `AddCountersTo`, or a
/// `Game.Ops.Chain` holding any of them. Plain `OperationTag`s for `GainLife`
/// and `DrawCards` are accepted too and read through `ResultItem`.
#[derive(Clone)]
pub(crate) struct OperationResultTag(pub(crate) Vec<ResultItem>);
impl UserData for OperationResultTag {}

/// The steps of a value a callback returns or a `Game.Ops.Chain` receives.
/// Only `GainLife` and `DrawCards` exist in both the declared and the
/// script-pick families; any other `OperationTag` needs the ability's own
/// targets and has no runtime form. The error is a message for the caller to
/// wrap.
pub(crate) fn result_items(value: &mlua::Value) -> Result<Vec<ResultItem>, String> {
    match value {
        mlua::Value::UserData(ud) if ud.is::<OperationResultTag>() => Ok(ud
            .borrow::<OperationResultTag>()
            .map_err(|e| e.to_string())?
            .0
            .clone()),
        mlua::Value::UserData(ud) if ud.is::<OperationTag>() => {
            match ud.borrow::<OperationTag>().map_err(|e| e.to_string())?.0.clone() {
                AbilityEffect::GainLife(amount) => Ok(vec![ResultItem::GainLife(amount)]),
                AbilityEffect::DrawCards(amount) => Ok(vec![ResultItem::DrawCards(amount)]),
                _ => Err("this Game.Ops value acts on the ability's targets and can't be returned from a custom operation or mixed with Game.Ops.SacrificeChosen, DiscardChosen, CastSpell, DealDamageTo or AddCountersTo".to_string()),
            }
        }
        other => Err(format!(
            "expected a Game.Ops.* value, got {}",
            other.type_name()
        )),
    }
}

/// `Game.Ops.ForEachPlayer(x)`'s return value (CR 101.4). `x` is a per-card
/// Lua function or a bare tag, so this can't be an `OperationTag`, whose
/// decode pairs a tag with `ScriptHandle(0)` (see `decode_operation_value`).
/// The function shape needs a real handle and the bare-tag shape needs the
/// wrapped `AbilityEffect`. `Game.Ops.ForEachPlayer` is a `create_function`
/// closure in `globals.rs` that branches on its argument's Lua type:
/// - a function: registered into the script registry at once, as a bare
///   function passed to `:SetOperation` is, returning `Function(handle)`.
/// - a bare-tag value, which is checked there and rejected at load time
///   otherwise: unwraps the `OperationTag` and returns `Static(effect)`.
///
/// `decode_operation_value` recognizes this type ahead of `OperationTag` and
/// returns `(Some(AbilityEffect::ForEachPlayer(None)), handle)` for
/// `Function`, or
/// `(Some(AbilityEffect::ForEachPlayer(Some(Box::new(effect)))), ScriptHandle(0))`
/// for `Static`.
#[derive(Clone)]
pub(crate) enum ForEachPlayerTag {
    Function(ScriptHandle),
    Static(AbilityEffect),
}
impl UserData for ForEachPlayerTag {}

/// `Game.Tokens.*` return value, wrapping a `TokenSpec` so
/// `Game.Ops.CreateToken` takes a typed argument and not a string.
#[derive(Clone)]
pub(crate) struct TokenTag(pub(crate) TokenSpec);
impl UserData for TokenTag {}

/// One `:AddCost(...)` component, an `AdditionalCost` (tap, sacrifice,
/// discard, life, CR 602.5a) or a mana cost (CR 107.4, from
/// `Game.Costs.Mana("{1}{R}")` and `ManaCost::parse`). One enum so every cost
/// component goes through `:AddCost`.
#[derive(Clone)]
pub(crate) enum CostComponent {
    Additional(AdditionalCost),
    Mana(ManaCost),
}

/// Like `ConditionTag`, for `Game.Costs.*`: `Game.Costs.TapSelf` is a plain
/// value, and `Sacrifice({...})` and `Mana("{1}{R}")` are functions returning
/// one. Decoded by `decode_cost_component_value`.
#[derive(Clone)]
pub(crate) struct CostTag(pub(crate) CostComponent);
impl UserData for CostTag {}

/// Like `ConditionTag`, for `Game.Layers.*`, decoded by `decode_layer_value`.
/// Only the 7 layers with a query path in `layers::effective_characteristics`
/// are bound (see `bind_game_globals`).
#[derive(Clone, Copy)]
pub(crate) struct LayerTag(pub(crate) Layer);
impl UserData for LayerTag {}

/// Like `ConditionTag`, for `Game.Effects.*`: `Game.Effects.ModifyPT(1, 1)`
/// returns one, decoded by `decode_effect_value`.
#[derive(Clone)]
pub(crate) struct EffectPrimitiveTag(pub(crate) EffectPrimitive);
impl UserData for EffectPrimitiveTag {}

/// Like `ConditionTag`, for `Game.Durations.*`, the third argument to
/// `Game.Ops.Pump`. Only the four durations with a drain path
/// (`Engine::advance_turn_step`'s Cleanup and EndOfCombat arms) are bound.
/// `WhileSourceOnBattlefield` and `UntilSourceLeaves` would be sugar for
/// `Pump`, since the target's id stops resolving when it leaves any zone (see
/// `AbilityEffect::Pump`), and `WhileConditionHolds` and `Custom` mean nothing
/// for a one-shot effect with no `ContinuousEffect.condition`.
#[derive(Clone)]
pub(crate) struct DurationTag(pub(crate) EffectDuration);
impl UserData for DurationTag {}

/// Which `EventFilter` shape a `Game.CreateReplacementEffect` builder builds,
/// before `:SetAffected` supplies the `EffectFilter`. Plain data and not
/// `EventFilter`, whose variants all carry `affected`, which doesn't exist yet
/// at `:SetFilter()` time (see `ReplacementEffectBuilder`). `WouldMoveZone`'s
/// `from` and `to` are carried here, unlike `WouldDie`, because
/// `Game.EventFilters.WouldMoveZone(fromName, toName)` is a function, and
/// either argument may be `nil` for any zone.
#[derive(Clone, Copy, Debug)]
pub(crate) enum EventFilterKind {
    DamageDealt,
    WouldDie,
    WouldMoveZone {
        from: Option<ZoneKind>,
        to: Option<ZoneKind>,
    },
    /// CR 702.89a (Umbra armor); see `EventFilter::WouldDieEnchanted`.
    /// Fieldless like `WouldDie`. `:SetAffected` plays no role, since the
    /// affected object is always the registering effect's own `attached_to`,
    /// and `ReplacementEffectBuilder::Register` ignores `this.affected` for
    /// this variant.
    WouldDieEnchanted,
    /// CR 303.4m, 616 (Wheel of Sun and Moon); see
    /// `EventFilter::WouldEnterEnchantedPlayerGraveyard`. Fieldless like
    /// `WouldDieEnchanted`, for the same reason: the affected side is the
    /// registering effect's own `attached_to_player`.
    WouldEnterEnchantedPlayerGraveyard,
}

/// Like `ConditionTag`, for `Game.EventFilters.*`: `Game.EventFilters.DamageDealt`
/// is the argument to `ReplacementEffectBuilder::SetFilter`.
#[derive(Clone, Copy)]
pub(crate) struct EventFilterKindTag(pub(crate) EventFilterKind);
impl UserData for EventFilterKindTag {}

/// Like `ConditionTag`, for `Game.ReplacementPayloads.*`, decoded by
/// `ReplacementEffectBuilder::SetPayload`. It wraps `ReplacementPayload`
/// directly, since a payload has no deferred data as `affected` is for a
/// filter. `RedirectDamage(Target)` is never wrapped; see
/// `RedirectDamageToSourceTag` and `SetPayload`.
#[derive(Clone)]
pub(crate) struct ReplacementPayloadTag(pub(crate) ReplacementPayload);
impl UserData for ReplacementPayloadTag {}

/// `Game.Counter.*`: a common counter kind or a custom flavor counter. The
/// common kinds mirror `CounterKind` so constructors that take a counter can
/// accept either a string for the known kinds or a `Game.Counter.Custom(name)`
/// tag for the long tail (CR 122.3).
#[derive(Clone)]
pub(crate) enum CounterTag {
    Known(CounterKind),
    Custom(String),
}
impl UserData for CounterTag {}

/// `Game.Affected.*`, the one namespace behind every `:SetAffected`. Each
/// builder accepts the variants that mean something to it and rejects the rest
/// by name (see `AffectedTag::describe`).
#[derive(Clone, Copy)]
pub(crate) enum AffectedTag {
    /// `Game.Affected.Self`: only the effect's own source object.
    SelfOnly,
    /// CR 303.4m: `Game.Affected.Enchanted`, the object the source Aura or Equipment is attached to.
    Enchanted,
    /// CR 303.4m: `Game.Affected.EnchantedPlayerControls`, permanents the enchanted player controls.
    EnchantedPlayerControls,
    /// `Game.Affected.You`, `.Opponent` and `.Any`, for the player modifiers.
    Players(FilterController),
    /// CR 303.4m: `Game.Affected.EnchantedPlayer`, for the player modifiers.
    EnchantedPlayer,
}
impl UserData for AffectedTag {}

impl AffectedTag {
    pub(crate) fn describe(self) -> &'static str {
        match self {
            AffectedTag::SelfOnly => "Game.Affected.Self",
            AffectedTag::Enchanted => "Game.Affected.Enchanted",
            AffectedTag::EnchantedPlayerControls => "Game.Affected.EnchantedPlayerControls",
            AffectedTag::Players(FilterController::You) => "Game.Affected.You",
            AffectedTag::Players(FilterController::Opponent) => "Game.Affected.Opponent",
            AffectedTag::Players(FilterController::Any) => "Game.Affected.Any",
            AffectedTag::EnchantedPlayer => "Game.Affected.EnchantedPlayer",
        }
    }
}

/// `Game.Filter({...})`: an object filter decoded once by
/// `decode_object_filter_keys`. Each consumer takes the keys it can match on
/// (`:SetAffected` an `EffectFilter`, `CostModifierBuilder::SetFilter` a
/// `FilterSpec`).
#[derive(Clone)]
pub(crate) struct FilterTag(pub(crate) ObjectFilterKeys);
impl UserData for FilterTag {}

/// Like `ConditionTag`, for `Game.HandSizeEffects.*`: `NoMaximum` and
/// `.SetTo(n)` are the argument to `HandSizeModifierBuilder::SetEffect`.
#[derive(Clone, Copy)]
pub(crate) struct HandSizeEffectTag(pub(crate) HandSizeEffect);
impl UserData for HandSizeEffectTag {}

/// `Game.ReplacementPayloads.RedirectDamageToSource`, a sentinel and not a
/// `ReplacementPayloadTag`: `RedirectDamage(Target)` needs a real `ObjectId`,
/// which doesn't exist when `Game.ReplacementPayloads` is built once at
/// `ScriptRuntime::new`. It binds one shape, redirect to the replacement
/// effect's own source ("damage that would be dealt to you is dealt to [this
/// permanent] instead", Pariah). CR 614.9 redirection to an arbitrary chosen
/// target, and to "enchanted/equipped creature", stays unbound, since a
/// redirect target isn't a static Lua literal and live target selection
/// doesn't compose with a builder that is queued and drained later.
/// TODO: redirect to a chosen target, and to the enchanted or equipped creature (CR 614.9)
#[derive(Clone, Copy)]
pub(crate) struct RedirectDamageToSourceTag;
impl UserData for RedirectDamageToSourceTag {}

/// Like `ConditionTag`, for `Game.Consumption.*` (`Repeatable`,
/// `LimitedUses(3)`), decoded by `ReplacementEffectBuilder::SetConsumption`. It
/// wraps `ConsumptionPolicy` directly, like `ReplacementPayloadTag`.
#[derive(Clone, Copy)]
pub(crate) struct ConsumptionPolicyTag(pub(crate) ConsumptionPolicy);
impl UserData for ConsumptionPolicyTag {}
