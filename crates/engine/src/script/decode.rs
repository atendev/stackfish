//! Lua value/table -> Rust type decoders, plus the `parse_*_name` string
//! lookups they're built from — split out of the original monolithic
//! `script.rs`. Two clusters in the original file (one near the top, one
//! right before `bind_game_globals`) are combined here since they're all
//! "Lua value -> Rust type" decoders regardless of where they used to sit.

use mlua::{Function, Lua, Table, Value};

use crate::types::ability::{AbilityEffect, ConditionSpec, CopyExceptions};
use crate::types::card::{CardTypeSet, KeywordSet, Subtype, SupertypeSet};
use crate::types::counter::CounterKind;
use crate::types::cost::{FilterController, FilterSpec};
use crate::types::effect::{AffectedFilter, EffectFilter, EffectPayload, LandwalkQuality, Layer};
use crate::types::ids::ScriptHandle;
use crate::types::mana::{Color, ColorSet, ManaUnit};
use crate::types::target::{TargetFilter, TargetRequirement};
use crate::types::zone::ZoneKind;

use super::tags::{ConditionTag, CostComponent, CostTag, EffectPrimitiveTag, LayerTag, OncePerTurnTag, OperationTag};
use super::types::{ScriptChoiceRequest, ScriptError, ScriptRegistry};

/// Shared by `ScriptRuntime::load_condition` (decoding a freshly-`eval`'d
/// chunk) and `TriggeredAbilityBuilder::SetCondition` (decoding a value
/// already in hand, passed as a Lua call argument) — same two recognized
/// shapes either way: a `Game.Conditions.*` tag, or a custom function. Any
/// other value (including a bare `nil`, e.g. a typo'd `Game.Conditions.Typo`)
/// is a load error, not guessed at — see `RegisteredAbility::condition`'s
/// doc comment for why `nil` isn't silently treated as "always".
pub(crate) fn decode_condition_value(
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
pub(crate) fn decode_operation_value(
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
pub(crate) fn decode_cost_component_value(value: Value) -> Result<CostComponent, ScriptError> {
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
pub(crate) fn decode_filter_table(table: Table) -> Result<FilterSpec, ScriptError> {
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
        other => return Err(ScriptError::LoadError(format!("unknown card type '{other}'"))),
    })
}

/// `KeywordSet` names, by CR 702 keyword — mirrors the scenario crate's
/// `parse_keywords`, same duplication rationale as `parse_card_type_name`.
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
pub(crate) fn parse_counter_kind_name(name: &str) -> CounterKind {
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

pub(crate) fn parse_color_letter(letter: &str) -> Result<ColorSet, ScriptError> {
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
            "unknown controller filter '{other}' - expected you/opponent/any"
        ))),
    }
}

pub(crate) fn parse_supertype_name(name: &str) -> Result<SupertypeSet, ScriptError> {
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
pub(crate) fn decode_subtypes(val: Value) -> Result<Vec<String>, ScriptError> {
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
pub(crate) fn parse_color_string(colors: &str) -> Result<ColorSet, ScriptError> {
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
            )))
        }
    })
}

/// Decodes the optional `opts` table for `Game.Tokens.Creature` /
/// `Game.Tokens.ArtifactCreature`. Returns
/// `(keywords, name_override, additional_types, additional_supertypes)`.
pub(crate) fn decode_creature_token_opts(
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

/// CR 702.14a's landwalk combination forms - `Game.Effects.
/// AddLandwalkQuality(opts)`'s `opts` table. Same `{subtypes/card_types/
/// supertypes/excluded_supertypes}` key vocabulary `decode_filter_table`'s
/// `"object"` branch already establishes for `TargetFilter::Object`, minus
/// `controller`/`zone` (a landwalk quality has no such axes - it's always
/// checked against the defending player's own battlefield). Every field is
/// optional; an empty table decodes to `LandwalkQuality::default()` (matches
/// any land at all, i.e. plain "landwalk" - not a real keyword, but harmless
/// to allow rather than reject).
pub(crate) fn decode_landwalk_quality(opts: mlua::Table) -> Result<LandwalkQuality, ScriptError> {
    let mut subtypes = Vec::new();
    if let Ok(Some(t)) = opts.get::<_, Option<mlua::Table>>("subtypes") {
        for pair in t.sequence_values::<String>() {
            subtypes.push(Subtype(pair.map_err(|e| ScriptError::LoadError(e.to_string()))?));
        }
    }
    let card_types = match opts.get::<_, Option<mlua::Table>>("card_types").map_err(|e| ScriptError::LoadError(e.to_string()))? {
        Some(t) => {
            let mut set = CardTypeSet::empty();
            for pair in t.sequence_values::<String>() {
                set |= parse_card_type_name(&pair.map_err(|e| ScriptError::LoadError(e.to_string()))?)?;
            }
            Some(set)
        }
        None => None,
    };
    let supertypes = match opts.get::<_, Option<mlua::Table>>("supertypes").map_err(|e| ScriptError::LoadError(e.to_string()))? {
        Some(t) => {
            let mut set = SupertypeSet::empty();
            for pair in t.sequence_values::<String>() {
                set |= parse_supertype_name(&pair.map_err(|e| ScriptError::LoadError(e.to_string()))?)?;
            }
            Some(set)
        }
        None => None,
    };
    let excluded_supertypes = match opts
        .get::<_, Option<mlua::Table>>("excluded_supertypes")
        .map_err(|e| ScriptError::LoadError(e.to_string()))?
    {
        Some(t) => {
            let mut set = SupertypeSet::empty();
            for pair in t.sequence_values::<String>() {
                set |= parse_supertype_name(&pair.map_err(|e| ScriptError::LoadError(e.to_string()))?)?;
            }
            Some(set)
        }
        None => None,
    };
    Ok(LandwalkQuality { subtypes, card_types, supertypes, excluded_supertypes })
}

/// CR 707.9's copy-effect exceptions/modifications - shared by
/// `Game.Ops.BecomeCopy(opts?)` and `Game.Tokens.CopyOf(objectId, opts?)`.
/// `opts` is `{ power = N, toughness = N }` (707.9b, both fields required
/// together - a P/T override wouldn't mean anything with only one), and/or
/// `{ keywords = { "Flying", ... } }` (707.9a), and/or `{ retain_name = true
/// }`/`{ retain_color = true }` (707.9c/d - the copy keeps its own original
/// name/color instead of the copied source's; a no-op for `CopyOf`, which
/// has no prior "own identity" to retain from - see `CopyExceptions::apply`'s
/// own doc comment). `None`/omitted is a bare copy - `CopyExceptions::default()`.
pub(crate) fn decode_copy_exceptions(opts: Option<mlua::Table>) -> Result<CopyExceptions, ScriptError> {
    let Some(opts) = opts else { return Ok(CopyExceptions::default()) };
    let power: Option<i32> = opts.get("power").ok().flatten();
    let toughness: Option<i32> = opts.get("toughness").ok().flatten();
    let set_power_toughness = match (power, toughness) {
        (Some(p), Some(t)) => Some((p, t)),
        (None, None) => None,
        _ => {
            return Err(ScriptError::LoadError(
                "CopyExceptions power/toughness must be set together, not just one".to_string(),
            ))
        }
    };
    let mut add_keywords = KeywordSet::empty();
    if let Ok(Some(kws)) = opts.get::<_, Option<mlua::Table>>("keywords") {
        for pair in kws.sequence_values::<String>() {
            add_keywords |= parse_keyword_name(&pair.map_err(|e| ScriptError::LoadError(e.to_string()))?)?;
        }
    }
    let retain_name: bool = opts.get::<_, Option<bool>>("retain_name").ok().flatten().unwrap_or(false);
    let retain_color: bool = opts.get::<_, Option<bool>>("retain_color").ok().flatten().unwrap_or(false);
    Ok(CopyExceptions { set_power_toughness, add_keywords, retain_name, retain_color })
}

/// `TriggeredAbilityBuilder`'s `:SetLayer` decoder — only a `Game.Layers.*`
/// tag is accepted (no custom-function fallback; unlike condition/operation,
/// there's no `Layer::Script` concept to fall back to).
pub(crate) fn decode_layer_value(value: Value) -> Result<Layer, ScriptError> {
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
///
/// CR 303.4m: `{ enchanted_only = true }` short-circuits to
/// `AffectedFilter::EnchantedOnly` before any of `controller`/`card_types`/
/// `self_only` are even read — same "no real card needs both" narrowing
/// `EnchantedOnly`'s own doc comment already states, so combining it with
/// the other keys is simply ignored rather than rejected.
/// The `controller`/`card_types`/`self_only` portion of `:SetAffected`'s
/// filter table, decoded straight to a bare `EffectFilter` - shared by
/// `decode_affected_value` (below, for `AffectedFilter::Primitive`) and
/// `Game.Ops.DestroyAll`'s own filter argument, which needs the identical
/// shape but has no `enchanted_only`/function-predicate cases to consider
/// (CR 701.8's "destroy all X" is never phrased as "enchanted X").
pub(crate) fn decode_effect_filter_table(table: &Table) -> Result<EffectFilter, ScriptError> {
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

    // CR 205.4b: "destroy all Zombies" / "Walls get -1/-0" - same plain
    // subtype-name-list shape decode_filter_table's TargetFilter::Object
    // branch already uses for its own `subtypes` key.
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

    // CR 601.2c/115/205.4b's negated form - "destroy all nonbasic lands,"
    // same shape/table key `subtypes` just above uses.
    let excluded_subtypes_table: Option<Table> =
        table.get("excluded_subtypes").map_err(|e| ScriptError::LoadError(e.to_string()))?;
    let excluded_subtypes = match excluded_subtypes_table {
        Some(t) => {
            let mut list = Vec::new();
            for entry in t.sequence_values::<String>() {
                list.push(Subtype(entry.map_err(|e| ScriptError::LoadError(e.to_string()))?));
            }
            Some(list)
        }
        None => None,
    };

    let self_only: Option<bool> =
        table.get("self_only").map_err(|e| ScriptError::LoadError(e.to_string()))?;

    Ok(EffectFilter {
        controller,
        card_types,
        subtypes,
        excluded_subtypes,
        self_only: self_only.unwrap_or(false),
    })
}

pub(crate) fn decode_affected_value(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    value: Value,
) -> Result<AffectedFilter, ScriptError> {
    match value {
        Value::Table(table) => {
            let enchanted_only: Option<bool> =
                table.get("enchanted_only").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            if enchanted_only.unwrap_or(false) {
                return Ok(AffectedFilter::EnchantedOnly);
            }

            Ok(AffectedFilter::Primitive(decode_effect_filter_table(&table)?))
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

/// Decodes `Game.SelectTargets`/`Game.MayChoose`'s yielded request table —
/// `{ op = "select_target", filter = {...} }` or `{ op = "may_choose" }`. An
/// unrecognized op is a real error, not guessed at, same stance
/// `decode_condition_value` takes for an unrecognized shape.
pub(crate) fn decode_script_choice_request(table: Table) -> Result<ScriptChoiceRequest, ScriptError> {
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
pub(crate) fn decode_target_filter_table(table: Table) -> Result<TargetFilter, ScriptError> {
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

            // CR 601.2c/115's negated form - "target nonland permanent,"
            // same shape/table key `card_types` just above uses.
            let excluded_card_types_table: Option<Table> =
                table.get("excluded_card_types").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let excluded_card_types = match excluded_card_types_table {
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

            // CR 601.2c/115/205.4b's negated form - "target creature that
            // isn't a Wall," same shape/table key `subtypes` just above
            // uses.
            let excluded_subtypes_table: Option<Table> =
                table.get("excluded_subtypes").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let excluded_subtypes = match excluded_subtypes_table {
                Some(t) => {
                    let mut list = Vec::new();
                    for entry in t.sequence_values::<String>() {
                        list.push(Subtype(entry.map_err(|e| ScriptError::LoadError(e.to_string()))?));
                    }
                    Some(list)
                }
                None => None,
            };

            // CR 601.2c/115/205.4a's negated form - "target nonlegendary
            // creature," same shape/table key `supertypes` just above uses.
            let excluded_supertypes_table: Option<Table> =
                table.get("excluded_supertypes").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let excluded_supertypes = match excluded_supertypes_table {
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

            Ok(TargetFilter::Object {
                card_types,
                excluded_card_types,
                subtypes,
                excluded_subtypes,
                supertypes,
                excluded_supertypes,
                controller,
                zone: None,
            })
        }
        other => Err(ScriptError::LoadError(format!(
            "unknown Game.SelectTargets filter type '{other}' - expected object/player/any"
        ))),
    }
}

/// `ScriptRuntime::build_target_requirement`'s return-table decoder — the
/// `count`/`up_to`/`at_least`/`exclude_source` siblings `TargetRequirement`
/// needs beyond a bare `TargetFilter`, same field names/defaults the
/// scenario crate's `TargetRequirementSpec` TOML shape already uses.
///
/// The filter portion supports two shapes, checked via an explicit `filter`
/// key: if present, its value is handed to `decode_target_filter_value` —
/// the same table-or-function-or-`{type="player",filter=fn}` decoder
/// `ActivatedAbilityBuilder::SetTarget` already uses, so a card's own base
/// target requirement can be a scripted predicate too (e.g. "Enchant
/// creature with power 4 or greater," not expressible as a flat option-bag
/// table) — see `TargetFilter::Script`'s own doc comment, already read
/// generically off any `CardDefinition.target_requirement` regardless of
/// where it came from, so no engine-side change was needed for this, only
/// this decode entry point. If `filter` is absent, this falls back to the
/// original flat shape unchanged: the *same* table (not a nested one) is
/// handed to `decode_target_filter_table`, which only reads the keys it
/// recognizes (`type`/`card_types`/`subtypes`/...), so the count/mode keys
/// read here don't need to be stripped out first — every pre-existing
/// script/TOML fixture that never sets a `filter` key keeps working exactly
/// as before.
pub(crate) fn decode_target_requirement_table(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    table: Table,
) -> Result<TargetRequirement, ScriptError> {
    let count: u32 = table.get("count").map_err(|e| {
        ScriptError::LoadError(format!("target_requirement() table missing required 'count': {e}"))
    })?;
    let up_to: bool = table.get::<_, Option<bool>>("up_to").map_err(|e| ScriptError::LoadError(e.to_string()))?.unwrap_or(false);
    let at_least: bool = table
        .get::<_, Option<bool>>("at_least")
        .map_err(|e| ScriptError::LoadError(e.to_string()))?
        .unwrap_or(false);
    let exclude_source: bool = table
        .get::<_, Option<bool>>("exclude_source")
        .map_err(|e| ScriptError::LoadError(e.to_string()))?
        .unwrap_or(false);
    let filter_value: Option<Value> =
        table.get("filter").map_err(|e| ScriptError::LoadError(e.to_string()))?;
    let filter = match filter_value {
        Some(value) => decode_target_filter_value(lua, registry, value)?,
        None => decode_target_filter_table(table)?,
    };
    Ok(TargetRequirement { filter, count, up_to, at_least, exclude_source })
}

/// `ActivatedAbilityBuilder::SetTarget`'s argument decoder — a `Value`
/// rather than `decode_target_filter_table`'s bare `Table`, same table-or-
/// function fallback `decode_affected_value` established for `:SetAffected`.
/// A bare function is registered and returned as `TargetFilter::Script`
/// (invoked later via `ScriptRuntime::invoke_target_legal`), for a legality
/// shape the flat option-bag table can't express (e.g. "target creature
/// with power 4 or greater").
pub(crate) fn decode_target_filter_value(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    value: Value,
) -> Result<TargetFilter, ScriptError> {
    match value {
        // `{ type = "player", filter = function(player, source, controller,
        // state) -> bool }` — `TargetFilter::PlayerScript`'s Lua entry
        // point. Checked ahead of the plain flat-option-bag decode below
        // since a bare `{type="player"}` (no `filter` function) must still
        // fall through to `decode_target_filter_table`'s existing
        // `TargetFilter::Player` path unchanged.
        Value::Table(ref table) if matches!(table.get::<_, Option<String>>("type"), Ok(Some(ref k)) if k == "player")
            && matches!(table.get::<_, Value>("filter"), Ok(Value::Function(_))) =>
        {
            let function: Function =
                table.get("filter").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let key = lua
                .create_registry_value(function)
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let handle = registry.next_script_handle();
            registry.scripts.insert(handle, key);
            Ok(TargetFilter::PlayerScript(handle))
        }
        Value::Table(table) => decode_target_filter_table(table),
        Value::Function(function) => {
            let key = lua
                .create_registry_value(function)
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let handle = registry.next_script_handle();
            registry.scripts.insert(handle, key);
            Ok(TargetFilter::Script(handle))
        }
        other => Err(ScriptError::LoadError(format!(
            "SetTarget expected a filter table or a function, got {}",
            other.type_name()
        ))),
    }
}
