//! Phase 5 vertical slice: card scripts run for cards sitting in a hand or
//! library, not just after a draw or on battlefield entry (CR 604.3, 113.6).
//!
//! `Engine::sync_off_battlefield_scripts` now sweeps Hand and Library as well
//! as Graveyard/Exile, so a CDA or hand-only ability registers for a card that
//! arrived by any producer (redirected death, mulligan, fixture), exactly
//! once. A bounce-to-hand verb doesn't exist yet (backlog), so these tests
//! place cards directly.

use stackfish_engine::engine::Engine;
use stackfish_engine::layers::effective_characteristics;
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, minimal_card_definition, two_player_state as base_two_player_state, zones};

const CDA_ALL_COLORS: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetCharacteristicDefining()
    e:SetLayer(Game.Layers.ColorChanging)
    e:SetEffect(Game.Effects.SetColors("WUBRG"))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

const PLAIN_STATIC: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.ColorChanging)
    e:SetEffect(Game.Effects.SetColors("WUBRG"))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

const CYCLING: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateActivatedAbility(c)
    e:AddCost(Game.Costs.DiscardSelf)
    e:SetOperation(Game.Ops.DrawCards(1))
    e:Register()
end
return s
"#;

const ETB_TRIGGER: &str = r#"
local s = {}
function s.register_abilities(c)
    local e = Game.CreateTriggeredAbility(c)
    e:SetCondition(Game.Conditions.EntersBattlefield)
    e:SetOperation(Game.Ops.GainLife(1))
    e:Register()
end
return s
"#;

const CARD: ObjectId = ObjectId(0);

fn scripted(script: &str) -> CardDefinition {
    CardDefinition {
        script_ref: ScriptReference::Inline(script.to_string()),
        ..minimal_card_definition(1, "Scripted", "unused", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE)
    }
}

fn engine_with(zone: ZoneKind, script: &str) -> Engine {
    let p0 = PlayerId(0);
    let mut state = GameState {
        step: Step::PreCombatMain,
        ..base_two_player_state(zones(
            p0,
            PlayerId(1),
            &[ZoneKind::Hand, ZoneKind::Library, ZoneKind::Battlefield, ZoneKind::Graveyard, ZoneKind::Exile],
        ))
    };
    state.objects.insert(CARD, card_object(CARD, scripted(script), p0, zone));
    state.zone_mut(zone, Some(p0)).expect("zone exists").push_bottom(CARD);
    Engine::new(state)
}

/// One full round of priority passes, so trigger collection (and the sweep)
/// runs at least once.
fn run_sweep(engine: &mut Engine) {
    for player in [PlayerId(0), PlayerId(1)] {
        engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
        drain(engine);
    }
}

fn colors(engine: &Engine, id: ObjectId) -> ColorSet {
    effective_characteristics(engine.state(), &ScriptRuntime::new(), id).colors
}

#[test]
fn a_cda_in_hand_or_library_registers_once_and_applies() {
    for zone in [ZoneKind::Hand, ZoneKind::Library] {
        let mut engine = engine_with(zone, CDA_ALL_COLORS);
        run_sweep(&mut engine);
        run_sweep(&mut engine);
        assert_eq!(engine.state().continuous_effects.len(), 1, "exactly one CDA in {zone:?}");
        assert_eq!(colors(&engine, CARD), ColorSet::all(), "CDA applies in {zone:?}");
    }
}

#[test]
fn a_plain_static_does_not_register_in_hand_or_library() {
    for zone in [ZoneKind::Hand, ZoneKind::Library] {
        let mut engine = engine_with(zone, PLAIN_STATIC);
        run_sweep(&mut engine);
        assert!(engine.state().continuous_effects.is_empty(), "plain static must not register in {zone:?}");
    }
}

#[test]
fn a_hand_only_ability_registers_once_for_a_fixture_placed_hand_card() {
    let mut engine = engine_with(ZoneKind::Hand, CYCLING);
    run_sweep(&mut engine);
    run_sweep(&mut engine);
    let count = engine.state().abilities.iter().filter(|a| a.source == CARD).count();
    assert_eq!(count, 1, "one Cycling ability, not duplicated across sweeps");
}

#[test]
fn nothing_script_registered_survives_in_the_library() {
    let mut engine = engine_with(ZoneKind::Library, CYCLING);
    run_sweep(&mut engine);
    assert!(engine.state().abilities.iter().all(|a| a.source != CARD), "no ability functions from the library");
    let mut engine = engine_with(ZoneKind::Library, ETB_TRIGGER);
    run_sweep(&mut engine);
    assert!(engine.state().abilities.iter().all(|a| a.source != CARD), "an ETB trigger doesn't go live in the library");
}

#[test]
fn repeated_sweeps_never_duplicate_a_hand_ability() {
    let mut engine = engine_with(ZoneKind::Hand, CYCLING);
    run_sweep(&mut engine);
    run_sweep(&mut engine);
    let by_source = |engine: &Engine| {
        let mut counts = std::collections::HashMap::new();
        for a in &engine.state().abilities {
            *counts.entry(a.source).or_insert(0) += 1;
        }
        counts
    };
    assert!(by_source(&engine).values().all(|&n| n == 1), "no source holds a duplicate ability");
}

#[test]
fn a_cda_card_moved_hand_to_library_registers_for_the_new_id() {
    let mut engine = engine_with(ZoneKind::Hand, CDA_ALL_COLORS);
    run_sweep(&mut engine);
    // Simulate a zone move minting a new id (what every producer does).
    let state = engine.state_mut();
    let old = state.object(CARD).unwrap().clone();
    let new_id = state.allocate_object_id();
    let mut moved = old.clone();
    moved.object_id = new_id;
    moved.zone = ZoneKind::Library;
    state.zone_mut(ZoneKind::Hand, Some(PlayerId(0))).unwrap().remove(CARD);
    // What `retire_object` does for the departed id.
    state.objects.remove(&CARD);
    state.continuous_effects.retain(|e| e.source != CARD);
    state.off_battlefield_scripts_run.remove(&CARD);
    state.zone_mut(ZoneKind::Library, Some(PlayerId(0))).unwrap().push_top(new_id);
    state.objects.insert(new_id, moved);
    run_sweep(&mut engine);
    assert_eq!(engine.state().continuous_effects.len(), 1, "old id's CDA purged, new id's registered");
    assert_eq!(colors(&engine, new_id), ColorSet::all());
}
