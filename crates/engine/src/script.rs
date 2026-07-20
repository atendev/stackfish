use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Function, Lua, RegistryKey, Table, Thread, ThreadStatus, UserData, UserDataMethods, Value};

use crate::types::ability::{
    AbilityEffect, AbilityKind, ConditionPrimitive, ConditionSpec, CreatureTokenSpec,
    InterveningIfPrimitive, InterveningIfSpec, NamedToken, RegisteredAbility, SearchDestination,
    TokenSpec,
};
use crate::types::card::{CardDefinition, CardTypeSet, KeywordSet, ScriptReference, Subtype, SupertypeSet};
use crate::types::counter::CounterKind;
use crate::types::cost::{AdditionalCost, CostSpec, FilterController, FilterSpec};
use crate::types::effect::{
    AffectedFilter, ConsumptionPolicy, ContinuousEffect, EffectDuration, EffectFilter, EffectPayload,
    EffectPrimitive, EventFilter, Layer, ReplacementEffect, ReplacementPayload, ReplacementSpec,
};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, SequenceNo, Target};
use crate::types::mana::{Color, ColorSet, ManaCost, ManaUnit};
use crate::types::object::GameObject;
use crate::types::target::{TargetFilter, TargetRequirement, TargetSpec};
use crate::types::zone::ZoneKind;

/// Thin `mlua::UserData` wrapper around `ConditionPrimitive` so
/// `Game.Conditions.Enters`/`Game.Conditions.Always` are real Lua values a
/// condition script can `return` directly — decoded back out via
/// `decode_condition_value`. `Clone`-only (not `Copy`) since
/// `ConditionPrimitive` itself dropped `Copy` (its `DiesMatching`/
/// `LeavesBattlefieldMatching` variants own a `FilterSpec`).
#[derive(Clone)]
struct ConditionTag(ConditionPrimitive);
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
struct InterveningIfTag(InterveningIfPrimitive);
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
struct OncePerTurnTag(ConditionPrimitive);
impl UserData for OncePerTurnTag {}

/// Same idea as `ConditionTag`, for `Game.Ops.*` — e.g. `Game.Ops.GainLife(1)`
/// is a Lua function that returns one of these, decoded back out via
/// `decode_operation_value`.
#[derive(Clone)]
struct OperationTag(AbilityEffect);
impl UserData for OperationTag {}

/// `Game.Tokens.*` return value — wraps a `TokenSpec` so `Game.Ops.CreateToken`
/// can receive it as a typed argument instead of parsing a string.
#[derive(Clone)]
struct TokenTag(TokenSpec);
impl UserData for TokenTag {}

/// One `:AddCost(...)` component — either an `AdditionalCost` (tap/
/// sacrifice/discard/life, CR 602.5a-ish) or a real mana-cost component
/// (CR 107.4, from `Game.Costs.Mana("{1}{R}")`/`ManaCost::parse`). A single
/// enum rather than two separate builder methods so `:AddCost` stays the
/// one call every cost component goes through — `Game.Costs.Mana(...)` just
/// slots into the same `Game.Costs.*` vocabulary as `TapSelf`/`Sacrifice`.
#[derive(Clone)]
enum CostComponent {
    Additional(AdditionalCost),
    Mana(ManaCost),
}

/// Same idea, for `Game.Costs.*` — e.g. `Game.Costs.TapSelf` is a plain
/// tagged value, `Game.Costs.Sacrifice({...})`/`Game.Costs.Mana("{1}{R}")`
/// are Lua functions returning one of these — decoded back out via
/// `decode_cost_component_value`.
#[derive(Clone)]
struct CostTag(CostComponent);
impl UserData for CostTag {}

/// Same idea, for `Game.Layers.*` — e.g. `Game.Layers.PowerToughnessModifying`
/// is a plain tagged value, decoded back out via `decode_layer_value`. Only
/// the 7 layers with a real query path (`layers::effective_characteristics`)
/// are ever bound onto `Game.Layers` — see `bind_game_globals`.
#[derive(Clone, Copy)]
struct LayerTag(Layer);
impl UserData for LayerTag {}

/// Same idea, for `Game.Effects.*` — e.g. `Game.Effects.ModifyPT(1, 1)` is a
/// Lua function returning one of these, decoded back out via
/// `decode_effect_value`.
#[derive(Clone)]
struct EffectPrimitiveTag(EffectPrimitive);
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
struct DurationTag(EffectDuration);
impl UserData for DurationTag {}

/// Which `EventFilter` shape a `Game.CreateReplacementEffect` builder should
/// build, before `:SetAffected` fills in the actual `EffectFilter` — plain
/// data, not `EventFilter` itself, since `EventFilter`'s two real variants
/// both carry an `affected: EffectFilter` this tag doesn't have yet at
/// `:SetFilter()` time (see `ReplacementEffectBuilder`'s own doc comment).
#[derive(Clone, Copy, Debug)]
enum EventFilterKind {
    DamageDealt,
    WouldDie,
}

/// Same idea, for `Game.EventFilters.*` — e.g. `Game.EventFilters.DamageDealt`
/// is a plain tagged value, the argument to `ReplacementEffectBuilder::
/// SetFilter`.
#[derive(Clone, Copy)]
struct EventFilterKindTag(EventFilterKind);
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
struct ReplacementPayloadTag(ReplacementPayload);
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
struct RedirectDamageToSourceTag;
impl UserData for RedirectDamageToSourceTag {}

/// Same idea, for `Game.Consumption.*` — e.g. `Game.Consumption.Repeatable`/
/// `Game.Consumption.LimitedUses(3)` are plain tagged values, decoded back
/// out by `ReplacementEffectBuilder::SetConsumption`. Wraps the real
/// `ConsumptionPolicy` directly, same reasoning `ReplacementPayloadTag` above
/// documents.
#[derive(Clone, Copy)]
struct ConsumptionPolicyTag(ConsumptionPolicy);
impl UserData for ConsumptionPolicyTag {}

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
    /// pushes onto instead. Shared by both `TriggeredAbilityBuilder` and
    /// `ActivatedAbilityBuilder` — the drain doesn't care about
    /// `AbilityKind`, so there's no need for a second, kind-specific queue.
    pending_triggered: Vec<RegisteredAbility>,
    /// Same idea as `pending_triggered`, for `Game.CreateStaticAbility`'s
    /// builder (CR 604/611) — drained into `GameState.continuous_effects` by
    /// `run_card_script`. `timestamp` on each queued `ContinuousEffect` is a
    /// placeholder (`SequenceNo(0)`) until drain time, since the builder has
    /// no `&mut GameState` to call `allocate_sequence_no()` from inside a Lua
    /// callback — the drain step stamps the real value.
    pending_static: Vec<ContinuousEffect>,
    /// Same idea as `pending_static`, for `Game.CreateReplacementEffect`'s
    /// builder (CR 614/701.19b) — drained into `GameState.replacement_effects`
    /// by `run_card_script`. Unlike `pending_static`'s `ContinuousEffect`, a
    /// `ReplacementEffect` has no `timestamp` field to stamp at drain time —
    /// nothing about CR 613.7 ordering applies to it.
    pending_replacement: Vec<ReplacementEffect>,
    /// Backlog item 9: a coroutine `Thread` suspended mid-`invoke_operation`
    /// at a `Game.SelectTargets` call, keyed by the same opaque
    /// `ScriptHandle` space `scripts` uses (shares `next_handle`, kept in a
    /// separate map since a suspended thread is consumed exactly once by
    /// `resume_operation`, unlike a loaded `Function` handle which is
    /// reusable). Same "`RegistryKey`, not the borrowed value itself"
    /// rationale as `scripts` — confirmed via a standalone spike
    /// (`phase5_coroutine_spike.rs`) that `Thread` round-trips through the
    /// registry the same way `Function` already does.
    suspended_threads: HashMap<ScriptHandle, RegistryKey>,
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
            // Try the plain tag first, then its once-per-turn-wrapped
            // sibling - see OncePerTurnTag's own doc comment for why these
            // are two distinct UserData types rather than one with a flag.
            if let Ok(tag) = ud.borrow::<ConditionTag>() {
                return Ok(ConditionSpec::Primitive(tag.0.clone()));
            }
            if let Ok(tag) = ud.borrow::<OncePerTurnTag>() {
                return Ok(ConditionSpec::PrimitiveOncePerTurn(tag.0.clone()));
            }
            Err(ScriptError::LoadError(
                "condition script returned an unrecognized userdata value".to_string(),
            ))
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
/// `Game.Costs.*` — no `Script` fallback (neither `AdditionalCost` nor
/// `ManaCost` has such a variant), so a bare custom function isn't accepted
/// here, unlike the condition/operation decoders. Shared by
/// `ActivatedAbilityBuilder::AddCost` and (indirectly, via a plain one-liner
/// eval) any future standalone-cost loader, same convention
/// `decode_condition_value` follows for `load_condition`/
/// `TriggeredAbilityBuilder::SetCondition`.
fn decode_cost_component_value(value: Value) -> Result<CostComponent, ScriptError> {
    match value {
        Value::UserData(ud) => {
            let tag: CostTag =
                ud.borrow::<CostTag>().map_err(|e| ScriptError::LoadError(e.to_string()))?.clone();
            Ok(tag.0)
        }
        Value::Nil => Err(ScriptError::LoadError(
            "AddCost() called with nil - expected a Game.Costs.* value".to_string(),
        )),
        other => Err(ScriptError::LoadError(format!(
            "AddCost() expects a Game.Costs.* value, got {}",
            other.type_name()
        ))),
    }
}

/// `ActivatedAbilityBuilder`'s `:AddCost`/`Game.Costs.Sacrifice`/`Discard`/
/// `TapPermanents`/`Game.Ops.Search` filter argument — a plain Lua table
/// (`{controller=.., card_types={..}, supertypes={..}, subtypes={..},
/// random=..}`, every key optional), same convention `decode_affected_value`
/// already established for `StaticAbilityBuilder::SetAffected`. `extra` is
/// never settable from Lua this slice (see `FilterSpec::extra`'s own doc
/// comment) — always `None`.
fn decode_filter_table(table: Table) -> Result<FilterSpec, ScriptError> {
    let controller_name: Option<String> =
        table.get("controller").map_err(|e| ScriptError::LoadError(e.to_string()))?;
    let controller = controller_name.map(|name| parse_filter_controller_name(&name)).transpose()?;

    let card_types_table: Option<Table> =
        table.get("card_types").map_err(|e| ScriptError::LoadError(e.to_string()))?;
    let card_types = match card_types_table {
        Some(t) => {
            let mut set = CardTypeSet::empty();
            for entry in t.sequence_values::<String>() {
                let name = entry.map_err(|e| ScriptError::LoadError(e.to_string()))?;
                set |= parse_card_type_name(&name)?;
            }
            Some(set)
        }
        None => None,
    };

    let supertypes_table: Option<Table> =
        table.get("supertypes").map_err(|e| ScriptError::LoadError(e.to_string()))?;
    let supertypes = match supertypes_table {
        Some(t) => {
            let mut set = SupertypeSet::empty();
            for entry in t.sequence_values::<String>() {
                let name = entry.map_err(|e| ScriptError::LoadError(e.to_string()))?;
                set |= parse_supertype_name(&name)?;
            }
            Some(set)
        }
        None => None,
    };

    let subtypes_table: Option<Table> =
        table.get("subtypes").map_err(|e| ScriptError::LoadError(e.to_string()))?;
    let subtypes = match subtypes_table {
        Some(t) => {
            let mut list = Vec::new();
            for entry in t.sequence_values::<String>() {
                let name = entry.map_err(|e| ScriptError::LoadError(e.to_string()))?;
                list.push(Subtype(name));
            }
            Some(list)
        }
        None => None,
    };

    let random: Option<bool> = table.get("random").map_err(|e| ScriptError::LoadError(e.to_string()))?;

    Ok(FilterSpec {
        card_types,
        supertypes,
        subtypes,
        controller,
        random: random.unwrap_or(false),
        extra: None,
    })
}

/// `CardTypeSet` names, by CR 108-ish printed card type — mirrors
/// `crates/scenario/src/lib.rs`'s `parse_card_types`, duplicated here rather
/// than shared since that's a separate crate with no common string-parsing
/// utility today. Single-type-at-a-time (`Game.Effects.AddCardType(name)`
/// takes one name, unlike the scenario crate's `Vec<String>`-per-card
/// shape) — `decode_affected_value`'s `card_types` list unions these.
fn parse_card_type_name(name: &str) -> Result<CardTypeSet, ScriptError> {
    Ok(match name {
        "Artifact" => CardTypeSet::ARTIFACT,
        "Creature" => CardTypeSet::CREATURE,
        "Enchantment" => CardTypeSet::ENCHANTMENT,
        "Instant" => CardTypeSet::INSTANT,
        "Land" => CardTypeSet::LAND,
        "Sorcery" => CardTypeSet::SORCERY,
        "Kindred" => CardTypeSet::KINDRED,
        "Planeswalker" => CardTypeSet::PLANESWALKER,
        "Battle" => CardTypeSet::BATTLE,
        other => return Err(ScriptError::LoadError(format!("unknown card type '{other}'"))),
    })
}

/// `KeywordSet` names, by CR 702 keyword — mirrors the scenario crate's
/// `parse_keywords`, same duplication rationale as `parse_card_type_name`.
fn parse_keyword_name(name: &str) -> Result<KeywordSet, ScriptError> {
    Ok(match name {
        "Flying" => KeywordSet::FLYING,
        "Reach" => KeywordSet::REACH,
        "Menace" => KeywordSet::MENACE,
        "Vigilance" => KeywordSet::VIGILANCE,
        "Haste" => KeywordSet::HASTE,
        "Defender" => KeywordSet::DEFENDER,
        "Flash" => KeywordSet::FLASH,
        "Deathtouch" => KeywordSet::DEATHTOUCH,
        "Indestructible" => KeywordSet::INDESTRUCTIBLE,
        "Lifelink" => KeywordSet::LIFELINK,
        "Wither" => KeywordSet::WITHER,
        "Infect" => KeywordSet::INFECT,
        "FirstStrike" => KeywordSet::FIRST_STRIKE,
        "DoubleStrike" => KeywordSet::DOUBLE_STRIKE,
        "Trample" => KeywordSet::TRAMPLE,
        "CantBeRegenerated" => KeywordSet::CANT_BE_REGENERATED,
        "Fear" => KeywordSet::FEAR,
        "Intimidate" => KeywordSet::INTIMIDATE,
        "Shadow" => KeywordSet::SHADOW,
        "Horsemanship" => KeywordSet::HORSEMANSHIP,
        other => return Err(ScriptError::LoadError(format!("unknown keyword '{other}'"))),
    })
}

/// `ColorSet` by single WUBRG letter — same duplication rationale as
/// `parse_card_type_name`.
/// `CounterKind` names, by CR 122.3's common-kind list. Unlike
/// `parse_card_type_name`/`parse_keyword_name`, an unrecognized name is not
/// an error — `CounterKind`'s own doc comment already declares `Custom` the
/// fallback for "the long tail of flavor-named counters," so this mirrors
/// that rather than rejecting a legitimate flavor counter name.
fn parse_counter_kind_name(name: &str) -> CounterKind {
    match name {
        "PlusOnePlusOne" => CounterKind::PlusOnePlusOne,
        "MinusOneMinusOne" => CounterKind::MinusOneMinusOne,
        "Loyalty" => CounterKind::Loyalty,
        "Poison" => CounterKind::Poison,
        "Energy" => CounterKind::Energy,
        "Experience" => CounterKind::Experience,
        "Charge" => CounterKind::Charge,
        other => CounterKind::Custom(other.to_string()),
    }
}

fn parse_color_letter(letter: &str) -> Result<ColorSet, ScriptError> {
    Ok(match letter {
        "W" => ColorSet::WHITE,
        "U" => ColorSet::BLUE,
        "B" => ColorSet::BLACK,
        "R" => ColorSet::RED,
        "G" => ColorSet::GREEN,
        other => return Err(ScriptError::LoadError(format!("unknown color '{other}'"))),
    })
}

/// `ReplacementPayloads.ReplaceZoneDestination(name)`'s argument decoder —
/// same match-on-string-with-a-real-error pattern every other `parse_*_name`
/// helper here uses. No such parser existed in `script.rs` before this slice
/// (the `scenario` crate has its own, separate `parse_zone_kind` for TOML
/// fixtures - not reused here, different crate, different error type).
fn parse_zone_kind_name(name: &str) -> Result<ZoneKind, ScriptError> {
    Ok(match name {
        "Library" => ZoneKind::Library,
        "Hand" => ZoneKind::Hand,
        "Battlefield" => ZoneKind::Battlefield,
        "Graveyard" => ZoneKind::Graveyard,
        "Stack" => ZoneKind::Stack,
        "Exile" => ZoneKind::Exile,
        "Command" => ZoneKind::Command,
        other => return Err(ScriptError::LoadError(format!("unknown zone '{other}'"))),
    })
}

fn parse_filter_controller_name(name: &str) -> Result<FilterController, ScriptError> {
    match name {
        "you" => Ok(FilterController::You),
        "opponent" => Ok(FilterController::Opponent),
        "any" => Ok(FilterController::Any),
        other => Err(ScriptError::LoadError(format!(
            "unknown controller filter '{other}' - expected you/opponent/any"
        ))),
    }
}

fn parse_supertype_name(name: &str) -> Result<SupertypeSet, ScriptError> {
    Ok(match name {
        "Basic" => SupertypeSet::BASIC,
        "Legendary" => SupertypeSet::LEGENDARY,
        "Snow" => SupertypeSet::SNOW,
        "World" => SupertypeSet::WORLD,
        other => return Err(ScriptError::LoadError(format!("unknown supertype '{other}'"))),
    })
}

/// Decodes the `subtypes` argument: either a Lua string `"Soldier"` (one
/// subtype) or a Lua table `{"Elf", "Warrior"}` (multiple).
fn decode_subtypes(val: Value) -> Result<Vec<String>, ScriptError> {
    match val {
        Value::String(s) => Ok(vec![s.to_str().map_err(|e| ScriptError::LoadError(e.to_string()))?.to_string()]),
        Value::Table(t) => {
            let mut out = Vec::new();
            for pair in t.sequence_values::<String>() {
                out.push(pair.map_err(|e| ScriptError::LoadError(e.to_string()))?);
            }
            Ok(out)
        }
        _ => Err(ScriptError::LoadError("subtypes must be a string or table".to_string())),
    }
}

/// Parses a WUBRG letter string like `"WU"` or `""` into a `ColorSet`.
fn parse_color_string(colors: &str) -> Result<ColorSet, ScriptError> {
    let mut out = ColorSet::empty();
    for ch in colors.chars() {
        out |= parse_color_letter(&ch.to_string())?;
    }
    Ok(out)
}

/// `Game.Ops.AddMana`'s single-symbol vocabulary — WUBRG plus `"C"` for
/// colorless (`ManaUnit::Colorless`, CR 605's actual literal-unit shape, not
/// a `Color` at all). No `SnowColored`/`SnowColorless` letter yet - real
/// snow-producing lands are rare enough to defer rather than invent an
/// unvalidated letter convention for them now (named in this slice's own
/// UNRESOLVED note).
fn parse_mana_unit_letter(letter: &str) -> Result<ManaUnit, ScriptError> {
    Ok(match letter {
        "W" => ManaUnit::Colored(Color::White),
        "U" => ManaUnit::Colored(Color::Blue),
        "B" => ManaUnit::Colored(Color::Black),
        "R" => ManaUnit::Colored(Color::Red),
        "G" => ManaUnit::Colored(Color::Green),
        "C" => ManaUnit::Colorless,
        other => {
            return Err(ScriptError::LoadError(format!(
                "unknown mana unit letter '{other}' - expected W/U/B/R/G/C"
            )))
        }
    })
}

/// Decodes the optional `opts` table for `Game.Tokens.Creature` /
/// `Game.Tokens.ArtifactCreature`. Returns
/// `(keywords, name_override, additional_types, additional_supertypes)`.
fn decode_creature_token_opts(
    opts: Option<mlua::Table>,
) -> Result<(KeywordSet, Option<String>, CardTypeSet, SupertypeSet), ScriptError> {
    let Some(opts) = opts else {
        return Ok((KeywordSet::empty(), None, CardTypeSet::empty(), SupertypeSet::empty()));
    };
    let mut keywords = KeywordSet::empty();
    if let Ok(Some(kws)) = opts.get::<_, Option<mlua::Table>>("keywords") {
        for pair in kws.sequence_values::<String>() {
            keywords |= parse_keyword_name(&pair.map_err(|e| ScriptError::LoadError(e.to_string()))?)?;
        }
    }
    let name: Option<String> = opts.get("name").ok().flatten();
    let mut additional_types = CardTypeSet::empty();
    if let Ok(Some(types)) = opts.get::<_, Option<mlua::Table>>("additional_types") {
        for pair in types.sequence_values::<String>() {
            additional_types |= parse_card_type_name(&pair.map_err(|e| ScriptError::LoadError(e.to_string()))?)?;
        }
    }
    let mut additional_supertypes = SupertypeSet::empty();
    if let Ok(Some(supers)) = opts.get::<_, Option<mlua::Table>>("additional_supertypes") {
        for pair in supers.sequence_values::<String>() {
            additional_supertypes |= parse_supertype_name(&pair.map_err(|e| ScriptError::LoadError(e.to_string()))?)?;
        }
    }
    Ok((keywords, name, additional_types, additional_supertypes))
}

/// `TriggeredAbilityBuilder`'s `:SetLayer` decoder — only a `Game.Layers.*`
/// tag is accepted (no custom-function fallback; unlike condition/operation,
/// there's no `Layer::Script` concept to fall back to).
fn decode_layer_value(value: Value) -> Result<Layer, ScriptError> {
    match value {
        Value::UserData(ud) => {
            let tag: LayerTag =
                ud.borrow::<LayerTag>().map_err(|e| ScriptError::LoadError(e.to_string()))?.clone();
            Ok(tag.0)
        }
        Value::Nil => Err(ScriptError::LoadError(
            "SetLayer() called with nil - expected a Game.Layers.* value".to_string(),
        )),
        other => Err(ScriptError::LoadError(format!(
            "SetLayer() expects a Game.Layers.* value, got {}",
            other.type_name()
        ))),
    }
}

/// `StaticAbilityBuilder`'s `:SetEffect` decoder — same two-shapes pattern
/// `decode_condition_value`/`decode_operation_value` already established: a
/// `Game.Effects.*` tag decodes to `EffectPayload::Primitive` with zero
/// further evaluation, a custom function is registered and returned as
/// `EffectPayload::Script` (invoked later via
/// `ScriptRuntime::invoke_continuous_effect`). Needs `&mut ScriptRegistry` to
/// register the function, same as `decode_condition_value`/
/// `decode_operation_value`.
fn decode_effect_value(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    value: Value,
) -> Result<EffectPayload, ScriptError> {
    match value {
        Value::UserData(ud) => {
            let tag: EffectPrimitiveTag = ud
                .borrow::<EffectPrimitiveTag>()
                .map_err(|e| ScriptError::LoadError(e.to_string()))?
                .clone();
            Ok(EffectPayload::Primitive(tag.0))
        }
        Value::Function(function) => {
            let key = lua
                .create_registry_value(function)
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let handle = registry.next_script_handle();
            registry.scripts.insert(handle, key);
            Ok(EffectPayload::Script(handle))
        }
        Value::Nil => Err(ScriptError::LoadError(
            "SetEffect() called with nil - expected a Game.Effects.* value or a function".to_string(),
        )),
        other => Err(ScriptError::LoadError(format!(
            "SetEffect() expects a Game.Effects.* value or a function, got {}",
            other.type_name()
        ))),
    }
}

/// `StaticAbilityBuilder`'s `:SetAffected` decoder — same two-shapes pattern
/// as `decode_effect_value` above: a plain Lua table (`{controller=..,
/// card_types={..}, self_only=..}`, every key optional) decodes to
/// `AffectedFilter::Primitive` with zero further evaluation, matching how
/// `EffectFilter`/the scenario TOML's `ContinuousEffectSpec` already read as
/// a flat option bag. Omitting the table entirely (or every key in it) means
/// "no filter, applies to everything" — a real, if unusual, shape (see
/// `EffectFilter`'s own fields, all optional/defaulted there too). A custom
/// function is registered and returned as `AffectedFilter::Script` (invoked
/// later via `ScriptRuntime::invoke_continuous_affects`), same registration
/// as every other function-fallback decoder in this file.
fn decode_affected_value(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    value: Value,
) -> Result<AffectedFilter, ScriptError> {
    match value {
        Value::Table(table) => {
            let controller_name: Option<String> =
                table.get("controller").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let controller = controller_name.map(|name| parse_filter_controller_name(&name)).transpose()?;

            let card_types_table: Option<Table> =
                table.get("card_types").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let card_types = match card_types_table {
                Some(t) => {
                    let mut set = CardTypeSet::empty();
                    for entry in t.sequence_values::<String>() {
                        let name = entry.map_err(|e| ScriptError::LoadError(e.to_string()))?;
                        set |= parse_card_type_name(&name)?;
                    }
                    Some(set)
                }
                None => None,
            };

            let self_only: Option<bool> =
                table.get("self_only").map_err(|e| ScriptError::LoadError(e.to_string()))?;

            Ok(AffectedFilter::Primitive(EffectFilter {
                controller,
                card_types,
                self_only: self_only.unwrap_or(false),
            }))
        }
        Value::Function(function) => {
            let key = lua
                .create_registry_value(function)
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let handle = registry.next_script_handle();
            registry.scripts.insert(handle, key);
            Ok(AffectedFilter::Script(handle))
        }
        other => Err(ScriptError::LoadError(format!(
            "SetAffected() expects a table or a function, got {}",
            other.type_name()
        ))),
    }
}

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
            | (Layer::ColorChanging, EffectPrimitive::AddColor(_))
            | (Layer::AbilityAdding, EffectPrimitive::AddKeyword(_))
            | (Layer::AbilityAdding, EffectPrimitive::RemoveAllAbilities)
            | (Layer::AbilityAdding, EffectPrimitive::AddProtectionColor(_))
            | (Layer::AbilityAdding, EffectPrimitive::AddProtectionCardType(_))
            | (Layer::AbilityAdding, EffectPrimitive::AddProtectionSubtype(_))
            | (Layer::AbilityAdding, EffectPrimitive::AddProtectionName(_))
            | (Layer::AbilityAdding, EffectPrimitive::AddProtectionEverything)
            | (Layer::AbilityAdding, EffectPrimitive::AddProtectionPlayer(_))
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
/// damage-event rewrite, not a zone-change one.
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
/// `:SetCost`/`:SetTarget` are deliberately not bound this slice —
/// `ScriptRuntime::invoke_cost`/`invoke_target` are still `todo!()` regardless,
/// so nothing would consume them yet.
struct TriggeredAbilityBuilder {
    source: ObjectId,
    condition: Option<ConditionSpec>,
    operation: Option<(Option<AbilityEffect>, ScriptHandle)>,
    /// CR 603.4's intervening-if clause — see `InterveningIfSpec`'s own doc
    /// comment. Set via `:SetInterveningIf(Game.InterveningIf.*)`, orthogonal
    /// to `condition` above (the trigger-event check).
    intervening_if: Option<InterveningIfSpec>,
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
                condition: this.condition.clone(),
                cost: None,
                target: None,
                operation,
                effect,
                intervening_if: this.intervening_if.clone(),
            };
            this.registry.borrow_mut().pending_triggered.push(ability);
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
/// `:SetTarget(table)` (added for `phase5/equip-ability`, CR 702.6a's
/// "target creature you control") reuses `decode_target_filter_table`, the
/// same table-shape decoder `Game.SelectTargets` already uses — e.g.
/// `{type="object", card_types={"Creature"}, controller="you"}`. Always a
/// single, exact-count target (`count: 1, up_to: false`) — no "up to N" or
/// multi-target shape, same narrowing precedent `Game.SelectTargets`'s own
/// doc comment already set for the identical reason (nothing built yet to
/// consume more than one). `CardDefinition.mana_cost` (spells) is untouched
/// by `ManaCost::parse` existing now — it's still sourced as a structured
/// `ManaCost` from the external card database, never parsed from a string.
struct ActivatedAbilityBuilder {
    source: ObjectId,
    cost: CostSpec,
    target: Option<TargetFilter>,
    operation: Option<(Option<AbilityEffect>, ScriptHandle)>,
    registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for ActivatedAbilityBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
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
        methods.add_method_mut("SetTarget", |_, this, table: Table| {
            this.target = Some(decode_target_filter_table(table).map_err(to_lua_err)?);
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
                .map(|filter| TargetSpec::Primitive(TargetRequirement { filter, count: 1, up_to: false }));
            let ability = RegisteredAbility {
                source: this.source,
                kind: AbilityKind::Activated,
                condition: None,
                cost,
                target,
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
struct StaticAbilityBuilder {
    source: ObjectId,
    layer: Option<Layer>,
    effect: Option<EffectPayload>,
    affected: AffectedFilter,
    /// CR 611.2c's "applies only as long as X is true" — see
    /// `ContinuousEffect::condition`'s own doc comment. Set via
    /// `:SetCondition(Game.InterveningIf.*)`, same decode shape
    /// `TriggeredAbilityBuilder::SetInterveningIf` already uses.
    condition: Option<InterveningIfSpec>,
    /// CR 611.2a-style limited-duration static effect (e.g. "artifacts you
    /// control gain flying until end of turn") — set via
    /// `:SetDuration(Game.Durations.*)`, same `DurationTag` decode
    /// `Game.Ops.Pump`'s duration argument already uses. Defaults to
    /// `Permanent` (this builder's original, only behavior before this
    /// field existed) when never called.
    duration: EffectDuration,
    registry: Rc<RefCell<ScriptRegistry>>,
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
struct ReplacementEffectBuilder {
    source: ObjectId,
    affected: EffectFilter,
    event_kind: EventFilterKind,
    payload: ReplacementPayload,
    consumption: ConsumptionPolicy,
    registry: Rc<RefCell<ScriptRegistry>>,
}

impl UserData for ReplacementEffectBuilder {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method_mut("SetAffected", |_, this, table: Table| {
            let controller_name: Option<String> =
                table.get("controller").map_err(|e| ScriptError::LoadError(e.to_string())).map_err(to_lua_err)?;
            let controller = controller_name
                .map(|name| parse_filter_controller_name(&name))
                .transpose()
                .map_err(to_lua_err)?;

            let card_types_table: Option<Table> =
                table.get("card_types").map_err(|e| ScriptError::LoadError(e.to_string())).map_err(to_lua_err)?;
            let card_types = match card_types_table {
                Some(t) => {
                    let mut set = CardTypeSet::empty();
                    for entry in t.sequence_values::<String>() {
                        let name = entry.map_err(|e| ScriptError::LoadError(e.to_string())).map_err(to_lua_err)?;
                        set |= parse_card_type_name(&name).map_err(to_lua_err)?;
                    }
                    Some(set)
                }
                None => None,
            };

            let self_only: Option<bool> =
                table.get("self_only").map_err(|e| ScriptError::LoadError(e.to_string())).map_err(to_lua_err)?;

            this.affected = EffectFilter { controller, card_types, self_only: self_only.unwrap_or(false) };
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
                EventFilterKind::DamageDealt => EventFilter::DamageDealt { affected: this.affected },
                EventFilterKind::WouldDie => EventFilter::WouldDie { affected: this.affected },
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
    /// with a callable `initial_effect` field (the ygopro-style
    /// `s.initial_effect(c)` module convention — see `docs/lua_scripts.md`),
    /// and calls it with `source.0`. Whatever the script's `initial_effect`
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
        let initial_effect: Function = module
            .get("initial_effect")
            .map_err(|e| ScriptError::LoadError(format!("card script has no initial_effect: {e}")))?;
        initial_effect
            .call::<_, ()>(source.0)
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;

        let mut registered: Vec<RegisteredAbility> =
            self.registry.borrow_mut().pending_triggered.drain(..).collect();
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
    pub fn invoke_continuous_affects(
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
    ///
    /// An empty/unrecognized table, or a script that errors, resolves to
    /// `None` — same "skip rather than guess" stance every other
    /// unresolvable value in this engine already takes; the caller
    /// (`layers::resolve_primitive`) doesn't distinguish "no effect this
    /// call" from "script errored," same as `invoke_cda`'s own caller.
    pub fn invoke_continuous_effect(
        &self,
        handle: ScriptHandle,
        state: &GameState,
        source: ObjectId,
        controller: PlayerId,
    ) -> Result<Option<EffectPrimitive>, ScriptError> {
        let function = self.function(handle)?;
        let state_value = state_to_lua(&self.lua, state)?;
        let result: Table = function
            .call((source.0, controller.0, state_value))
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
        Ok(None)
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

/// Decodes `Game.SelectTargets`/`Game.MayChoose`'s yielded request table —
/// `{ op = "select_target", filter = {...} }` or `{ op = "may_choose" }`. An
/// unrecognized op is a real error, not guessed at, same stance
/// `decode_condition_value` takes for an unrecognized shape.
fn decode_script_choice_request(table: Table) -> Result<ScriptChoiceRequest, ScriptError> {
    let op: String = table.get("op").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    match op.as_str() {
        "select_target" => {
            let filter_table: Table =
                table.get("filter").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            Ok(ScriptChoiceRequest::SelectTarget { filter: decode_target_filter_table(filter_table)? })
        }
        "may_choose" => Ok(ScriptChoiceRequest::MayChoose),
        other => Err(ScriptError::RuntimeError(format!("unknown script choice request op '{other}'"))),
    }
}

/// `Game.SelectTargets`'s filter argument — same flat option-bag shape
/// `decode_affected_value`/`decode_filter_table` already use for their own
/// filter tables, plus a `type` discriminator (`"object"` default/`"player"`/
/// `"any"`) matching the scenario crate's own TOML `target_requirement.filter`
/// convention (`crates/scenario/src/lib.rs`'s `build_target_requirement`)
/// exactly, rather than inventing a second vocabulary. `type` defaults to
/// `"object"` so every pre-existing script that omits it (this slice's own
/// predecessor) keeps working unchanged. No zone override (backlog item 9's
/// own narrowing, unchanged by this slice).
fn decode_target_filter_table(table: Table) -> Result<TargetFilter, ScriptError> {
    let kind: Option<String> = table.get("type").map_err(|e| ScriptError::LoadError(e.to_string()))?;
    match kind.as_deref().unwrap_or("object") {
        "player" => Ok(TargetFilter::Player),
        "any" => Ok(TargetFilter::AnyTarget),
        "object" => {
            let controller_name: Option<String> =
                table.get("controller").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let controller = controller_name.map(|name| parse_filter_controller_name(&name)).transpose()?;

            let card_types_table: Option<Table> =
                table.get("card_types").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let card_types = match card_types_table {
                Some(t) => {
                    let mut set = CardTypeSet::empty();
                    for entry in t.sequence_values::<String>() {
                        set |=
                            parse_card_type_name(&entry.map_err(|e| ScriptError::LoadError(e.to_string()))?)?;
                    }
                    Some(set)
                }
                None => None,
            };

            // CR 205.4b: "target Human"/"Equip Dragons" - a plain list of
            // subtype name strings, same shape card_types just above uses,
            // matched as an "or" (TargetFilter::Object::subtypes's own doc
            // comment). No parser needed - Subtype is a bare string newtype,
            // same "any string is legal" stance AddProtectionSubtype/
            // AddProtectionName already took.
            let subtypes_table: Option<Table> =
                table.get("subtypes").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let subtypes = match subtypes_table {
                Some(t) => {
                    let mut list = Vec::new();
                    for entry in t.sequence_values::<String>() {
                        list.push(Subtype(entry.map_err(|e| ScriptError::LoadError(e.to_string()))?));
                    }
                    Some(list)
                }
                None => None,
            };

            // CR 205.4a: "target legendary creature" - same shape/table key
            // decode_filter_table's own supertypes decode already uses.
            let supertypes_table: Option<Table> =
                table.get("supertypes").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let supertypes = match supertypes_table {
                Some(t) => {
                    let mut set = SupertypeSet::empty();
                    for entry in t.sequence_values::<String>() {
                        let name = entry.map_err(|e| ScriptError::LoadError(e.to_string()))?;
                        set |= parse_supertype_name(&name)?;
                    }
                    Some(set)
                }
                None => None,
            };

            Ok(TargetFilter::Object { card_types, subtypes, supertypes, controller, zone: None })
        }
        other => Err(ScriptError::LoadError(format!(
            "unknown Game.SelectTargets filter type '{other}' - expected object/player/any"
        ))),
    }
}

/// Builds the `Game.Conditions`/`Game.Ops`/`Game.Costs`/`Game.Layers`/
/// `Game.Effects`/`Game.CreateTriggeredAbility`/`Game.CreateStaticAbility`/
/// `Game.CreateActivatedAbility` sugar and binds it into `lua`'s globals.
/// Deliberately not the same thing as binding all of `script_api.rs`'s
/// functions (`Game.ReplaceEvent`/`Game.CastSpell`/`Game.PayCost`/
/// `Game.SelectTargets` stay unbound — `Game.AddContinuousEffect`
/// specifically is now superseded by `Game.CreateStaticAbility`'s builder,
/// same relationship `register_triggered_ability` has to
/// `Game.CreateTriggeredAbility`) — just enough sugar for a
/// condition/operation source string to be a one-liner, and for a card
/// script to self-register a triggered, static, or activated ability via
/// the builders above.
fn bind_game_globals(lua: &Lua, registry: Rc<RefCell<ScriptRegistry>>) -> mlua::Result<()> {
    let conditions = lua.create_table()?;
    conditions.set("Enters", ConditionTag(ConditionPrimitive::EntersBattlefield))?;
    conditions.set("Always", ConditionTag(ConditionPrimitive::Always))?;
    conditions.set("DealsDamage", ConditionTag(ConditionPrimitive::DealsDamage))?;
    conditions.set("IsDealtDamage", ConditionTag(ConditionPrimitive::IsDealtDamage))?;
    conditions.set("Upkeep", ConditionTag(ConditionPrimitive::BeginningOfYourUpkeep))?;
    conditions.set("EachUpkeep", ConditionTag(ConditionPrimitive::BeginningOfEachUpkeep))?;
    conditions.set("Untap", ConditionTag(ConditionPrimitive::BeginningOfYourUntap))?;
    conditions.set("EachUntap", ConditionTag(ConditionPrimitive::BeginningOfEachUntap))?;
    conditions.set("Dies", ConditionTag(ConditionPrimitive::Dies))?;
    conditions.set("LeavesBattlefield", ConditionTag(ConditionPrimitive::LeavesBattlefield))?;
    conditions.set("DrawsCard", ConditionTag(ConditionPrimitive::DrawsCard))?;
    conditions.set("EachDrawsCard", ConditionTag(ConditionPrimitive::EachDrawsCard))?;
    conditions.set("DiscardsCard", ConditionTag(ConditionPrimitive::DiscardsCard))?;
    conditions.set("EachDiscardsCard", ConditionTag(ConditionPrimitive::EachDiscardsCard))?;
    conditions.set("Discarded", ConditionTag(ConditionPrimitive::Discarded))?;
    conditions.set("Scries", ConditionTag(ConditionPrimitive::Scries))?;
    conditions.set("EachScries", ConditionTag(ConditionPrimitive::EachScries))?;
    conditions.set("Surveils", ConditionTag(ConditionPrimitive::Surveils))?;
    conditions.set("EachSurveils", ConditionTag(ConditionPrimitive::EachSurveils))?;
    // CR 700.4/603.10: "whenever a creature you control dies" - filter-
    // scoped sibling of Dies/LeavesBattlefield, reusing decode_filter_table
    // (same table shape Game.Ops.Search/Game.Costs.Sacrifice already
    // accept) rather than a bespoke argument shape.
    let dies_matching = lua.create_function(|_, table: Table| {
        let filter = decode_filter_table(table).map_err(to_lua_err)?;
        Ok(ConditionTag(ConditionPrimitive::DiesMatching(filter)))
    })?;
    conditions.set("DiesMatching", dies_matching)?;
    let leaves_battlefield_matching = lua.create_function(|_, table: Table| {
        let filter = decode_filter_table(table).map_err(to_lua_err)?;
        Ok(ConditionTag(ConditionPrimitive::LeavesBattlefieldMatching(filter)))
    })?;
    conditions.set("LeavesBattlefieldMatching", leaves_battlefield_matching)?;
    conditions.set("BeginningOfCombat", ConditionTag(ConditionPrimitive::BeginningOfYourCombat))?;
    conditions.set("EachBeginningOfCombat", ConditionTag(ConditionPrimitive::BeginningOfEachCombat))?;
    conditions.set("EndStep", ConditionTag(ConditionPrimitive::BeginningOfYourEndStep))?;
    conditions.set("EachEndStep", ConditionTag(ConditionPrimitive::BeginningOfEachEndStep))?;
    // CR 603.2h: `Game.Conditions.OncePerTurn(Game.Conditions.X)` wraps any
    // other tag above so the resulting ability only ever triggers once each
    // turn - see OncePerTurnTag's own doc comment.
    let once_per_turn = lua.create_function(|_, tag: mlua::AnyUserData| {
        let primitive = tag.borrow::<ConditionTag>()?.0.clone();
        Ok(OncePerTurnTag(primitive))
    })?;
    conditions.set("OncePerTurn", once_per_turn)?;

    // CR 603.4: `Game.InterveningIf.*` tags, consumed by
    // `TriggeredAbilityBuilder:SetInterveningIf` and (CR 611.2c)
    // `StaticAbilityBuilder:SetCondition` — see `InterveningIfTag`'s own doc
    // comment for why this is a separate table from `Conditions`.
    let intervening_if = lua.create_table()?;
    let at_or_below_life = lua.create_function(|_, threshold: i32| {
        Ok(InterveningIfTag(InterveningIfPrimitive::ControllerAtOrBelowLife(threshold)))
    })?;
    intervening_if.set("AtOrBelowLife", at_or_below_life)?;
    // CR 611.2c's overwhelmingly common real shape - "as long as you control
    // N or more [filter]" - reusing decode_filter_table (same table shape
    // Game.Conditions.DiesMatching/Game.Costs.Sacrifice already accept).
    let controls_at_least = lua.create_function(|_, (table, count): (Table, u32)| {
        let filter = decode_filter_table(table).map_err(to_lua_err)?;
        Ok(InterveningIfTag(InterveningIfPrimitive::ControllerControlsAtLeast { filter, count }))
    })?;
    intervening_if.set("ControlsAtLeast", controls_at_least)?;

    let ops = lua.create_table()?;
    // CR 605: `Game.Ops.AddMana(letter, count?)` - `letter` is a single
    // WUBRG/"C" (colorless) symbol, `count` (default 1) repeats that same
    // fixed unit - covers the vast majority of real mana abilities ("add
    // {G}", "add two colorless"). No mixed-color-in-one-call shape (e.g.
    // "add one white and one blue") - named as a follow-up in this slice's
    // own UNRESOLVED note rather than guessed at.
    let add_mana = lua.create_function(|_, (letter, count): (String, Option<u32>)| {
        let unit = parse_mana_unit_letter(&letter).map_err(to_lua_err)?;
        Ok(OperationTag(AbilityEffect::AddMana(vec![unit; count.unwrap_or(1) as usize])))
    })?;
    ops.set("AddMana", add_mana)?;
    // CR 605's "add mana of any color" shape -
    // `Game.Ops.AddManaChoice(count, colors)`: `count` is how many
    // independently-chosen mana this ability produces, `colors` is a WUBRG
    // letter string (reuses `parse_color_string`, same vocabulary
    // `Game.Effects.AddColor` already uses) naming which colors are allowed.
    // The actual color(s) picked arrive later on `PlayerActionKind::
    // ActivateAbility::chosen_colors`, at activation time (CR 602.2b) - this
    // binding only declares the shape, same "Lua asks, Rust decides" split
    // every other primitive in this table already follows.
    let add_mana_choice = lua.create_function(|_, (count, colors): (u32, String)| {
        let choices = parse_color_string(&colors).map_err(to_lua_err)?;
        Ok(OperationTag(AbilityEffect::AddManaChoice { count, choices }))
    })?;
    ops.set("AddManaChoice", add_mana_choice)?;
    let gain_life = lua.create_function(|_, amount: i32| Ok(OperationTag(AbilityEffect::GainLife(amount))))?;
    ops.set("GainLife", gain_life)?;
    let damage = lua.create_function(|_, amount: u32| Ok(OperationTag(AbilityEffect::Damage(amount))))?;
    ops.set("Damage", damage)?;
    ops.set("Destroy", OperationTag(AbilityEffect::Destroy))?;
    ops.set("Sacrifice", OperationTag(AbilityEffect::Sacrifice))?;
    ops.set("Exile", OperationTag(AbilityEffect::Exile))?;
    let counters = lua.create_function(|_, (kind, amount): (String, i32)| {
        Ok(OperationTag(AbilityEffect::Counters { kind: parse_counter_kind_name(&kind), amount }))
    })?;
    ops.set("Counters", counters)?;
    // `Game.Ops.Pump(power, toughness, duration)` - `duration` is a
    // `Game.Durations.*` tag (see `DurationTag`'s own doc comment for which
    // ones exist and why).
    let pump = lua.create_function(|_, (power, toughness, duration): (i32, i32, mlua::AnyUserData)| {
        let duration = duration.borrow::<DurationTag>()?.0.clone();
        Ok(OperationTag(AbilityEffect::Pump { power, toughness, duration }))
    })?;
    ops.set("Pump", pump)?;
    // `Game.Ops.GainControl(duration)` - same `Game.Durations.*` tag decode
    // as Pump's own duration argument. No player argument - the new
    // controller is always the resolving spell/ability's own controller
    // (see AbilityEffect::GainControl's own doc comment for why).
    let gain_control = lua.create_function(|_, duration: mlua::AnyUserData| {
        let duration = duration.borrow::<DurationTag>()?.0.clone();
        Ok(OperationTag(AbilityEffect::GainControl(duration)))
    })?;
    ops.set("GainControl", gain_control)?;
    // `Game.Ops.Regenerate()` (CR 701.19a) - no arguments, same toggle shape
    // as `Game.Ops.Destroy`/`Sacrifice`/`Exile`/`Shuffle`.
    ops.set("Regenerate", OperationTag(AbilityEffect::Regenerate))?;
    let draw_cards = lua.create_function(|_, amount: u32| Ok(OperationTag(AbilityEffect::DrawCards(amount))))?;
    ops.set("DrawCards", draw_cards)?;
    let create_token = lua.create_function(|_, (spec_ud, count): (mlua::AnyUserData, Option<u32>)| {
        let spec = spec_ud.borrow::<TokenTag>()?.0.clone();
        Ok(OperationTag(AbilityEffect::CreateToken { spec, count: count.unwrap_or(1) }))
    })?;
    ops.set("CreateToken", create_token)?;
    ops.set("Shuffle", OperationTag(AbilityEffect::Shuffle))?;
    let scry = lua.create_function(|_, n: u32| Ok(OperationTag(AbilityEffect::Scry(n))))?;
    ops.set("Scry", scry)?;
    let surveil = lua.create_function(|_, n: u32| Ok(OperationTag(AbilityEffect::Surveil(n))))?;
    ops.set("Surveil", surveil)?;
    // `Game.Ops.Search(filter_table, count, opts?)` — `filter_table` reuses
    // `decode_filter_table` (same table shape `Game.Costs.Sacrifice`/
    // `Discard`/`TapPermanents` already accept, extended with
    // `supertypes`/`subtypes` alongside this backlog-item-14b slice).
    // `opts.destination` is `"hand"` (default) or `"battlefield"`;
    // `opts.tapped` only matters for `"battlefield"`; `opts.shuffle`
    // defaults to `true` (CR 701.24a: nearly every real search effect pairs
    // with "then shuffle").
    let search = lua.create_function(|_, (filter_table, count, opts): (Table, u32, Option<Table>)| {
        let filter = decode_filter_table(filter_table).map_err(to_lua_err)?;
        let destination_name: Option<String> = match &opts {
            Some(t) => t.get("destination").map_err(|e| to_lua_err(ScriptError::LoadError(e.to_string())))?,
            None => None,
        };
        let destination = match destination_name.as_deref() {
            None | Some("hand") => SearchDestination::Hand,
            Some("battlefield") => {
                let tapped: Option<bool> = match &opts {
                    Some(t) => t.get("tapped").map_err(|e| to_lua_err(ScriptError::LoadError(e.to_string())))?,
                    None => None,
                };
                SearchDestination::Battlefield { tapped: tapped.unwrap_or(false) }
            }
            Some(other) => {
                return Err(to_lua_err(ScriptError::LoadError(format!(
                    "unknown Game.Ops.Search destination '{other}' - expected hand/battlefield"
                ))))
            }
        };
        let shuffle_after: Option<bool> = match &opts {
            Some(t) => t.get("shuffle").map_err(|e| to_lua_err(ScriptError::LoadError(e.to_string())))?,
            None => None,
        };
        Ok(OperationTag(AbilityEffect::Search {
            filter,
            count,
            destination,
            shuffle_after: shuffle_after.unwrap_or(true),
        }))
    })?;
    ops.set("Search", search)?;
    // `Game.Ops.BecomeCopy()` (CR 707.2, layer 1) - no arguments, same
    // toggle shape as `Game.Ops.Destroy`/`Sacrifice`/`Exile`/`Shuffle`; the
    // copy target is whatever the ability's own targeting already resolved,
    // not a value passed here.
    ops.set("BecomeCopy", OperationTag(AbilityEffect::BecomeCopy))?;
    // `Game.Ops.Attach` (CR 702.6a, Equip) - no arguments, same toggle shape
    // as `Game.Ops.Destroy`/`Sacrifice`/`Exile`/`Shuffle`/`BecomeCopy`; the
    // attach target is whatever the activated ability's own `:SetTarget`
    // resolved, not a value passed here.
    ops.set("Attach", OperationTag(AbilityEffect::Attach))?;

    let costs = lua.create_table()?;
    costs.set("TapSelf", CostTag(CostComponent::Additional(AdditionalCost::TapSelf)))?;
    costs.set(
        "Sacrifice",
        lua.create_function(|_, table: Table| {
            let filter = decode_filter_table(table).map_err(to_lua_err)?;
            Ok(CostTag(CostComponent::Additional(AdditionalCost::Sacrifice { filter })))
        })?,
    )?;
    costs.set(
        "Discard",
        lua.create_function(|_, table: Table| {
            let filter = decode_filter_table(table).map_err(to_lua_err)?;
            Ok(CostTag(CostComponent::Additional(AdditionalCost::Discard { filter })))
        })?,
    )?;
    costs.set(
        "TapPermanents",
        lua.create_function(|_, (table, count): (Table, u32)| {
            let filter = decode_filter_table(table).map_err(to_lua_err)?;
            Ok(CostTag(CostComponent::Additional(AdditionalCost::TapPermanents { filter, count })))
        })?,
    )?;
    costs.set(
        "Life",
        lua.create_function(|_, amount: u32| {
            Ok(CostTag(CostComponent::Additional(AdditionalCost::LifePayment { amount })))
        })?,
    )?;
    // CR 107.4/202.3: a printed mana-cost string, e.g. `Game.Costs.Mana("{1}{R}")`
    // - see `ManaCost::parse`'s own doc comment for the grammar and for why
    // this is the fix for the "no Lua-facing mana-cost-string parser exists
    // anywhere yet" gap `ActivatedAbilityBuilder` previously flagged.
    costs.set(
        "Mana",
        lua.create_function(|_, s: String| {
            let mana = ManaCost::parse(&s)
                .map_err(|e| mlua::Error::RuntimeError(format!("Game.Costs.Mana(\"{s}\"): {}", e.0)))?;
            Ok(CostTag(CostComponent::Mana(mana)))
        })?,
    )?;

    // Only the 4 durations with a real drain path are exposed - see
    // `DurationTag`'s own doc comment for why the rest aren't.
    let durations = lua.create_table()?;
    durations.set("UntilEndOfTurn", DurationTag(EffectDuration::UntilEndOfTurn))?;
    durations.set("UntilYourNextTurn", DurationTag(EffectDuration::UntilYourNextTurn))?;
    durations.set("UntilEndOfCombat", DurationTag(EffectDuration::UntilEndOfCombat))?;
    durations.set("Permanent", DurationTag(EffectDuration::Permanent))?;

    // Only the 7 layers with a real query path (layers::effective_characteristics)
    // are exposed - a static ability can't request layer 1/2/7d at all (see
    // `Layer`'s own doc comment for what's unbuilt and why).
    let layers = lua.create_table()?;
    layers.set("TextChanging", LayerTag(Layer::TextChanging))?;
    layers.set("TypeChanging", LayerTag(Layer::TypeChanging))?;
    layers.set("ColorChanging", LayerTag(Layer::ColorChanging))?;
    layers.set("AbilityAdding", LayerTag(Layer::AbilityAdding))?;
    layers.set("PowerToughnessSetting", LayerTag(Layer::PowerToughnessSetting))?;
    layers.set("PowerToughnessModifying", LayerTag(Layer::PowerToughnessModifying))?;
    layers.set("PowerToughnessSwitching", LayerTag(Layer::PowerToughnessSwitching))?;

    let effects = lua.create_table()?;
    effects.set(
        "SetName",
        lua.create_function(|_, new_name: String| Ok(EffectPrimitiveTag(EffectPrimitive::SetName(new_name))))?,
    )?;
    effects.set(
        "SetPT",
        lua.create_function(|_, (power, toughness): (i32, i32)| {
            Ok(EffectPrimitiveTag(EffectPrimitive::SetPT { power, toughness }))
        })?,
    )?;
    effects.set(
        "ModifyPT",
        lua.create_function(|_, (power, toughness): (i32, i32)| {
            Ok(EffectPrimitiveTag(EffectPrimitive::ModifyPT { power, toughness }))
        })?,
    )?;
    effects.set(
        "AddKeyword",
        lua.create_function(|_, name: String| {
            let set = parse_keyword_name(&name).map_err(to_lua_err)?;
            Ok(EffectPrimitiveTag(EffectPrimitive::AddKeyword(set)))
        })?,
    )?;
    effects.set(
        "AddProtectionColor",
        lua.create_function(|_, letter: String| {
            let set = parse_color_letter(&letter).map_err(to_lua_err)?;
            Ok(EffectPrimitiveTag(EffectPrimitive::AddProtectionColor(set)))
        })?,
    )?;
    effects.set(
        "AddProtectionCardType",
        lua.create_function(|_, name: String| {
            let set = parse_card_type_name(&name).map_err(to_lua_err)?;
            Ok(EffectPrimitiveTag(EffectPrimitive::AddProtectionCardType(set)))
        })?,
    )?;
    effects.set(
        "AddProtectionSubtype",
        lua.create_function(|_, name: String| Ok(EffectPrimitiveTag(EffectPrimitive::AddProtectionSubtype(Subtype(name)))))?,
    )?;
    effects.set(
        "AddProtectionName",
        lua.create_function(|_, name: String| Ok(EffectPrimitiveTag(EffectPrimitive::AddProtectionName(name))))?,
    )?;
    effects.set(
        "AddCardType",
        lua.create_function(|_, name: String| {
            let set = parse_card_type_name(&name).map_err(to_lua_err)?;
            Ok(EffectPrimitiveTag(EffectPrimitive::AddCardType(set)))
        })?,
    )?;
    effects.set(
        "AddColor",
        lua.create_function(|_, letter: String| {
            let set = parse_color_letter(&letter).map_err(to_lua_err)?;
            Ok(EffectPrimitiveTag(EffectPrimitive::AddColor(set)))
        })?,
    )?;
    effects.set("SwitchPT", EffectPrimitiveTag(EffectPrimitive::SwitchPT))?;
    effects.set("RemoveAllAbilities", EffectPrimitiveTag(EffectPrimitive::RemoveAllAbilities))?;
    effects.set("AddProtectionEverything", EffectPrimitiveTag(EffectPrimitive::AddProtectionEverything))?;
    effects.set(
        "AddProtectionPlayer",
        lua.create_function(|_, who: String| {
            let filter = parse_filter_controller_name(&who).map_err(to_lua_err)?;
            if filter == FilterController::Any {
                return Err(to_lua_err(ScriptError::LoadError(
                    "AddProtectionPlayer(\"any\") isn't a CR 702.16k quality - use \
                     Game.Effects.AddProtectionEverything for CR 702.16j instead"
                        .to_string(),
                )));
            }
            Ok(EffectPrimitiveTag(EffectPrimitive::AddProtectionPlayer(filter)))
        })?,
    )?;

    // `Game.EventFilters.*` - which EventFilter shape a ReplacementEffect
    // watches (ReplacementEffectBuilder::SetFilter). Only the two shapes
    // with a real query path (ReplacementEffect::matches) are ever bound -
    // see EventFilter's own doc comment for what's unmodeled.
    let event_filters = lua.create_table()?;
    event_filters.set("DamageDealt", EventFilterKindTag(EventFilterKind::DamageDealt))?;
    event_filters.set("WouldDie", EventFilterKindTag(EventFilterKind::WouldDie))?;

    // `Game.ReplacementPayloads.*` - what a ReplacementEffect rewrites the
    // matched event into (ReplacementEffectBuilder::SetPayload).
    // `RedirectDamageToSource` is a sentinel, not a ReplacementPayloadTag -
    // see RedirectDamageToSourceTag's own doc comment for why.
    let replacement_payloads = lua.create_table()?;
    replacement_payloads.set("PreventDamage", ReplacementPayloadTag(ReplacementPayload::PreventDamage))?;
    replacement_payloads.set("Regenerate", ReplacementPayloadTag(ReplacementPayload::Regenerate))?;
    replacement_payloads.set("RedirectDamageToSource", RedirectDamageToSourceTag)?;
    replacement_payloads.set(
        "ReplaceZoneDestination",
        lua.create_function(|_, zone_name: String| {
            let zone = parse_zone_kind_name(&zone_name).map_err(to_lua_err)?;
            Ok(ReplacementPayloadTag(ReplacementPayload::ReplaceZoneDestination(zone)))
        })?,
    )?;

    // `Game.Consumption.*` - a ReplacementEffect's ConsumptionPolicy
    // (ReplacementEffectBuilder::SetConsumption).
    let consumption = lua.create_table()?;
    consumption.set("OneShot", ConsumptionPolicyTag(ConsumptionPolicy::OneShot))?;
    consumption.set("Repeatable", ConsumptionPolicyTag(ConsumptionPolicy::Repeatable))?;
    consumption.set(
        "LimitedUses",
        lua.create_function(|_, uses: u32| Ok(ConsumptionPolicyTag(ConsumptionPolicy::LimitedUses(uses))))?,
    )?;

    let create_triggered_ability = {
        let registry = Rc::clone(&registry);
        lua.create_function(move |_, source: u64| {
            Ok(TriggeredAbilityBuilder {
                source: ObjectId(source),
                condition: None,
                operation: None,
                intervening_if: None,
                registry: Rc::clone(&registry),
            })
        })?
    };

    let create_static_ability = {
        let registry = Rc::clone(&registry);
        lua.create_function(move |_, source: u64| {
            Ok(StaticAbilityBuilder {
                source: ObjectId(source),
                layer: None,
                effect: None,
                affected: AffectedFilter::Primitive(EffectFilter {
                    controller: None,
                    card_types: None,
                    self_only: false,
                }),
                condition: None,
                duration: EffectDuration::Permanent,
                registry: Rc::clone(&registry),
            })
        })?
    };

    let create_replacement_effect = {
        let registry = Rc::clone(&registry);
        lua.create_function(move |_, source: u64| {
            Ok(ReplacementEffectBuilder {
                source: ObjectId(source),
                affected: EffectFilter { controller: None, card_types: None, self_only: false },
                event_kind: EventFilterKind::WouldDie,
                payload: ReplacementPayload::Regenerate,
                consumption: ConsumptionPolicy::Repeatable,
                registry: Rc::clone(&registry),
            })
        })?
    };

    let create_activated_ability = {
        let registry = Rc::clone(&registry);
        lua.create_function(move |_, source: u64| {
            Ok(ActivatedAbilityBuilder {
                source: ObjectId(source),
                cost: CostSpec { mana: None, additional: Vec::new(), alternative: Vec::new() },
                target: None,
                operation: None,
                registry: Rc::clone(&registry),
            })
        })?
    };

    let tokens = lua.create_table()?;
    tokens.set("Clue", TokenTag(TokenSpec::Named(NamedToken::Clue)))?;
    tokens.set("Food", TokenTag(TokenSpec::Named(NamedToken::Food)))?;
    // TODO: Game.Tokens.Treasure blocked on coroutines (backlog item 9)
    tokens.set(
        "Creature",
        lua.create_function(|_, (subtypes_val, colors_str, power, toughness, opts): (Value, String, i32, i32, Option<mlua::Table>)| {
            let subtypes = decode_subtypes(subtypes_val).map_err(to_lua_err)?;
            let colors = parse_color_string(&colors_str).map_err(to_lua_err)?;
            let (keywords, name, additional_types, additional_supertypes) =
                decode_creature_token_opts(opts).map_err(to_lua_err)?;
            Ok(TokenTag(TokenSpec::Creature(CreatureTokenSpec {
                subtypes, colors, power, toughness,
                keywords, name, additional_types, additional_supertypes,
            })))
        })?,
    )?;
    tokens.set(
        "ArtifactCreature",
        lua.create_function(|_, (subtypes_val, colors_str, power, toughness, opts): (Value, String, i32, i32, Option<mlua::Table>)| {
            let subtypes = decode_subtypes(subtypes_val).map_err(to_lua_err)?;
            let colors = parse_color_string(&colors_str).map_err(to_lua_err)?;
            let (keywords, name, extra_types, additional_supertypes) =
                decode_creature_token_opts(opts).map_err(to_lua_err)?;
            let additional_types = extra_types | CardTypeSet::ARTIFACT;
            Ok(TokenTag(TokenSpec::Creature(CreatureTokenSpec {
                subtypes, colors, power, toughness,
                keywords, name, additional_types, additional_supertypes,
            })))
        })?,
    )?;
    // `Game.Tokens.CopyOf(objectId)` (CR 707.10f) - see `TokenSpec::CopyOf`'s
    // own doc comment for why this token's `CardDefinition` isn't built
    // from any of the opts the other `Game.Tokens.*` entries above take.
    tokens.set(
        "CopyOf",
        lua.create_function(|_, source: u64| Ok(TokenTag(TokenSpec::CopyOf(ObjectId(source)))))?,
    )?;

    // Backlog item 9: `Game.SelectTargets(filter)` yields the request
    // straight to Lua's own `coroutine.yield` - deliberately NOT a
    // `create_function` Rust closure. mlua 0.9 (no `async` feature enabled
    // here) only lets code running *as* a coroutine call `coroutine.yield`;
    // a plain Rust-bound function can't suspend the Lua stack from inside a
    // callback the way `create_async_function` could. A tiny literal Lua
    // definition sidesteps needing that entirely - confirmed against real
    // vendored mlua in `phase5_coroutine_spike.rs` that a plain
    // `coroutine.yield` inside a `Thread` round-trips real values correctly.
    // `ScriptRuntime::invoke_operation`/`resume_operation` are what actually
    // decode the yielded table and drive the resulting pause/resume -
    // this binding only ever produces the request, never interprets it.
    let select_targets: Function = lua
        .load(r#"function(filter) return coroutine.yield({ op = "select_target", filter = filter or {} }) end"#)
        .eval()?;

    // Backlog item 9a (CR 608.2d): `Game.MayChoose()` yields a plain yes/no
    // request the same way `Game.SelectTargets` yields a targeting request —
    // same "literal Lua function calling coroutine.yield directly" reason
    // (mlua 0.9 without the `async` feature can't yield from inside a
    // Rust-bound closure). `resume_operation_bool` resumes with a raw
    // boolean, which becomes this call's own return value.
    let may_choose: Function =
        lua.load(r#"function() return coroutine.yield({ op = "may_choose" }) end"#).eval()?;

    let game = lua.create_table()?;
    game.set("Conditions", conditions)?;
    game.set("InterveningIf", intervening_if)?;
    game.set("Ops", ops)?;
    game.set("Costs", costs)?;
    game.set("Layers", layers)?;
    game.set("Durations", durations)?;
    game.set("Effects", effects)?;
    game.set("EventFilters", event_filters)?;
    game.set("ReplacementPayloads", replacement_payloads)?;
    game.set("Consumption", consumption)?;
    game.set("Tokens", tokens)?;
    game.set("CreateTriggeredAbility", create_triggered_ability)?;
    game.set("CreateStaticAbility", create_static_ability)?;
    game.set("CreateReplacementEffect", create_replacement_effect)?;
    game.set("CreateActivatedAbility", create_activated_ability)?;
    game.set("SelectTargets", select_targets)?;
    game.set("MayChoose", may_choose)?;
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

/// State snapshot passed as the trailing arg to every state-aware `invoke_*`
/// call (`invoke_condition`, `invoke_intervening_if`, `invoke_cda`,
/// `invoke_continuous_affects`, `invoke_continuous_effect`) — same "convert
/// to a plain Lua table snapshot, not a live handle" convention `event_to_lua`
/// already established. This is the real resolution of backlog item 8's
/// hidden-information-safe `ScriptContext` question (see DESIGN.md's open
/// question, and `ScriptContext`'s own doc comment for why the Rust-side
/// struct didn't need narrowing — this function is the actual boundary).
///
/// Per CR 400.2/402.3: battlefield/graveyard/exile/command are public zones —
/// full contents exposed as one card-summary table per object (see
/// `zone_cards_to_lua`/`card_summary_to_lua`). Hand/library are hidden zones
/// per CR 400.2 (even a fully-revealed hand stays modeled as hidden) — only a
/// *count* is exposed (`hand_count`/`library_count`), matching CR 402.3's
/// "may count those cards at any time, but can't look at them" split. There
/// is deliberately no binding anywhere that returns hand/library contents —
/// the legitimate ways to see into a hidden zone (search, scry, surveil) are
/// already their own dedicated `Game.Ops.*` primitives (CR 701.23/701.22d/
/// 701.25d) that commit a real zone change, not a live peek from an arbitrary
/// condition/CDA/static-ability script.
///
/// Card summaries are base (`CardDefinition`, netted against counters) values
/// only, not layer-resolved `effective_characteristics` — computing another
/// object's *effective* P/T to answer this snapshot would mean invoking
/// *that* object's own CDA/continuous-effect scripts too, which would build
/// their own snapshot right back through this same function: unbounded
/// recursion the moment any card on the battlefield has a CDA or scripted
/// continuous effect. Deferred as a named follow-up (a single-object
/// effective-characteristics query binding, invoked directly rather than
/// folded into a whole-board snapshot) rather than guessed at with an ad hoc
/// depth limit.
///
/// The stack (CR 400.2, also public) isn't included — `StackObject` isn't
/// `GameObject`-shaped (its characteristics come from `source`/`source_lki`),
/// so it needs its own decode, split out rather than bolted on here.
fn state_to_lua<'lua>(lua: &'lua Lua, state: &GameState) -> Result<Value<'lua>, ScriptError> {
    let table = lua.create_table().map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let life = lua.create_table().map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let hand_count = lua.create_table().map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let library_count = lua.create_table().map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let graveyard = lua.create_table().map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let command = lua.create_table().map_err(|e| ScriptError::RuntimeError(e.to_string()))?;

    for player in &state.players {
        let pid = player.player_id.0;
        life.set(pid, player.life).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        let hand_len =
            state.zone(ZoneKind::Hand, Some(player.player_id)).map(|z| z.len()).unwrap_or(0) as u32;
        hand_count.set(pid, hand_len).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        let library_len =
            state.zone(ZoneKind::Library, Some(player.player_id)).map(|z| z.len()).unwrap_or(0) as u32;
        library_count.set(pid, library_len).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        graveyard
            .set(pid, zone_cards_to_lua(lua, state, ZoneKind::Graveyard, Some(player.player_id))?)
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        command
            .set(pid, zone_cards_to_lua(lua, state, ZoneKind::Command, Some(player.player_id))?)
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    }

    table.set("life", life).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table.set("hand_count", hand_count).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table.set("library_count", library_count).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table
        .set("battlefield", zone_cards_to_lua(lua, state, ZoneKind::Battlefield, None)?)
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table
        .set("exile", zone_cards_to_lua(lua, state, ZoneKind::Exile, None)?)
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table.set("graveyard", graveyard).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table.set("command", command).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    Ok(Value::Table(table))
}

/// One public zone's contents as a 1-indexed array of `card_summary_to_lua`
/// tables, in zone order. `owner` is `None` for the shared battlefield/exile
/// zones, `Some(player)` for the per-player graveyard/command zones — same
/// split `Zone`'s own doc comment establishes. An object id present in
/// `zone.contents` with no matching `GameState::object` entry is skipped
/// rather than panicking — shouldn't happen (zone contents and the object
/// store are supposed to stay in sync) but this is a read-only snapshot, not
/// the place to newly enforce that invariant (Phase 6).
fn zone_cards_to_lua<'lua>(
    lua: &'lua Lua,
    state: &GameState,
    kind: ZoneKind,
    owner: Option<PlayerId>,
) -> Result<Value<'lua>, ScriptError> {
    let table = lua.create_table().map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    if let Some(zone) = state.zone(kind, owner) {
        for (i, object_id) in zone.contents.iter().enumerate() {
            if let Some(obj) = state.object(*object_id) {
                table
                    .set((i + 1) as i64, card_summary_to_lua(lua, obj)?)
                    .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            }
        }
    }
    Ok(Value::Table(table))
}

/// One object's public-safe summary: id, name, base power/toughness (netted
/// against `+1/+1`/`-1/-1` counters via `PTValue::net_with_counters` — public
/// information, CR 121.3 — but not layer-resolved, see `state_to_lua`'s own
/// doc comment for why), whether it's a creature, and controller. Narrower
/// than the full `EffectiveCharacteristics` shape (no mana cost/colors/full
/// type or keyword set) — named as a follow-up rather than guessed at; this
/// slice's own tests only need power/toughness/creature-ness.
fn card_summary_to_lua<'lua>(lua: &'lua Lua, obj: &GameObject) -> Result<Value<'lua>, ScriptError> {
    let table = lua.create_table().map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table.set("id", obj.object_id.0).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table.set("name", obj.definition.name.clone()).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let power = obj.definition.power.as_ref().and_then(|p| p.net_with_counters(&obj.counters));
    table.set("power", power).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let toughness = obj.definition.toughness.as_ref().and_then(|t| t.net_with_counters(&obj.counters));
    table.set("toughness", toughness).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table
        .set("is_creature", obj.definition.card_types.contains(CardTypeSet::CREATURE))
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table.set("controller", obj.controller.0).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    Ok(Value::Table(table))
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
