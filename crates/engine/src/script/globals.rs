//! Builds the `Game.*` Lua DSL and binds it into a fresh `Lua` instance's
//! globals, as one registration pass in one function.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{AnyUserData, Function, Lua, Table, Value};

use crate::types::ability::{
    AbilityEffect, ConditionPrimitive, CreatureTokenSpec, EntryControl, InterveningIfPrimitive, NamedToken, SearchDestination,
    TokenSpec, UnlessPaidConsequence,
};
use crate::types::card::{CardTypeSet, Subtype};
use crate::types::cost::{AdditionalCost, CostModifierTarget, CostSpec, FilterController};
use crate::types::effect::{
    AffectedFilter, ConsumptionPolicy, EffectDuration, EffectFilter, EffectPrimitive, Layer, ReplacementPayload,
};
use crate::types::ids::ObjectId;
use crate::types::mana::{ManaCost, ManaUnit};
use crate::types::zone::ZoneKind;
use crate::types::player::{DrawLimitAffected, HandRevealAffected, HandSizeAffected, HandSizeEffect, LandPlayAffected};

use super::builders::{
    ActivatedAbilityBuilder, CostModifierBuilder, DrawLimitModifierBuilder, HandRevealModifierBuilder,
    HandSizeModifierBuilder, LandPlayModifierBuilder, ManaAbilityBuilder, ReplacementEffectBuilder,
    StaticAbilityBuilder, TriggeredAbilityBuilder,
};
use super::decode::{
    decode_card_type_names, decode_copy_exceptions, decode_creature_token_opts, decode_effect_filter_table,
    decode_filter_table, decode_landwalk_quality, decode_subtypes, parse_card_type_name, parse_color_letter,
    parse_color_string, parse_counter_kind_name, parse_filter_controller_name, parse_keyword_name,
    parse_mana_unit_letter, parse_supertype_name, parse_zone_kind_name,
};
use super::tags::{
    ConditionTag, ConsumptionPolicyTag, CostComponent, CostTag, DrawLimitAffectedTag, DurationTag,
    EffectPrimitiveTag, EventFilterKind, EventFilterKindTag, ForEachPlayerTag, HandRevealAffectedTag,
    HandSizeAffectedTag, HandSizeEffectTag, InterveningIfTag, LandPlayAffectedTag, LayerTag, OncePerTurnTag,
    OperationTag, RedirectDamageToSourceTag, ReplacementPayloadTag, TokenTag,
};
use super::types::{to_lua_err, ScriptError, ScriptRegistry};

/// Builds `Game.Conditions`, `Ops`, `Costs`, `Layers`, `Effects` and the
/// `Create*Ability` sugar and binds it into `lua`'s globals. It is not all of
/// `script_api.rs`: `Game.ReplaceEvent`, `Game.CastSpell`, `Game.PayCost` and
/// `Game.SelectTargets` stay unbound there, and `Game.AddContinuousEffect` is
/// superseded by `Game.CreateStaticAbility`, as `register_triggered_ability` is
/// by `Game.CreateTriggeredAbility`. The sugar lets a condition or operation
/// source string be a one-liner and lets a card script self-register a
/// triggered, static or activated ability.
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
    conditions.set("MadnessExiled", ConditionTag(ConditionPrimitive::MadnessExiled))?;
    // CR 601.2i: "when you cast this spell" (self-referential). No Lua producer self-registers it, because that needs the card's script to run again on the stack (CR 601.2a), which double-registers ETB triggers; see `ConditionPrimitive::Cast`. Direct `RegisteredAbility` construction only.
    conditions.set("Cast", ConditionTag(ConditionPrimitive::Cast))?;
    // CR 702.21a: Ward's trigger event, "this permanent becomes the target of a spell or ability an opponent controls".
    conditions.set("BecomesTargetOfOpponent", ConditionTag(ConditionPrimitive::BecomesTargetOfOpponent))?;
    conditions.set("Scries", ConditionTag(ConditionPrimitive::Scries))?;
    conditions.set("EachScries", ConditionTag(ConditionPrimitive::EachScries))?;
    conditions.set("Surveils", ConditionTag(ConditionPrimitive::Surveils))?;
    conditions.set("EachSurveils", ConditionTag(ConditionPrimitive::EachSurveils))?;
    // CR 700.4, 603.10: "whenever a creature you control dies", the filter-scoped sibling of Dies and LeavesBattlefield, reusing `decode_filter_table`.
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
    // CR 603.2h: `Game.Conditions.OncePerTurn(Game.Conditions.X)` wraps any tag above so the ability triggers once each turn; see `OncePerTurnTag`.
    let once_per_turn = lua.create_function(|_, tag: AnyUserData| {
        let primitive = tag.borrow::<ConditionTag>()?.0.clone();
        Ok(OncePerTurnTag(primitive))
    })?;
    conditions.set("OncePerTurn", once_per_turn)?;

    // CR 603.4: `Game.InterveningIf.*` tags, consumed by `TriggeredAbilityBuilder:SetInterveningIf` and (CR 611.2c) `StaticAbilityBuilder:SetCondition`. A separate table from `Conditions`; see `InterveningIfTag`.
    let intervening_if = lua.create_table()?;
    let at_or_below_life = lua.create_function(|_, threshold: i32| {
        Ok(InterveningIfTag(InterveningIfPrimitive::ControllerAtOrBelowLife(threshold)))
    })?;
    intervening_if.set("AtOrBelowLife", at_or_below_life)?;
    // CR 611.2c: "as long as you control N or more [filter]", reusing `decode_filter_table`.
    let controls_at_least = lua.create_function(|_, (table, count): (Table, u32)| {
        let filter = decode_filter_table(table).map_err(to_lua_err)?;
        Ok(InterveningIfTag(InterveningIfPrimitive::ControllerControlsAtLeast { filter, count }))
    })?;
    intervening_if.set("ControlsAtLeast", controls_at_least)?;
    // CR 603.4, 702.138b: "if it escaped" (Kroxa, Uro), an ordinary ETB trigger's intervening-if.
    intervening_if.set("Escaped", InterveningIfTag(InterveningIfPrimitive::Escaped))?;
    // CR 603.4, 702.79a, 702.93a: "if it had no [kind] counters on it" (Persist, Undying), checked against the object as it died; see `InterveningIfPrimitive::HadNoCountersOfKind`.
    let had_no_counters = lua.create_function(|_, kind: String| {
        Ok(InterveningIfTag(InterveningIfPrimitive::HadNoCountersOfKind(parse_counter_kind_name(&kind))))
    })?;
    intervening_if.set("HadNoCounters", had_no_counters)?;

    let ops = lua.create_table()?;
    // CR 605: `Game.Ops.AddMana(letter, count?)`. `letter` is one WUBRG or "C" symbol and `count` (default 1) repeats that fixed unit, which covers most real mana abilities.
    // TODO: a mixed-color `AddMana` call ("add one white and one blue")
    let add_mana = lua.create_function(|_, (letter, count): (String, Option<u32>)| {
        let unit = parse_mana_unit_letter(&letter).map_err(to_lua_err)?;
        Ok(OperationTag(AbilityEffect::AddMana(vec![unit; count.unwrap_or(1) as usize])))
    })?;
    ops.set("AddMana", add_mana)?;
    // CR 106.11: `Game.Ops.AddRestrictedMana(letter, cardType, count?)`. The `AddMana` vocabulary, with every unit tagged so `PaymentPlan::validate` accepts it only toward casting a spell of `cardType` (Mishra's Workshop). Limited to one card-type filter.
    // TODO: object-specific ("only to cast the exiled card") and ability-activation restrictions
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
    // CR 605: `Game.Ops.AddManaChoice(count, colors)`. `count` is how many independently chosen mana the ability makes, and `colors` is a WUBRG string (`parse_color_string`, as in `Game.Effects.AddColor`) naming the allowed colors. The picked colors arrive later on `ActivateAbility::chosen_colors` (CR 602.2b), so this declares only the shape.
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
    // CR 701.21a: "sacrifice this permanent" - no target, always the source.
    ops.set("SacrificeSelf", OperationTag(AbilityEffect::SacrificeSelf))?;
    // CR 701.17a: mill `n` cards. The player is the controller unless the ability declares a player target (`s.target_requirement()`).
    let mill = lua.create_function(|_, n: u32| Ok(OperationTag(AbilityEffect::Mill(n))))?;
    ops.set("Mill", mill)?;
    // CR 119.3: same player selection as Mill. A negative `n` is a Lua error;
    // use `GainLife` for signed amounts.
    let lose_life = lua.create_function(|_, n: u32| Ok(OperationTag(AbilityEffect::LoseLife(n))))?;
    ops.set("LoseLife", lose_life)?;
    ops.set("Exile", OperationTag(AbilityEffect::Exile))?;
    // CR 610.3: targeted exile that returns when the ability's source leaves.
    ops.set("ExileUntilSourceLeaves", OperationTag(AbilityEffect::ExileUntilSourceLeaves))?;
    ops.set("ReturnToHand", OperationTag(AbilityEffect::ReturnToHand))?;
    ops.set("ReturnSelfToHand", OperationTag(AbilityEffect::ReturnSelfToHand))?;
    // CR 400.3: `zone` defaults to "Battlefield"; only the zones
    // `ScriptedZoneMoveKind::Bounce` accepts as a source are allowed.
    let return_all_to_hand = lua.create_function(|_, (table, zone): (Table, Option<String>)| {
        let filter = decode_effect_filter_table(&table).map_err(to_lua_err)?;
        let zone = match zone {
            None => ZoneKind::Battlefield,
            Some(name) => match crate::script::decode::parse_zone_kind_name(&name).map_err(to_lua_err)? {
                z @ (ZoneKind::Battlefield | ZoneKind::Graveyard | ZoneKind::Exile) => z,
                other => {
                    return Err(mlua::Error::RuntimeError(format!(
                        "ReturnAllToHand zone must be \"Battlefield\", \"Graveyard\" or \"Exile\", got {other:?}"
                    )))
                }
            },
        };
        Ok(OperationTag(AbilityEffect::ReturnAllToHand { filter, zone }))
    })?;
    ops.set("ReturnAllToHand", return_all_to_hand)?;
    // CR 110.2a/400.7: reanimation. Both are call-form ops: `opts` is an
    // optional `{ control = "You" | "Owner", tapped = bool }` table.
    let return_to_battlefield = lua.create_function(|_, opts: Option<Table>| {
        let (control, tapped) = decode_entry_options(opts, "ReturnToBattlefield")?;
        Ok(OperationTag(AbilityEffect::ReturnToBattlefield { control, tapped }))
    })?;
    ops.set("ReturnToBattlefield", return_to_battlefield)?;
    // `zone` defaults to "Graveyard"; only the zones
    // `ScriptedZoneMoveKind::Reanimate` accepts as a source are allowed.
    let return_all_to_battlefield =
        lua.create_function(|_, (table, zone, opts): (Table, Option<String>, Option<Table>)| {
            let filter = decode_effect_filter_table(&table).map_err(to_lua_err)?;
            let zone = match zone {
                None => ZoneKind::Graveyard,
                Some(name) => match crate::script::decode::parse_zone_kind_name(&name).map_err(to_lua_err)? {
                    z @ (ZoneKind::Graveyard | ZoneKind::Exile) => z,
                    other => {
                        return Err(mlua::Error::RuntimeError(format!(
                            "ReturnAllToBattlefield zone must be \"Graveyard\" or \"Exile\", got {other:?}"
                        )))
                    }
                },
            };
            let (control, tapped) = decode_entry_options(opts, "ReturnAllToBattlefield")?;
            Ok(OperationTag(AbilityEffect::ReturnAllToBattlefield { filter, zone, control, tapped }))
        })?;
    ops.set("ReturnAllToBattlefield", return_all_to_battlefield)?;
    ops.set("ExileSelf", OperationTag(AbilityEffect::ExileSelf))?;
    // CR 406.2: `zone` defaults to "Battlefield"; only the zones
    // `ScriptedZoneMoveKind::Exile` accepts as a source are allowed.
    let exile_all = lua.create_function(|_, (table, zone): (Table, Option<String>)| {
        let filter = decode_effect_filter_table(&table).map_err(to_lua_err)?;
        let zone = match zone {
            None => ZoneKind::Battlefield,
            Some(name) => match crate::script::decode::parse_zone_kind_name(&name).map_err(to_lua_err)? {
                z @ (ZoneKind::Battlefield | ZoneKind::Graveyard) => z,
                other => {
                    return Err(mlua::Error::RuntimeError(format!("ExileAll zone must be \"Battlefield\" or \"Graveyard\", got {other:?}")))
                }
            },
        };
        Ok(OperationTag(AbilityEffect::ExileAll { filter, zone }))
    })?;
    ops.set("ExileAll", exile_all)?;
    // CR 701.26: tap/untap. Battlefield only (CR 110.5d), so the `All`
    // forms take just the filter, no zone argument.
    ops.set("Tap", OperationTag(AbilityEffect::Tap))?;
    ops.set("Untap", OperationTag(AbilityEffect::Untap))?;
    ops.set("TapSelf", OperationTag(AbilityEffect::TapSelf))?;
    ops.set("UntapSelf", OperationTag(AbilityEffect::UntapSelf))?;
    let tap_all = lua.create_function(|_, table: Table| {
        let filter = decode_effect_filter_table(&table).map_err(to_lua_err)?;
        Ok(OperationTag(AbilityEffect::TapAll { filter }))
    })?;
    ops.set("TapAll", tap_all)?;
    let untap_all = lua.create_function(|_, table: Table| {
        let filter = decode_effect_filter_table(&table).map_err(to_lua_err)?;
        Ok(OperationTag(AbilityEffect::UntapAll { filter }))
    })?;
    ops.set("UntapAll", untap_all)?;
    // CR 701.6a: "counter target spell". The target comes from `s.target_requirement()` (a filter with `zone = "Stack"`, see `AbilityEffect::Counter`), not a parameter.
    ops.set("Counter", OperationTag(AbilityEffect::Counter))?;
    // CR 701.6a, 118.12: "counter target spell unless that player pays [cost]". Same target declaration as Counter, plus the mana-cost string (`ManaCost::parse` grammar). The Lua name stays `CounterUnlessPaid`, which is how card authors read it.
    // TODO: Condescend's {X} amount (see `AbilityEffect::UnlessPaid`)
    let counter_unless_paid = lua.create_function(|_, s: String| {
        let cost = ManaCost::parse(&s)
            .map_err(|e| mlua::Error::RuntimeError(format!("Game.Ops.CounterUnlessPaid(\"{s}\"): {}", e.0)))?;
        Ok(OperationTag(AbilityEffect::UnlessPaid {
            cost: CostSpec::mana_only(cost),
            otherwise: UnlessPaidConsequence::Counter,
        }))
    })?;
    ops.set("CounterUnlessPaid", counter_unless_paid)?;
    // `Game.Ops.SacrificeRandom(filter, count)` (CR 701.21, at random), decoding the same `EffectFilter` table as DestroyAll.
    let sacrifice_random = lua.create_function(|_, (table, count): (Table, u32)| {
        let filter = decode_effect_filter_table(&table).map_err(to_lua_err)?;
        Ok(OperationTag(AbilityEffect::SacrificeRandom { filter, count }))
    })?;
    ops.set("SacrificeRandom", sacrifice_random)?;
    // `Game.Ops.DiscardRandom(filter, count)` (CR 701.9b), decoding the same table as SacrificeRandom.
    let discard_random = lua.create_function(|_, (table, count): (Table, u32)| {
        let filter = decode_effect_filter_table(&table).map_err(to_lua_err)?;
        Ok(OperationTag(AbilityEffect::DiscardRandom { filter, count }))
    })?;
    ops.set("DiscardRandom", discard_random)?;
    // `Game.Ops.Discard(count)` (CR 701.9, 701.9b): "target player discards `count` cards", the player's own choice. The target comes from `s.target_requirement()` (a single `TargetFilter::Player`), as for `Counter` and `UnlessPaid`.
    let discard = lua.create_function(|_, count: u32| Ok(OperationTag(AbilityEffect::Discard { count })))?;
    ops.set("Discard", discard)?;
    // `Game.Ops.ForEachPlayer(x)` (CR 101.4). `x` is a per-card Lua function, registered into the script registry at once as the `Value::Function` arm of `decode_operation_value` does for a bare `:SetOperation(function...)`, or a bare tag unwrapped from its `OperationTag` with nothing to register. Anything else is a load-time error and not a silent no-op; see `ForEachPlayerTag`.
    let for_each_player = {
        let registry = Rc::clone(&registry);
        lua.create_function(move |lua, value: Value| match value {
            Value::Function(func) => {
                let key = lua.create_registry_value(func)?;
                let mut reg = registry.borrow_mut();
                let handle = reg.next_script_handle();
                reg.scripts.insert(handle, key);
                Ok(ForEachPlayerTag::Function(handle))
            }
            Value::UserData(ud) => {
                let tag: OperationTag = ud
                    .borrow::<OperationTag>()
                    .map_err(|_| {
                        mlua::Error::RuntimeError(
                            "Game.Ops.ForEachPlayer expects a function or a Game.Ops.SacrificeRandom/DiscardRandom/Mill/LoseLife \
                             value"
                                .to_string(),
                        )
                    })?
                    .clone();
                match &tag.0 {
                    AbilityEffect::SacrificeRandom { .. }
                    | AbilityEffect::DiscardRandom { .. }
                    | AbilityEffect::Mill(_)
                    | AbilityEffect::LoseLife(_) => {
                        Ok(ForEachPlayerTag::Static(tag.0.clone()))
                    }
                    _ => Err(mlua::Error::RuntimeError(
                        "Game.Ops.ForEachPlayer can only wrap a function or a bare SacrificeRandom/DiscardRandom/Mill/LoseLife \
                         value directly - wrap any other Game.Ops.* primitive in a function instead"
                            .to_string(),
                    )),
                }
            }
            other => Err(mlua::Error::RuntimeError(format!(
                "Game.Ops.ForEachPlayer expects a function or a Game.Ops.* value, got {}",
                other.type_name()
            ))),
        })?
    };
    ops.set("ForEachPlayer", for_each_player)?;
    // `Game.Ops.Chain(a, b, ...)` (CR 608.2c): several `Game.Ops.*` values in the order written, all sharing the ability's whole target set. See `chain_entry` for what an entry may be.
    let chain = lua.create_function(|lua, args: mlua::Variadic<Value>| {
        if args.is_empty() {
            return Err(mlua::Error::RuntimeError(
                "Game.Ops.Chain needs at least one Game.Ops.* value".to_string(),
            ));
        }
        let mut entries = Vec::with_capacity(args.len());
        for (index, arg) in args.into_iter().enumerate() {
            entries.push(chain_entry(lua, "Chain", index + 1, arg)?);
        }
        Ok(OperationTag(AbilityEffect::Chain { entries, words: None }))
    })?;
    ops.set("Chain", chain)?;
    // `Game.Ops.ChainEach({ op, words }, ...)`: the same, but entry i owns `words` consecutive target words (0 for an entry that acts on its controller). `check_chain_words` verifies the counts when the ability registers or the spell loads, because the operation and the target requirement are declared separately.
    let chain_each = lua.create_function(|lua, args: mlua::Variadic<Value>| {
        if args.is_empty() {
            return Err(mlua::Error::RuntimeError(
                "Game.Ops.ChainEach needs at least one { op, words } pair".to_string(),
            ));
        }
        let mut entries = Vec::with_capacity(args.len());
        let mut words = Vec::with_capacity(args.len());
        for (index, arg) in args.into_iter().enumerate() {
            let position = index + 1;
            let Value::Table(pair) = arg else {
                return Err(mlua::Error::RuntimeError(format!(
                    "Game.Ops.ChainEach argument {position} must be a {{ op, words }} table, got {}",
                    arg.type_name()
                )));
            };
            let op: Value = pair.get(1)?;
            let count: Value = pair.get(2)?;
            let count = match count {
                Value::Integer(n) if n >= 0 => n as usize,
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                other => {
                    return Err(mlua::Error::RuntimeError(format!(
                        "Game.Ops.ChainEach argument {position}: words must be a whole number of 0 or more, got {}",
                        other.type_name()
                    )));
                }
            };
            entries.push(chain_entry(lua, "ChainEach", position, op)?);
            words.push(count);
        }
        Ok(OperationTag(AbilityEffect::Chain { entries, words: Some(words) }))
    })?;
    ops.set("ChainEach", chain_each)?;
    let counters = lua.create_function(|_, (kind, amount): (String, i32)| {
        Ok(OperationTag(AbilityEffect::Counters { kind: parse_counter_kind_name(&kind), amount }))
    })?;
    ops.set("Counters", counters)?;
    // CR 702.79a, 702.93a: `Game.Ops.ReturnSelfWithCounter(kind)`, Persist and Undying's resolution effect, paired with `Game.Conditions.Dies` and `Game.InterveningIf.HadNoCounters(kind)` (see `AbilityEffect::ReturnSelfWithCounter`).
    let return_self_with_counter = lua.create_function(|_, kind: String| {
        Ok(OperationTag(AbilityEffect::ReturnSelfWithCounter(parse_counter_kind_name(&kind))))
    })?;
    ops.set("ReturnSelfWithCounter", return_self_with_counter)?;
    // `Game.Ops.Pump(power, toughness, duration)`; `duration` is a `Game.Durations.*` tag (see `DurationTag`).
    let pump = lua.create_function(|_, (power, toughness, duration): (i32, i32, AnyUserData)| {
        let duration = duration.borrow::<DurationTag>()?.0.clone();
        Ok(OperationTag(AbilityEffect::Pump { power, toughness, duration }))
    })?;
    ops.set("Pump", pump)?;
    // `Game.Ops.GainControl(duration)`, with Pump's `Game.Durations.*` decode. No player argument, since the new controller is always the resolving object's controller (see `AbilityEffect::GainControl`).
    let gain_control = lua.create_function(|_, duration: AnyUserData| {
        let duration = duration.borrow::<DurationTag>()?.0.clone();
        Ok(OperationTag(AbilityEffect::GainControl(duration)))
    })?;
    ops.set("GainControl", gain_control)?;
    // `Game.Ops.Regenerate()` (CR 701.19a): a no-argument toggle like `Destroy`.
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
    // `Game.Ops.Search(filter_table, count, opts?)`. `filter_table` goes through `decode_filter_table`, as `Game.Costs.Sacrifice` does. `opts.destination` is `"hand"` (default) or `"battlefield"`, and `opts.tapped` and `opts.enters_attached` matter only for `"battlefield"` (CR 303.4f-h, meaningful for a found Aura; see `SearchDestination::Battlefield`). `opts.shuffle` defaults to `true` (CR 701.24a).
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
    // `Game.Ops.BecomeCopy(opts?)` (CR 707.2, 707.9, layer 1). The copy target is what the ability's targeting resolved, not a value passed here. `opts` is `CopyExceptions`: `{ power = N, toughness = N }` (CR 707.9b) and/or `{ keywords = { "Flying", ... } }` (CR 707.9a). Omitted or `nil` is a bare copy.
    let become_copy = lua.create_function(|_, opts: Option<Table>| {
        let exceptions = decode_copy_exceptions(opts).map_err(to_lua_err)?;
        Ok(OperationTag(AbilityEffect::BecomeCopy(exceptions)))
    })?;
    ops.set("BecomeCopy", become_copy)?;
    // `Game.Ops.Attach` (CR 702.6a, Equip): a no-argument toggle. The attach target is what the activated ability's `:SetTarget` resolved.
    ops.set("Attach", OperationTag(AbilityEffect::Attach))?;
    // `Game.Ops.ExchangeTextBoxes` (CR 612.5): a no-argument toggle. The two targets are a `TargetSpec::Multiple` of exactly two entries from the ability's `:SetTarget` or `target_requirement`.
    ops.set("ExchangeTextBoxes", OperationTag(AbilityEffect::ExchangeTextBoxes))?;
    // `Game.Ops.Fight` (CR 701.14) and `Game.Ops.Bite` (one-sided) are bare tags. The creatures come from the ability's target spec: two words (first fights or bites second), one count-2 word (Fight only), or one count-1 word (the source is the first creature).
    ops.set("Fight", OperationTag(AbilityEffect::Fight))?;
    ops.set("Bite", OperationTag(AbilityEffect::Bite))?;
    // `Game.Ops.MadnessCastOrDiscard` (CR 702.35a): a no-argument toggle. The exiled card is found through `ObjectLineage` off the triggered ability's source (see `AbilityEffect::MadnessCastOrDiscard`).
    ops.set("MadnessCastOrDiscard", OperationTag(AbilityEffect::MadnessCastOrDiscard))?;
    // `Game.Ops.Cascade` (CR 702.85a): a no-argument toggle. The spell's mana value is derived from `top.source` and `top.x_value` at resolution.
    ops.set("Cascade", OperationTag(AbilityEffect::Cascade))?;

    let costs = lua.create_table()?;
    costs.set("TapSelf", CostTag(CostComponent::Additional(AdditionalCost::TapSelf)))?;
    // CR 702.29a Cycling's "Discard this card": always the ability's own source, with no filter table (unlike the filter-driven Discard binding below).
    costs.set("DiscardSelf", CostTag(CostComponent::Additional(AdditionalCost::DiscardSelf)))?;
    // "Sacrifice this permanent": always the ability's own source, with no filter table (unlike the filter-driven Sacrifice binding below).
    costs.set("SacrificeSelf", CostTag(CostComponent::Additional(AdditionalCost::SacrificeSelf)))?;
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
    // CR 107.4, 202.3: a printed mana-cost string, such as `Game.Costs.Mana("{1}{R}")`; see `ManaCost::parse` for the grammar.
    costs.set(
        "Mana",
        lua.create_function(|_, s: String| {
            let mana = ManaCost::parse(&s)
                .map_err(|e| mlua::Error::RuntimeError(format!("Game.Costs.Mana(\"{s}\"): {}", e.0)))?;
            Ok(CostTag(CostComponent::Mana(mana)))
        })?,
    )?;

    // Only the 4 durations with a drain path are exposed; see `DurationTag`.
    let durations = lua.create_table()?;
    durations.set("UntilEndOfTurn", DurationTag(EffectDuration::UntilEndOfTurn))?;
    durations.set("UntilYourNextTurn", DurationTag(EffectDuration::UntilYourNextTurn))?;
    durations.set("UntilEndOfCombat", DurationTag(EffectDuration::UntilEndOfCombat))?;
    durations.set("Permanent", DurationTag(EffectDuration::Permanent))?;

    // Only the 7 layers with a query path (`layers::effective_characteristics`) are exposed. A static ability can't request layer 1, 2 or 7d (see `Layer`).
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
        "IgnoreKeyword",
        lua.create_function(|_, name: String| {
            let set = parse_keyword_name(&name).map_err(to_lua_err)?;
            Ok(EffectPrimitiveTag(EffectPrimitive::IgnoreKeyword(set)))
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
        "SetCardTypes",
        lua.create_function(|_, val: Value| {
            let names = decode_card_type_names(val).map_err(to_lua_err)?;
            let mut set = CardTypeSet::empty();
            for name in names {
                set |= parse_card_type_name(&name).map_err(to_lua_err)?;
            }
            Ok(EffectPrimitiveTag(EffectPrimitive::SetCardTypes(set)))
        })?,
    )?;
    effects.set(
        "AddColor",
        lua.create_function(|_, letter: String| {
            let set = parse_color_letter(&letter).map_err(to_lua_err)?;
            Ok(EffectPrimitiveTag(EffectPrimitive::AddColor(set)))
        })?,
    )?;
    effects.set(
        "SetColors",
        lua.create_function(|_, letters: String| {
            let set = parse_color_string(&letters).map_err(to_lua_err)?;
            Ok(EffectPrimitiveTag(EffectPrimitive::SetColors(set)))
        })?,
    )?;
    effects.set(
        "AddSupertype",
        lua.create_function(|_, name: String| {
            let set = parse_supertype_name(&name).map_err(to_lua_err)?;
            Ok(EffectPrimitiveTag(EffectPrimitive::AddSupertype(set)))
        })?,
    )?;
    effects.set(
        "AddSubtype",
        lua.create_function(|_, name: String| Ok(EffectPrimitiveTag(EffectPrimitive::AddSubtype(vec![Subtype(name)]))))?,
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

    // `Game.EventFilters.*`: which `EventFilter` shape a ReplacementEffect watches (`ReplacementEffectBuilder::SetFilter`). Only shapes with a query path (`ReplacementEffect::matches`) are bound; see `EventFilter`.
    let event_filters = lua.create_table()?;
    event_filters.set("DamageDealt", EventFilterKindTag(EventFilterKind::DamageDealt))?;
    event_filters.set("WouldDie", EventFilterKindTag(EventFilterKind::WouldDie))?;
    // `Game.EventFilters.WouldMoveZone(fromName, toName)`: either argument may be nil for any zone, so `WouldMoveZone("Battlefield", nil)` matches any move off the battlefield and `WouldMoveZone(nil, "Exile")` any move into exile.
    event_filters.set(
        "WouldMoveZone",
        lua.create_function(|_, (from_name, to_name): (Option<String>, Option<String>)| {
            let from = from_name.as_deref().map(parse_zone_kind_name).transpose().map_err(to_lua_err)?;
            let to = to_name.as_deref().map(parse_zone_kind_name).transpose().map_err(to_lua_err)?;
            Ok(EventFilterKindTag(EventFilterKind::WouldMoveZone { from, to }))
        })?,
    )?;
    // CR 702.89a (Umbra armor): "if enchanted permanent would be destroyed", pairing only with `Game.ReplacementPayloads.UmbraArmor`. `:SetAffected` is a no-op, since the affected object is always this effect's `attached_to`.
    event_filters.set("WouldDieEnchanted", EventFilterKindTag(EventFilterKind::WouldDieEnchanted))?;
    // CR 303.4m, 616 (Wheel of Sun and Moon): "if a card would be put into enchanted player's graveyard from anywhere", pairing only with `Game.ReplacementPayloads.ReplaceZoneDestination`. `:SetAffected` is a no-op here too.
    event_filters.set(
        "WouldEnterEnchantedPlayerGraveyard",
        EventFilterKindTag(EventFilterKind::WouldEnterEnchantedPlayerGraveyard),
    )?;

    // `Game.ReplacementPayloads.*`: what a ReplacementEffect rewrites the matched event into (`ReplacementEffectBuilder::SetPayload`). `RedirectDamageToSource` is a sentinel and not a `ReplacementPayloadTag`; see `RedirectDamageToSourceTag`.
    let replacement_payloads = lua.create_table()?;
    replacement_payloads.set("PreventDamage", ReplacementPayloadTag(ReplacementPayload::PreventDamage))?;
    replacement_payloads.set("Regenerate", ReplacementPayloadTag(ReplacementPayload::Regenerate))?;
    // CR 702.89a (Umbra armor): pairs only with `Game.EventFilters.WouldDieEnchanted`.
    replacement_payloads.set("UmbraArmor", ReplacementPayloadTag(ReplacementPayload::UmbraArmor))?;
    replacement_payloads.set("RedirectDamageToSource", RedirectDamageToSourceTag)?;
    replacement_payloads.set(
        "ReplaceZoneDestination",
        lua.create_function(|_, zone_name: String| {
            let zone = parse_zone_kind_name(&zone_name).map_err(to_lua_err)?;
            Ok(ReplacementPayloadTag(ReplacementPayload::ReplaceZoneDestination(zone)))
        })?,
    )?;
    // CR 603.6d: pairs only with `Game.EventFilters.WouldMoveZone(_, "Battlefield")` (see `validate_replacement_filter_payload_pairing`). Applied by `replacement::apply_enters_battlefield_modifiers`, not `ReplacementEffect::apply`.
    replacement_payloads.set("EntersTapped", ReplacementPayloadTag(ReplacementPayload::EntersTapped))?;
    // CR 614.1c, 122.6: `Game.ReplacementPayloads.EntersWithCounters(kindName, amount)`, fixed printed amounts only, with the `parse_counter_kind_name` table of `Game.Ops.Counters`. Sunburst has its own payload. Devour and Amplify's fixed per-unit rate fits, but choosing which units count needs a pause; see `ReplacementPayload::EntersWithCounters`.
    replacement_payloads.set(
        "EntersWithCounters",
        lua.create_function(|_, (kind_name, amount): (String, i32)| {
            let kind = parse_counter_kind_name(&kind_name);
            Ok(ReplacementPayloadTag(ReplacementPayload::EntersWithCounters { kind, amount }))
        })?,
    )?;
    // CR 702.54a: `EntersWithCountersBloodthirst(kindName, amount)`, the fixed-amount shape above gated on `replacement::opponent_damaged_this_turn`.
    replacement_payloads.set(
        "EntersWithCountersBloodthirst",
        lua.create_function(|_, (kind_name, amount): (String, i32)| {
            let kind = parse_counter_kind_name(&kind_name);
            Ok(ReplacementPayloadTag(ReplacementPayload::EntersWithCountersBloodthirst { kind, amount }))
        })?,
    )?;
    // CR 702.138c: `EntersWithCountersEscape(kindName, amount)`, gated on `GameObject::entered_via_escape`.
    replacement_payloads.set(
        "EntersWithCountersEscape",
        lua.create_function(|_, (kind_name, amount): (String, i32)| {
            let kind = parse_counter_kind_name(&kind_name);
            Ok(ReplacementPayloadTag(ReplacementPayload::EntersWithCountersEscape { kind, amount }))
        })?,
    )?;
    // CR 702.44: `EntersWithCountersSunburst` takes no arguments, since kind and amount are derived at apply time.
    replacement_payloads.set("EntersWithCountersSunburst", ReplacementPayloadTag(ReplacementPayload::EntersWithCountersSunburst))?;
    // CR 702.82a, 702.82c: `EntersWithCountersDevour(perCreature, quality)`. Which creatures to sacrifice is decided outside this payload, which carries the fixed per-creature rate and an optional `quality` subtype ("Devour [quality] N", such as `(2, "Nightmare")`). `nil` means any creature.
    replacement_payloads.set(
        "EntersWithCountersDevour",
        lua.create_function(|_, (per_creature, quality): (i32, Option<String>)| {
            Ok(ReplacementPayloadTag(ReplacementPayload::EntersWithCountersDevour {
                per_creature,
                quality: quality.map(Subtype),
            }))
        })?,
    )?;
    // CR 702.38a: `EntersWithCountersAmplify(perCard)`. Which cards to reveal is decided outside this payload, which carries the fixed per-card rate.
    replacement_payloads.set(
        "EntersWithCountersAmplify",
        lua.create_function(|_, per_card: i32| {
            Ok(ReplacementPayloadTag(ReplacementPayload::EntersWithCountersAmplify { per_card }))
        })?,
    )?;
    // CR 707.6, 614.12a: `EntersAsCopy(opts?)` (Clone). Which creature to become is decided outside this payload, which carries only `CopyExceptions`, decoded as `Game.Ops.BecomeCopy(opts?)` does.
    replacement_payloads.set(
        "EntersAsCopy",
        lua.create_function(|_, opts: Option<Table>| {
            let exceptions = decode_copy_exceptions(opts).map_err(to_lua_err)?;
            Ok(ReplacementPayloadTag(ReplacementPayload::EntersAsCopy(exceptions)))
        })?,
    )?;

    // `Game.Consumption.*`: a ReplacementEffect's `ConsumptionPolicy` (`ReplacementEffectBuilder::SetConsumption`).
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
                function_zone: None,
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
                characteristic_defining: false,
                function_zone: None,
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
                function_zone: None,
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
                function_zone: None,
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
                function_zone: None,
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
                function_zone: None,
                registry: Rc::clone(&registry),
            })
        })?
    };

    // `Game.HandSizeAffected.*`: which players a `Game.CreateHandSizeModifier` applies to (`HandSizeModifierBuilder::SetAffected`). `EnchantedPlayer` (CR 303.4m) is `HandSizeAffected`'s second variant, not a `FilterController`.
    let hand_size_affected = lua.create_table()?;
    hand_size_affected.set("You", HandSizeAffectedTag(HandSizeAffected::Filter(FilterController::You)))?;
    hand_size_affected.set("Opponent", HandSizeAffectedTag(HandSizeAffected::Filter(FilterController::Opponent)))?;
    hand_size_affected.set("Any", HandSizeAffectedTag(HandSizeAffected::Filter(FilterController::Any)))?;
    hand_size_affected.set("EnchantedPlayer", HandSizeAffectedTag(HandSizeAffected::EnchantedPlayer))?;

    // `Game.HandSizeEffects.*`: what a `Game.CreateHandSizeModifier` does (`HandSizeModifierBuilder::SetEffect`).
    let hand_size_effects = lua.create_table()?;
    hand_size_effects.set("NoMaximum", HandSizeEffectTag(HandSizeEffect::NoMaximum))?;
    hand_size_effects.set(
        "SetTo",
        lua.create_function(|_, n: u32| Ok(HandSizeEffectTag(HandSizeEffect::SetTo(n))))?,
    )?;

    let create_hand_reveal_modifier = {
        let registry = Rc::clone(&registry);
        lua.create_function(move |_, source: u64| {
            Ok(HandRevealModifierBuilder { source: ObjectId(source), affected: None, function_zone: None, registry: Rc::clone(&registry) })
        })?
    };

    // `Game.HandRevealAffected.*`: which players a `Game.CreateHandRevealModifier` applies to (`HandRevealModifierBuilder::SetAffected`). The shape of `Game.HandSizeAffected.*`, in a separate table; see `HandRevealAffected`.
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
                function_zone: None,
                registry: Rc::clone(&registry),
            })
        })?
    };

    // `Game.DrawLimitAffected.*`: which players a `Game.CreateDrawLimitModifier` applies to (`DrawLimitModifierBuilder::SetAffected`), with no `EnchantedPlayer`; see `DrawLimitAffected`.
    let draw_limit_affected = lua.create_table()?;
    draw_limit_affected.set("You", DrawLimitAffectedTag(DrawLimitAffected::Filter(FilterController::You)))?;
    draw_limit_affected.set("Opponent", DrawLimitAffectedTag(DrawLimitAffected::Filter(FilterController::Opponent)))?;
    draw_limit_affected.set("Any", DrawLimitAffectedTag(DrawLimitAffected::Filter(FilterController::Any)))?;

    let create_land_play_modifier = {
        let registry = Rc::clone(&registry);
        lua.create_function(move |_, source: u64| {
            Ok(LandPlayModifierBuilder {
                source: ObjectId(source),
                affected: None,
                additional: None,
                function_zone: None,
                registry: Rc::clone(&registry),
            })
        })?
    };

    // `Game.LandPlayAffected.*`: which players a `Game.CreateLandPlayModifier` applies to (`LandPlayModifierBuilder::SetAffected`), with no `EnchantedPlayer`; see `LandPlayAffected`.
    let land_play_affected = lua.create_table()?;
    land_play_affected.set("You", LandPlayAffectedTag(LandPlayAffected::Filter(FilterController::You)))?;
    land_play_affected.set("Opponent", LandPlayAffectedTag(LandPlayAffected::Filter(FilterController::Opponent)))?;
    land_play_affected.set("Any", LandPlayAffectedTag(LandPlayAffected::Filter(FilterController::Any)))?;

    let tokens = lua.create_table()?;
    tokens.set("Clue", TokenTag(TokenSpec::Named(NamedToken::Clue)))?;
    tokens.set("Food", TokenTag(TokenSpec::Named(NamedToken::Food)))?;
    tokens.set("Treasure", TokenTag(TokenSpec::Named(NamedToken::Treasure)))?;
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
    // `Game.Tokens.CopyOf(objectId, opts?)` (CR 707.10f, 707.9): see `TokenSpec::CopyOf` for why the token's `CardDefinition` ignores the opts of the other `Game.Tokens.*`. `opts` is `CopyExceptions`, decoded by `decode_copy_exceptions`.
    tokens.set(
        "CopyOf",
        lua.create_function(|_, (source, opts): (u64, Option<mlua::Table>)| {
            let exceptions = decode_copy_exceptions(opts).map_err(to_lua_err)?;
            Ok(TokenTag(TokenSpec::CopyOf(ObjectId(source), exceptions)))
        })?,
    )?;

    // `Game.SelectTargets(filter)` yields its request to Lua's own `coroutine.yield`, not a `create_function` closure: mlua 0.9 without the `async` feature lets only code running as a coroutine call `coroutine.yield`, so a Rust-bound function can't suspend the Lua stack. A literal Lua definition avoids that (`coroutine_spike.rs` confirms a `coroutine.yield` inside a `Thread` round-trips values). `ScriptRuntime::invoke_operation` and `resume_operation` decode the yielded table and drive the pause and resume. This binding only produces the request.
    let select_targets: Function = lua
        .load(r#"function(filter) return coroutine.yield({ op = "select_target", filter = filter or {} }) end"#)
        .eval()?;

    // CR 608.2d: `Game.MayChoose()` yields a plain yes/no request as `Game.SelectTargets` does, as a literal Lua function for the same reason. `resume_operation_bool` resumes with a raw boolean, which becomes the call's return value.
    let may_choose: Function =
        lua.load(r#"function() return coroutine.yield({ op = "may_choose" }) end"#).eval()?;

    // `Game.ChooseN(filter, n, opts)`: ask the player to choose N objects mid-resolution, built from the two yields above. Each pick makes its own `Game.SelectTargets`-style ask and grows an `exclude_ids` list (see `ScriptChoiceRequest::SelectTarget`) so one object isn't picked twice. Returns a plain array of `{kind, id}` tables, as `Game.SelectTargets` does. `Game.Ops.SacrificeChosen` and `DiscardChosen` consume it, and a script can read `.id` off each entry.
    //
    // `opts.up_to` and `opts.at_least` mirror `TargetRequirement`'s count modes (CR 601.2c, 115.6). At most one should be `true`, with no Lua-side validation, and `at_least` wins if both are:
    // - neither: exactly `n`, with no yes/no ask.
    // - `up_to`: 0 to `n`, asking "one more?" before every pick (the first included) and stopping on `false`.
    // - `at_least`: `n` or more with no upper bound beyond the legal candidates. The first `n` picks are mandatory, and "one more?" is asked before each later pick until declined.
    let choose_n: Function = lua
        .load(
            r#"
            function(filter, n, opts)
                opts = opts or {}
                local results = {}
                local ids = {}
                local i = 1
                while true do
                    local past_minimum = i > n
                    if past_minimum and not opts.at_least then break end
                    if (opts.up_to and not opts.at_least) or (opts.at_least and past_minimum) then
                        local more = coroutine.yield({ op = "may_choose" })
                        if not more then break end
                    end
                    local picked = coroutine.yield({ op = "select_target", filter = filter or {}, exclude_ids = ids })
                    table.insert(results, picked)
                    if picked.kind == "object" then
                        table.insert(ids, picked.id)
                    end
                    i = i + 1
                end
                return results
            end
            "#,
        )
        .eval()?;

    // `Game.Ops.SacrificeChosen(list)` and `DiscardChosen(list)` (CR 701.21, 701.9): take `Game.ChooseN`'s return shape and build the `sacrifice_chosen` or `discard_chosen` result key that `ScriptRuntime::handle_operation_result` decodes into `OperationOutcome::PendingZoneMoves` (see it for why this can't mutate `GameState` as `{ gain_life = n }` does). Plain functions and not yields, so `e:SetOperation(function(...) ... return Game.Ops.SacrificeChosen(ids) end)` returns normally.
    let sacrifice_chosen: Function = lua
        .load(
            r#"
            function(list)
                local ids = {}
                for _, v in ipairs(list) do
                    if v.kind == "object" then table.insert(ids, v.id) end
                end
                return { sacrifice_chosen = ids }
            end
            "#,
        )
        .eval()?;
    ops.set("SacrificeChosen", sacrifice_chosen)?;
    let discard_chosen: Function = lua
        .load(
            r#"
            function(list)
                local ids = {}
                for _, v in ipairs(list) do
                    if v.kind == "object" then table.insert(ids, v.id) end
                end
                return { discard_chosen = ids }
            end
            "#,
        )
        .eval()?;
    ops.set("DiscardChosen", discard_chosen)?;

    // `Game.Ops.CastSpell(card, opts)` (CR 601.2, 702.85a-style "cast this without paying its mana cost"), reshaped into a result-table key that `handle_operation_result` decodes into an `OperationOutcome` the engine acts on, like `SacrificeChosen` (see `OperationOutcome::PendingCast`). `opts.targets` takes `Game.SelectTargets` or `Game.ChooseN` picks (`{kind, id}` tables) for the cast spell's targets, gathered by the calling script first. `opts.free` (default `false`) is CR 702.85a's "without paying its mana cost".
    // TODO: `opts.free` hardcodes casting from exile (see `OperationOutcome::PendingCast`), so only Cascade-shaped "free from exile" works, not an arbitrary zone
    let cast_spell: Function = lua
        .load(
            r#"
            function(card, opts)
                opts = opts or {}
                return {
                    cast_spell = {
                        card = card,
                        free = opts.free or false,
                        modes = opts.modes or {},
                        targets = opts.targets or {},
                        x_value = opts.x_value,
                    }
                }
            end
            "#,
        )
        .eval()?;
    ops.set("CastSpell", cast_spell)?;

    // `Game.RegisterKeyword(source, name)`: sugar for the layer-6 (`AbilityAdding`) self-only static ability every printed keyword needs (CR 613.1f). Equivalent to:
    //   local e = Game.CreateStaticAbility(source)
    //   e:SetLayer(Game.Layers.AbilityAdding)
    //   e:SetEffect(Game.Effects.AddKeyword(name))
    //   e:SetAffected({ self_only = true })
    //   e:Register()
    // It resolves `Game.*` at call time, so it must run after `Game` is assigned to the Lua globals below.
    let register_keyword: Function = lua
        .load(
            r#"
            function(source, name)
                local e = Game.CreateStaticAbility(source)
                e:SetLayer(Game.Layers.AbilityAdding)
                e:SetEffect(Game.Effects.AddKeyword(name))
                e:SetAffected({ self_only = true })
                e:Register()
            end
            "#,
        )
        .eval()?;

    // `Game.RegisterWard(source, cost)` (CR 702.21a): "Whenever this permanent becomes the target of a spell or ability an opponent controls, counter that spell or ability unless that player pays [cost]", with `cost` the mana-cost string `Game.Ops.CounterUnlessPaid` takes. Each call is one instance (CR 702.21b). Equivalent to:
    //   local e = Game.CreateTriggeredAbility(source)
    //   e:SetCondition(Game.Conditions.BecomesTargetOfOpponent)
    //   e:SetTarget({ type = "triggering" })
    //   e:SetOperation(Game.Ops.CounterUnlessPaid(cost))
    //   e:Register()
    // It resolves `Game.*` at call time, like `Game.RegisterKeyword`.
    let register_ward: Function = lua
        .load(
            r#"
            function(source, cost)
                local e = Game.CreateTriggeredAbility(source)
                e:SetCondition(Game.Conditions.BecomesTargetOfOpponent)
                e:SetTarget({ type = "triggering" })
                e:SetOperation(Game.Ops.CounterUnlessPaid(cost))
                e:Register()
            end
            "#,
        )
        .eval()?;

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
    game.set("RegisterKeyword", register_keyword)?;
    game.set("RegisterWard", register_ward)?;
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
    game.set("CreateLandPlayModifier", create_land_play_modifier)?;
    game.set("LandPlayAffected", land_play_affected)?;
    game.set("SelectTargets", select_targets)?;
    game.set("MayChoose", may_choose)?;
    game.set("ChooseN", choose_n)?;
    lua.globals().set("Game", game)
}

/// Decodes the `{ control = "You" | "Owner", tapped = bool }` options table
/// of the reanimation ops. Defaults are CR 110.2a's "under your control" and
/// untapped; an unknown key or value is a script error so typos don't pass.
fn decode_entry_options(opts: Option<Table>, op_name: &str) -> mlua::Result<(EntryControl, bool)> {
    let mut control = EntryControl::You;
    let mut tapped = false;
    let Some(opts) = opts else { return Ok((control, tapped)) };
    for pair in opts.pairs::<String, mlua::Value>() {
        let (key, value) = pair?;
        match (key.as_str(), value) {
            ("control", mlua::Value::String(name)) => {
                control = match name.to_str()?.to_ascii_lowercase().as_str() {
                    "you" => EntryControl::You,
                    "owner" => EntryControl::Owner,
                    other => {
                        return Err(mlua::Error::RuntimeError(format!(
                            "{op_name} control must be \"You\" or \"Owner\", got {other:?}"
                        )))
                    }
                };
            }
            ("tapped", mlua::Value::Boolean(flag)) => tapped = flag,
            (other, _) => {
                return Err(mlua::Error::RuntimeError(format!(
                    "{op_name} option {other:?} is unknown or has the wrong type (expected control = \"You\"|\"Owner\", tapped = boolean)"
                )))
            }
        }
    }
    Ok((control, tapped))
}

/// One argument of `Game.Ops.Chain`/`ChainEach` (`ctor` names which). It must
/// be a called `Game.Ops.*` value that `is_modal_resolvable` accepts, so a
/// chain never holds an entry the modal loop would skip. A custom function,
/// an uncalled constructor, a `ForEachPlayer`, a nested chain and a
/// non-operation value are load errors (see `AbilityEffect::Chain`).
fn chain_entry(lua: &Lua, ctor: &str, position: usize, arg: Value) -> mlua::Result<AbilityEffect> {
    match arg {
        Value::UserData(ud) if ud.is::<OperationTag>() => {
            let effect = ud.borrow::<OperationTag>()?.0.clone();
            if !crate::commands::is_modal_resolvable(&effect) {
                return Err(mlua::Error::RuntimeError(format!(
                    "Game.Ops.{ctor} argument {position} can't be chained (a nested chain and some \
                     effects aren't supported) - see is_modal_resolvable for the supported kinds"
                )));
            }
            Ok(effect)
        }
        Value::UserData(ud) if ud.is::<ForEachPlayerTag>() => Err(mlua::Error::RuntimeError(format!(
            "Game.Ops.{ctor} argument {position} is a ForEachPlayer, which can't be chained yet"
        ))),
        Value::Function(function) => {
            if let Err(crate::script::ScriptError::LoadError(message)) =
                crate::script::decode::reject_uncalled_op(lua, &function)
            {
                return Err(mlua::Error::RuntimeError(format!("Game.Ops.{ctor} argument {position}: {message}")));
            }
            Err(mlua::Error::RuntimeError(format!(
                "Game.Ops.{ctor} argument {position} is a custom function, which can't be chained yet - \
                 use Game.Ops.* values"
            )))
        }
        other => Err(mlua::Error::RuntimeError(format!(
            "Game.Ops.{ctor} argument {position} must be a Game.Ops.* value, got {}",
            other.type_name()
        ))),
    }
}
