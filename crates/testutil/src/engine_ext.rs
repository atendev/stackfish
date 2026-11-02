use stackfish_engine::engine::{ActionError, Engine, StepStatus};
use stackfish_engine::sba::SbaOutcome;
use stackfish_engine::*;

/// Runs `engine.step()` until it stops making progress and returns the terminal `StepStatus`.
pub fn drain(engine: &mut Engine) -> StepStatus {
    loop {
        match engine.step().status {
            StepStatus::Progressed => continue,
            other => return other,
        }
    }
}

/// `p0` passes priority, then `p1`, draining after each pass. Returns the status after `p1`'s pass.
/// Paying a cost only puts a spell on the stack (CR 601.2i); both passes let it resolve (CR 608.1).
pub fn pass_both_and_drain(engine: &mut Engine, p0: PlayerId, p1: PlayerId) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player: p0,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine);
    engine
        .submit_action(PlayerAction {
            player: p1,
            kind: PlayerActionKind::PassPriority,
        })
        .unwrap();
    drain(engine)
}

/// Passes priority with whoever holds it until the game reaches the declare blockers step.
/// Uses the real turn-step pipeline because `Engine::state()` is read-only. Panics after ten passes.
pub fn pass_to_declare_blockers(engine: &mut Engine) {
    for _ in 0..10 {
        if engine.state().step == Step::DeclareBlockers {
            return;
        }
        let active = engine.state().priority_player;
        engine
            .submit_action(PlayerAction {
                player: active,
                kind: PlayerActionKind::PassPriority,
            })
            .expect("priority window");
        drain(engine);
    }
    panic!(
        "never reached DeclareBlockers - state.step = {:?}",
        engine.state().step
    );
}

/// Casts `card` with no modes, X, alternative cost or buyback, and returns the engine's verdict.
/// Use `cast_spell` when the cast must be accepted.
pub fn try_cast_spell(
    engine: &mut Engine,
    player: PlayerId,
    card: ObjectId,
    targets: Vec<Target>,
) -> Result<(), ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::CastSpell {
            card,
            modes: Vec::new(),
            targets,
            x_value: None,
            alternative_cost: None,
            pay_buyback: false,
        },
    })
}

/// `try_cast_spell` that panics if the engine rejects the cast.
/// Does not drain, so the caller sees the payment pause.
pub fn cast_spell(engine: &mut Engine, player: PlayerId, card: ObjectId, targets: Vec<Target>) {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::CastSpell {
                card,
                modes: Vec::new(),
                targets,
                x_value: None,
                alternative_cost: None,
                pay_buyback: false,
            },
        })
        .expect("player holds priority, spell is in hand");
}

/// Drains to the payment pause of a free cast, pays it with an empty plan, then drains again.
/// Panics if the engine does not pause for the payment CR 118.5a still requires.
pub fn pay_free_and_drain(engine: &mut Engine) -> StepStatus {
    let status = drain(engine);
    assert!(
        matches!(status, StepStatus::AwaitingPayment { .. }),
        "expected a pause for the cast's (free) cost, got {status:?}"
    );
    engine
        .submit_payment(PaymentPlan {
            payments: Vec::new(),
        })
        .expect("empty plan pays a {0} cost");
    drain(engine)
}

/// Submits `player`'s attack declaration and drains on success.
/// A rejected declaration comes back as the `Err` without draining.
pub fn declare_attackers(
    engine: &mut Engine,
    player: PlayerId,
    attackers: Vec<(ObjectId, Target)>,
) -> Result<StepStatus, ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::DeclareAttackers { attackers },
    })?;
    Ok(drain(engine))
}

/// Submits `player`'s block declaration as `(blocker, attacker)` pairs and drains on success.
/// A rejected declaration comes back as the `Err` without draining.
pub fn declare_blockers(
    engine: &mut Engine,
    player: PlayerId,
    blocks: Vec<(ObjectId, ObjectId)>,
) -> Result<StepStatus, ActionError> {
    engine.submit_action(PlayerAction {
        player,
        kind: PlayerActionKind::DeclareBlockers { blocks },
    })?;
    Ok(drain(engine))
}

/// Submits `player`'s combat damage assignment and drains.
/// Panics if the engine rejects the assignment.
pub fn assign_combat_damage(
    engine: &mut Engine,
    player: PlayerId,
    assignments: Vec<(ObjectId, Vec<(ObjectId, u32)>)>,
) -> StepStatus {
    engine
        .submit_action(PlayerAction {
            player,
            kind: PlayerActionKind::AssignCombatDamage { assignments },
        })
        .expect("turn-based action, no priority required");
    drain(engine)
}

/// `player`'s life total. Panics if the player does not exist.
pub fn life(engine: &Engine, player: PlayerId) -> i32 {
    engine.state().player(player).unwrap().life
}

/// The most recent `ZoneChange` in the ledger as `(object, from, to)`. Panics if there is none.
/// Each move mints a new `ObjectId` (CR 400.7), so tests read this, not the original id.
pub fn last_zone_change(engine: &Engine) -> (ObjectId, ZoneKind, ZoneKind) {
    for event in engine.state().ledger.iter().rev() {
        if let GameEventKind::ZoneChange {
            object, from, to, ..
        } = event.kind
        {
            return (object, from, to);
        }
    }
    panic!("expected a ZoneChange event in the ledger");
}

/// The events of an SBA pass that finished without pausing.
/// Panics on any choice pause. No fixture using it registers a replacement on a dying
/// creature, so a CR 616.1 pause means the fixture is wrong.
pub fn progressed(outcome: SbaOutcome) -> Vec<GameEvent> {
    match outcome {
        SbaOutcome::Progressed(events, _) => events,
        SbaOutcome::NeedsOrderingChoice { .. } => {
            panic!("fixture didn't expect a replacement ordering-choice pause")
        }
        SbaOutcome::NeedsLegendRuleChoice { .. } => {
            panic!("fixture didn't expect a legend-rule choice pause")
        }
        SbaOutcome::NeedsCommanderReturnChoice { .. } => {
            panic!("fixture didn't expect a commander-return choice pause")
        }
    }
}
