//! Lua value and table to Rust type decoders, plus the `parse_*_name` string
//! lookups they use.

use mlua::{Function, Lua, Table, Value};

use crate::types::ability::{AbilityEffect, ConditionSpec, CopyExceptions, CounterAmount};
use crate::types::card::{CardTypeSet, KeywordSet, Subtype, SupertypeSet};
use crate::types::cost::{AlternativeCost, CostSpec, FilterController, FilterSpec};
use crate::types::counter::CounterKind;
use crate::types::effect::{AffectedFilter, EffectFilter, EffectPayload, LandwalkQuality, Layer};
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::mana::{Color, ColorSet, ManaUnit};
use crate::types::target::{
    ManaValueBound, ManaValueComparison, ManaValueRequirement, TargetFilter, TargetRequirement,
    TargetSpec,
};
use crate::types::zone::ZoneKind;

use super::tags::{
    AffectedTag, ConditionTag, CostComponent, CostTag, CounterTag, EffectPrimitiveTag, FilterTag,
    ForEachPlayerTag, InterveningIfTag, LayerTag, OncePerTurnTag, OperationTag,
};
use super::types::{ScriptChoiceRequest, ScriptError, ScriptRegistry};

/// Shared by `ScriptRuntime::load_condition` (a freshly eval'd chunk) and
/// `TriggeredAbilityBuilder::SetCondition` (a value passed as a Lua argument).
/// Two shapes either way: a `Game.Conditions.*` tag or a custom function.
/// Anything else, including a bare `nil` from a typo'd `Game.Conditions.Typo`,
/// is a load error (see `RegisteredAbility::condition`).
pub(crate) fn decode_condition_value(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    value: Value,
) -> Result<ConditionSpec, ScriptError> {
    match value {
        Value::UserData(ud) => {
            // Try the plain tag first, then its once-per-turn wrapper; they are distinct `UserData` types (see `OncePerTurnTag`).
            if let Ok(tag) = ud.borrow::<ConditionTag>() {
                return Ok(ConditionSpec::Primitive(tag.0.clone()));
            }
            if let Ok(tag) = ud.borrow::<OncePerTurnTag>() {
                return Ok(ConditionSpec::PrimitiveOncePerTurn(tag.0.clone()));
            }
            if ud.is::<InterveningIfTag>() {
                return Err(ScriptError::LoadError(
                    "that Game.Conditions value checks game state (AtOrBelowLife, ControlsAtLeast, Escaped, \
                     HadNoCounters), so it belongs in :SetInterveningIf, not :SetCondition"
                        .to_string(),
                ));
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
            "condition script returned nil - expected a Game.Conditions.* value or a function; \
             omit :SetCondition for an unconditional ability"
                .to_string(),
        )),
        other => Err(ScriptError::LoadError(format!(
            "condition script must return a Game.Conditions.* value or a function, got {}",
            other.type_name()
        ))),
    }
}

/// A `Game.Ops.*` constructor passed uncalled (`SetOperation(Game.Ops.Mill)`)
/// would register as a custom script function and fail confusingly at
/// resolution. Name the mistake up front instead.
pub(crate) fn reject_uncalled_op(lua: &Lua, function: &Function) -> Result<(), ScriptError> {
    let Ok(ops) = lua
        .globals()
        .get::<_, mlua::Table>("Game")
        .and_then(|g| g.get::<_, mlua::Table>("Ops"))
    else {
        return Ok(());
    };
    for (name, candidate) in ops.pairs::<String, Value>().flatten() {
        if matches!(&candidate, Value::Function(f) if f == function) {
            return Err(ScriptError::LoadError(format!(
                "Game.Ops.{name} builds an operation and must be called: Game.Ops.{name}(...)"
            )));
        }
    }
    Ok(())
}

/// Same two-shapes pattern as `decode_condition_value`, for operations.
/// Shared by `ScriptRuntime::load_operation` and
/// `TriggeredAbilityBuilder::SetOperation`.
pub(crate) fn decode_operation_value(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    value: Value,
) -> Result<(Option<AbilityEffect>, ScriptHandle), ScriptError> {
    match value {
        // `Game.Ops.ForEachPlayer(x)` returns a `ForEachPlayerTag`, a different `UserData` type than `OperationTag` (see its doc), so check it first.
        Value::UserData(ref ud) if ud.is::<ForEachPlayerTag>() => {
            let tag: ForEachPlayerTag = ud
                .borrow::<ForEachPlayerTag>()
                .map_err(|e| ScriptError::LoadError(e.to_string()))?
                .clone();
            match tag {
                ForEachPlayerTag::Function(handle) => {
                    Ok((Some(AbilityEffect::ForEachPlayer(None)), handle))
                }
                ForEachPlayerTag::Static(effect) => Ok((
                    Some(AbilityEffect::ForEachPlayer(Some(Box::new(effect)))),
                    ScriptHandle(0),
                )),
            }
        }
        Value::UserData(ud) => {
            let tag: OperationTag = ud
                .borrow::<OperationTag>()
                .map_err(|e| ScriptError::LoadError(e.to_string()))?
                .clone();
            Ok((Some(tag.0), ScriptHandle(0)))
        }
        Value::Function(function) => {
            reject_uncalled_op(lua, &function)?;
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

/// Like `decode_condition_value` and `decode_operation_value`, for
/// `Game.Costs.*`. There is no `Script` fallback, since neither
/// `AdditionalCost` nor `ManaCost` has one, so a bare custom function is
/// rejected. Shared by `ActivatedAbilityBuilder::AddCost`.
pub(crate) fn decode_cost_component_value(value: Value) -> Result<CostComponent, ScriptError> {
    match value {
        Value::UserData(ud) => {
            let tag: CostTag = ud
                .borrow::<CostTag>()
                .map_err(|e| ScriptError::LoadError(e.to_string()))?
                .clone();
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

/// Decodes one `s.alternative_costs()` return-table entry into an
/// `AlternativeCost::Replacement`, called by
/// `ScriptRuntime::build_alternative_costs`. `costs` accumulates as
/// `ActivatedAbilityBuilder::AddCost` does (one `Game.Costs.*` value per array
/// entry, through `decode_cost_component_value`), and `Game.Costs.Mana` may
/// appear at most once. `from_zone` uses `parse_zone_kind_name`.
/// `exile_on_leaving_stack` and `escape` default to `false`, since most
/// alternatives (anything but Flashback and Escape) need neither.
const ALTERNATIVE_COST_KEYS: &[&str] = &[
    "description",
    "from_zone",
    "exile_on_leaving_stack",
    "escape",
    "costs",
];

pub(crate) fn decode_alternative_cost_table(table: Table) -> Result<AlternativeCost, ScriptError> {
    reject_unknown_keys(
        &table,
        ALTERNATIVE_COST_KEYS,
        &[],
        "alternative_costs() entry",
    )?;
    let description: String = table.get("description").map_err(|e| {
        ScriptError::LoadError(format!(
            "alternative_costs() entry missing required 'description': {e}"
        ))
    })?;
    let from_zone_name: String = table.get("from_zone").map_err(|e| {
        ScriptError::LoadError(format!(
            "alternative_costs() entry missing required 'from_zone': {e}"
        ))
    })?;
    let from_zone = parse_zone_kind_name(&from_zone_name)?;
    let exile_on_leaving_stack: bool = table
        .get::<_, Option<bool>>("exile_on_leaving_stack")
        .map_err(|e| ScriptError::LoadError(e.to_string()))?
        .unwrap_or(false);
    let escape: bool = table
        .get::<_, Option<bool>>("escape")
        .map_err(|e| ScriptError::LoadError(e.to_string()))?
        .unwrap_or(false);

    let costs_table: Table = table.get("costs").map_err(|e| {
        ScriptError::LoadError(format!(
            "alternative_costs() entry missing required 'costs': {e}"
        ))
    })?;
    let mut cost = CostSpec {
        mana: None,
        additional: Vec::new(),
        alternative: Vec::new(),
    };
    for entry in costs_table.sequence_values::<Value>() {
        let value = entry.map_err(|e| ScriptError::LoadError(e.to_string()))?;
        match decode_cost_component_value(value)? {
            CostComponent::Additional(additional) => cost.additional.push(additional),
            CostComponent::Mana(mana) => {
                if cost.mana.is_some() {
                    return Err(ScriptError::LoadError(
                        "alternative_costs() entry's costs contains Game.Costs.Mana(...) more \
                         than once - an alternative cost has only one mana-cost component"
                            .to_string(),
                    ));
                }
                cost.mana = Some(mana);
            }
        }
    }

    Ok(AlternativeCost::Replacement {
        description,
        cost,
        from_zone,
        exile_on_leaving_stack,
        escape,
    })
}

/// Every object filter key a script can write, decoded once. `Game.Filter`,
/// the raw filter tables of `Game.Ops.*` and `Game.Costs.*`, and target tables
/// all read their keys through `decode_object_filter_keys`, then each takes the
/// fields it can match on (`into_filter_spec`, `into_effect_filter`, or
/// `TargetFilter::Object`).
#[derive(Clone, Default)]
pub(crate) struct ObjectFilterKeys {
    pub(crate) card_types: Option<CardTypeSet>,
    pub(crate) excluded_card_types: Option<CardTypeSet>,
    pub(crate) subtypes: Option<Vec<Subtype>>,
    pub(crate) excluded_subtypes: Option<Vec<Subtype>>,
    pub(crate) supertypes: Option<SupertypeSet>,
    pub(crate) excluded_supertypes: Option<SupertypeSet>,
    pub(crate) controller: Option<FilterController>,
    pub(crate) zone: Option<ZoneKind>,
    pub(crate) mana_value: Option<ManaValueRequirement>,
    pub(crate) random: bool,
}

impl ObjectFilterKeys {
    /// `extra` is never settable from Lua (see `FilterSpec::extra`) and is always `None`.
    pub(crate) fn into_filter_spec(self) -> FilterSpec {
        FilterSpec {
            card_types: self.card_types,
            supertypes: self.supertypes,
            subtypes: self.subtypes,
            controller: self.controller,
            random: self.random,
            extra: None,
        }
    }

    pub(crate) fn into_effect_filter(self) -> EffectFilter {
        EffectFilter {
            controller: self.controller,
            card_types: self.card_types,
            subtypes: self.subtypes,
            excluded_subtypes: self.excluded_subtypes,
            supertypes: self.supertypes,
            excluded_supertypes: self.excluded_supertypes,
            self_only: false,
        }
    }
}

fn lua_load_err(error: mlua::Error) -> ScriptError {
    ScriptError::LoadError(error.to_string())
}

/// Rejects a Lua table key that is not in `allowed` or `extra_allowed`.
/// Every load-time table decoder uses this so typos fail at script load time.
/// Game-time callback readers pass `extra_allowed` when a table mixes several
/// concerns (target requirement keys beside filter keys).
pub(crate) fn reject_unknown_keys(
    table: &Table,
    allowed: &[&str],
    extra_allowed: &[&str],
    context: &str,
) -> Result<(), ScriptError> {
    for pair in table.clone().pairs::<String, Value>() {
        let (key, _) = pair.map_err(lua_load_err)?;
        if !allowed.contains(&key.as_str()) && !extra_allowed.contains(&key.as_str()) {
            return Err(ScriptError::LoadError(format!(
                "{context} has unknown key '{key}'"
            )));
        }
    }
    Ok(())
}

fn read_names(table: &Table, key: &str) -> Result<Option<Vec<String>>, ScriptError> {
    let list: Option<Table> = table.get(key).map_err(lua_load_err)?;
    list.map(|list| {
        list.sequence_values::<String>()
            .map(|entry| entry.map_err(lua_load_err))
            .collect()
    })
    .transpose()
}

fn read_card_types(table: &Table, key: &str) -> Result<Option<CardTypeSet>, ScriptError> {
    let Some(names) = read_names(table, key)? else {
        return Ok(None);
    };
    let mut set = CardTypeSet::empty();
    for name in names {
        set |= parse_card_type_name(&name)?;
    }
    Ok(Some(set))
}

fn read_supertypes(table: &Table, key: &str) -> Result<Option<SupertypeSet>, ScriptError> {
    let Some(names) = read_names(table, key)? else {
        return Ok(None);
    };
    let mut set = SupertypeSet::empty();
    for name in names {
        set |= parse_supertype_name(&name)?;
    }
    Ok(Some(set))
}

fn read_subtypes(table: &Table, key: &str) -> Result<Option<Vec<Subtype>>, ScriptError> {
    Ok(read_names(table, key)?.map(|names| names.into_iter().map(Subtype).collect()))
}

fn read_controller(table: &Table) -> Result<Option<FilterController>, ScriptError> {
    let name: Option<String> = table.get("controller").map_err(lua_load_err)?;
    name.map(|name| parse_filter_controller_name(&name))
        .transpose()
}

const OBJECT_FILTER_KEYS: &[&str] = &[
    "card_types",
    "excluded_card_types",
    "subtypes",
    "excluded_subtypes",
    "supertypes",
    "excluded_supertypes",
    "controller",
    "zone",
    "mana_value_equals",
    "mana_value_at_most",
    "mana_value_at_least",
    "random",
];

/// The one reader of a flat object filter table (`{ controller = .., card_types
/// = {..}, supertypes = {..}, subtypes = {..}, excluded_* = {..}, zone = ..,
/// mana_value_* = .., random = .. }`, every key optional). CR 205.4a and
/// 205.4b types, CR 601.2c and 115 negation ("nonland", "nonlegendary"), CR
/// 202.3 mana value.
///
/// `extra_allowed` covers keys the caller consumes before this function runs,
/// such as `type` and the target requirement count keys.
///
/// TODO: keywords, colors, name, other, power/toughness bounds, is_token, tapped, attacking, blocking have no engine backing yet
pub(crate) fn decode_object_filter_keys(
    table: &Table,
    extra_allowed: &[&str],
) -> Result<ObjectFilterKeys, ScriptError> {
    reject_unknown_keys(
        table,
        OBJECT_FILTER_KEYS,
        extra_allowed,
        "Game.Filter/Game.SelectTargets filter",
    )?;
    let zone_name: Option<String> = table.get("zone").map_err(lua_load_err)?;
    let random: Option<bool> = table.get("random").map_err(lua_load_err)?;
    Ok(ObjectFilterKeys {
        card_types: read_card_types(table, "card_types")?,
        excluded_card_types: read_card_types(table, "excluded_card_types")?,
        subtypes: read_subtypes(table, "subtypes")?,
        excluded_subtypes: read_subtypes(table, "excluded_subtypes")?,
        supertypes: read_supertypes(table, "supertypes")?,
        excluded_supertypes: read_supertypes(table, "excluded_supertypes")?,
        controller: read_controller(table)?,
        zone: zone_name
            .map(|name| parse_zone_kind_name(&name))
            .transpose()?,
        mana_value: decode_mana_value_keys(table)?,
        random: random.unwrap_or(false),
    })
}

/// The filter argument of `:AddCost`, `Game.Costs.Sacrifice`, `Discard`,
/// `TapPermanents` and `Game.Ops.Search`: a flat table read by
/// `decode_object_filter_keys`, of which a `FilterSpec` matches on
/// `controller`, `card_types`, `supertypes`, `subtypes` and `random`.
pub(crate) fn decode_filter_table(table: Table) -> Result<FilterSpec, ScriptError> {
    Ok(decode_object_filter_keys(&table, &[])?.into_filter_spec())
}

/// CR 202.3: "target spell with mana value X (or less, or greater)". Three
/// independent optional keys, each a plain number (`ManaValueBound::Fixed`) or
/// the string "X" (`ManaValueBound::X`, the cast or resolution's announced X).
/// If several are set, `equals` wins over `at_most` over `at_least`, with no
/// validation.
fn decode_mana_value_keys(table: &Table) -> Result<Option<ManaValueRequirement>, ScriptError> {
    for (key, comparison) in [
        ("mana_value_equals", ManaValueComparison::Equal),
        ("mana_value_at_most", ManaValueComparison::AtMost),
        ("mana_value_at_least", ManaValueComparison::AtLeast),
    ] {
        let value: Option<Value> = table.get(key).map_err(lua_load_err)?;
        if let Some(value) = value {
            return Ok(Some(ManaValueRequirement {
                comparison,
                bound: decode_mana_value_bound(value)?,
            }));
        }
    }
    Ok(None)
}

/// `CardTypeSet` names by printed card type. Mirrors `parse_card_types` in the
/// scenario crate, duplicated because that crate shares no string-parsing
/// utility. One type at a time (`Game.Effects.AddCardType(name)`), and
/// `decode_affected_value`'s `card_types` list unions them.
pub(crate) fn parse_card_type_name(name: &str) -> Result<CardTypeSet, ScriptError> {
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
        other => {
            return Err(ScriptError::LoadError(format!(
                "unknown card type '{other}'"
            )));
        }
    })
}

/// `KeywordSet` names by CR 702 keyword. Mirrors `parse_keywords` in the
/// scenario crate, duplicated for the same reason as `parse_card_type_name`.
pub(crate) fn parse_keyword_name(name: &str) -> Result<KeywordSet, ScriptError> {
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
        "Hexproof" => KeywordSet::HEXPROOF,
        "Shroud" => KeywordSet::SHROUD,
        "AttacksEachCombat" => KeywordSet::ATTACKS_EACH_COMBAT,
        other => return Err(ScriptError::LoadError(format!("unknown keyword '{other}'"))),
    })
}

/// `CounterKind` names, from CR 122.3's common kinds. Unknown strings now
/// raise a load error; use `Game.Counter.Custom(name)` for flavor counters.
pub(crate) fn parse_counter_kind_name(name: &str) -> Result<CounterKind, ScriptError> {
    Ok(match name {
        "PlusOnePlusOne" => CounterKind::PlusOnePlusOne,
        "MinusOneMinusOne" => CounterKind::MinusOneMinusOne,
        "Loyalty" => CounterKind::Loyalty,
        "Poison" => CounterKind::Poison,
        "Energy" => CounterKind::Energy,
        "Experience" => CounterKind::Experience,
        "Charge" => CounterKind::Charge,
        other => {
            return Err(ScriptError::LoadError(format!(
                "unknown counter kind '{other}' - use Game.Counter.Custom(...) for flavor counters"
            )));
        }
    })
}

/// Decodes a counter argument that is either a known kind string
/// (`"PlusOnePlusOne"`) or a `Game.Counter.*` value (common tag or
/// `Game.Counter.Custom(name)`).
pub(crate) fn decode_counter_arg(value: Value) -> Result<CounterKind, ScriptError> {
    match value {
        Value::String(s) => parse_counter_kind_name(
            s.to_str()
                .map_err(|e| ScriptError::LoadError(e.to_string()))?,
        ),
        Value::UserData(ud) if ud.is::<CounterTag>() => {
            let tag = ud
                .borrow::<CounterTag>()
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
            Ok(match *tag {
                CounterTag::Known(ref kind) => kind.clone(),
                CounterTag::Custom(ref name) => CounterKind::Custom(name.clone()),
            })
        }
        other => Err(ScriptError::LoadError(format!(
            "expected a counter kind string or Game.Counter.* value, got {}",
            other.type_name()
        ))),
    }
}

pub(crate) fn parse_color_letter(letter: &str) -> Result<ColorSet, ScriptError> {
    Ok(match letter {
        "W" => ColorSet::WHITE,
        "U" => ColorSet::BLUE,
        "B" => ColorSet::BLACK,
        "R" => ColorSet::RED,
        "G" => ColorSet::GREEN,
        "C" => ColorSet::empty(),
        other => return Err(ScriptError::LoadError(format!("unknown color '{other}'"))),
    })
}

/// `parse_color_letter`'s letters returning a single `Color` and not a
/// `ColorSet`, for `CostModifierTarget::Colored`, which names exactly one.
/// Colorless is not a color (CR 105.2c), so "C" is rejected here.
pub(crate) fn parse_single_color_letter(letter: &str) -> Result<Color, ScriptError> {
    Ok(match letter {
        "W" => Color::White,
        "U" => Color::Blue,
        "B" => Color::Black,
        "R" => Color::Red,
        "G" => Color::Green,
        other => return Err(ScriptError::LoadError(format!("unknown color '{other}'"))),
    })
}

/// `ReplacementPayloads.ReplaceZoneDestination(name)`'s argument decoder, a
/// string match with a real error like the other `parse_*_name` helpers. The
/// scenario crate has its own `parse_zone_kind`, not shared since it is a
/// separate crate with its own error type.
pub(crate) fn parse_zone_kind_name(name: &str) -> Result<ZoneKind, ScriptError> {
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

pub(crate) fn parse_filter_controller_name(name: &str) -> Result<FilterController, ScriptError> {
    match name {
        "you" => Ok(FilterController::You),
        "opponent" => Ok(FilterController::Opponent),
        "any" => Ok(FilterController::Any),
        other => Err(ScriptError::LoadError(format!(
            "unknown controller filter '{other}' - expected \"you\", \"opponent\" or \"any\""
        ))),
    }
}

pub(crate) fn parse_supertype_name(name: &str) -> Result<SupertypeSet, ScriptError> {
    Ok(match name {
        "Basic" => SupertypeSet::BASIC,
        "Legendary" => SupertypeSet::LEGENDARY,
        "Snow" => SupertypeSet::SNOW,
        "World" => SupertypeSet::WORLD,
        other => {
            return Err(ScriptError::LoadError(format!(
                "unknown supertype '{other}'"
            )));
        }
    })
}

/// A string (`"Soldier"`) or a Lua list (`{"Elf", "Warrior"}`) of names, for
/// the `subtypes` argument of `Game.Tokens.Creature` and
/// `Game.Effects.SetCardTypes`. A "set" must express the whole resulting type
/// line in one call ("becomes an artifact creature" sets two types), because
/// two `SetCardTypes` registrations wouldn't compose: the later wipes the
/// earlier (see the layer 4 fold in `layers::effective_characteristics`).
/// `what` names the argument in the error.
pub(crate) fn decode_name_list(val: Value, what: &str) -> Result<Vec<String>, ScriptError> {
    match val {
        Value::String(s) => Ok(vec![
            s.to_str()
                .map_err(|e| ScriptError::LoadError(e.to_string()))?
                .to_string(),
        ]),
        Value::Table(t) => {
            let mut out = Vec::new();
            for pair in t.sequence_values::<String>() {
                out.push(pair.map_err(|e| ScriptError::LoadError(e.to_string()))?);
            }
            Ok(out)
        }
        _ => Err(ScriptError::LoadError(format!(
            "{what} must be a string or table"
        ))),
    }
}

/// Parses a WUBRG letter string like `"WU"` or `""` into a `ColorSet`.
pub(crate) fn parse_color_string(colors: &str) -> Result<ColorSet, ScriptError> {
    let mut out = ColorSet::empty();
    for ch in colors.chars() {
        out |= parse_color_letter(&ch.to_string())?;
    }
    Ok(out)
}

/// `Game.Ops.AddMana`'s symbol vocabulary: WUBRG plus `"C"` for colorless
/// (`ManaUnit::Colorless`, the literal-unit shape of CR 605).
/// TODO: snow letters for `SnowColored` and `SnowColorless`; snow-producing lands are rare enough that no unvalidated convention was invented
pub(crate) fn parse_mana_unit_letter(letter: &str) -> Result<ManaUnit, ScriptError> {
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
            )));
        }
    })
}

const CREATURE_TOKEN_OPT_KEYS: &[&str] = &[
    "keywords",
    "name",
    "additional_types",
    "additional_supertypes",
];

/// Decodes the optional `opts` table for `Game.Tokens.Creature`. Returns
/// `(keywords, name_override, additional_types, additional_supertypes)`.
pub(crate) fn decode_creature_token_opts(
    opts: Option<mlua::Table>,
) -> Result<(KeywordSet, Option<String>, CardTypeSet, SupertypeSet), ScriptError> {
    let Some(opts) = opts else {
        return Ok((
            KeywordSet::empty(),
            None,
            CardTypeSet::empty(),
            SupertypeSet::empty(),
        ));
    };
    reject_unknown_keys(
        &opts,
        CREATURE_TOKEN_OPT_KEYS,
        &[],
        "Game.Tokens.Creature opts",
    )?;
    let mut keywords = KeywordSet::empty();
    if let Some(kws) = opts
        .get::<_, Option<mlua::Table>>("keywords")
        .map_err(lua_load_err)?
    {
        for pair in kws.sequence_values::<String>() {
            keywords |=
                parse_keyword_name(&pair.map_err(|e| ScriptError::LoadError(e.to_string()))?)?;
        }
    }
    let name: Option<String> = opts.get("name").map_err(lua_load_err)?;
    let mut additional_types = CardTypeSet::empty();
    if let Some(types) = opts
        .get::<_, Option<mlua::Table>>("additional_types")
        .map_err(lua_load_err)?
    {
        for pair in types.sequence_values::<String>() {
            additional_types |=
                parse_card_type_name(&pair.map_err(|e| ScriptError::LoadError(e.to_string()))?)?;
        }
    }
    let mut additional_supertypes = SupertypeSet::empty();
    if let Some(supers) = opts
        .get::<_, Option<mlua::Table>>("additional_supertypes")
        .map_err(lua_load_err)?
    {
        for pair in supers.sequence_values::<String>() {
            additional_supertypes |=
                parse_supertype_name(&pair.map_err(|e| ScriptError::LoadError(e.to_string()))?)?;
        }
    }
    Ok((keywords, name, additional_types, additional_supertypes))
}

const LANDWALK_QUALITY_KEYS: &[&str] = &[
    "subtypes",
    "card_types",
    "supertypes",
    "excluded_supertypes",
];

/// CR 702.14a's landwalk combination forms: `Game.Effects.AddLandwalkQuality(opts)`'s
/// `opts` table. It uses the `{subtypes, card_types, supertypes,
/// excluded_supertypes}` keys of the `"object"` branch of
/// `decode_filter_table`, without `controller` and `zone`, since a landwalk
/// quality is always checked against the defending player's battlefield.
/// Every field is optional, and an empty table decodes to
/// `LandwalkQuality::default()`, which matches any land and is harmless to
/// allow.
pub(crate) fn decode_landwalk_quality(opts: mlua::Table) -> Result<LandwalkQuality, ScriptError> {
    reject_unknown_keys(
        &opts,
        LANDWALK_QUALITY_KEYS,
        &[],
        "Game.Effects.AddLandwalkQuality opts",
    )?;
    let mut subtypes = Vec::new();
    if let Some(t) = opts
        .get::<_, Option<mlua::Table>>("subtypes")
        .map_err(lua_load_err)?
    {
        for pair in t.sequence_values::<String>() {
            subtypes.push(Subtype(
                pair.map_err(|e| ScriptError::LoadError(e.to_string()))?,
            ));
        }
    }
    let card_types = match opts
        .get::<_, Option<mlua::Table>>("card_types")
        .map_err(lua_load_err)?
    {
        Some(t) => {
            let mut set = CardTypeSet::empty();
            for pair in t.sequence_values::<String>() {
                set |= parse_card_type_name(
                    &pair.map_err(|e| ScriptError::LoadError(e.to_string()))?,
                )?;
            }
            Some(set)
        }
        None => None,
    };
    let supertypes = match opts
        .get::<_, Option<mlua::Table>>("supertypes")
        .map_err(lua_load_err)?
    {
        Some(t) => {
            let mut set = SupertypeSet::empty();
            for pair in t.sequence_values::<String>() {
                set |= parse_supertype_name(
                    &pair.map_err(|e| ScriptError::LoadError(e.to_string()))?,
                )?;
            }
            Some(set)
        }
        None => None,
    };
    let excluded_supertypes = match opts
        .get::<_, Option<mlua::Table>>("excluded_supertypes")
        .map_err(lua_load_err)?
    {
        Some(t) => {
            let mut set = SupertypeSet::empty();
            for pair in t.sequence_values::<String>() {
                set |= parse_supertype_name(
                    &pair.map_err(|e| ScriptError::LoadError(e.to_string()))?,
                )?;
            }
            Some(set)
        }
        None => None,
    };
    Ok(LandwalkQuality {
        subtypes,
        card_types,
        supertypes,
        excluded_supertypes,
    })
}

/// CR 707.9 copy-effect exceptions, shared by `Game.Ops.BecomeCopy(opts?)` and
const COPY_EXCEPTION_KEYS: &[&str] = &[
    "power",
    "toughness",
    "keywords",
    "retain_name",
    "retain_color",
    "grant_recurring_copy_trigger",
    "counter",
    "amount",
];

/// `Game.Tokens.CopyOf(objectId, opts?)`. `opts` holds
/// `{ power = N, toughness = N }` (CR 707.9b, both required together, since a
/// lone P/T override means nothing), `{ keywords = { "Flying", ... } }`
/// (CR 707.9a) and `{ retain_name = true }` or `{ retain_color = true }`
/// (CR 707.9c, 707.9d: keep the copy's own name or color). Retaining is a
/// no-op for `CopyOf`; see `CopyExceptions::apply`. `None` is a bare copy,
/// `CopyExceptions::default()`.
pub(crate) fn decode_copy_exceptions(
    opts: Option<mlua::Table>,
) -> Result<CopyExceptions, ScriptError> {
    let Some(opts) = opts else {
        return Ok(CopyExceptions::default());
    };
    reject_unknown_keys(
        &opts,
        COPY_EXCEPTION_KEYS,
        &[],
        "Game.Ops.BecomeCopy/Game.Tokens.CopyOf opts",
    )?;
    let power: Option<i32> = opts.get("power").map_err(lua_load_err)?;
    let toughness: Option<i32> = opts.get("toughness").map_err(lua_load_err)?;
    let set_power_toughness = match (power, toughness) {
        (Some(p), Some(t)) => Some((p, t)),
        (None, None) => None,
        _ => {
            return Err(ScriptError::LoadError(
                "CopyExceptions power/toughness must be set together, not just one".to_string(),
            ));
        }
    };
    let mut add_keywords = KeywordSet::empty();
    if let Some(kws) = opts
        .get::<_, Option<mlua::Table>>("keywords")
        .map_err(lua_load_err)?
    {
        for pair in kws.sequence_values::<String>() {
            add_keywords |=
                parse_keyword_name(&pair.map_err(|e| ScriptError::LoadError(e.to_string()))?)?;
        }
    }
    let retain_name: bool = opts
        .get::<_, Option<bool>>("retain_name")
        .map_err(lua_load_err)?
        .unwrap_or(false);
    let retain_color: bool = opts
        .get::<_, Option<bool>>("retain_color")
        .map_err(lua_load_err)?
        .unwrap_or(false);
    // CR 707.9a: Vesuvan Doppelganger's "except it has [linked upkeep ability]" (see `CopyExceptions::grant_recurring_copy_trigger`). A `CopyOf` token could carry it, though no real card needs that.
    let grant_recurring_copy_trigger: bool = opts
        .get::<_, Option<bool>>("grant_recurring_copy_trigger")
        .map_err(lua_load_err)?
        .unwrap_or(false);
    // CR 707.9e: Spark Double's "enters with an additional +1/+1 counter" (see `CopyExceptions::enters_with_additional_counters`): `{ counter = "PlusOnePlusOne", amount = 1 }`, both required together as power and toughness are. `amount` may be the string `"X"` (Altered Ego) for `CounterAmount::X`. It is read as a raw `mlua::Value` and matched by hand, because `.get::<u32>()` would silently fail to match a string.
    let counter: Option<String> = opts.get("counter").map_err(lua_load_err)?;
    let amount_value: Option<mlua::Value> = opts.get("amount").map_err(lua_load_err)?;
    let amount = match amount_value {
        Some(mlua::Value::Integer(n)) if n >= 0 => Some(CounterAmount::Fixed(n as u32)),
        Some(mlua::Value::Number(n)) if n >= 0.0 => Some(CounterAmount::Fixed(n as u32)),
        Some(mlua::Value::String(s)) if s.to_str().ok() == Some("X") => Some(CounterAmount::X),
        Some(mlua::Value::Nil) | None => None,
        _ => {
            return Err(ScriptError::LoadError(
                "CopyExceptions amount must be a non-negative number or \"X\"".to_string(),
            ));
        }
    };
    let enters_with_additional_counters = match (counter, amount) {
        (Some(kind), Some(amount)) => Some((parse_counter_kind_name(&kind)?, amount)),
        (None, None) => None,
        _ => {
            return Err(ScriptError::LoadError(
                "CopyExceptions counter/amount must be set together, not just one".to_string(),
            ));
        }
    };
    Ok(CopyExceptions {
        set_power_toughness,
        add_keywords,
        retain_name,
        retain_color,
        grant_recurring_copy_trigger,
        enters_with_additional_counters,
    })
}

/// `:SetLayer`'s decoder. Only a `Game.Layers.*` tag is accepted, with no
/// custom-function fallback, since there is no `Layer::Script`.
pub(crate) fn decode_layer_value(value: Value) -> Result<Layer, ScriptError> {
    match value {
        Value::UserData(ud) => {
            let tag: LayerTag = *ud
                .borrow::<LayerTag>()
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
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

/// `StaticAbilityBuilder`'s `:SetEffect` decoder, in the two-shapes pattern of
/// `decode_condition_value`: a `Game.Effects.*` tag decodes to
/// `EffectPayload::Primitive` with no further evaluation, and a custom
/// function is registered and returned as `EffectPayload::Script`, invoked
/// later through `ScriptRuntime::invoke_continuous_effect`. It needs
/// `&mut ScriptRegistry` to register the function.
pub(crate) fn decode_effect_value(
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
            "SetEffect() called with nil - expected a Game.Effects.* value or a function"
                .to_string(),
        )),
        other => Err(ScriptError::LoadError(format!(
            "SetEffect() expects a Game.Effects.* value or a function, got {}",
            other.type_name()
        ))),
    }
}

fn self_only_filter() -> EffectFilter {
    EffectFilter {
        controller: None,
        card_types: None,
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
        self_only: true,
    }
}

/// The raw-table form of `Game.Filter` for `Game.Ops.DestroyAll` and the other
/// mass operations, decoded to an `EffectFilter` through
/// `decode_object_filter_keys`. It has no `self_only`, `enchanted_only` or
/// function-predicate case (CR 701.8's "destroy all X" is never "enchanted
/// X").
pub(crate) fn decode_effect_filter_table(table: &Table) -> Result<EffectFilter, ScriptError> {
    Ok(decode_object_filter_keys(table, &[])?.into_effect_filter())
}

const RAW_AFFECTED_TABLE_HINT: &str = "wrap object filter keys in Game.Filter({...}), and write the flags as \
    Game.Affected.Self, Game.Affected.Enchanted or Game.Affected.EnchantedPlayerControls";

/// `StaticAbilityBuilder::SetAffected`'s decoder, in the two-shapes pattern of
/// `decode_effect_value`. A `Game.Filter({...})` or `Game.Affected.Self`
/// decodes to `AffectedFilter::Primitive`, and omitting `:SetAffected` means no
/// filter, so the effect applies to everything. CR 303.4m's
/// `Game.Affected.Enchanted` and `Game.Affected.EnchantedPlayerControls` (Curse
/// of Vitality) decode to their own variants. A custom function becomes
/// `AffectedFilter::Script`, invoked later through
/// `ScriptRuntime::invoke_continuous_affects`. A raw table or a player tag
/// (`Game.Affected.You`) is a load error.
pub(crate) fn decode_affected_value(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    value: Value,
) -> Result<AffectedFilter, ScriptError> {
    match value {
        Value::UserData(ud) if ud.is::<AffectedTag>() => {
            let tag = *ud
                .borrow::<AffectedTag>()
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
            match tag {
                AffectedTag::SelfOnly => Ok(AffectedFilter::Primitive(self_only_filter())),
                AffectedTag::Enchanted => Ok(AffectedFilter::EnchantedOnly),
                AffectedTag::EnchantedPlayerControls => Ok(AffectedFilter::EnchantedPlayerControls),
                other => Err(ScriptError::LoadError(format!(
                    "{} selects players for a player modifier, not objects for a static ability's SetAffected",
                    other.describe()
                ))),
            }
        }
        Value::UserData(ud) if ud.is::<FilterTag>() => {
            let tag = ud
                .borrow::<FilterTag>()
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
            Ok(AffectedFilter::Primitive(
                tag.0.clone().into_effect_filter(),
            ))
        }
        Value::Function(function) => {
            let key = lua
                .create_registry_value(function)
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let handle = registry.next_script_handle();
            registry.scripts.insert(handle, key);
            Ok(AffectedFilter::Script(handle))
        }
        Value::Table(_) => Err(ScriptError::LoadError(format!(
            "SetAffected no longer takes a raw table - {RAW_AFFECTED_TABLE_HINT}"
        ))),
        other => Err(ScriptError::LoadError(format!(
            "SetAffected expects a Game.Affected.* value, Game.Filter({{...}}) or a function, got {}",
            other.type_name()
        ))),
    }
}

/// `ReplacementEffectBuilder::SetAffected`'s decoder. `EventFilter` has no
/// scripted-affected shape (see `ReplacementSpec`), so a function is an error,
/// and so is every tag but `Game.Affected.Self` and `Game.Filter({...})`.
pub(crate) fn decode_replacement_affected(value: Value) -> Result<EffectFilter, ScriptError> {
    match value {
        Value::UserData(ud) if ud.is::<AffectedTag>() => {
            let tag = *ud
                .borrow::<AffectedTag>()
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
            match tag {
                AffectedTag::SelfOnly => Ok(self_only_filter()),
                other => Err(ScriptError::LoadError(format!(
                    "{} isn't valid for a replacement effect's SetAffected - use Game.Affected.Self or Game.Filter({{...}})",
                    other.describe()
                ))),
            }
        }
        Value::UserData(ud) if ud.is::<FilterTag>() => {
            let tag = ud
                .borrow::<FilterTag>()
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
            Ok(tag.0.clone().into_effect_filter())
        }
        Value::Table(_) => Err(ScriptError::LoadError(format!(
            "SetAffected no longer takes a raw table - {RAW_AFFECTED_TABLE_HINT}"
        ))),
        other => Err(ScriptError::LoadError(format!(
            "a replacement effect's SetAffected expects Game.Affected.Self or Game.Filter({{...}}), got {}",
            other.type_name()
        ))),
    }
}

/// Decodes `Game.SelectTargets`, `Game.MayChoose` and `Game.ChooseN`'s yielded
/// request table: `{ op = "select_target", filter = ... }` or
/// `{ op = "may_choose" }`. An unknown op is a real error, as in
/// `decode_condition_value`. `lua` and `registry` are needed because `filter`
/// can be a scripted predicate (a bare function, or
/// `{type="player", filter=fn}`) like `:SetTarget`'s; see
/// `decode_target_filter_value`. Its function is registered like any other
/// custom-function value.
pub(crate) fn decode_script_choice_request(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    table: Table,
) -> Result<ScriptChoiceRequest, ScriptError> {
    let op: String = table
        .get("op")
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    match op.as_str() {
        "select_target" => {
            reject_unknown_keys(
                &table,
                &["op", "filter", "exclude_ids"],
                &[],
                "Game.SelectTargets/Game.ChooseN select_target request",
            )
            .map_err(|e| ScriptError::RuntimeError(format!("{e:?}")))?;
            let filter_value: Value = table
                .get("filter")
                .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            // `Game.ChooseN`'s Lua loop grows this by one id per iteration so a repeat ask can't return an object already picked this batch (see `ScriptChoiceRequest::SelectTarget`). Absent for a bare `Game.SelectTargets` call.
            let exclude_ids: Option<Vec<i64>> = table
                .get("exclude_ids")
                .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            let exclude_ids = exclude_ids
                .unwrap_or_default()
                .into_iter()
                .map(|id| ObjectId(id as u64))
                .collect();
            Ok(ScriptChoiceRequest::SelectTarget {
                filter: decode_target_filter_value(lua, registry, filter_value, &["type"])?,
                exclude_ids,
            })
        }
        "may_choose" => {
            reject_unknown_keys(&table, &["op"], &[], "Game.MayChoose request")
                .map_err(|e| ScriptError::RuntimeError(format!("{e:?}")))?;
            Ok(ScriptChoiceRequest::MayChoose)
        }
        other => Err(ScriptError::RuntimeError(format!(
            "unknown script choice request op '{other}'"
        ))),
    }
}

/// Decodes one `{kind, id}` picked-target table, the shape
/// `ScriptRuntime::resume_operation` encodes as the answer to a
/// `Game.SelectTargets` or `Game.ChooseN` yield (`kind` is `"object"`,
/// `"player"` or `"ability"`). `Game.Ops.CastSpell`'s `opts.targets` decode
/// (`handle_operation_result`) uses it to turn a script's already-picked
/// targets back into a `Target`.
pub(crate) fn decode_target_pick(table: &Table) -> Result<Target, ScriptError> {
    reject_unknown_keys(table, &["kind", "id"], &[], "target pick")
        .map_err(|e| ScriptError::RuntimeError(format!("{e:?}")))?;
    let kind: String = table
        .get("kind")
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let id: u64 = table
        .get("id")
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    match kind.as_str() {
        "object" => Ok(Target::Object(ObjectId(id))),
        "player" => Ok(Target::Player(PlayerId(id as u32))),
        "ability" => Ok(Target::Ability(ObjectId(id))),
        other => Err(ScriptError::RuntimeError(format!(
            "unknown target pick kind '{other}'"
        ))),
    }
}

/// `Game.SelectTargets`'s filter argument: the flat object keys of
/// `decode_object_filter_keys`, plus a `type`
/// discriminator (`"object"` default, `"player"`, `"any"`, `"ability"`)
/// matching the scenario crate's TOML `target_requirement.filter`
/// (`build_target_requirement`). `type` defaults to `"object"` so scripts that
/// omit it still work. `zone` is a zone name parsed by `parse_zone_kind_name`,
/// as in the TOML fixtures (`Game.ChooseN({ zone = "Hand", ... }, n)` for
/// CR 701.9 discard), and `None` still means the battlefield.
pub(crate) fn decode_target_filter_table(
    table: Table,
    extra_allowed: &[&str],
) -> Result<TargetFilter, ScriptError> {
    let kind: Option<String> = table
        .get("type")
        .map_err(|e| ScriptError::LoadError(e.to_string()))?;
    match kind.as_deref().unwrap_or("object") {
        "player" => {
            let controller_name: Option<String> = table
                .get("controller")
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let controller = controller_name
                .map(|name| parse_filter_controller_name(&name))
                .transpose()?;
            Ok(TargetFilter::Player { controller })
        }
        "any" => Ok(TargetFilter::AnyTarget),
        // CR 113.9, 701.6a: "counter target activated or triggered ability" (Stifle, Trickbind); see `TargetFilter::Ability`.
        "ability" => Ok(TargetFilter::Ability),
        // CR 702.21a (Ward): the spell or ability that triggered this one.
        "triggering" => Ok(TargetFilter::TriggeringStackObject),
        "object" => {
            let keys = decode_object_filter_keys(&table, extra_allowed)?;
            Ok(TargetFilter::Object {
                card_types: keys.card_types,
                excluded_card_types: keys.excluded_card_types,
                subtypes: keys.subtypes,
                excluded_subtypes: keys.excluded_subtypes,
                supertypes: keys.supertypes,
                excluded_supertypes: keys.excluded_supertypes,
                controller: keys.controller,
                zone: keys.zone,
                mana_value: keys.mana_value,
            })
        }
        other => Err(ScriptError::LoadError(format!(
            "unknown Game.SelectTargets filter type '{other}' - expected object/player/any/ability"
        ))),
    }
}

/// Shared value decode of `mana_value_equals`, `mana_value_at_most` and
/// `mana_value_at_least`: a nonnegative integer (`ManaValueBound::Fixed`) or
/// the string `"X"` (`ManaValueBound::X`). Anything else is a load-time error.
fn decode_mana_value_bound(value: Value) -> Result<ManaValueBound, ScriptError> {
    match value {
        Value::Integer(n) if n >= 0 => Ok(ManaValueBound::Fixed(n as u32)),
        Value::Number(n) if n >= 0.0 => Ok(ManaValueBound::Fixed(n as u32)),
        Value::String(s) if s.to_str().is_ok_and(|s| s == "X") => Ok(ManaValueBound::X),
        other => Err(ScriptError::LoadError(format!(
            "mana_value bound must be a nonnegative number or the string \"X\", got {other:?}"
        ))),
    }
}

/// `ScriptRuntime::build_target_requirement`'s return-table decoder, adding
/// `count`, `up_to`, `at_least` and `exclude_source` to a bare `TargetFilter`,
/// with the field names and defaults of the scenario crate's
/// `TargetRequirementSpec`.
///
/// The same table goes to `decode_target_filter_value`, so the filter keys sit
/// flat beside the count keys and a `filter` function gives a scripted
/// predicate ("Enchant creature with power 4 or greater"). See
/// `TargetFilter::Script`.
pub(crate) fn decode_target_requirement_table(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    table: Table,
) -> Result<TargetRequirement, ScriptError> {
    let count: u32 = table.get("count").map_err(|e| {
        ScriptError::LoadError(format!(
            "target_requirement() table missing required 'count': {e}"
        ))
    })?;
    let up_to: bool = table
        .get::<_, Option<bool>>("up_to")
        .map_err(|e| ScriptError::LoadError(e.to_string()))?
        .unwrap_or(false);
    let at_least: bool = table
        .get::<_, Option<bool>>("at_least")
        .map_err(|e| ScriptError::LoadError(e.to_string()))?
        .unwrap_or(false);
    let exclude_source: bool = table
        .get::<_, Option<bool>>("exclude_source")
        .map_err(|e| ScriptError::LoadError(e.to_string()))?
        .unwrap_or(false);
    let filter = decode_target_filter_value(
        lua,
        registry,
        Value::Table(table),
        &["type", "count", "up_to", "at_least", "exclude_source"],
    )?;
    Ok(TargetRequirement {
        filter,
        count,
        up_to,
        at_least,
        exclude_source,
    })
}

/// `ScriptRuntime::build_target_requirement`'s whole-return decoder: one
/// requirement table (a table with no array part, decoded by
/// `decode_target_requirement_table` into `TargetSpec::Primitive`) or a
/// sequence of them, one per "target" word in card order
/// (`TargetSpec::Multiple`, "target creature you control fights target
/// creature you don't control"). A one-entry sequence is a single word and
/// decodes to `Primitive`.
///
/// Rejected with an error: an empty table, a table mixing array entries with
/// other keys (such as `count`), a sequence with gaps, and a sequence entry
/// that is not a table.
pub(crate) fn decode_target_spec_table(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    table: Table,
) -> Result<TargetSpec, ScriptError> {
    let array_len = table.raw_len();
    let mut key_count = 0usize;
    for pair in table.clone().pairs::<Value, Value>() {
        pair.map_err(|e| ScriptError::LoadError(e.to_string()))?;
        key_count += 1;
    }
    if key_count == 0 {
        return Err(ScriptError::LoadError(
            "target_requirement() returned an empty table - return one requirement table, or a \
             non-empty array of requirement tables"
                .to_string(),
        ));
    }
    if array_len == 0 {
        return Ok(TargetSpec::Primitive(decode_target_requirement_table(
            lua, registry, table,
        )?));
    }
    if key_count != array_len {
        return Err(ScriptError::LoadError(
            "target_requirement() returned a table that mixes array entries with other keys (or \
             has gaps) - return either one requirement table or a plain array of requirement \
             tables"
                .to_string(),
        ));
    }
    let mut requirements = Vec::with_capacity(array_len);
    for index in 1..=array_len {
        let entry: Value = table
            .get(index)
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let Value::Table(entry) = entry else {
            return Err(ScriptError::LoadError(format!(
                "target_requirement() array entry {index} must be a requirement table, got {}",
                entry.type_name()
            )));
        };
        requirements.push(decode_target_requirement_table(lua, registry, entry)?);
    }
    if requirements.len() == 1 {
        Ok(TargetSpec::Primitive(requirements.remove(0)))
    } else {
        Ok(TargetSpec::Multiple(requirements))
    }
}

/// `:SetTarget`'s argument for activated and triggered abilities. A plain
/// filter table or function is one target word (count 1). An array of them is
/// one word per entry in order (CR 601.2c, "target creature and target
/// player"), a one-entry array being the same single word. Each entry decodes
/// like a bare `:SetTarget` argument, so a function or a player-script table
/// works in an array too.
pub(crate) fn decode_target_spec_value(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    value: Value,
) -> Result<TargetSpec, ScriptError> {
    let single = |filter| TargetRequirement {
        filter,
        count: 1,
        up_to: false,
        at_least: false,
        exclude_source: false,
    };
    if let Value::Table(ref table) = value {
        let array_len = table.raw_len();
        if array_len > 0 {
            let mut key_count = 0usize;
            for pair in table.clone().pairs::<Value, Value>() {
                pair.map_err(|e| ScriptError::LoadError(e.to_string()))?;
                key_count += 1;
            }
            if key_count != array_len {
                return Err(ScriptError::LoadError(
                    "SetTarget got a table that mixes array entries with other keys (or has gaps) - pass one \
                     filter table, or a plain array of filter tables"
                        .to_string(),
                ));
            }
            let mut requirements = Vec::with_capacity(array_len);
            for index in 1..=array_len {
                let entry: Value = table
                    .get(index)
                    .map_err(|e| ScriptError::LoadError(e.to_string()))?;
                requirements.push(single(decode_target_filter_value(
                    lua,
                    registry,
                    entry,
                    &["type"],
                )?));
            }
            return Ok(if requirements.len() == 1 {
                TargetSpec::Primitive(requirements.remove(0))
            } else {
                TargetSpec::Multiple(requirements)
            });
        }
    }
    Ok(TargetSpec::Primitive(single(decode_target_filter_value(
        lua,
        registry,
        value,
        &["type"],
    )?)))
}

/// A `Game.Ops.ChainEach` whose per-entry word counts don't add up to the
/// words the ability or spell declares can't resolve, so the load or the
/// registration fails. Plain `Chain`, other effects and `words: None` pass.
/// A `Primitive` spec is one word, a `Multiple` one per requirement, no spec
/// none.
pub(crate) fn check_chain_words(
    effect: Option<&AbilityEffect>,
    target: Option<&TargetSpec>,
) -> Result<(), ScriptError> {
    let Some(AbilityEffect::Chain {
        words: Some(words), ..
    }) = effect
    else {
        return Ok(());
    };
    let declared = match target {
        None => 0,
        Some(TargetSpec::Primitive(_)) => 1,
        Some(TargetSpec::Multiple(requirements)) => requirements.len(),
    };
    let owned: usize = words.iter().sum();
    if owned == declared {
        Ok(())
    } else {
        Err(ScriptError::LoadError(format!(
            "Game.Ops.ChainEach entries own {owned} target word(s) but the ability declares {declared}"
        )))
    }
}

/// `ActivatedAbilityBuilder::SetTarget`'s argument decoder, taking a `Value`
/// and not `decode_target_filter_table`'s bare `Table`. A bare function is
/// registered and returned as `TargetFilter::Script` (invoked later through
/// `ScriptRuntime::invoke_target_legal`), for a legality shape the flat table
/// can't express ("power 4 or greater"). A table's `filter` key takes a
/// function the same way (`TargetFilter::PlayerScript` with `type = "player"`),
/// and a nested filter table is an error, since the filter keys sit beside
/// `count` and `type`.
pub(crate) fn decode_target_filter_value(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    value: Value,
    extra_allowed: &[&str],
) -> Result<TargetFilter, ScriptError> {
    let mut register = |function: Function| -> Result<ScriptHandle, ScriptError> {
        let key = lua
            .create_registry_value(function)
            .map_err(|e| ScriptError::LoadError(e.to_string()))?;
        let handle = registry.next_script_handle();
        registry.scripts.insert(handle, key);
        Ok(handle)
    };
    match value {
        Value::Table(table) => {
            let predicate: Value = table
                .get("filter")
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
            match predicate {
                Value::Nil => decode_target_filter_table(table, extra_allowed),
                // CR 601.2c, 602.2b: `{ type = "player", filter = function(player, source, controller, state) -> bool }` is the Lua entry point of `TargetFilter::PlayerScript`.
                Value::Function(function) => {
                    let kind: Option<String> =
                        table.get("type").map_err(|e| ScriptError::LoadError(e.to_string()))?;
                    match kind.as_deref().unwrap_or("object") {
                        "player" => Ok(TargetFilter::PlayerScript(register(function)?)),
                        "object" => Ok(TargetFilter::Script(register(function)?)),
                        other => Err(ScriptError::LoadError(format!(
                            "a target `filter` function pairs with type \"object\" or \"player\", got '{other}'"
                        ))),
                    }
                }
                Value::Table(_) => Err(ScriptError::LoadError(
                    "a target table's `filter` key takes a function; put the filter keys beside `count` and `type` \
                     and drop the nested `filter` table"
                        .to_string(),
                )),
                other => Err(ScriptError::LoadError(format!(
                    "a target table's `filter` key must be a function, got {}",
                    other.type_name()
                ))),
            }
        }
        Value::Function(function) => Ok(TargetFilter::Script(register(function)?)),
        other => Err(ScriptError::LoadError(format!(
            "SetTarget expected a filter table or a function, got {}",
            other.type_name()
        ))),
    }
}

#[cfg(test)]
mod chain_word_tests {
    use super::*;
    use crate::types::target::{TargetFilter, TargetRequirement};

    fn word() -> TargetRequirement {
        TargetRequirement {
            filter: TargetFilter::Player { controller: None },
            count: 1,
            up_to: false,
            at_least: false,
            exclude_source: false,
        }
    }

    fn each(words: Vec<usize>) -> AbilityEffect {
        AbilityEffect::Chain {
            entries: vec![AbilityEffect::DrawCards(1); words.len()],
            words: Some(words),
        }
    }

    #[test]
    fn counts_that_match_the_declared_words_pass() {
        assert!(
            check_chain_words(
                Some(&each(vec![0, 1])),
                Some(&TargetSpec::Primitive(word()))
            )
            .is_ok()
        );
        assert!(
            check_chain_words(
                Some(&each(vec![1, 1])),
                Some(&TargetSpec::Multiple(vec![word(), word()]))
            )
            .is_ok()
        );
        assert!(check_chain_words(Some(&each(vec![0, 0])), None).is_ok());
    }

    #[test]
    fn counts_that_do_not_match_fail_and_name_both_numbers() {
        let error = check_chain_words(
            Some(&each(vec![1, 1])),
            Some(&TargetSpec::Primitive(word())),
        )
        .expect_err("2 vs 1");
        assert!(
            format!("{error:?}").contains("own 2") && format!("{error:?}").contains("declares 1"),
            "{error:?}"
        );
        assert!(check_chain_words(Some(&each(vec![0, 1])), None).is_err());
    }

    #[test]
    fn a_shared_chain_and_other_effects_are_not_checked() {
        let shared = AbilityEffect::Chain {
            entries: vec![AbilityEffect::Destroy],
            words: None,
        };
        assert!(
            check_chain_words(
                Some(&shared),
                Some(&TargetSpec::Multiple(vec![word(), word()]))
            )
            .is_ok()
        );
        assert!(check_chain_words(Some(&AbilityEffect::Destroy), None).is_ok());
        assert!(check_chain_words(None, None).is_ok());
    }
}
