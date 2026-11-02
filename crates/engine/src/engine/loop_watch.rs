//! CR 104.4b mandatory-loop detection: the watch over forced priority passes
//! and the state fingerprint it compares. See `DESIGN.md`.

use crate::commands::find_ability;
use crate::decision::Entry;
use crate::types::cost::AdditionalCost;
use crate::types::game::GameState;
use crate::types::ids::PlayerId;

use super::{Engine, ProcessState};

const RELABELED: [&[u8]; 5] = [
    b"ObjectId",
    b"AbilityId",
    b"EventId",
    b"SequenceNo",
    b"ScriptHandle",
];

impl Engine {
    /// Runs on an accepted priority action before it is applied. Returns true
    /// when the action ended the game as a draw.
    // TODO: a fragmented loop never makes the active player choose differently (CR 732.3)
    // TODO: a loop through an extra combat or step passes on an empty stack and is not watched (CR 104.4b)
    pub(super) fn watch_for_loop(&mut self, is_pass: bool) -> bool {
        if !is_pass || self.state.stack.is_empty() || !self.pass_is_forced() {
            self.loop_watch.clear();
            return false;
        }
        let print = fingerprint(&self.state);
        if self.loop_watch.contains(&print) {
            self.loop_watch.clear();
            // CR 104.4b
            // TODO: limited range of influence draws only the players in the loop (CR 104.4f)
            self.process_state = ProcessState::GameOver { winner: None };
            return true;
        }
        self.loop_watch.push(print);
        false
    }

    /// The priority holder can do nothing but pass, apart from a mana ability
    /// whose whole cost is `{T}`, or no cost, which taps the source. No condition
    /// watches a tap, so floating mana cannot stop a loop (CR 732.5).
    // TODO: a card the player cannot pay for still counts as a way out (CR 104.4b)
    fn pass_is_forced(&self) -> bool {
        self.legal_actions().iter().all(|entry| match entry {
            Entry::Pass => true,
            Entry::Activate(a) => {
                a.mana_ability
                    && find_ability(&self.state, a.source, a.ability_index).is_some_and(|ability| {
                        ability.cost.as_ref().is_none_or(|c| {
                            c.mana.is_none()
                                && c.alternative.is_empty()
                                && c.additional
                                    .iter()
                                    .all(|x| matches!(x, AdditionalCost::TapSelf))
                        })
                    })
            }
            _ => false,
        })
    }
}

/// `state` as text, equal for two states that differ only by an order-preserving
/// renaming of object, ability, event, sequence and script ids. The ledger is left
/// out, so each value the engine reads from it at priority is folded in
/// (`cards_drawn_this_turn`, `opponent_damaged_this_turn`). Any other ledger reader
/// that runs at priority belongs in that list, or two different futures match.
// TODO: Lua globals a card script keeps are not in the fingerprint, so a script that counts in Lua could draw a game that would end (CR 104.4b)
pub(crate) fn fingerprint(state: &GameState) -> String {
    let GameState {
        players,
        turn_order,
        active_player,
        turn_number,
        phase,
        step,
        priority_player,
        consecutive_passes,
        lands_played_this_turn,
        combat,
        last_active_turn_number,
        objects,
        zones,
        stack,
        continuous_effects,
        replacement_effects,
        cost_modifiers,
        hand_size_modifiers,
        hand_reveal_modifiers,
        draw_limit_modifiers,
        land_play_modifiers,
        abilities,
        ledger: _,
        rng,
        next_event_id: _,
        next_sequence_no: _,
        next_object_id: _,
        pending_step_skips,
        once_per_turn_triggered,
        commander_return_offered,
        linked_ability_generation,
        script_ability_ids,
        ability_function_zones,
        off_battlefield_scripts_run,
        linked_exiles,
        pending_timing_bypass,
        pending_free_cast,
    } = state;
    let ledger_reads: Vec<(PlayerId, u32, bool)> = turn_order
        .iter()
        .map(|&p| {
            (
                p,
                crate::types::player::cards_drawn_this_turn(state, p),
                crate::replacement::opponent_damaged_this_turn(state, p),
            )
        })
        .collect();
    let raw = format!(
        "{players:?};{turn_order:?};{active_player:?};{turn_number:?};{phase:?};{step:?};\
         {priority_player:?};{consecutive_passes:?};{lands_played_this_turn:?};{combat:?};\
         {last_active_turn_number:?};{objects:?};{zones:?};{stack:?};{continuous_effects:?};\
         {replacement_effects:?};{cost_modifiers:?};{hand_size_modifiers:?};\
         {hand_reveal_modifiers:?};{draw_limit_modifiers:?};{land_play_modifiers:?};\
         {abilities:?};{rng:?};{pending_step_skips:?};{once_per_turn_triggered:?};\
         {commander_return_offered:?};{linked_ability_generation:?};{script_ability_ids:?};\
         {ability_function_zones:?};{off_battlefield_scripts_run:?};{linked_exiles:?};\
         {pending_timing_bypass:?};{pending_free_cast:?};{ledger_reads:?}"
    );
    let relabeled = relabel(raw.as_bytes());
    let mut i = 0;
    let mut out = Vec::with_capacity(relabeled.len());
    sort_hash_collections(&relabeled, &mut i, false, &mut out);
    // Only ASCII spans were moved, so multi-byte characters stay whole.
    String::from_utf8(out).expect("fingerprint stays UTF-8")
}

/// Index just past the quoted literal starting at `start`.
fn skip_literal(text: &[u8], start: usize) -> usize {
    let quote = text[start];
    let mut i = start + 1;
    while i < text.len() {
        match text[i] {
            b'\\' => i += 2,
            c if c == quote => return i + 1,
            _ => i += 1,
        }
    }
    text.len()
}

/// The id tokens outside literals, as (kind, digit start, digit end, value).
fn id_tokens(text: &[u8]) -> Vec<(usize, usize, usize, u64)> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c == b'"' || c == b'\'' {
            i = skip_literal(text, i);
            continue;
        }
        let word_start = i == 0 || !(text[i - 1].is_ascii_alphanumeric() || text[i - 1] == b'_');
        if word_start && c.is_ascii_alphabetic() {
            let end = i + text[i..]
                .iter()
                .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
                .count();
            if let Some(kind) = RELABELED.iter().position(|k| *k == &text[i..end])
                && text.get(end) == Some(&b'(')
            {
                let digits = end + 1;
                let len = text[digits..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                if len > 0 && text.get(digits + len) == Some(&b')') {
                    let value = std::str::from_utf8(&text[digits..digits + len])
                        .ok()
                        .and_then(|s| s.parse().ok());
                    if let Some(value) = value {
                        found.push((kind, digits, digits + len, value));
                    }
                }
            }
            i = end;
            continue;
        }
        i += 1;
    }
    found
}

/// Replaces each id with its rank among the ids of its kind, which keeps any
/// ordering the engine takes from ids.
fn relabel(text: &[u8]) -> Vec<u8> {
    let tokens = id_tokens(text);
    let mut ranks: Vec<Vec<u64>> = vec![Vec::new(); RELABELED.len()];
    for &(kind, _, _, value) in &tokens {
        ranks[kind].push(value);
    }
    for values in &mut ranks {
        values.sort_unstable();
        values.dedup();
    }
    let mut out = Vec::with_capacity(text.len());
    let mut copied = 0;
    for (kind, start, end, value) in tokens {
        out.extend_from_slice(&text[copied..start]);
        let rank = ranks[kind].binary_search(&value).unwrap_or_default();
        out.extend_from_slice(rank.to_string().as_bytes());
        copied = end;
    }
    out.extend_from_slice(&text[copied..]);
    out
}

/// Copies `text` from `i` to the next unmatched closer, or to the next
/// top-level `", "` when `in_element`. A derived `Debug` prints a hash map or
/// set as `{` followed directly by an entry, and a struct as `{ `, so only the
/// former has its entries sorted.
fn sort_hash_collections(text: &[u8], i: &mut usize, in_element: bool, out: &mut Vec<u8>) {
    while *i < text.len() {
        match text[*i] {
            b'"' | b'\'' => {
                let end = skip_literal(text, *i);
                out.extend_from_slice(&text[*i..end]);
                *i = end;
            }
            b')' | b']' | b'}' => return,
            b',' if in_element && text.get(*i + 1) == Some(&b' ') => return,
            b'{' if !matches!(text.get(*i + 1), Some(b' ' | b'}') | None) => {
                *i += 1;
                let mut entries = Vec::new();
                loop {
                    let mut entry = Vec::new();
                    sort_hash_collections(text, i, true, &mut entry);
                    entries.push(entry);
                    if text.get(*i) == Some(&b',') {
                        *i += 2;
                    } else {
                        break;
                    }
                }
                *i += 1;
                entries.sort();
                out.push(b'{');
                out.extend_from_slice(&entries.join(&b", "[..]));
                out.push(b'}');
            }
            open @ (b'(' | b'[' | b'{') => {
                out.push(open);
                *i += 1;
                sort_hash_collections(text, i, false, out);
                if let Some(&close) = text.get(*i) {
                    out.push(close);
                    *i += 1;
                }
            }
            c => {
                out.push(c);
                *i += 1;
            }
        }
    }
}
