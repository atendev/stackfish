use crate::types::card::CardTypeSet;
use crate::types::command::{EngineCommand, PlayerActionKind};
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::PlayerId;
use crate::types::turn::Phase;
use crate::types::zone::ZoneKind;

/// Carrying out an already-validated `EngineCommand` failed partway —
/// distinct from `ActionError` (engine.rs), which rejects an action before
/// it's even accepted. Reaching a `CommandError` mid-execution should be
/// rare/exceptional once cost/target validation happens up front; this
/// exists mainly for cases validation can't fully rule out ahead of time.
#[derive(Debug)]
pub enum CommandError {
    CostNotPaid,
    InvalidTarget,
    IllegalAction(String),
}

/// Whether/how priority should move as a result of one `EngineCommand`.
/// Returned explicitly by `execute_command` rather than the command
/// mutating `state.priority_player` itself, so `engine.rs` is the one
/// place that actually writes that field — same reasoning as
/// `CommandOutcome::broke_pass_chain` not mutating `consecutive_passes`
/// directly.
pub enum PriorityHandoff {
    /// Most variants: priority stays with whoever currently has it.
    Unchanged,
    /// `PassPriority` (CR 117.3c): priority moves to the next player in
    /// `state.turn_order`. Carries the resolved target player rather than
    /// engine.rs re-deriving "next" from `turn_order`, so `execute_command`
    /// (which already has `state` to hand) stays the single place that
    /// knows the rotation rule.
    To(PlayerId),
}

/// What carrying out one `EngineCommand` produced, for `engine.rs` to act
/// on. Returned explicitly rather than left for `engine.rs` to infer from
/// `state` afterward — see `broke_pass_chain`/`priority_handoff` below.
pub struct CommandOutcome {
    /// Proposed events, not yet final — `engine.rs` runs these through
    /// `ReplacementCheck` -> `CommitEvents` to allocate ids and append to
    /// `state.ledger`. See `execute_command`'s doc comment for why this
    /// function doesn't write the ledger itself.
    pub events: Vec<GameEventKind>,
    /// `false` only for `PassPriority`; `true` for every other variant.
    /// CR 117.3c: taking any action other than passing breaks a
    /// consecutive-pass count. `engine.rs` owns `state.consecutive_passes`
    /// entirely (increment on `false`, reset on `true`) and decides "did
    /// everyone just pass" from that — this flag is the one thing it needs
    /// from execution to do so, rather than relying on every match arm
    /// here remembering to mutate `consecutive_passes` itself correctly.
    pub broke_pass_chain: bool,
    /// See `PriorityHandoff`. `engine.rs` applies this to
    /// `state.priority_player` itself.
    pub priority_handoff: PriorityHandoff,
}

/// CR 117.1a's "sorcery speed" window — `player` may take an action that
/// requires it (playing a land, casting a sorcery, most activated
/// abilities) only when all of: they hold priority, it's their own turn
/// (CR 305.3), it's a main phase of that turn (CR 305.1), and the stack is
/// empty (CR 307.1/305.1). Pulled out as a standalone predicate rather than
/// inlined into `PlayLand` alone, since `CastSpell`'s sorcery-timing branch
/// and (most) `ActivateAbility` checks will need the exact same condition
/// once they're real.
///
/// TODO: doesn't account for effects that grant timing permission beyond
/// the base rule (e.g. "you may play an additional land," "you may cast
/// this spell as though it had flash," "you may play lands as though it
/// were a main phase") - those are all CR 113.6-style rules-text
/// permissions layered on top of this base check, and nothing here
/// consults state.continuous_effects/abilities for them yet. Revisit once
/// such an effect needs representing.
fn is_sorcery_speed(state: &GameState, player: PlayerId) -> bool {
    state.priority_player == player
        && state.active_player == player
        && state.phase == Phase::Main
        && state.stack.is_empty()
}

/// Executes one already-accepted `EngineCommand` against `state`, mutating
/// non-ledger state directly (zones, etc.) and returning a
/// `CommandOutcome`. Per the rationale in `command.rs`: this does not
/// decompose `command` into further sub-commands — the sub-steps of
/// carrying it out are events, not commands.
///
/// Does NOT append to `state.ledger` and does NOT allocate `EventId`s/
/// `SequenceNo`s — those, plus replacement/prevention checks (CR 614-616),
/// are engine.rs's job (`ReplacementCheck` -> `CommitEvents`) so there is
/// exactly one place that writes the ledger. Does NOT mutate
/// `state.consecutive_passes` or `state.priority_player` either — see
/// `CommandOutcome::broke_pass_chain`/`priority_handoff`.
pub fn execute_command(
    state: &mut GameState,
    command: EngineCommand,
) -> Result<CommandOutcome, CommandError> {
    match command.action {
        PlayerActionKind::PassPriority => {
            // CR 117.3c: priority moves to the next player in turn_order
            // after whoever just passed. Read-only against turn_order -
            // does not touch state.priority_player/consecutive_passes,
            // both of which engine.rs owns (see PriorityHandoff/
            // broke_pass_chain doc comments above).
            let current_idx = state
                .turn_order
                .iter()
                .position(|p| *p == command.actor)
                .ok_or_else(|| {
                    CommandError::IllegalAction("actor not in turn_order".to_string())
                })?;
            let next_idx = (current_idx + 1) % state.turn_order.len();
            return Ok(CommandOutcome {
                events: vec![GameEventKind::PriorityPassed { player: command.actor }],
                broke_pass_chain: false,
                priority_handoff: PriorityHandoff::To(state.turn_order[next_idx]),
            });
        }
        PlayerActionKind::MakeChoice { .. } => {
            // Real choice application is genuinely generic - "depends
            // entirely on what asked for the choice" (see the TODO block
            // below) - no fixed event shape exists yet, so this produces
            // none. Exists as a minimal non-PassPriority stub so
            // broke_pass_chain's reset wiring (CR 117.3c) has something
            // other than PassPriority to exercise in tests; not a real
            // implementation of MakeChoice itself.
            return Ok(CommandOutcome {
                events: Vec::new(),
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
            });
        }
        PlayerActionKind::PlayLand { card } => {
            // CR 305.1: lands aren't spells, so this goes straight to the
            // battlefield - no stack object. Playing a land is still
            // restricted to sorcery speed (CR 305.1/305.3, via
            // is_sorcery_speed) and to once per turn (CR 305.2).
            if !is_sorcery_speed(state, command.actor) {
                return Err(CommandError::IllegalAction(
                    "a land can only be played at sorcery speed: actor's turn, a main phase, \
                     actor holding priority, and an empty stack"
                        .to_string(),
                ));
            }
            // TODO(CR 305.2): hardcoded against the literal `1` - doesn't
            // read state.continuous_effects for anything that raises a
            // player's land-plays-per-turn limit (e.g. "you may play an
            // additional land each turn"). continuous_effects has no query
            // API yet (layers.rs is still mostly todo!()) - once it does,
            // this needs "limit = 1 + sum of applicable continuous
            // effects" instead of the bare literal below.
            let already_played =
                *state.lands_played_this_turn.get(&command.actor).unwrap_or(&0);
            if already_played >= 1 {
                return Err(CommandError::IllegalAction(
                    "actor has already played their land for this turn".to_string(),
                ));
            }

            let object = state.object(card).ok_or(CommandError::InvalidTarget)?;
            if !object.definition.card_types.contains(CardTypeSet::LAND) {
                return Err(CommandError::IllegalAction("card is not a Land".to_string()));
            }
            if object.zone != ZoneKind::Hand || object.controller != command.actor {
                return Err(CommandError::IllegalAction(
                    "card is not in actor's hand".to_string(),
                ));
            }
            // Snapshot before the move - CR 603.10's last_known_info (see
            // GameEventKind::ZoneChange's doc comment).
            let last_known_info = object.clone();

            state
                .zone_mut(ZoneKind::Hand, Some(command.actor))
                .ok_or_else(|| CommandError::IllegalAction("actor has no hand zone".to_string()))?
                .remove(card);
            state
                .zone_mut(ZoneKind::Battlefield, None)
                .ok_or_else(|| CommandError::IllegalAction("no battlefield zone".to_string()))?
                .push_bottom(card);
            state.object_mut(card).expect("looked up above").zone = ZoneKind::Battlefield;
            *state.lands_played_this_turn.entry(command.actor).or_insert(0) += 1;

            return Ok(CommandOutcome {
                events: vec![GameEventKind::ZoneChange {
                    object: card,
                    from: ZoneKind::Hand,
                    to: ZoneKind::Battlefield,
                    last_known_info: Some(last_known_info),
                }],
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
            });
        }
        _ => {}
    }
    // TODO: match command.action (PlayerActionKind) and handle each variant:
    //   CastSpell: move card ObjectId from hand to stack zone, create a
    //     StackObject (needs the card's registered "cast" operation
    //     ScriptHandle - where that comes from for a plain card-as-spell,
    //     not just abilities, isn't settled yet), deduct cost (after
    //     SubmitPayment has separately validated/paid it).
    //     broke_pass_chain = true, priority_handoff = Unchanged.
    //   ActivateAbility: similar, but source stays where it is (CR 602) -
    //     only the ability goes on the stack, look up the RegisteredAbility
    //     by source+ability_index in state.abilities.
    //     broke_pass_chain = true, priority_handoff = Unchanged.
    //   PassPriority/PlayLand are implemented above (the match arms before
    //     this comment block) - everything else here is still unimplemented.
    //   DeclareAttackers/DeclareBlockers: validate legality (CR 508/509)
    //     and record the declarations into combat state - GameState doesn't
    //     have a dedicated combat-state field yet, may need one.
    //     broke_pass_chain = true, priority_handoff = Unchanged.
    //   SubmitPayment: validate via PaymentPlan::validate, commit if valid.
    //     broke_pass_chain = true, priority_handoff = Unchanged.
    //   MakeChoice: generic - depends entirely on what asked for the
    //     choice. broke_pass_chain = true, priority_handoff = Unchanged.
    // Every branch ends in returning
    // Ok(CommandOutcome { events, broke_pass_chain, priority_handoff }).
    todo!()
}
