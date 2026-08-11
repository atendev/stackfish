//! Rust -> Lua encoders — the hidden-information-safe boundary every
//! state-aware `ScriptRuntime::invoke_*` call passes through, split out of
//! the original monolithic `script.rs`. See `state_to_lua`'s own doc
//! comment for what is and isn't exposed here.

use mlua::{Lua, Value};

use crate::types::card::CardTypeSet;
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::PlayerId;
use crate::types::object::GameObject;
use crate::types::zone::ZoneKind;

use super::types::ScriptError;

/// Bridges the one event shape this slice's condition scripts need to read —
/// `GameEventKind::ZoneChange` becomes a Lua table with `kind`/`object`/
/// `from`/`to` fields (zone names as lowercase strings). Every other
/// `GameEventKind` variant becomes `Value::Nil` for now.
///
/// TODO: extend as real conditions need more event shapes bridged (damage
/// dealt, life changed, tapped, ...) - this only covers what "did `source`
/// just enter the battlefield" needs to read.
pub(crate) fn event_to_lua<'lua>(lua: &'lua Lua, event: Option<&GameEvent>) -> Result<Value<'lua>, ScriptError> {
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
pub(crate) fn state_to_lua<'lua>(lua: &'lua Lua, state: &GameState) -> Result<Value<'lua>, ScriptError> {
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
/// doc comment for why), whether it's a creature, controller, and (CR 303.4m)
/// attachment state. Narrower than the full `EffectiveCharacteristics` shape
/// (no mana cost/colors/full type or keyword set) — named as a follow-up
/// rather than guessed at; this slice's own tests only need power/toughness/
/// creature-ness/attachment.
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
    // `phase5/mass-choice-effects`: the raw card type list (e.g. `{"Land"}`),
    // same name strings a filter table's own `card_types` key accepts - added
    // alongside `is_creature` (not replacing it) since a `ForEachPlayer` body
    // computing something like Balance's "how many lands does each player
    // control" needs to distinguish more than just creature-or-not from this
    // snapshot. `is_creature` stays for whatever already reads it.
    table
        .set("card_types", card_type_names_to_lua(lua, obj.definition.card_types)?)
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    table.set("controller", obj.controller.0).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    // CR 303.4m/703.4b: what this object is attached to (an Aura/Equipment/
    // Fortification's own "enchanted"/"equipped" reference), and what's
    // attached to it (e.g. an enchanted creature's own Auras) - both public
    // information.
    table
        .set("attached_to", obj.attached_to.map(|id| id.0))
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    // CR 303.4b/m (phase5/aura-enchants-a-player): the player half of
    // `attached_to` above, when this object enchants a player instead of
    // an object - mutually exclusive with `attached_to`, same as the two
    // Rust-side fields themselves.
    table
        .set("attached_to_player", obj.attached_to_player.map(|id| id.0))
        .map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    let attachments = lua.create_table().map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    for (i, id) in obj.attachments.iter().enumerate() {
        attachments.set((i + 1) as i64, id.0).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    }
    table.set("attachments", attachments).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
    Ok(Value::Table(table))
}

/// `CardTypeSet` -> the same name-string vocabulary a filter table's own
/// `card_types` key accepts (`decode::parse_card_type_name`'s inverse) - a
/// 1-indexed array of every type this object has, in a fixed check order.
/// `phase5/mass-choice-effects`'s own `card_summary_to_lua` is the only
/// caller so far.
fn card_type_names_to_lua<'lua>(lua: &'lua Lua, types: CardTypeSet) -> Result<Value<'lua>, ScriptError> {
    let table = lua.create_table().map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
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
            table.set(i, *name).map_err(|e| ScriptError::RuntimeError(e.to_string()))?;
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
