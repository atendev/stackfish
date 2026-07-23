//! Phase 5 vertical slice: CR 702.16e/616.1 — protection's damage prevention
//! now participates in the same CR 616 ordering-choice decision a
//! registered `ReplacementEffect` does, instead of applying unconditionally
//! ahead of `apply_replacements_from`'s loop the way this engine used to.
//! `replacement::protection_prevents` is the new predicate (folds the
//! protected object's own id into the loop's `matching` candidate list);
//! `replacement::apply_one_replacement_or_protection` is the dispatch that
//! tells a real registered effect's source apart from the protection
//! sentinel.
//!
//! Concrete scenario this closes (previously unexercised by any test — see
//! `docs/implementation/08-replacements.md`'s own slice notes): a creature
//! with protection from red is dealt damage by a red source, while a
//! *different* permanent also has a `RedirectDamageToSource` effect
//! registered against the same damage. Per CR 616.1e, the affected player's
//! choice of which applies first is real and produces genuinely different
//! outcomes — protection-first prevents the damage entirely (nothing lands
//! anywhere); redirect-first moves the *unprevented* damage onto the
//! (unprotected) redirect target, which then takes it in full, since
//! protection no longer matches once the event's target has moved away from
//! the protected creature.

use std::collections::HashMap;

use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::script::ScriptRuntime;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, zones, two_player_state as base_two_player_state};

const STATIC_PROTECTION_FROM_RED: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddProtectionColor("R"))
    e:SetAffected({ self_only = true })
    e:Register()
end
return s
"#;

const REDIRECT_DAMAGE_TO_SELF: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateReplacementEffect(c)
    e:SetFilter(Game.EventFilters.DamageDealt)
    e:SetPayload(Game.ReplacementPayloads.RedirectDamageToSource)
    e:SetAffected({ controller = "you" })
    e:Register()
end
return s
"#;

/// Same permanent has both: its own protection *and* its own registered
/// replacement effect - the documented sentinel-collision limitation
/// (`apply_one_replacement_or_protection`'s own doc comment).
const STATIC_PROTECTION_FROM_RED_AND_PREVENTS_ALL_DAMAGE_TO_SELF: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateStaticAbility(c)
    e:SetLayer(Game.Layers.AbilityAdding)
    e:SetEffect(Game.Effects.AddProtectionColor("R"))
    e:SetAffected({ self_only = true })
    e:Register()

    local r = Game.CreateReplacementEffect(c)
    r:SetFilter(Game.EventFilters.DamageDealt)
    r:SetPayload(Game.ReplacementPayloads.PreventDamage)
    r:SetAffected({ self_only = true })
    r:Register()
end
return s
"#;

fn creature_definition(
    definition_id: u64,
    name: &str,
    power: i32,
    toughness: i32,
    color_indicator: Option<ColorSet>,
    script: &str,
) -> CardDefinition {
    CardDefinition {
        definition_id,
        name: name.to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::CREATURE,
        subtypes: Vec::new(),
        oracle_text: "Protection-as-replacement test fixture.".to_string(),
        power: Some(PTValue::Fixed(power)),
        toughness: Some(PTValue::Fixed(toughness)),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(script.to_string()),
        keywords: KeywordSet::empty(),
        target_requirement: None,
        alternative_costs: Vec::new(),
        effect: None,
        modes: Vec::new(),
    }
}

fn vanilla_creature(id: u64, name: &str, power: i32, toughness: i32, color: Option<ColorSet>) -> CardDefinition {
    CardDefinition { script_ref: ScriptReference::ByKey(name.to_string()), ..creature_definition(id, name, power, toughness, color, "") }
}

fn permanent(id: ObjectId, definition: CardDefinition, owner: PlayerId) -> GameObject {
    card_object(id, definition, owner, ZoneKind::Battlefield)
}

fn put_on_battlefield(state: &mut GameState, object: GameObject) {
    let id = object.object_id;
    state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(id);
    state.objects.insert(id, object);
}

fn combat_damage_state(combat: CombatState) -> GameState {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    GameState {
        phase: Phase::Combat,
        step: Step::CombatDamage,
        combat: Some(combat),
        last_active_turn_number: HashMap::from([(p0, 1), (p1, 0)]),
        ..base_two_player_state(zones(p0, p1, &[ZoneKind::Battlefield]))
    }
}

fn assign_combat_damage(engine: &mut Engine, player: PlayerId) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::AssignCombatDamage { assignments: Vec::new() },
        })
        .expect("turn-based action, no priority required");
    drain(engine)
}

/// Attacker (p0, red, power 3) blocked by Protected (p1, protection from
/// red, power 0 so it never fights back and confuses the scenario) - a
/// third permanent, Redirector (p1), also registers `RedirectDamageToSource`
/// scoped to `controller = "you"` (matches any damage dealt to a
/// p1-controlled object, including Protected).
fn protection_vs_redirect_state() -> (GameState, ObjectId, ObjectId, ObjectId) {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let protected = ObjectId(1);
    let redirector = ObjectId(2);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(protected, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature(0, "Bear", 3, 3, Some(ColorSet::RED)), p0));
    let protected_definition = creature_definition(1, "Protected", 0, 4, None, STATIC_PROTECTION_FROM_RED);
    put_on_battlefield(&mut state, permanent(protected, protected_definition.clone(), p1));
    // Toughness 4, not a low number - must survive the redirected 3 damage
    // so the tests below can still inspect its post-damage id/state rather
    // than an SBA-driven death retiring it to a fresh ObjectId first.
    let redirector_definition = creature_definition(2, "Redirector", 0, 4, None, REDIRECT_DAMAGE_TO_SELF);
    put_on_battlefield(&mut state, permanent(redirector, redirector_definition.clone(), p1));

    let mut runtime = ScriptRuntime::new();
    runtime.run_card_script(&mut state, &protected_definition, protected).expect("protection script registers cleanly");
    runtime.run_card_script(&mut state, &redirector_definition, redirector).expect("redirect script registers cleanly");

    (state, attacker, protected, redirector)
}

#[test]
fn protection_and_a_competing_redirect_both_matching_pauses_for_a_real_ordering_choice() {
    let (state, _attacker, protected, redirector) = protection_vs_redirect_state();
    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, PlayerId(0));

    let StepStatus::AwaitingReplacementChoice { candidates, .. } = status else {
        panic!("expected a CR 616.1 ordering-choice pause - protection and the redirect both match, got {status:?}");
    };
    assert_eq!(candidates.len(), 2, "protection's sentinel id and the redirect's real source must both be offered");
    assert!(candidates.contains(&protected), "protection must be offered as a real candidate, not applied silently ahead of the choice");
    assert!(candidates.contains(&redirector), "the registered redirect must still be offered too");
}

#[test]
fn choosing_protection_first_prevents_the_damage_entirely() {
    let (state, _attacker, protected, redirector) = protection_vs_redirect_state();
    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, PlayerId(0));

    engine.submit_replacement_choice(protected).expect("protection is a real candidate");
    drain(&mut engine);

    assert_eq!(engine.state().object(protected).unwrap().damage_marked, 0, "protection zeroed it before the redirect ever got a nonzero amount to move");
    assert_eq!(engine.state().object(redirector).unwrap().damage_marked, 0, "the redirect still applies afterward, but only to an already-zeroed amount");
}

#[test]
fn choosing_the_redirect_first_sends_the_full_unprevented_damage_to_the_unprotected_redirector() {
    let (state, _attacker, protected, redirector) = protection_vs_redirect_state();
    let mut engine = Engine::new(state);
    assign_combat_damage(&mut engine, PlayerId(0));

    engine.submit_replacement_choice(redirector).expect("the redirect is a real candidate");
    drain(&mut engine);

    assert_eq!(
        engine.state().object(redirector).unwrap().damage_marked,
        3,
        "once the target moved to the (unprotected) redirector, protection no longer matches - full damage lands"
    );
    assert_eq!(engine.state().object(protected).unwrap().damage_marked, 0, "the damage moved away from Protected entirely, it was never marked there");
}

/// Regression: protection alone (no competing registered effect) must still
/// resolve synchronously, exactly as before this slice - no spurious pause
/// introduced for the overwhelmingly common case.
#[test]
fn protection_alone_still_resolves_synchronously_with_no_pause() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let protected = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(protected, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature(0, "Bear", 3, 3, Some(ColorSet::RED)), p0));
    let protected_definition = creature_definition(1, "Protected", 0, 4, None, STATIC_PROTECTION_FROM_RED);
    put_on_battlefield(&mut state, permanent(protected, protected_definition.clone(), p1));
    ScriptRuntime::new()
        .run_card_script(&mut state, &protected_definition, protected)
        .expect("protection script registers cleanly");

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0);

    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "no competing effect - must resolve without a pause, got {status:?}");
    assert_eq!(engine.state().object(protected).unwrap().damage_marked, 0, "protection still actually prevented the damage");
}

/// Documented limitation (`apply_one_replacement_or_protection`'s own doc
/// comment): when the SAME permanent has both innate protection and its own
/// registered replacement effect on itself, the sentinel id collides - the
/// registered effect always wins the dispatch. Locking in the accepted
/// behavior rather than leaving it silently unverified.
#[test]
fn a_permanent_with_both_protection_and_its_own_registered_effect_only_ever_resolves_the_registered_one() {
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let attacker = ObjectId(0);
    let protected = ObjectId(1);
    let mut state = combat_damage_state(CombatState {
        attackers: vec![(attacker, Target::Player(p1))],
        blocks: vec![(protected, attacker)],
        declared_defenders: vec![p1],
        bands: Vec::new(),
    });
    put_on_battlefield(&mut state, permanent(attacker, vanilla_creature(0, "Bear", 3, 3, Some(ColorSet::RED)), p0));
    let protected_definition =
        creature_definition(1, "Protected", 0, 4, None, STATIC_PROTECTION_FROM_RED_AND_PREVENTS_ALL_DAMAGE_TO_SELF);
    put_on_battlefield(&mut state, permanent(protected, protected_definition.clone(), p1));
    ScriptRuntime::new()
        .run_card_script(&mut state, &protected_definition, protected)
        .expect("both abilities register cleanly");
    assert_eq!(state.replacement_effects.len(), 1, "only the registered PreventDamage effect lives in state.replacement_effects");

    let mut engine = Engine::new(state);
    let status = assign_combat_damage(&mut engine, p0);

    // Only one real candidate (the registered effect) - protection's own
    // sentinel id collides with it and is folded into the same single
    // match, so no ordering pause is offered at all here.
    assert!(matches!(status, StepStatus::AwaitingAction { .. }));
    assert_eq!(engine.state().object(protected).unwrap().damage_marked, 0, "damage is still prevented either way - both effects agree on the outcome here");
}
