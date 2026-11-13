//! Rust-to-Lua encoders, the hidden-information-safe boundary every
//! state-aware `ScriptRuntime::invoke_*` call passes through. See
//! `state_to_lua` for what is exposed.

use mlua::{Lua, Value};

use crate::types::card::CardTypeSet;
use crate::types::counter::CounterKind;
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::object::GameObject;
use crate::types::zone::ZoneKind;

use super::types::ScriptError;

fn runtime_err(e: mlua::Error) -> ScriptError {
    ScriptError::RuntimeError(e.to_string())
}

/// Bridges the events a trigger condition reads into flat Lua tables: `kind` is
/// the snake_case variant name and the other fields keep their Rust names, ids
/// as plain integers. Every other kind is `Value::Nil`. Only ids, zone and
/// counter names, players, numbers and flags are written, never card
/// characteristics, so a table leaks nothing about a hidden card.
pub(crate) fn event_to_lua<'lua>(
    lua: &'lua Lua,
    event: Option<&GameEvent>,
) -> Result<Value<'lua>, ScriptError> {
    let Some(event) = event else {
        return Ok(Value::Nil);
    };
    let table = lua.create_table().map_err(runtime_err)?;
    match &event.kind {
        GameEventKind::ZoneChange {
            object, from, to, ..
        } => {
            table.set("kind", "zone_change").map_err(runtime_err)?;
            table.set("object", object.0).map_err(runtime_err)?;
            table
                .set("from", zone_kind_name(*from))
                .map_err(runtime_err)?;
            table.set("to", zone_kind_name(*to)).map_err(runtime_err)?;
        }
        GameEventKind::DamageDealt {
            source,
            target,
            amount,
            is_combat_damage,
        } => {
            table.set("kind", "damage_dealt").map_err(runtime_err)?;
            table.set("source", source.0).map_err(runtime_err)?;
            table
                .set("target", target_to_lua(lua, *target)?)
                .map_err(runtime_err)?;
            table.set("amount", *amount).map_err(runtime_err)?;
            table
                .set("is_combat_damage", *is_combat_damage)
                .map_err(runtime_err)?;
        }
        GameEventKind::LifeChanged { player, delta } => {
            table.set("kind", "life_changed").map_err(runtime_err)?;
            table.set("player", player.0).map_err(runtime_err)?;
            table.set("delta", *delta).map_err(runtime_err)?;
        }
        GameEventKind::Tapped { object } => {
            table.set("kind", "tapped").map_err(runtime_err)?;
            table.set("object", object.0).map_err(runtime_err)?;
        }
        GameEventKind::Untapped { object } => {
            table.set("kind", "untapped").map_err(runtime_err)?;
            table.set("object", object.0).map_err(runtime_err)?;
        }
        GameEventKind::CounterAdded {
            object,
            kind,
            amount,
        } => {
            table.set("kind", "counter_added").map_err(runtime_err)?;
            table.set("object", object.0).map_err(runtime_err)?;
            table
                .set("counter", counter_kind_name(kind))
                .map_err(runtime_err)?;
            table.set("amount", *amount).map_err(runtime_err)?;
        }
        GameEventKind::PlayerCounterAdded {
            player,
            kind,
            amount,
        } => {
            table
                .set("kind", "player_counter_added")
                .map_err(runtime_err)?;
            table.set("player", player.0).map_err(runtime_err)?;
            table
                .set("counter", counter_kind_name(kind))
                .map_err(runtime_err)?;
            table.set("amount", *amount).map_err(runtime_err)?;
        }
        GameEventKind::AttackerDeclared { attacker, target } => {
            table
                .set("kind", "attacker_declared")
                .map_err(runtime_err)?;
            table.set("attacker", attacker.0).map_err(runtime_err)?;
            table
                .set("target", target_to_lua(lua, *target)?)
                .map_err(runtime_err)?;
        }
        GameEventKind::BlockerDeclared { blocker, attacker } => {
            table.set("kind", "blocker_declared").map_err(runtime_err)?;
            table.set("blocker", blocker.0).map_err(runtime_err)?;
            table.set("attacker", attacker.0).map_err(runtime_err)?;
        }
        GameEventKind::CardsMilled { player, cards } => {
            table.set("kind", "cards_milled").map_err(runtime_err)?;
            table.set("player", player.0).map_err(runtime_err)?;
            table
                .set("cards", id_array_to_lua(lua, cards)?)
                .map_err(runtime_err)?;
        }
        // `kept_on_top` would show library order to a condition any player controls.
        GameEventKind::CardsScried { .. } | GameEventKind::CardsSurveiled { .. } => {
            return Ok(Value::Nil);
        }
        // No card reads these, so they stay unbridged. Listed by name so a new
        // variant fails to compile until someone decides whether to bridge it.
        GameEventKind::ManaAdded { .. }
        | GameEventKind::CostPaid { .. }
        | GameEventKind::PrioritySet { .. }
        | GameEventKind::PriorityPassed { .. }
        | GameEventKind::StackObjectPushed { .. }
        | GameEventKind::StackObjectResolved { .. }
        | GameEventKind::Fizzled { .. }
        | GameEventKind::Countered { .. }
        | GameEventKind::CastingUnwound { .. }
        | GameEventKind::TriggerQueued { .. }
        | GameEventKind::ReplacementApplied { .. }
        | GameEventKind::StateBasedActionApplied { .. }
        | GameEventKind::ScriptError { .. }
        | GameEventKind::TurnStarted { .. }
        | GameEventKind::PhaseStarted { .. }
        | GameEventKind::StepStarted { .. }
        | GameEventKind::PlayerEliminated { .. }
        | GameEventKind::LeftGame { .. }
        | GameEventKind::RngDrawn { .. }
        | GameEventKind::LibraryShuffled { .. }
        | GameEventKind::BecameCopy { .. }
        | GameEventKind::TextBoxesExchanged { .. }
        | GameEventKind::Regenerated { .. }
        | GameEventKind::TokenCeased { .. }
        | GameEventKind::Attached { .. }
        | GameEventKind::AttachedToPlayer { .. }
        | GameEventKind::Unattached { .. }
        | GameEventKind::UmbraArmorConsumed { .. }
        | GameEventKind::Cycled { .. }
        | GameEventKind::MadnessExiled { .. }
        | GameEventKind::SpellCast { .. }
        | GameEventKind::BecameTarget { .. }
        | GameEventKind::Revealed { .. } => return Ok(Value::Nil),
    }
    Ok(Value::Table(table))
}

/// A `Target` as the target-result table `{kind = "object"|"player"|"ability", id = n}`
/// that `Game.SelectTargets` answers and bridged events share.
pub(crate) fn target_to_lua<'lua>(
    lua: &'lua Lua,
    target: Target,
) -> Result<Value<'lua>, ScriptError> {
    let table = lua.create_table().map_err(runtime_err)?;
    let (kind, id) = match target {
        Target::Object(id) => ("object", id.0),
        Target::Player(id) => ("player", u64::from(id.0)),
        Target::Ability(id) => ("ability", id.0),
    };
    table.set("kind", kind).map_err(runtime_err)?;
    table.set("id", id).map_err(runtime_err)?;
    Ok(Value::Table(table))
}

fn id_array_to_lua<'lua>(lua: &'lua Lua, ids: &[ObjectId]) -> Result<Value<'lua>, ScriptError> {
    let table = lua.create_table().map_err(runtime_err)?;
    for (i, id) in ids.iter().enumerate() {
        table.set((i + 1) as i64, id.0).map_err(runtime_err)?;
    }
    Ok(Value::Table(table))
}

/// The inverse of `decode::parse_counter_kind_name`. A `Custom` kind gives its own
/// name, which reads back through `Game.Counter.Custom(name)`, not the name parser.
pub(crate) fn counter_kind_name(kind: &CounterKind) -> String {
    match kind {
        CounterKind::PlusOnePlusOne => "PlusOnePlusOne".to_string(),
        CounterKind::MinusOneMinusOne => "MinusOneMinusOne".to_string(),
        CounterKind::Loyalty => "Loyalty".to_string(),
        CounterKind::Poison => "Poison".to_string(),
        CounterKind::Energy => "Energy".to_string(),
        CounterKind::Experience => "Experience".to_string(),
        CounterKind::Charge => "Charge".to_string(),
        CounterKind::Custom(name) => name.clone(),
    }
}

/// State snapshot passed as the trailing argument to every state-aware
/// `invoke_*` (`invoke_condition`, `invoke_intervening_if`, `invoke_cda`,
/// `invoke_continuous_affects`, `invoke_continuous_effect`): a plain Lua table
/// and not a live handle, as `event_to_lua` does. This is the hidden-information
/// boundary, and `ScriptContext` needs no narrowing.
///
/// CR 400.2, 402.3: battlefield, graveyard, exile and command are public and
/// exposed in full as card-summary tables (`zone_cards_to_lua`,
/// `card_summary_to_lua`). Hand and library are hidden (even a revealed hand
/// stays modeled as hidden), so only counts are exposed (`hand_count`,
/// `library_count`), matching CR 402.3. No binding returns hand or library
/// contents; search, scry and surveil are their own `Game.Ops.*` primitives
/// that commit a zone change (CR 701.23, 701.22d, 701.25d).
///
/// Card summaries are base values (`CardDefinition` netted against counters),
/// not layer-resolved `effective_characteristics`. Resolving another object's
/// effective P/T would invoke its own CDA and effect scripts, which build a
/// snapshot through this function again, recursing without bound once any
/// battlefield card has one.
/// TODO: a single-object effective-characteristics query binding, invoked directly and not folded into the whole-board snapshot, instead of a depth limit
///
/// The stack (CR 400.2, also public) is not included, since a `StackObject`
/// takes its characteristics from `source` and `source_lki` and needs its own
/// decode.
/// TODO: stack contents in the snapshot
pub(crate) fn state_to_lua<'lua>(
    lua: &'lua Lua,
    state: &GameState,
) -> Result<Value<'lua>, ScriptError> {
    let table = lua
        .create_table()
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let life = lua
        .create_table()
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let hand_count = lua
        .create_table()
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let library_count = lua
        .create_table()
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let graveyard = lua
        .create_table()
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let command = lua
        .create_table()
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;

    for player in &state.players {
        let pid = player.player_id.0;
        life.set(pid, player.life)
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        let hand_len = state
            .zone(ZoneKind::Hand, Some(player.player_id))
            .map(|z| z.len())
            .unwrap_or(0) as u32;
        hand_count
            .set(pid, hand_len)
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        let library_len = state
            .zone(ZoneKind::Library, Some(player.player_id))
            .map(|z| z.len())
            .unwrap_or(0) as u32;
        library_count
            .set(pid, library_len)
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        graveyard
            .set(
                pid,
                zone_cards_to_lua(lua, state, ZoneKind::Graveyard, Some(player.player_id))?,
            )
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
        command
            .set(
                pid,
                zone_cards_to_lua(lua, state, ZoneKind::Command, Some(player.player_id))?,
            )
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    }

    table
        .set("life", life)
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table
        .set("hand_count", hand_count)
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table
        .set("library_count", library_count)
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table
        .set(
            "battlefield",
            zone_cards_to_lua(lua, state, ZoneKind::Battlefield, None)?,
        )
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table
        .set(
            "exile",
            zone_cards_to_lua(lua, state, ZoneKind::Exile, None)?,
        )
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table
        .set("graveyard", graveyard)
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table
        .set("command", command)
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    Ok(Value::Table(table))
}

/// One public zone's contents as a 1-indexed array of `card_summary_to_lua`
/// tables, in zone order. `owner` is `None` for the shared battlefield and
/// exile and `Some(player)` for a per-player graveyard or command zone. An id
/// in `zone.contents` with no `GameState::object` entry is skipped, since this
/// read-only snapshot isn't where to enforce that invariant.
fn zone_cards_to_lua<'lua>(
    lua: &'lua Lua,
    state: &GameState,
    kind: ZoneKind,
    owner: Option<PlayerId>,
) -> Result<Value<'lua>, ScriptError> {
    let table = lua
        .create_table()
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
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

/// One object's public-safe summary: id, name, base power and toughness
/// (netted against `+1/+1` and `-1/-1` counters through
/// `PTValue::net_with_counters`, CR 121.3, but not layer-resolved; see
/// `state_to_lua`), whether it is a creature, controller, and attachment state
/// (CR 303.4m). Narrower than `EffectiveCharacteristics`, with no mana cost,
/// colors, full type set or keywords.
/// TODO: the rest of the effective characteristics
fn card_summary_to_lua<'lua>(lua: &'lua Lua, obj: &GameObject) -> Result<Value<'lua>, ScriptError> {
    let table = lua
        .create_table()
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table
        .set("id", obj.object_id.0)
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table
        .set("name", obj.definition.name.clone())
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let power = obj
        .definition
        .power
        .as_ref()
        .and_then(|p| p.net_with_counters(&obj.counters));
    table
        .set("power", power)
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let toughness = obj
        .definition
        .toughness
        .as_ref()
        .and_then(|t| t.net_with_counters(&obj.counters));
    table
        .set("toughness", toughness)
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table
        .set(
            "is_creature",
            obj.definition.card_types.contains(CardTypeSet::CREATURE),
        )
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    // The raw card type list (`{"Land"}`), in the same name strings a filter table's `card_types` accepts, beside `is_creature`. A `ForEachPlayer` body such as Balance's land count needs more than creature-or-not.
    table
        .set(
            "card_types",
            card_type_names_to_lua(lua, obj.definition.card_types)?,
        )
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table
        .set("controller", obj.controller.0)
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    // CR 303.4m, 703.4b: what this object is attached to and what is attached to it, both public.
    table
        .set("attached_to", obj.attached_to.map(|id| id.0))
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    // CR 303.4b, 303.4m: the player half of `attached_to`, exclusive with it.
    table
        .set("attached_to_player", obj.attached_to_player.map(|id| id.0))
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let attachments = lua
        .create_table()
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    for (i, id) in obj.attachments.iter().enumerate() {
        attachments
            .set((i + 1) as i64, id.0)
            .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    }
    table
        .set("attachments", attachments)
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    Ok(Value::Table(table))
}

/// `CardTypeSet` to the name strings a filter table's `card_types` accepts (the
/// inverse of `decode::parse_card_type_name`), as a 1-indexed array in a fixed
/// check order.
fn card_type_names_to_lua<'lua>(
    lua: &'lua Lua,
    types: CardTypeSet,
) -> Result<Value<'lua>, ScriptError> {
    let table = lua
        .create_table()
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let mut i = 1i64;
    let named: &[(CardTypeSet, &str)] = &[
        (CardTypeSet::ARTIFACT, "Artifact"),
        (CardTypeSet::CREATURE, "Creature"),
        (CardTypeSet::ENCHANTMENT, "Enchantment"),
        (CardTypeSet::INSTANT, "Instant"),
        (CardTypeSet::LAND, "Land"),
        (CardTypeSet::SORCERY, "Sorcery"),
        (CardTypeSet::KINDRED, "Kindred"),
        (CardTypeSet::PLANESWALKER, "Planeswalker"),
        (CardTypeSet::BATTLE, "Battle"),
    ];
    for (bit, name) in named {
        if types.contains(*bit) {
            table
                .set(i, *name)
                .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
            i += 1;
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::decode::parse_counter_kind_name;

    // CR 120.1, 508.1b: no damage or attack event names an ability, but encoding one must not panic.
    #[test]
    fn an_ability_target_encodes_as_kind_ability() {
        let lua = Lua::new();
        let Value::Table(table) = target_to_lua(&lua, Target::Ability(ObjectId(7))).unwrap() else {
            panic!("a target encodes as a table");
        };
        assert_eq!(table.get::<_, String>("kind").unwrap(), "ability");
        assert_eq!(table.get::<_, u64>("id").unwrap(), 7);
    }

    // CR 122.1
    #[test]
    fn counter_names_round_trip_through_the_name_parser() {
        for kind in [
            CounterKind::PlusOnePlusOne,
            CounterKind::MinusOneMinusOne,
            CounterKind::Loyalty,
            CounterKind::Poison,
            CounterKind::Energy,
            CounterKind::Experience,
            CounterKind::Charge,
        ] {
            assert_eq!(
                parse_counter_kind_name(&counter_kind_name(&kind)).unwrap(),
                kind
            );
        }
        assert_eq!(
            counter_kind_name(&CounterKind::Custom("Fade".to_string())),
            "Fade"
        );
    }
}
