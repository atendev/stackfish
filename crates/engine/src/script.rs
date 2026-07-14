use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Function, Lua, RegistryKey, Table, UserData, UserDataMethods, Value};

use crate::types::ability::{
    AbilityEffect, AbilityKind, ConditionPrimitive, ConditionSpec, CreatureTokenSpec, NamedToken,
    RegisteredAbility, TokenSpec,
};
use crate::types::card::{CardDefinition, CardTypeSet, KeywordSet, ScriptReference, SupertypeSet};
use crate::types::counter::CounterKind;
use crate::types::cost::{AdditionalCost, CostSpec, FilterController, FilterSpec};
use crate::types::effect::{
    AffectedFilter, ContinuousEffect, EffectDuration, EffectFilter, EffectPayload, EffectPrimitive,
    Layer,
};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, SequenceNo, Target};
use crate::types::mana::{ColorSet, ManaCost};
use crate::types::zone::ZoneKind;

/// Thin `mlua::UserData` wrapper around `ConditionPrimitive` so
/// `Game.Conditions.Enters`/`Game.Conditions.Always` are real Lua values a
/// condition script can `return` directly — decoded back out via
/// `decode_condition_value`.
#[derive(Clone, Copy)]
struct ConditionTag(ConditionPrimitive);
impl UserData for ConditionTag {}

/// `Game.Conditions.OncePerTurn(tag)`'s return value — wraps whichever
/// `ConditionPrimitive` `tag` (any other `ConditionTag`) already carries, so
/// `decode_condition_value` can tell "wrap this one as `ConditionSpec::
/// PrimitiveOncePerTurn` instead of `Primitive`" apart from a bare
/// `ConditionTag` at decode time (CR 603.2h). A wrapper struct rather than a
/// second field on `ConditionTag` itself, so the two Lua-visible shapes
/// (plain tag vs. once-per-turn-wrapped tag) stay distinguishable by type at
/// the `AnyUserData::borrow` check in `decode_condition_value`.
#[derive(Clone, Copy)]
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
/// the 5 layers with a real query path (`layers::effective_characteristics`)
/// are ever bound onto `Game.Layers` — see `bind_game_globals`.
#[derive(Clone, Copy)]
struct LayerTag(Layer);
impl UserData for LayerTag {}

/// Same idea, for `Game.Effects.*` — e.g. `Game.Effects.ModifyPT(1, 1)` is a
/// Lua function returning one of these, decoded back out via
/// `decode_effect_value`.
#[derive(Clone, Copy)]
struct EffectPrimitiveTag(EffectPrimitive);
impl UserData for EffectPrimitiveTag {}

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
                return Ok(ConditionSpec::Primitive(tag.0));
            }
            if let Ok(tag) = ud.borrow::<OncePerTurnTag>() {
                return Ok(ConditionSpec::PrimitiveOncePerTurn(tag.0));
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
/// `TapPermanents` filter argument — a plain Lua table
/// (`{controller=.., card_types={..}, random=..}`, every key optional), same
/// convention `decode_affected_value` already established for
/// `StaticAbilityBuilder::SetAffected`. `extra` is never settable from Lua
/// this slice (see `FilterSpec::extra`'s own doc comment) — always `None`.
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

    let random: Option<bool> = table.get("random").map_err(|e| ScriptError::LoadError(e.to_string()))?;

    Ok(FilterSpec { card_types, controller, random: random.unwrap_or(false), extra: None })
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
        (Layer::TypeChanging, EffectPrimitive::AddCardType(_))
            | (Layer::ColorChanging, EffectPrimitive::AddColor(_))
            | (Layer::AbilityAdding, EffectPrimitive::AddKeyword(_))
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
/// `:SetTarget` is deliberately not bound this slice, same "nothing built
/// yet to consume it" stance `TriggeredAbilityBuilder`'s own doc comment
/// takes for its own `:SetTarget` — a script-registered activated ability is
/// targetless until that's designed. `CardDefinition.mana_cost` (spells) is
/// untouched by `ManaCost::parse` existing now — it's still sourced as a
/// structured `ManaCost` from the external card database, never parsed from
/// a string.
struct ActivatedAbilityBuilder {
    source: ObjectId,
    cost: CostSpec,
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
            let ability = RegisteredAbility {
                source: this.source,
                kind: AbilityKind::Activated,
                condition: None,
                cost,
                target: None,
                operation,
                effect,
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
                duration: EffectDuration::Permanent,
            };
            this.registry.borrow_mut().pending_static.push(continuous_effect);
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
    /// GameState` to allocate one). Neither `Game.CreateTriggeredAbility`'s
    /// nor `Game.CreateStaticAbility`'s builder ever touches `state` itself
    /// — only this drain does.
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

    /// CR 604.3/208.2a: resolves a characteristic-defining power/toughness
    /// callback. `source_code` is evaluated fresh every call — no
    /// `ScriptHandle`/registry entry, same one-shot-eval convention
    /// `run_card_script` already uses for `ScriptReference::Inline` — and is
    /// expected to evaluate to `function(source, controller) -> integer`,
    /// same argument shape `invoke_operation` already uses. `&self`, not
    /// `&mut self`: `Lua::load`/`.eval()`/`.call()` only need `&Lua`, and
    /// this doesn't touch the script registry at all.
    ///
    /// No `GameState` is passed into the Lua call — nothing in this script
    /// bridge exposes state-querying `Game.*` bindings yet (checked: neither
    /// `invoke_condition` nor `invoke_operation` passes state into Lua
    /// either), so a real "count cards in your graveyard"-style CDA isn't
    /// buildable until that lands (docs/implementation.md's slice backlog,
    /// script API expansion). This only wires the calling convention; a
    /// script is free to compute a constant, or something from `source`/
    /// `controller` alone, until that binding exists.
    pub fn invoke_cda(
        &self,
        source_code: &str,
        source: ObjectId,
        controller: PlayerId,
    ) -> Result<i32, ScriptError> {
        let function: Function =
            self.lua.load(source_code).eval().map_err(|e| ScriptError::LoadError(e.to_string()))?;
        function
            .call::<_, i32>((source.0, controller.0))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// `AffectedFilter::Script`'s real body (CR 613's "affects" predicate,
    /// evaluated per candidate object) — expects `function(object, source,
    /// controller) -> bool`. Same "no `GameState` passed into Lua" limitation
    /// `invoke_cda` already carries (see that method's own doc comment): a
    /// script can only decide from object/source/controller *identity*, not
    /// real board state, until `Game.*` state-query bindings exist (backlog
    /// item 8's hidden-information-safe `ScriptContext`, still open). Called
    /// from `layers::applicable_effects_in_layer`, not from
    /// `ContinuousEffect::applies_to` — that method stays `Primitive`-only so
    /// `types/effect.rs` never needs to depend on `ScriptRuntime`, same
    /// separation `layers::resolve_base_pt` already keeps for CDA.
    pub fn invoke_continuous_affects(
        &self,
        handle: ScriptHandle,
        object: ObjectId,
        source: ObjectId,
        controller: PlayerId,
    ) -> Result<bool, ScriptError> {
        let function = self.function(handle)?;
        function
            .call::<_, bool>((object.0, source.0, controller.0))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))
    }

    /// `EffectPayload::Script`'s real body — the "Lua asks, Rust decides"
    /// table convention `invoke_operation` already established, generalized
    /// to the six `EffectPrimitive` shapes a continuous effect can take
    /// instead of `invoke_operation`'s single `gain_life` key. Expects
    /// `function(source, controller) -> table`, same argument shape
    /// `invoke_cda` uses (no `GameState` passed in — same limitation noted on
    /// `invoke_continuous_affects` above). Checks each recognized key in a
    /// fixed order and returns the first match:
    /// - `set_pt = {power=.., toughness=..}` -> `SetPT`
    /// - `modify_pt = {power=.., toughness=..}` -> `ModifyPT`
    /// - `add_keyword = {"Flying", ...}` -> `AddKeyword`
    /// - `add_card_type = {"Creature", ...}` -> `AddCardType`
    /// - `add_color = "WU"` -> `AddColor`
    /// - `switch_pt = true` -> `SwitchPT`
    ///
    /// An empty/unrecognized table, or a script that errors, resolves to
    /// `None` — same "skip rather than guess" stance every other
    /// unresolvable value in this engine already takes; the caller
    /// (`layers::resolve_primitive`) doesn't distinguish "no effect this
    /// call" from "script errored," same as `invoke_cda`'s own caller.
    pub fn invoke_continuous_effect(
        &self,
        handle: ScriptHandle,
        source: ObjectId,
        controller: PlayerId,
    ) -> Result<Option<EffectPrimitive>, ScriptError> {
        let function = self.function(handle)?;
        let result: Table = function
            .call((source.0, controller.0))
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;

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
    conditions.set("BeginningOfCombat", ConditionTag(ConditionPrimitive::BeginningOfYourCombat))?;
    conditions.set("EachBeginningOfCombat", ConditionTag(ConditionPrimitive::BeginningOfEachCombat))?;
    conditions.set("EndStep", ConditionTag(ConditionPrimitive::BeginningOfYourEndStep))?;
    conditions.set("EachEndStep", ConditionTag(ConditionPrimitive::BeginningOfEachEndStep))?;
    // CR 603.2h: `Game.Conditions.OncePerTurn(Game.Conditions.X)` wraps any
    // other tag above so the resulting ability only ever triggers once each
    // turn - see OncePerTurnTag's own doc comment.
    let once_per_turn = lua.create_function(|_, tag: mlua::AnyUserData| {
        let primitive = tag.borrow::<ConditionTag>()?.0;
        Ok(OncePerTurnTag(primitive))
    })?;
    conditions.set("OncePerTurn", once_per_turn)?;

    let ops = lua.create_table()?;
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
    let draw_cards = lua.create_function(|_, amount: u32| Ok(OperationTag(AbilityEffect::DrawCards(amount))))?;
    ops.set("DrawCards", draw_cards)?;
    let create_token = lua.create_function(|_, (spec_ud, count): (mlua::AnyUserData, Option<u32>)| {
        let spec = spec_ud.borrow::<TokenTag>()?.0.clone();
        Ok(OperationTag(AbilityEffect::CreateToken { spec, count: count.unwrap_or(1) }))
    })?;
    ops.set("CreateToken", create_token)?;
    ops.set("Shuffle", OperationTag(AbilityEffect::Shuffle))?;

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

    // Only the 6 layers with a real query path (layers::effective_characteristics)
    // are exposed - a static ability can't request layer 1/2/3/7d at all
    // (see `Layer`'s own doc comment for what's unbuilt and why).
    let layers = lua.create_table()?;
    layers.set("TypeChanging", LayerTag(Layer::TypeChanging))?;
    layers.set("ColorChanging", LayerTag(Layer::ColorChanging))?;
    layers.set("AbilityAdding", LayerTag(Layer::AbilityAdding))?;
    layers.set("PowerToughnessSetting", LayerTag(Layer::PowerToughnessSetting))?;
    layers.set("PowerToughnessModifying", LayerTag(Layer::PowerToughnessModifying))?;
    layers.set("PowerToughnessSwitching", LayerTag(Layer::PowerToughnessSwitching))?;

    let effects = lua.create_table()?;
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

    let game = lua.create_table()?;
    game.set("Conditions", conditions)?;
    game.set("Ops", ops)?;
    game.set("Costs", costs)?;
    game.set("Layers", layers)?;
    game.set("Effects", effects)?;
    game.set("Tokens", tokens)?;
    game.set("CreateTriggeredAbility", create_triggered_ability)?;
    game.set("CreateStaticAbility", create_static_ability)?;
    game.set("CreateActivatedAbility", create_activated_ability)?;
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
