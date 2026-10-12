//! Lua value/table -> Rust type decoders, plus the `parse_*_name` string
//! lookups they're built from — split out of the original monolithic
//! `script.rs`. Two clusters in the original file (one near the top, one
//! right before `bind_game_globals`) are combined here since they're all
//! "Lua value -> Rust type" decoders regardless of where they used to sit.

use mlua::{Function, Lua, Table, Value};

use crate::types::ability::{AbilityEffect, ConditionSpec, CopyExceptions, CounterAmount};
use crate::types::card::{CardTypeSet, KeywordSet, Subtype, SupertypeSet};
use crate::types::counter::CounterKind;
use crate::types::cost::{AlternativeCost, CostSpec, FilterController, FilterSpec};
use crate::types::effect::{AffectedFilter, EffectFilter, EffectPayload, LandwalkQuality, Layer};
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::mana::{Color, ColorSet, ManaUnit};
use crate::types::target::{ManaValueBound, ManaValueComparison, ManaValueRequirement, TargetFilter, TargetRequirement, TargetSpec};
use crate::types::zone::ZoneKind;

use super::tags::{
    ConditionTag, CostComponent, CostTag, EffectPrimitiveTag, ForEachPlayerTag, LayerTag, OncePerTurnTag,
    OperationTag,
};
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

/// A `Game.Ops.*` constructor passed uncalled (`SetOperation(Game.Ops.Mill)`)
/// would register as a custom script function and fail confusingly at
/// resolution. Name the mistake up front instead.
fn reject_uncalled_op(lua: &Lua, function: &Function) -> Result<(), ScriptError> {
    let Ok(ops) = lua.globals().get::<_, mlua::Table>("Game").and_then(|g| g.get::<_, mlua::Table>("Ops")) else {
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
        // `phase5/mass-choice-effects`: `Game.Ops.ForEachPlayer(x)` returns a
        // `ForEachPlayerTag` - checked ahead of the plain `OperationTag` case
        // below since it's a different concrete `UserData` type (see that
        // tag's own doc comment for why it can't just be an `OperationTag`
        // either way).
        Value::UserData(ref ud) if ud.is::<ForEachPlayerTag>() => {
            let tag: ForEachPlayerTag =
                ud.borrow::<ForEachPlayerTag>().map_err(|e| ScriptError::LoadError(e.to_string()))?.clone();
            match tag {
                ForEachPlayerTag::Function(handle) => Ok((Some(AbilityEffect::ForEachPlayer(None)), handle)),
                ForEachPlayerTag::Static(effect) => {
                    Ok((Some(AbilityEffect::ForEachPlayer(Some(Box::new(effect)))), ScriptHandle(0)))
                }
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

/// `ScriptRuntime::build_alternative_costs`'s per-entry table decoder — one
/// `s.alternative_costs()` return-table entry becomes one
/// `AlternativeCost::Replacement`. `costs` accumulates the same way
/// `ActivatedAbilityBuilder::AddCost` does (one `Game.Costs.*` value per
/// array entry, via `decode_cost_component_value` above) rather than
/// inventing a second cost-table shape — `Game.Costs.Mana` may appear at
/// most once, same "an activated ability has only one mana-cost component"
/// rule `AddCost` already enforces. `from_zone` reuses `parse_zone_kind_name`
/// below. `exile_on_leaving_stack`/`escape` default to `false` when omitted —
/// the overwhelming majority of `Replacement` alternatives (anything that
/// isn't Flashback/Escape) need neither.
pub(crate) fn decode_alternative_cost_table(table: Table) -> Result<AlternativeCost, ScriptError> {
    let description: String = table.get("description").map_err(|e| {
        ScriptError::LoadError(format!("alternative_costs() entry missing required 'description': {e}"))
    })?;
    let from_zone_name: String = table.get("from_zone").map_err(|e| {
        ScriptError::LoadError(format!("alternative_costs() entry missing required 'from_zone': {e}"))
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
        ScriptError::LoadError(format!("alternative_costs() entry missing required 'costs': {e}"))
    })?;
    let mut cost = CostSpec { mana: None, additional: Vec::new(), alternative: Vec::new() };
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

    Ok(AlternativeCost::Replacement { description, cost, from_zone, exile_on_leaving_stack, escape })
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
        "Hexproof" => KeywordSet::HEXPROOF,
        "Shroud" => KeywordSet::SHROUD,
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

/// Same letters as `parse_color_letter` above, but returning a single
/// `Color` rather than a `ColorSet` — needed by `CostModifierTarget::Colored`
/// (`phase5/cost-modifier-colored-pip`), which names exactly one color, not
/// a combinable set.
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

/// Same "string or table" shape as `decode_subtypes`, for `Game.Effects.
/// SetCardTypes` specifically — unlike `AddCardType`'s single-name-at-a-time
/// convention (safe since additive composition across several registered
/// static abilities works fine), a "set" overwrite must express its whole
/// resulting type line in one call (e.g. "becomes an artifact creature" is
/// two card types set together) - two separate `SetCardTypes` registrations
/// wouldn't compose, the later one would just wipe out the earlier one's
/// result entirely (see `layers::effective_characteristics`'s own layer-4
/// fold).
pub(crate) fn decode_card_type_names(val: Value) -> Result<Vec<String>, ScriptError> {
    match val {
        Value::String(s) => Ok(vec![s.to_str().map_err(|e| ScriptError::LoadError(e.to_string()))?.to_string()]),
        Value::Table(t) => {
            let mut out = Vec::new();
            for pair in t.sequence_values::<String>() {
                out.push(pair.map_err(|e| ScriptError::LoadError(e.to_string()))?);
            }
            Ok(out)
        }
        _ => Err(ScriptError::LoadError("card types must be a string or table".to_string())),
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
    // CR 707.9a: Vesuvan Doppelganger's own "except it has [linked upkeep
    // ability]" - see CopyExceptions::grant_recurring_copy_trigger's own doc
    // comment. Only meaningful on `EntersAsCopy`/`BecomeCopy` in practice (a
    // freshly created `CopyOf` token could carry it too, though no real card
    // needs a token-generator with this shape).
    let grant_recurring_copy_trigger: bool =
        opts.get::<_, Option<bool>>("grant_recurring_copy_trigger").ok().flatten().unwrap_or(false);
    // CR 707.9e: Spark Double's own "enters with an additional +1/+1
    // counter" - see CopyExceptions::enters_with_additional_counters's own
    // doc comment. `{ counter_kind = "PlusOnePlusOne", counter_amount = 1 }`,
    // both required together (same "set together, not just one" stance
    // power/toughness above already takes). `counter_amount` may also be the
    // string `"X"` (Altered Ego, `phase5/altered-ego-x-counters`) for
    // `CounterAmount::X` instead of a fixed number - mlua's own `Value`
    // coercion means a plain `.get::<u32>()` would silently fail to match a
    // string, so `counter_amount` is read as a raw `mlua::Value` first and
    // matched by hand.
    let counter_kind: Option<String> = opts.get("counter_kind").ok().flatten();
    let counter_amount_value: Option<mlua::Value> = opts.get("counter_amount").ok();
    let counter_amount = match counter_amount_value {
        Some(mlua::Value::Integer(n)) if n >= 0 => Some(CounterAmount::Fixed(n as u32)),
        Some(mlua::Value::Number(n)) if n >= 0.0 => Some(CounterAmount::Fixed(n as u32)),
        Some(mlua::Value::String(s)) if s.to_str().ok() == Some("X") => Some(CounterAmount::X),
        Some(mlua::Value::Nil) | None => None,
        _ => {
            return Err(ScriptError::LoadError(
                "CopyExceptions counter_amount must be a non-negative number or \"X\"".to_string(),
            ))
        }
    };
    let enters_with_additional_counters = match (counter_kind, counter_amount) {
        (Some(kind), Some(amount)) => Some((parse_counter_kind_name(&kind), amount)),
        (None, None) => None,
        _ => {
            return Err(ScriptError::LoadError(
                "CopyExceptions counter_kind/counter_amount must be set together, not just one".to_string(),
            ))
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
/// the other keys is simply ignored rather than rejected. `{
/// enchanted_player_controls = true }` mirrors it for `AffectedFilter::
/// EnchantedPlayerControls` (CR 303.4m's player-side sibling, e.g. Curse of
/// Vitality's "Creatures enchanted player controls have lifelink"), checked
/// second so an (invalid, but not rejected) table setting both keys
/// deterministically prefers `enchanted_only`.
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

    // CR 205.4a: "destroy all legendary permanents," same shape/table key
    // `decode_filter_table`'s `TargetFilter::Object` branch already uses for
    // its own `supertypes` key.
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

    // CR 601.2c/115/205.4a's negated form - "destroy all nonbasic lands,"
    // same shape/table key `supertypes` just above uses.
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

    let self_only: Option<bool> =
        table.get("self_only").map_err(|e| ScriptError::LoadError(e.to_string()))?;

    Ok(EffectFilter {
        controller,
        card_types,
        subtypes,
        excluded_subtypes,
        supertypes,
        excluded_supertypes,
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

            let enchanted_player_controls: Option<bool> = table
                .get("enchanted_player_controls")
                .map_err(|e| ScriptError::LoadError(e.to_string()))?;
            if enchanted_player_controls.unwrap_or(false) {
                return Ok(AffectedFilter::EnchantedPlayerControls);
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

/// Decodes `Game.SelectTargets`/`Game.MayChoose`/`Game.ChooseN`'s yielded
/// request table — `{ op = "select_target", filter = ... }` or `{ op =
/// "may_choose" }`. An unrecognized op is a real error, not guessed at, same
/// stance `decode_condition_value` takes for an unrecognized shape. `lua`/
/// `registry` (`phase5/mass-choice-effects` follow-up) are needed because
/// `filter` can now be a scripted predicate (a bare function, or `{type=
/// "player", filter=fn}`) same as `:SetTarget`'s own filter argument — see
/// `decode_target_filter_value`'s own doc comment for the full shape; a
/// script-backed filter needs its function registered into the registry the
/// same way any other custom-function value does.
pub(crate) fn decode_script_choice_request(
    lua: &Lua,
    registry: &mut ScriptRegistry,
    table: Table,
) -> Result<ScriptChoiceRequest, ScriptError> {
    let op: String = table.get("op").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    match op.as_str() {
        "select_target" => {
            let filter_value: Value =
                table.get("filter").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            // `phase5/mass-choice-effects`: `Game.ChooseN`'s Lua loop grows
            // this by one id per iteration so a repeat ask can't hand back
            // an object already picked this batch - see `ScriptChoiceRequest
            // ::SelectTarget`'s own doc comment for why this lives here
            // rather than on `TargetFilter` itself. Absent for a bare
            // `Game.SelectTargets` call, same as every pre-existing script.
            let exclude_ids: Option<Vec<i64>> =
                table.get("exclude_ids").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            let exclude_ids =
                exclude_ids.unwrap_or_default().into_iter().map(|id| ObjectId(id as u64)).collect();
            Ok(ScriptChoiceRequest::SelectTarget {
                filter: decode_target_filter_value(lua, registry, filter_value)?,
                exclude_ids,
            })
        }
        "may_choose" => Ok(ScriptChoiceRequest::MayChoose),
        other => Err(ScriptError::RuntimeError(format!("unknown script choice request op '{other}'"))),
    }
}

/// Decodes one `{kind, id}` picked-target table — the same shape
/// `ScriptRuntime::resume_operation`'s own encode side produces as the
/// answer to a `Game.SelectTargets`/`Game.ChooseN` yield (`kind` one of
/// `"object"`/`"player"`/`"ability"`). Used by `Game.Ops.CastSpell`'s
/// `opts.targets` decode (`handle_operation_result`) to turn a script's
/// already-picked targets (gathered via its own prior `Game.SelectTargets`/
/// `Game.ChooseN` calls) back into a real `Target`.
pub(crate) fn decode_target_pick(table: &Table) -> Result<Target, ScriptError> {
    let kind: String = table.get("kind").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let id: u64 = table.get("id").map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    match kind.as_str() {
        "object" => Ok(Target::Object(ObjectId(id))),
        "player" => Ok(Target::Player(PlayerId(id as u32))),
        "ability" => Ok(Target::Ability(ObjectId(id))),
        other => Err(ScriptError::RuntimeError(format!("unknown target pick kind '{other}'"))),
    }
}

/// `Game.SelectTargets`'s filter argument — same flat option-bag shape
/// `decode_affected_value`/`decode_filter_table` already use for their own
/// filter tables, plus a `type` discriminator (`"object"` default/`"player"`/
/// `"any"`/`"ability"`) matching the scenario crate's own TOML `target_requirement.filter`
/// convention (`crates/scenario/src/lib.rs`'s `build_target_requirement`)
/// exactly, rather than inventing a second vocabulary. `type` defaults to
/// `"object"` so every pre-existing script that omits it (this slice's own
/// predecessor) keeps working unchanged. `zone` (`phase5/mass-choice-effects`,
/// e.g. `Game.ChooseN({ zone = "Hand", ... }, n)` for CR 701.9 discard) is a
/// zone name string parsed via `parse_zone_kind_name`, same names/casing the
/// TOML fixture format already uses - `None` still defaults to `Battlefield`
/// exactly as before for every filter that omits it.
pub(crate) fn decode_target_filter_table(table: Table) -> Result<TargetFilter, ScriptError> {
    let kind: Option<String> = table.get("type").map_err(|e| ScriptError::LoadError(e.to_string()))?;
    match kind.as_deref().unwrap_or("object") {
        "player" => {
            let controller_name: Option<String> =
                table.get("controller").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let controller = controller_name.map(|name| parse_filter_controller_name(&name)).transpose()?;
            Ok(TargetFilter::Player { controller })
        }
        "any" => Ok(TargetFilter::AnyTarget),
        // CR 113.9/701.6a: "counter target activated or triggered ability"
        // (Stifle, Trickbind) - see TargetFilter::Ability's own doc comment.
        "ability" => Ok(TargetFilter::Ability),
        // CR 702.21a (Ward): the spell or ability that triggered this one.
        "triggering" => Ok(TargetFilter::TriggeringStackObject),
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

            let zone_name: Option<String> = table.get("zone").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let zone = zone_name.map(|name| parse_zone_kind_name(&name)).transpose()?;

            // CR 202.3: "target spell with mana value X (or less/or
            // greater)" - three independent optional keys (matching
            // `up_to`/`at_least`'s own flat-boolean-flags convention rather
            // than a nested comparison table), each accepting either a plain
            // number (`ManaValueBound::Fixed`) or the literal string "X"
            // (`ManaValueBound::X`, resolving against whatever this
            // cast/resolution's own announced X is - see `ManaValueBound`'s
            // own doc comment). At most one is expected to be set on any
            // real card; if more than one is, `equals` wins over `at_most`
            // over `at_least`, same "no validation, just precedence" stance
            // `at_least` already takes over `up_to` elsewhere in this file.
            let mana_value_equals: Option<Value> =
                table.get("mana_value_equals").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let mana_value_at_most: Option<Value> =
                table.get("mana_value_at_most").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let mana_value_at_least: Option<Value> =
                table.get("mana_value_at_least").map_err(|e| ScriptError::LoadError(e.to_string()))?;
            let mana_value = if let Some(v) = mana_value_equals {
                Some(ManaValueRequirement { comparison: ManaValueComparison::Equal, bound: decode_mana_value_bound(v)? })
            } else if let Some(v) = mana_value_at_most {
                Some(ManaValueRequirement { comparison: ManaValueComparison::AtMost, bound: decode_mana_value_bound(v)? })
            } else if let Some(v) = mana_value_at_least {
                Some(ManaValueRequirement { comparison: ManaValueComparison::AtLeast, bound: decode_mana_value_bound(v)? })
            } else {
                None
            };

            Ok(TargetFilter::Object {
                card_types,
                excluded_card_types,
                subtypes,
                excluded_subtypes,
                supertypes,
                excluded_supertypes,
                controller,
                zone,
                mana_value,
            })
        }
        other => Err(ScriptError::LoadError(format!(
            "unknown Game.SelectTargets filter type '{other}' - expected object/player/any/ability"
        ))),
    }
}

/// `mana_value_equals`/`mana_value_at_most`/`mana_value_at_least`'s shared
/// value decode — a nonnegative integer (`ManaValueBound::Fixed`) or the
/// literal string `"X"` (`ManaValueBound::X`). Any other shape (a negative
/// number, a bool, some other string) is a loud load-time error, same
/// "typo'd input fails to load rather than silently guessing" stance every
/// other decoder in this file already takes.
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

/// `ScriptRuntime::build_target_requirement`'s whole-return decoder:
/// either one requirement table (any table with no array part, decoded
/// exactly as `decode_target_requirement_table` always did, into
/// `TargetSpec::Primitive`) or a sequence of requirement tables, one per
/// distinct "target" word in card order (`TargetSpec::Multiple`, e.g. "target
/// creature you control fights target creature you don't control"). A
/// one-entry sequence is the same single word as a bare table, so it decodes
/// to `Primitive` too.
///
/// Rejected with a clear error: an empty table, a table that mixes array
/// entries with other keys (such as `count`), a sequence with gaps, and a
/// sequence entry that is not a table.
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
        return Ok(TargetSpec::Primitive(decode_target_requirement_table(lua, registry, table)?));
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
        let entry: Value = table.get(index).map_err(|e| ScriptError::LoadError(e.to_string()))?;
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
