//! Phase 5 vertical slice: a mechanism for CR 701.20a/701.18c "plays with
//! hand revealed"-style effects (Telepathy's own worked example under CR
//! 701.20c: "Your opponents play with their hands revealed") — the second
//! player-level continuous-effect shape following `phase5/enchanted-player-
//! hand-size`'s own explicit non-goal naming this as a separate future case.
//!
//! Deliberately a **mechanism only**, not wired to any real enforcement —
//! confirmed during scoping that no consumer exists to hook into yet:
//! `GameObject.visibility`/`VisibilityPolicy::is_visible_to` is itself still
//! `todo!()` with zero callers, and `script::encode::state_to_lua`/
//! `card_summary_to_lua` don't parameterize by viewer at all — they
//! structurally never encode hand *contents* for any player, revealed or
//! not (only a `hand_count`), so there's nothing today for "revealed" to
//! change from that consumer's perspective. Same "real but currently
//! unreachable, named not hidden" stance `VisibilityPolicy` itself already
//! takes. Proven here only via `is_hand_revealed`'s own direct unit tests.
//!
//! `HandRevealAffected`/`HandRevealModifier` mirror `HandSizeAffected`/
//! `HandSizeModifier`'s exact shape (`Filter(FilterController)`/
//! `EnchantedPlayer`, `EnchantedPlayer` resolved fresh against `source`'s
//! own live `attached_to_player`) — a separate sibling type, not shared,
//! since this is only the second occurrence of the identical shape.
//! `is_hand_revealed` differs from `effective_max_hand_size` in one real way:
//! plain boolean OR across every applicable modifier, no "latest timestamp
//! wins" override — CR 701.20c's "a card that is currently revealed may be
//! revealed again" means two reveal sources never conflict.

use std::collections::HashMap;

use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, hand_battlefield_graveyard_zones, two_player_state as base_two_player_state};

const ENCHANTED_PLAYER_HAND_REVEALED: &str = r#"
local s = {}
function s.register_abilities(c)
    local m = Game.CreateHandRevealModifier(c)
    m:SetAffected(Game.HandRevealAffected.EnchantedPlayer)
    m:Register()
end
return s
"#;

fn aura_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test Enchant Player Hand Reveal Aura".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::ENCHANTMENT,
        subtypes: vec![Subtype("Aura".to_string())],
        oracle_text: "Enchant player. Enchanted player plays with their hand revealed.".to_string(),
        power: None,
        toughness: None,
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
    }
}

fn two_player_state() -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 1)]),
        ..base_two_player_state(hand_battlefield_graveyard_zones(p0, p1))
    }
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

#[test]
fn run_card_script_self_registers_an_enchanted_player_hand_reveal_modifier() {
    let mut state = two_player_state();
    let aura = ObjectId(0);
    let definition = aura_definition(ENCHANTED_PLAYER_HAND_REVEALED);
    state.objects.insert(aura, card_object(aura, definition.clone(), PlayerId(1), ZoneKind::Battlefield));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &definition, aura).expect("script self-registers cleanly");

    assert_eq!(state.hand_reveal_modifiers.len(), 1);
    let modifier = &state.hand_reveal_modifiers[0];
    assert_eq!(modifier.source, aura);
    assert_eq!(modifier.controller, PlayerId(1), "controller resolved from the Aura's own live controller at drain time");
    assert!(matches!(modifier.affected, HandRevealAffected::EnchantedPlayer));
}

#[test]
fn enchanted_players_hand_is_revealed() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(1);
    let mut aura_obj = card_object(aura, aura_definition(""), p1, ZoneKind::Battlefield);
    aura_obj.attached_to_player = Some(p0);
    put_on_battlefield(&mut state, aura_obj);
    state.hand_reveal_modifiers.push(HandRevealModifier {
        source: aura,
        controller: p1,
        affected: HandRevealAffected::EnchantedPlayer,
        timestamp: SequenceNo(0),
    });

    assert!(is_hand_revealed(&state, &state.hand_reveal_modifiers, p0), "the enchanted player's hand should be revealed");
    assert!(!is_hand_revealed(&state, &state.hand_reveal_modifiers, p1), "p1 isn't enchanted - unaffected");
}

#[test]
fn the_reveal_stops_once_the_aura_is_no_longer_attached_to_that_player() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(1);
    // Same source, same modifier, but the Aura fell off - re-resolved fresh
    // every check, not cached from whenever the modifier was registered.
    let aura_obj = card_object(aura, aura_definition(""), p1, ZoneKind::Battlefield);
    put_on_battlefield(&mut state, aura_obj);
    state.hand_reveal_modifiers.push(HandRevealModifier {
        source: aura,
        controller: p1,
        affected: HandRevealAffected::EnchantedPlayer,
        timestamp: SequenceNo(0),
    });

    assert!(!is_hand_revealed(&state, &state.hand_reveal_modifiers, p0), "the Aura isn't attached to anyone - not revealed");
}

#[test]
fn filter_controller_forms_resolve_independent_of_enchantment() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let source = ObjectId(1);
    put_on_battlefield(&mut state, card_object(source, aura_definition(""), p0, ZoneKind::Battlefield));
    state.hand_reveal_modifiers.push(HandRevealModifier {
        source,
        controller: p0,
        affected: HandRevealAffected::Filter(FilterController::Opponent),
        timestamp: SequenceNo(0),
    });

    assert!(!is_hand_revealed(&state, &state.hand_reveal_modifiers, p0), "the modifier's own controller isn't their own opponent");
    assert!(is_hand_revealed(&state, &state.hand_reveal_modifiers, p1), "p1 is p0's opponent - revealed");
}

#[test]
fn two_independent_reveal_sources_never_conflict() {
    let mut state = two_player_state();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let aura = ObjectId(1);
    let telepathy_like = ObjectId(2);
    let mut aura_obj = card_object(aura, aura_definition(""), p1, ZoneKind::Battlefield);
    aura_obj.attached_to_player = Some(p0);
    put_on_battlefield(&mut state, aura_obj);
    put_on_battlefield(&mut state, card_object(telepathy_like, aura_definition(""), p1, ZoneKind::Battlefield));
    // Two independent sources both revealing p0's hand - CR 701.20c: "a card
    // that is currently revealed may be revealed again," no override/
    // conflict the way HandSizeModifier's own SetTo/NoMaximum can have.
    state.hand_reveal_modifiers.push(HandRevealModifier {
        source: aura,
        controller: p1,
        affected: HandRevealAffected::EnchantedPlayer,
        timestamp: SequenceNo(0),
    });
    state.hand_reveal_modifiers.push(HandRevealModifier {
        source: telepathy_like,
        controller: p1,
        affected: HandRevealAffected::Filter(FilterController::Opponent),
        timestamp: SequenceNo(1),
    });

    assert!(is_hand_revealed(&state, &state.hand_reveal_modifiers, p0), "revealed via either source");

    // Remove the earlier-timestamped source entirely (unlike hand size,
    // there's no "latest wins" to worry about - the later Filter-based
    // source alone still holds).
    state.hand_reveal_modifiers.retain(|m| m.source != aura);
    assert!(is_hand_revealed(&state, &state.hand_reveal_modifiers, p0), "still revealed via the remaining Filter-based source");
}
