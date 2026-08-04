//! Builds the `Game.*` Lua DSL and binds it into a fresh `Lua` instance's
//! globals — split out of the original monolithic `script.rs`. Kept as one
//! big function/file rather than broken up per `Game.*` sub-table: it's one
//! cohesive registration pass, not several independent concerns.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{AnyUserData, Function, Lua, Table, Value};

use crate::types::ability::{
    AbilityEffect, ConditionPrimitive, CreatureTokenSpec, InterveningIfPrimitive, NamedToken, SearchDestination,
    TokenSpec,
};
use crate::types::card::{CardTypeSet, Subtype};
use crate::types::cost::{AdditionalCost, CostModifierTarget, CostSpec, FilterController};
use crate::types::effect::{
    AffectedFilter, ConsumptionPolicy, EffectDuration, EffectFilter, EffectPrimitive, Layer, ReplacementPayload,
};
use crate::types::ids::ObjectId;
use crate::types::mana::{ManaCost, ManaUnit};
use crate::types::player::{DrawLimitAffected, HandRevealAffected, HandSizeAffected, HandSizeEffect};

use super::builders::{
    ActivatedAbilityBuilder, CostModifierBuilder, DrawLimitModifierBuilder, HandRevealModifierBuilder,
    HandSizeModifierBuilder, ManaAbilityBuilder, ReplacementEffectBuilder, StaticAbilityBuilder,
    TriggeredAbilityBuilder,
};
use super::decode::{
    decode_copy_exceptions, decode_creature_token_opts, decode_effect_filter_table, decode_filter_table,
    decode_landwalk_quality, decode_subtypes, parse_card_type_name, parse_color_letter, parse_color_string,
    parse_counter_kind_name, parse_filter_controller_name, parse_keyword_name, parse_mana_unit_letter,
    parse_supertype_name, parse_zone_kind_name,
};
use super::tags::{
    ConditionTag, ConsumptionPolicyTag, CostComponent, CostTag, DrawLimitAffectedTag, DurationTag,
    EffectPrimitiveTag, EventFilterKind, EventFilterKindTag, HandRevealAffectedTag, HandSizeAffectedTag,
    HandSizeEffectTag, InterveningIfTag, LayerTag, OncePerTurnTag, OperationTag, RedirectDamageToSourceTag,
    ReplacementPayloadTag, TokenTag,
};
use super::types::{to_lua_err, ScriptError, ScriptRegistry};

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
pub(crate) fn bind_game_globals(lua: &Lua, registry: Rc<RefCell<ScriptRegistry>>) -> mlua::Result<()> {
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
    conditions.set("Cycled", ConditionTag(ConditionPrimitive::Cycled))?;
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
    let once_per_turn = lua.create_function(|_, tag: AnyUserData| {
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
    // CR 603.4/702.138b: "if it escaped" - Kroxa/Uro's own real card
    // pattern (an ordinary ETB trigger's intervening-if).
    intervening_if.set("Escaped", InterveningIfTag(InterveningIfPrimitive::Escaped))?;

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
    // CR 106.11: `Game.Ops.AddRestrictedMana(letter, cardType, count?)` -
    // same single-symbol vocabulary as `AddMana`, but every produced unit
    // is tagged so `PaymentPlan::validate` only accepts it toward casting a
    // spell of `cardType` (Mishra's Workshop: "Spend this mana only to cast
    // artifact spells"). Narrowed to a single card-type filter - no
    // object-specific ("only to cast the exiled card") or ability-
    // activation restriction shape exists yet, named in this slice's own
    // backlog note rather than guessed at.
    let add_restricted_mana = lua.create_function(|_, (letter, card_type, count): (String, String, Option<u32>)| {
        let unit = parse_mana_unit_letter(&letter).map_err(to_lua_err)?;
        let types = parse_card_type_name(&card_type).map_err(to_lua_err)?;
        let restricted = match unit {
            ManaUnit::Colored(c) => ManaUnit::RestrictedColored(c, types),
            ManaUnit::Colorless => ManaUnit::RestrictedColorless(types),
            _ => unreachable!("parse_mana_unit_letter only ever returns Colored/Colorless"),
        };
        Ok(OperationTag(AbilityEffect::AddMana(vec![restricted; count.unwrap_or(1) as usize])))
    })?;
    ops.set("AddRestrictedMana", add_restricted_mana)?;
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
    let gain_life = lua.create_function(|_, amount: i32| {
        Ok(OperationTag(AbilityEffect::GainLife(amount)))
    })?;
    ops.set("GainLife", gain_life)?;
    let damage = lua.create_function(|_, amount: u32| {
        Ok(OperationTag(AbilityEffect::Damage(amount)))
    })?;
    ops.set("Damage", damage)?;
    ops.set("Destroy", OperationTag(AbilityEffect::Destroy))?;
    ops.set("BecomeBlocked", OperationTag(AbilityEffect::BecomeBlocked))?;
    let destroy_all = lua.create_function(|_, table: Table| {
        let filter = decode_effect_filter_table(&table).map_err(to_lua_err)?;
        Ok(OperationTag(AbilityEffect::DestroyAll(filter)))
    })?;
    ops.set("DestroyAll", destroy_all)?;
    ops.set("Sacrifice", OperationTag(AbilityEffect::Sacrifice))?;
    ops.set("Exile", OperationTag(AbilityEffect::Exile))?;
    let counters = lua.create_function(|_, (kind, amount): (String, i32)| {
        Ok(OperationTag(AbilityEffect::Counters { kind: parse_counter_kind_name(&kind), amount }))
    })?;
    ops.set("Counters", counters)?;
    // `Game.Ops.Pump(power, toughness, duration)` - `duration` is a
    // `Game.Durations.*` tag (see `DurationTag`'s own doc comment for which
    // ones exist and why).
    let pump = lua.create_function(|_, (power, toughness, duration): (i32, i32, AnyUserData)| {
        let duration = duration.borrow::<DurationTag>()?.0.clone();
        Ok(OperationTag(AbilityEffect::Pump { power, toughness, duration }))
    })?;
    ops.set("Pump", pump)?;
    // `Game.Ops.GainControl(duration)` - same `Game.Durations.*` tag decode
    // as Pump's own duration argument. No player argument - the new
    // controller is always the resolving spell/ability's own controller
    // (see AbilityEffect::GainControl's own doc comment for why).
    let gain_control = lua.create_function(|_, duration: AnyUserData| {
        let duration = duration.borrow::<DurationTag>()?.0.clone();
        Ok(OperationTag(AbilityEffect::GainControl(duration)))
    })?;
    ops.set("GainControl", gain_control)?;
    // `Game.Ops.Regenerate()` (CR 701.19a) - no arguments, same toggle shape
    // as `Game.Ops.Destroy`/`Sacrifice`/`Exile`/`Shuffle`.
    ops.set("Regenerate", OperationTag(AbilityEffect::Regenerate))?;
    let draw_cards = lua.create_function(|_, amount: u32| {
        Ok(OperationTag(AbilityEffect::DrawCards(amount)))
    })?;
    ops.set("DrawCards", draw_cards)?;
    let create_token = lua.create_function(|_, (spec_ud, count): (AnyUserData, Option<u32>)| {
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
    // `opts.tapped`/`opts.enters_attached` only matter for `"battlefield"`
    // (CR 303.4f/g/h, `phase5/aura-enters-attached` — meaningful only for a
    // found Aura card, see `SearchDestination::Battlefield`'s own doc
    // comment); `opts.shuffle` defaults to `true` (CR 701.24a: nearly every
    // real search effect pairs with "then shuffle").
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
                let enters_attached: Option<bool> = match &opts {
                    Some(t) => t.get("enters_attached").map_err(|e| to_lua_err(ScriptError::LoadError(e.to_string())))?,
                    None => None,
                };
                SearchDestination::Battlefield {
                    tapped: tapped.unwrap_or(false),
                    enters_attached: enters_attached.unwrap_or(false),
                }
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
    // `Game.Ops.BecomeCopy(opts?)` (CR 707.2/707.9, layer 1) - the copy
    // target is whatever the ability's own targeting already resolved, not a
    // value passed here. `opts` is `CopyExceptions` (CR 707.9's "copy,
    // except..." shapes) - `{ power = N, toughness = N }` (707.9b) and/or
    // `{ keywords = { "Flying", ... } }` (707.9a); omitted or `nil` is a
    // bare copy, unchanged from before this field existed.
    let become_copy = lua.create_function(|_, opts: Option<Table>| {
        let exceptions = decode_copy_exceptions(opts).map_err(to_lua_err)?;
        Ok(OperationTag(AbilityEffect::BecomeCopy(exceptions)))
    })?;
    ops.set("BecomeCopy", become_copy)?;
    // `Game.Ops.Attach` (CR 702.6a, Equip) - no arguments, same toggle shape
    // as `Game.Ops.Destroy`/`Sacrifice`/`Exile`/`Shuffle`/`BecomeCopy`; the
    // attach target is whatever the activated ability's own `:SetTarget`
    // resolved, not a value passed here.
    ops.set("Attach", OperationTag(AbilityEffect::Attach))?;
    // `Game.Ops.ExchangeTextBoxes` (CR 612.5) - no arguments, same toggle
    // shape as `Game.Ops.Attach`/`BecomeCopy`; the two targets are whatever
    // the spell/ability's own `:SetTarget`/`target_requirement` resolved
    // (a `TargetSpec::Multiple` of exactly two entries), not values passed
    // here.
    ops.set("ExchangeTextBoxes", OperationTag(AbilityEffect::ExchangeTextBoxes))?;

    let costs = lua.create_table()?;
    costs.set("TapSelf", CostTag(CostComponent::Additional(AdditionalCost::TapSelf)))?;
    // CR 702.29a Cycling's "Discard this card" - always the ability's own
    // source, no filter table (mirrors TapSelf's own no-argument shape,
    // distinct from the filter-driven Discard binding just below).
    costs.set("DiscardSelf", CostTag(CostComponent::Additional(AdditionalCost::DiscardSelf)))?;
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
    // CR 702.138a Escape's "Exile N other cards from your graveyard" - same
    // filter+count shape as TapPermanents, just against the graveyard.
    costs.set(
        "ExileFromGraveyard",
        lua.create_function(|_, (table, count): (Table, u32)| {
            let filter = decode_filter_table(table).map_err(to_lua_err)?;
            Ok(CostTag(CostComponent::Additional(AdditionalCost::ExileFromGraveyard { filter, count })))
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
        "RemoveKeyword",
        lua.create_function(|_, name: String| {
            let set = parse_keyword_name(&name).map_err(to_lua_err)?;
            Ok(EffectPrimitiveTag(EffectPrimitive::RemoveKeyword(set)))
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
        "AddLandwalkSubtype",
        lua.create_function(|_, name: String| Ok(EffectPrimitiveTag(EffectPrimitive::AddLandwalkSubtype(Subtype(name)))))?,
    )?;
    effects.set(
        "AddLandwalkQuality",
        lua.create_function(|_, opts: Table| {
            let quality = decode_landwalk_quality(opts).map_err(to_lua_err)?;
            Ok(EffectPrimitiveTag(EffectPrimitive::AddLandwalkQuality(quality)))
        })?,
    )?;
    effects.set(
        "AddBandsWithOther",
        lua.create_function(|_, name: String| Ok(EffectPrimitiveTag(EffectPrimitive::AddBandsWithOther(Subtype(name)))))?,
    )?;
    effects.set(
        "AddBandsWithOtherSupertype",
        lua.create_function(|_, name: String| {
            let set = parse_supertype_name(&name).map_err(to_lua_err)?;
            Ok(EffectPrimitiveTag(EffectPrimitive::AddBandsWithOtherSupertype(set)))
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
    // watches (ReplacementEffectBuilder::SetFilter). Only the shapes with a
    // real query path (ReplacementEffect::matches) are ever bound - see
    // EventFilter's own doc comment for what's unmodeled.
    let event_filters = lua.create_table()?;
    event_filters.set("DamageDealt", EventFilterKindTag(EventFilterKind::DamageDealt))?;
    event_filters.set("WouldDie", EventFilterKindTag(EventFilterKind::WouldDie))?;
    // `Game.EventFilters.WouldMoveZone(fromName, toName)` - either argument
    // may be nil for "any zone" (EventFilter::WouldMoveZone's own wildcard
    // meaning), e.g. `Game.EventFilters.WouldMoveZone("Battlefield", nil)`
    // matches any move off the battlefield, `Game.EventFilters.
    // WouldMoveZone(nil, "Exile")` matches any move into exile.
    event_filters.set(
        "WouldMoveZone",
        lua.create_function(|_, (from_name, to_name): (Option<String>, Option<String>)| {
            let from = from_name.as_deref().map(parse_zone_kind_name).transpose().map_err(to_lua_err)?;
            let to = to_name.as_deref().map(parse_zone_kind_name).transpose().map_err(to_lua_err)?;
            Ok(EventFilterKindTag(EventFilterKind::WouldMoveZone { from, to }))
        })?,
    )?;
    // CR 702.89a (Umbra armor): "if enchanted permanent would be destroyed" -
    // pairs only with Game.ReplacementPayloads.UmbraArmor. :SetAffected is a
    // no-op for this shape (the affected object is always this effect's own
    // attached_to, not a controller/card_types/self_only filter).
    event_filters.set("WouldDieEnchanted", EventFilterKindTag(EventFilterKind::WouldDieEnchanted))?;
    // CR 303.4m/616 (Wheel of Sun and Moon): "if a card would be put into
    // enchanted player's graveyard from anywhere" - pairs only with
    // Game.ReplacementPayloads.ReplaceZoneDestination. :SetAffected is a
    // no-op here too, same reason WouldDieEnchanted's own comment gives.
    event_filters.set(
        "WouldEnterEnchantedPlayerGraveyard",
        EventFilterKindTag(EventFilterKind::WouldEnterEnchantedPlayerGraveyard),
    )?;

    // `Game.ReplacementPayloads.*` - what a ReplacementEffect rewrites the
    // matched event into (ReplacementEffectBuilder::SetPayload).
    // `RedirectDamageToSource` is a sentinel, not a ReplacementPayloadTag -
    // see RedirectDamageToSourceTag's own doc comment for why.
    let replacement_payloads = lua.create_table()?;
    replacement_payloads.set("PreventDamage", ReplacementPayloadTag(ReplacementPayload::PreventDamage))?;
    replacement_payloads.set("Regenerate", ReplacementPayloadTag(ReplacementPayload::Regenerate))?;
    // CR 702.89a (Umbra armor) - pairs only with
    // Game.EventFilters.WouldDieEnchanted.
    replacement_payloads.set("UmbraArmor", ReplacementPayloadTag(ReplacementPayload::UmbraArmor))?;
    replacement_payloads.set("RedirectDamageToSource", RedirectDamageToSourceTag)?;
    replacement_payloads.set(
        "ReplaceZoneDestination",
        lua.create_function(|_, zone_name: String| {
            let zone = parse_zone_kind_name(&zone_name).map_err(to_lua_err)?;
            Ok(ReplacementPayloadTag(ReplacementPayload::ReplaceZoneDestination(zone)))
        })?,
    )?;
    // CR 603.6d: pairs only with `Game.EventFilters.WouldMoveZone(_, "Battlefield")`
    // - see `validate_replacement_filter_payload_pairing`. Applied by
    // `replacement::apply_enters_battlefield_modifiers`, not `ReplacementEffect::apply`
    // (see `ReplacementPayload::EntersTapped`'s own doc comment).
    replacement_payloads.set("EntersTapped", ReplacementPayloadTag(ReplacementPayload::EntersTapped))?;
    // CR 614.1c/122.6: `Game.ReplacementPayloads.EntersWithCounters(kindName, amount)`
    // - fixed printed amounts only, same `parse_counter_kind_name` table
    // `Game.Ops.Counters` already uses. Sunburst's game-state-dependent
    // amount isn't representable this way (own dedicated payload instead);
    // Devour/Amplify's fixed per-unit rate is representable, but their
    // *choice* of which units count still needs a real pause, not this
    // payload alone - see `ReplacementPayload::EntersWithCounters`'s own
    // doc comment.
    replacement_payloads.set(
        "EntersWithCounters",
        lua.create_function(|_, (kind_name, amount): (String, i32)| {
            let kind = parse_counter_kind_name(&kind_name);
            Ok(ReplacementPayloadTag(ReplacementPayload::EntersWithCounters { kind, amount }))
        })?,
    )?;
    // CR 702.54a: `Game.ReplacementPayloads.EntersWithCountersBloodthirst(kindName, amount)`
    // - same fixed-amount shape as `EntersWithCounters` above, gated on
    // `replacement::opponent_damaged_this_turn` instead of unconditional.
    replacement_payloads.set(
        "EntersWithCountersBloodthirst",
        lua.create_function(|_, (kind_name, amount): (String, i32)| {
            let kind = parse_counter_kind_name(&kind_name);
            Ok(ReplacementPayloadTag(ReplacementPayload::EntersWithCountersBloodthirst { kind, amount }))
        })?,
    )?;
    // CR 702.138c: `Game.ReplacementPayloads.EntersWithCountersEscape(kindName, amount)`
    // - same fixed-amount shape as `EntersWithCountersBloodthirst` above,
    // gated on `GameObject::entered_via_escape` instead.
    replacement_payloads.set(
        "EntersWithCountersEscape",
        lua.create_function(|_, (kind_name, amount): (String, i32)| {
            let kind = parse_counter_kind_name(&kind_name);
            Ok(ReplacementPayloadTag(ReplacementPayload::EntersWithCountersEscape { kind, amount }))
        })?,
    )?;
    // CR 702.44: `Game.ReplacementPayloads.EntersWithCountersSunburst` - no
    // arguments, unlike every other `EntersWithCounters*` above. Both kind
    // and amount are derived at apply time (see the variant's own doc
    // comment) instead of declared here.
    replacement_payloads.set("EntersWithCountersSunburst", ReplacementPayloadTag(ReplacementPayload::EntersWithCountersSunburst))?;
    // CR 702.82a/c: `Game.ReplacementPayloads.EntersWithCountersDevour(perCreature, quality)`
    // - the choice of *which* creatures (if any) to sacrifice is handled
    // entirely outside this payload (see its own doc comment) - all it
    // carries is the fixed printed per-creature rate and an optional
    // `quality` subtype name (CR 702.82c's "Devour [quality] N" - e.g.
    // `EntersWithCountersDevour(2, "Nightmare")`), `nil`/omitted meaning "any
    // creature," same as before this slice.
    replacement_payloads.set(
        "EntersWithCountersDevour",
        lua.create_function(|_, (per_creature, quality): (i32, Option<String>)| {
            Ok(ReplacementPayloadTag(ReplacementPayload::EntersWithCountersDevour {
                per_creature,
                quality: quality.map(Subtype),
            }))
        })?,
    )?;
    // CR 702.38a: `Game.ReplacementPayloads.EntersWithCountersAmplify(perCard)`
    // - the choice of *which* cards (if any) to reveal from hand is handled
    // entirely outside this payload (see its own doc comment) - all it
    // carries is the fixed printed per-card rate.
    replacement_payloads.set(
        "EntersWithCountersAmplify",
        lua.create_function(|_, per_card: i32| {
            Ok(ReplacementPayloadTag(ReplacementPayload::EntersWithCountersAmplify { per_card }))
        })?,
    )?;
    // CR 707.6/614.12a: `Game.ReplacementPayloads.EntersAsCopy(opts?)`
    // (Clone itself) - the choice of *which* creature (if any) to become is
    // handled entirely outside this payload (see its own doc comment); this
    // only carries `CopyExceptions`, same shape/decoder `Game.Ops.
    // BecomeCopy(opts?)` already uses (`{ power = N, toughness = N }`
    // and/or `{ keywords = { "Flying", ... } }`; omitted or `nil` is a bare
    // copy).
    replacement_payloads.set(
        "EntersAsCopy",
        lua.create_function(|_, opts: Option<Table>| {
            let exceptions = decode_copy_exceptions(opts).map_err(to_lua_err)?;
            Ok(ReplacementPayloadTag(ReplacementPayload::EntersAsCopy(exceptions)))
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
                target: None,
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
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
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
                affected: EffectFilter {
                    controller: None,
                    card_types: None,
                    subtypes: None,
                    excluded_subtypes: None,
                    supertypes: None,
                    excluded_supertypes: None,
                    self_only: false,
                },
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

    let create_mana_ability = {
        let registry = Rc::clone(&registry);
        lua.create_function(move |_, source: u64| {
            Ok(ManaAbilityBuilder {
                source: ObjectId(source),
                cost: CostSpec { mana: None, additional: Vec::new(), alternative: Vec::new() },
                operation: None,
                registry: Rc::clone(&registry),
            })
        })?
    };

    let create_cost_modifier = {
        let registry = Rc::clone(&registry);
        lua.create_function(move |_, source: u64| {
            Ok(CostModifierBuilder {
                source: ObjectId(source),
                filter: None,
                amount: None,
                target: CostModifierTarget::Generic,
                registry: Rc::clone(&registry),
            })
        })?
    };

    let create_hand_size_modifier = {
        let registry = Rc::clone(&registry);
        lua.create_function(move |_, source: u64| {
            Ok(HandSizeModifierBuilder {
                source: ObjectId(source),
                affected: None,
                effect: None,
                registry: Rc::clone(&registry),
            })
        })?
    };

    // `Game.HandSizeAffected.*` - which player(s) a `Game.CreateHandSizeModifier`
    // builder's effect applies to (`HandSizeModifierBuilder::SetAffected`).
    // `EnchantedPlayer` (CR 303.4m) is `HandSizeAffected`'s own second
    // variant, not a `FilterController` - see that enum's own doc comment.
    let hand_size_affected = lua.create_table()?;
    hand_size_affected.set("You", HandSizeAffectedTag(HandSizeAffected::Filter(FilterController::You)))?;
    hand_size_affected.set("Opponent", HandSizeAffectedTag(HandSizeAffected::Filter(FilterController::Opponent)))?;
    hand_size_affected.set("Any", HandSizeAffectedTag(HandSizeAffected::Filter(FilterController::Any)))?;
    hand_size_affected.set("EnchantedPlayer", HandSizeAffectedTag(HandSizeAffected::EnchantedPlayer))?;

    // `Game.HandSizeEffects.*` - what a `Game.CreateHandSizeModifier` builder's
    // effect actually does (`HandSizeModifierBuilder::SetEffect`).
    let hand_size_effects = lua.create_table()?;
    hand_size_effects.set("NoMaximum", HandSizeEffectTag(HandSizeEffect::NoMaximum))?;
    hand_size_effects.set(
        "SetTo",
        lua.create_function(|_, n: u32| Ok(HandSizeEffectTag(HandSizeEffect::SetTo(n))))?,
    )?;

    let create_hand_reveal_modifier = {
        let registry = Rc::clone(&registry);
        lua.create_function(move |_, source: u64| {
            Ok(HandRevealModifierBuilder { source: ObjectId(source), affected: None, registry: Rc::clone(&registry) })
        })?
    };

    // `Game.HandRevealAffected.*` - which player(s) a `Game.
    // CreateHandRevealModifier` builder applies to
    // (`HandRevealModifierBuilder::SetAffected`). Same shape as
    // `Game.HandSizeAffected.*` above, kept as a separate table - see
    // `HandRevealAffected`'s own doc comment for why the Rust type isn't
    // shared either.
    let hand_reveal_affected = lua.create_table()?;
    hand_reveal_affected.set("You", HandRevealAffectedTag(HandRevealAffected::Filter(FilterController::You)))?;
    hand_reveal_affected.set("Opponent", HandRevealAffectedTag(HandRevealAffected::Filter(FilterController::Opponent)))?;
    hand_reveal_affected.set("Any", HandRevealAffectedTag(HandRevealAffected::Filter(FilterController::Any)))?;
    hand_reveal_affected.set("EnchantedPlayer", HandRevealAffectedTag(HandRevealAffected::EnchantedPlayer))?;

    let create_draw_limit_modifier = {
        let registry = Rc::clone(&registry);
        lua.create_function(move |_, source: u64| {
            Ok(DrawLimitModifierBuilder {
                source: ObjectId(source),
                affected: None,
                limit: None,
                registry: Rc::clone(&registry),
            })
        })?
    };

    // `Game.DrawLimitAffected.*` - which player(s) a `Game.
    // CreateDrawLimitModifier` builder applies to
    // (`DrawLimitModifierBuilder::SetAffected`). No `EnchantedPlayer` entry -
    // see `DrawLimitAffected`'s own doc comment for why.
    let draw_limit_affected = lua.create_table()?;
    draw_limit_affected.set("You", DrawLimitAffectedTag(DrawLimitAffected::Filter(FilterController::You)))?;
    draw_limit_affected.set("Opponent", DrawLimitAffectedTag(DrawLimitAffected::Filter(FilterController::Opponent)))?;
    draw_limit_affected.set("Any", DrawLimitAffectedTag(DrawLimitAffected::Filter(FilterController::Any)))?;

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
    // `Game.Tokens.CopyOf(objectId, opts?)` (CR 707.10f/707.9) - see
    // `TokenSpec::CopyOf`'s own doc comment for why this token's
    // `CardDefinition` isn't built from any of the opts the other
    // `Game.Tokens.*` entries above take. `opts` is `CopyExceptions`, same
    // shape `Game.Ops.BecomeCopy`'s own `opts` takes (`decode_copy_exceptions`).
    tokens.set(
        "CopyOf",
        lua.create_function(|_, (source, opts): (u64, Option<mlua::Table>)| {
            let exceptions = decode_copy_exceptions(opts).map_err(to_lua_err)?;
            Ok(TokenTag(TokenSpec::CopyOf(ObjectId(source), exceptions)))
        })?,
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
    game.set("CreateManaAbility", create_mana_ability)?;
    game.set("CreateCostModifier", create_cost_modifier)?;
    game.set("CreateHandSizeModifier", create_hand_size_modifier)?;
    game.set("HandSizeAffected", hand_size_affected)?;
    game.set("HandSizeEffects", hand_size_effects)?;
    game.set("CreateHandRevealModifier", create_hand_reveal_modifier)?;
    game.set("HandRevealAffected", hand_reveal_affected)?;
    game.set("CreateDrawLimitModifier", create_draw_limit_modifier)?;
    game.set("DrawLimitAffected", draw_limit_affected)?;
    game.set("SelectTargets", select_targets)?;
    game.set("MayChoose", may_choose)?;
    lua.globals().set("Game", game)
}
