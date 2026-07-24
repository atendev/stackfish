//! Phase 5 vertical slice: backlog item 9a, `Game.MayChoose` (CR 608.2d) — a
//! plain yes/no choice mid-resolution, same Lua coroutine mechanism backlog
//! item 9 (`Game.SelectTargets`, see `phase5_script_target_selection.rs`)
//! already built. `Game.MayChoose()` is a plain Lua function that calls
//! `coroutine.yield({ op = "may_choose" })` directly, mirroring
//! `Game.SelectTargets`'s own reason (mlua 0.9 without the `async` feature
//! can't yield from inside a Rust-bound closure). `Engine::
//! submit_script_may_choice(bool)` answers the pause, resuming the same
//! suspended script with the chosen boolean.
//!
//! Scoped narrowly on purpose: a plain yes/no with no filter to satisfy (any
//! `bool` answer is legal, unlike `SelectTarget`'s `target_is_legal` check).
//! True modal spell mode-selection (CR 700.2a, chosen before the stack as
//! part of casting) is a different, unattempted mechanism - see this
//! backlog item's own split-off follow-up.
//!
//! Uses real card-script self-registration (`Game.CreateActivatedAbility`),
//! same convention `phase5_script_target_selection.rs` already established.

use stackfish_engine::engine::{Engine, ScriptChoiceError, StepStatus};
use stackfish_engine::script::ScriptChoiceRequest;
use stackfish_engine::*;
use stackfish_testutil::{card_object, drain, single_player_state};

/// Answering `true` gains 4 life, `false` gains 1 - two distinct,
/// unambiguous life totals prove which branch the resumed boolean actually
/// took, without needing a new effect-shape key just for this test.
const SELF_REGISTERING_MAY_CHOOSE: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateActivatedAbility(c)
    e:SetOperation(function(source, controller)
        if Game.MayChoose() then
            return { gain_life = 4 }
        end
        return { gain_life = 1 }
    end)
    e:Register()
end
return s
"#;

const SELF_REGISTERING_NEVER_YIELDS: &str = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateActivatedAbility(c)
    e:SetOperation(function(source, controller)
        return { gain_life = 1 }
    end)
    e:Register()
end
return s
"#;

fn land_definition(script: &str) -> CardDefinition {
    CardDefinition {
        definition_id: 1,
        name: "Test May-Choose Land".to_string(),
        mana_cost: ManaCost { symbols: Vec::new() },
        color_indicator: None,
        supertypes: SupertypeSet::empty(),
        card_types: CardTypeSet::LAND,
        subtypes: Vec::new(),
        oracle_text: "{0}: You may gain 4 life. If you don't, gain 1 life instead.".to_string(),
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

fn base_state() -> GameState {
    let p0 = PlayerId(0);
    single_player_state(
        p0,
        vec![
            Zone::new(ZoneKind::Hand, Some(p0)),
            Zone::new(ZoneKind::Battlefield, None),
            Zone::new(ZoneKind::Graveyard, Some(p0)),
        ],
    )
}

/// Plays the self-registering land, then activates its (only) ability and
/// pays its `{0}` cost - drains to whatever the operation's own execution
/// produces next. Same shape as `phase5_script_target_selection.rs`'s own
/// `play_activate_and_drain`, minus the pre-seeded target-eligible objects
/// this slice's script never asks for.
fn play_activate_and_drain(engine: &mut Engine, player: PlayerId, land_in_hand: ObjectId) -> StepStatus {
    engine
        .submit_action(PlayerAction { player, kind: PlayerActionKind::PlayLand { card: land_in_hand } })
        .expect("player holds priority, land is in hand");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingAction { .. }), "expected priority to reopen after playing the land");
    assert_eq!(engine.state().abilities.len(), 1, "the land's activated ability self-registered");

    let land = *engine.state().zone(ZoneKind::Battlefield, None).unwrap().contents.back().unwrap();
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::ActivateAbility {
                source: land,
                ability_index: 0,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
            },
        })
        .expect("player controls the land and holds priority");
    let status = drain(engine);
    assert!(matches!(status, StepStatus::AwaitingPayment { .. }), "expected a pause for the {{0}} cost, got {status:?}");
    engine.submit_payment(PaymentPlan { payments: Vec::new() }).expect("empty plan pays a {0} cost");
    drain(engine);

    engine.submit_action(PlayerAction { player, kind: PlayerActionKind::PassPriority }).unwrap();
    drain(engine)
}

fn seed_land(state: &mut GameState, land: ObjectId, script: &str, p0: PlayerId) {
    state.objects.insert(land, card_object(land, land_definition(script), p0, ZoneKind::Hand));
    state.zone_mut(ZoneKind::Hand, Some(p0)).unwrap().push_bottom(land);
}

#[test]
fn a_script_calling_may_choose_pauses_with_a_may_choose_request() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let mut state = base_state();
    seed_land(&mut state, land, SELF_REGISTERING_MAY_CHOOSE, p0);

    let mut engine = Engine::new(state);
    let status = play_activate_and_drain(&mut engine, p0, land);
    let StepStatus::AwaitingScriptChoice { player, request } = status else {
        panic!("expected the operation to pause on Game.MayChoose, got {status:?}")
    };
    assert_eq!(player, p0);
    assert!(matches!(request, ScriptChoiceRequest::MayChoose), "expected a MayChoose request, got {request:?}");
}

#[test]
fn answering_true_resumes_into_the_yes_branch() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let mut state = base_state();
    seed_land(&mut state, land, SELF_REGISTERING_MAY_CHOOSE, p0);

    let mut engine = Engine::new(state);
    let status = play_activate_and_drain(&mut engine, p0, land);
    assert!(matches!(status, StepStatus::AwaitingScriptChoice { .. }));

    engine.submit_script_may_choice(true).expect("any bool is a legal MayChoose answer");
    let status = drain(&mut engine);
    assert!(matches!(status, StepStatus::AwaitingAction { player } if player == p0));

    assert_eq!(engine.state().player(p0).unwrap().life, 20 + 4, "answering true must take the yes branch");
    assert!(engine.state().stack.is_empty(), "the ability resolved off the stack for good");
}

#[test]
fn answering_false_resumes_into_the_no_branch() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let mut state = base_state();
    seed_land(&mut state, land, SELF_REGISTERING_MAY_CHOOSE, p0);

    let mut engine = Engine::new(state);
    let status = play_activate_and_drain(&mut engine, p0, land);
    assert!(matches!(status, StepStatus::AwaitingScriptChoice { .. }));

    engine.submit_script_may_choice(false).expect("any bool is a legal MayChoose answer");
    drain(&mut engine);

    assert_eq!(engine.state().player(p0).unwrap().life, 20 + 1, "answering false must take the no branch");
}

#[test]
fn submit_script_choice_rejects_a_pending_may_choose_request() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let mut state = base_state();
    seed_land(&mut state, land, SELF_REGISTERING_MAY_CHOOSE, p0);

    let mut engine = Engine::new(state);
    play_activate_and_drain(&mut engine, p0, land);

    let err = engine.submit_script_choice(Target::Player(p0)).unwrap_err();
    assert!(matches!(err, ScriptChoiceError::WrongRequestKind), "a target answer can't satisfy a MayChoose pause");
    assert!(matches!(engine.step().status, StepStatus::AwaitingScriptChoice { .. }), "the pause must stay open");
}

#[test]
fn submit_script_may_choice_rejects_a_pending_select_target_request() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let mut state = base_state();
    // Reuses phase5_script_target_selection.rs's own scenario shape inline -
    // a SelectTarget request pending, answered via the wrong method.
    let script = r#"
local s = {}
function s.initial_effect(c)
    local e = Game.CreateActivatedAbility(c)
    e:SetOperation(function(source, controller)
        local target = Game.SelectTargets({ type = "player" })
        return { gain_life = target.id }
    end)
    e:Register()
end
return s
"#;
    seed_land(&mut state, land, script, p0);

    let mut engine = Engine::new(state);
    play_activate_and_drain(&mut engine, p0, land);
    assert!(matches!(engine.step().status, StepStatus::AwaitingScriptChoice { .. }));

    let err = engine.submit_script_may_choice(true).unwrap_err();
    assert!(matches!(err, ScriptChoiceError::WrongRequestKind), "a bool answer can't satisfy a SelectTarget pause");
}

#[test]
fn submit_script_may_choice_rejects_when_not_awaiting() {
    let mut engine = Engine::new(base_state());
    let err = engine.submit_script_may_choice(true).unwrap_err();
    assert!(matches!(err, ScriptChoiceError::NotAwaitingChoice));
}

#[test]
fn a_script_that_never_calls_may_choose_still_resolves_synchronously() {
    let p0 = PlayerId(0);
    let land = ObjectId(0);
    let mut state = base_state();
    seed_land(&mut state, land, SELF_REGISTERING_NEVER_YIELDS, p0);

    let mut engine = Engine::new(state);
    let status = play_activate_and_drain(&mut engine, p0, land);
    assert!(
        matches!(status, StepStatus::AwaitingAction { player } if player == p0),
        "a script that never calls Game.MayChoose must resolve synchronously, not pause: {status:?}"
    );
    assert_eq!(engine.state().player(p0).unwrap().life, 21);
}
