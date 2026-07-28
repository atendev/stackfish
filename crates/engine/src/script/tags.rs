//! Thin `mlua::UserData` wrapper structs that let a Lua script hold an
//! opaque Rust enum value (e.g. `Game.Conditions.Enters` is a `ConditionTag`
//! under the hood) — split out of the original monolithic `script.rs`. See
//! each tag's own doc comment for what it wraps and which `decode_*`
//! function (`super::decode`) unwraps it.

use mlua::UserData;

use crate::types::ability::{AbilityEffect, ConditionPrimitive, InterveningIfPrimitive, TokenSpec};
use crate::types::cost::AdditionalCost;
use crate::types::effect::{ConsumptionPolicy, EffectDuration, EffectPrimitive, Layer, ReplacementPayload};
use crate::types::mana::ManaCost;
use crate::types::zone::ZoneKind;

/// Thin `mlua::UserData` wrapper around `ConditionPrimitive` so
/// `Game.Conditions.Enters`/`Game.Conditions.Always` are real Lua values a
/// condition script can `return` directly — decoded back out via
/// `decode_condition_value`. `Clone`-only (not `Copy`) since
/// `ConditionPrimitive` itself dropped `Copy` (its `DiesMatching`/
/// `LeavesBattlefieldMatching` variants own a `FilterSpec`).
#[derive(Clone)]
pub(crate) struct ConditionTag(pub(crate) ConditionPrimitive);
impl UserData for ConditionTag {}

/// Thin `mlua::UserData` wrapper around `InterveningIfPrimitive`, same idea
/// as `ConditionTag` — `Game.InterveningIf.AtOrBelowLife(n)` returns one of
/// these, consumed directly by `TriggeredAbilityBuilder::SetInterveningIf`
/// (CR 603.4). No `decode_*` function needed the way `ConditionTag`/
/// `OperationTag` have one — `SetInterveningIf` is the only place this is
/// ever consumed, unlike a condition/operation source string which can be
/// `load_condition`/`load_operation`'d standalone. `Clone`-only (not `Copy`)
/// since `InterveningIfPrimitive` itself dropped `Copy` (its
/// `ControllerControlsAtLeast` variant owns a `FilterSpec`).
#[derive(Clone)]
pub(crate) struct InterveningIfTag(pub(crate) InterveningIfPrimitive);
impl UserData for InterveningIfTag {}

/// `Game.Conditions.OncePerTurn(tag)`'s return value — wraps whichever
/// `ConditionPrimitive` `tag` (any other `ConditionTag`) already carries, so
/// `decode_condition_value` can tell "wrap this one as `ConditionSpec::
/// PrimitiveOncePerTurn` instead of `Primitive`" apart from a bare
/// `ConditionTag` at decode time (CR 603.2h). A wrapper struct rather than a
/// second field on `ConditionTag` itself, so the two Lua-visible shapes
/// (plain tag vs. once-per-turn-wrapped tag) stay distinguishable by type at
/// the `AnyUserData::borrow` check in `decode_condition_value`.
#[derive(Clone)]
pub(crate) struct OncePerTurnTag(pub(crate) ConditionPrimitive);
impl UserData for OncePerTurnTag {}

/// Same idea as `ConditionTag`, for `Game.Ops.*` — e.g. `Game.Ops.GainLife(1)`
/// is a Lua function that returns one of these, decoded back out via
/// `decode_operation_value`.
#[derive(Clone)]
pub(crate) struct OperationTag(pub(crate) AbilityEffect);
impl UserData for OperationTag {}

/// `Game.Tokens.*` return value — wraps a `TokenSpec` so `Game.Ops.CreateToken`
/// can receive it as a typed argument instead of parsing a string.
#[derive(Clone)]
pub(crate) struct TokenTag(pub(crate) TokenSpec);
impl UserData for TokenTag {}

/// One `:AddCost(...)` component — either an `AdditionalCost` (tap/
/// sacrifice/discard/life, CR 602.5a-ish) or a real mana-cost component
/// (CR 107.4, from `Game.Costs.Mana("{1}{R}")`/`ManaCost::parse`). A single
/// enum rather than two separate builder methods so `:AddCost` stays the
/// one call every cost component goes through — `Game.Costs.Mana(...)` just
/// slots into the same `Game.Costs.*` vocabulary as `TapSelf`/`Sacrifice`.
#[derive(Clone)]
pub(crate) enum CostComponent {
    Additional(AdditionalCost),
    Mana(ManaCost),
}

/// Same idea, for `Game.Costs.*` — e.g. `Game.Costs.TapSelf` is a plain
/// tagged value, `Game.Costs.Sacrifice({...})`/`Game.Costs.Mana("{1}{R}")`
/// are Lua functions returning one of these — decoded back out via
/// `decode_cost_component_value`.
#[derive(Clone)]
pub(crate) struct CostTag(pub(crate) CostComponent);
impl UserData for CostTag {}

/// Same idea, for `Game.Layers.*` — e.g. `Game.Layers.PowerToughnessModifying`
/// is a plain tagged value, decoded back out via `decode_layer_value`. Only
/// the 7 layers with a real query path (`layers::effective_characteristics`)
/// are ever bound onto `Game.Layers` — see `bind_game_globals`.
#[derive(Clone, Copy)]
pub(crate) struct LayerTag(pub(crate) Layer);
impl UserData for LayerTag {}

/// Same idea, for `Game.Effects.*` — e.g. `Game.Effects.ModifyPT(1, 1)` is a
/// Lua function returning one of these, decoded back out via
/// `decode_effect_value`.
#[derive(Clone)]
pub(crate) struct EffectPrimitiveTag(pub(crate) EffectPrimitive);
impl UserData for EffectPrimitiveTag {}

/// Same idea, for `Game.Durations.*` — e.g. `Game.Durations.UntilEndOfTurn`
/// is a plain tagged value, the third argument to `Game.Ops.Pump`. Only the
/// four durations with a real drain path (`Engine::advance_turn_step`'s
/// Cleanup/EndOfCombat arms, `phase5/effect-duration-expiration`) are ever
/// bound here - `WhileSourceOnBattlefield`/`UntilSourceLeaves` would be pure
/// sugar for `Pump` specifically (the target's own id already stops
/// resolving the moment it leaves any zone, regardless of `duration` - see
/// `AbilityEffect::Pump`'s own doc comment), and `WhileConditionHolds`/
/// `Custom` have no meaning for a one-shot resolution effect with no
/// `ContinuousEffect.condition` to attach.
#[derive(Clone)]
pub(crate) struct DurationTag(pub(crate) EffectDuration);
impl UserData for DurationTag {}

/// Which `EventFilter` shape a `Game.CreateReplacementEffect` builder should
/// build, before `:SetAffected` fills in the actual `EffectFilter` — plain
/// data, not `EventFilter` itself, since `EventFilter`'s real variants all
/// carry an `affected: EffectFilter` this tag doesn't have yet at
/// `:SetFilter()` time (see `ReplacementEffectBuilder`'s own doc comment).
/// `WouldMoveZone`'s `from`/`to` are carried here (unlike `WouldDie`, which
/// hardcodes Battlefield/Graveyard) since they're chosen at `:SetFilter()`
/// call time via `Game.EventFilters.WouldMoveZone(fromName, toName)`, a
/// function rather than a bare tagged value - either argument may be `nil`
/// for "any zone" (`EventFilter::WouldMoveZone`'s own wildcard meaning).
#[derive(Clone, Copy, Debug)]
pub(crate) enum EventFilterKind {
    DamageDealt,
    WouldDie,
    WouldMoveZone { from: Option<ZoneKind>, to: Option<ZoneKind> },
    /// CR 702.89a (Umbra armor) — see `EventFilter::WouldDieEnchanted`'s own
    /// doc comment. Fieldless like `WouldDie`, since `:SetAffected` plays no
    /// role for this shape at all (the affected object is always the
    /// registering effect's own `attached_to`, never a `controller`/
    /// `card_types`/`self_only` filter) — `ReplacementEffectBuilder::
    /// Register` ignores `this.affected` entirely for this variant.
    WouldDieEnchanted,
}

/// Same idea, for `Game.EventFilters.*` — e.g. `Game.EventFilters.DamageDealt`
/// is a plain tagged value, the argument to `ReplacementEffectBuilder::
/// SetFilter`.
#[derive(Clone, Copy)]
pub(crate) struct EventFilterKindTag(pub(crate) EventFilterKind);
impl UserData for EventFilterKindTag {}

/// Same idea, for `Game.ReplacementPayloads.*` — e.g.
/// `Game.ReplacementPayloads.PreventDamage` is a plain tagged value, decoded
/// back out by `ReplacementEffectBuilder::SetPayload`. Wraps the real
/// `ReplacementPayload` directly (unlike `EventFilterKindTag` above, a
/// payload needs no deferred data the way `affected` is for a filter) —
/// `RedirectDamage(Target)` itself is never wrapped here (see
/// `RedirectDamageToSourceTag`, its narrow bound sibling, and
/// `ReplacementEffectBuilder::SetPayload` for why).
#[derive(Clone)]
pub(crate) struct ReplacementPayloadTag(pub(crate) ReplacementPayload);
impl UserData for ReplacementPayloadTag {}

/// `Game.ReplacementPayloads.RedirectDamageToSource` — a sentinel, not a
/// `ReplacementPayloadTag`, because `RedirectDamage(Target)` needs a real
/// `ObjectId` that doesn't exist yet when `Game.ReplacementPayloads` itself
/// is built (a plain global table, constructed once at `ScriptRuntime::new`,
/// long before any card's `Game.CreateReplacementEffect(c)` call). CR 614.9
/// redirection to an arbitrary chosen target stays unbound — a redirect
/// target isn't a static Lua literal and live target selection doesn't
/// obviously compose with a builder that's queued and drained later rather
/// than resolved synchronously (same gap `ReplacementEffectBuilder`'s own
/// doc comment already named). This binds exactly one real shape: redirect
/// to the replacement effect's own source, e.g. "damage that would be dealt
/// to you is dealt to [this permanent] instead" (Pariah/Pariah's Shield-
/// style, composes with `phase5/aura-attach`/`phase5/equip-ability`'s
/// attach machinery once a card wants "enchanted/equipped creature" instead
/// of literally itself — that's a different, still-unbound target shape).
#[derive(Clone, Copy)]
pub(crate) struct RedirectDamageToSourceTag;
impl UserData for RedirectDamageToSourceTag {}

/// Same idea, for `Game.Consumption.*` — e.g. `Game.Consumption.Repeatable`/
/// `Game.Consumption.LimitedUses(3)` are plain tagged values, decoded back
/// out by `ReplacementEffectBuilder::SetConsumption`. Wraps the real
/// `ConsumptionPolicy` directly, same reasoning `ReplacementPayloadTag` above
/// documents.
#[derive(Clone, Copy)]
pub(crate) struct ConsumptionPolicyTag(pub(crate) ConsumptionPolicy);
impl UserData for ConsumptionPolicyTag {}
