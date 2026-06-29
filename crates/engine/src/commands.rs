use crate::types::ability::AbilityEffect;
use crate::types::ability::AbilityKind;
use crate::types::card::CardTypeSet;
use crate::types::command::{EngineCommand, PlayerActionKind};
use crate::types::cost::CostSpec;
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId, ScriptHandle};
use crate::types::stack::{StackEntryKind, StackObject};
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
    /// CR 601.2a puts a spell on the stack *before* its cost is determined/
    /// paid (601.2f-h) — set by `CastSpell` once it's pushed `stack_id` onto
    /// `state.stack`, so `engine.rs` can pause the rest of this pipeline run
    /// on `ProcessState::AwaitingPayment` until `Engine::submit_payment`
    /// answers, rather than this function deducting mana synchronously
    /// (which would assume the cost is always payable/free, and skip the
    /// CR 118.5a "must explicitly acknowledge a {0} cost" requirement).
    /// `None` for every other variant — nothing else here puts something on
    /// the stack pending payment.
    pub requires_payment: Option<PendingPaymentRequest>,
}

/// See `CommandOutcome::requires_payment`'s doc comment for why this exists
/// as its own pause rather than a synchronous deduction.
pub struct PendingPaymentRequest {
    pub stack_id: ObjectId,
    pub payer: PlayerId,
    pub cost: CostSpec,
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
                requires_payment: None,
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
                requires_payment: None,
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

            // CR 400.7: this zone change creates a new object - mint a fresh
            // ObjectId rather than reusing `card`'s, and link back to it via
            // ObjectLineage. `card`'s entry in state.objects is removed
            // entirely once moved; anything that still needs to know what it
            // looked like in the hand must use `last_known_info` above, not
            // a live lookup by the old id.
            let new_id = state.allocate_object_id();
            let mut new_object = last_known_info.clone();
            new_object.object_id = new_id;
            new_object.lineage = ObjectLineage { previous: Some(card) };
            new_object.zone = ZoneKind::Battlefield;
            new_object.timestamp = state.allocate_sequence_no();

            state
                .zone_mut(ZoneKind::Hand, Some(command.actor))
                .ok_or_else(|| CommandError::IllegalAction("actor has no hand zone".to_string()))?
                .remove(card);
            state.objects.remove(&card);
            state
                .zone_mut(ZoneKind::Battlefield, None)
                .ok_or_else(|| CommandError::IllegalAction("no battlefield zone".to_string()))?
                .push_bottom(new_id);
            state.objects.insert(new_id, new_object);
            *state.lands_played_this_turn.entry(command.actor).or_insert(0) += 1;

            return Ok(CommandOutcome {
                events: vec![GameEventKind::ZoneChange {
                    object: new_id,
                    from: ZoneKind::Hand,
                    to: ZoneKind::Battlefield,
                    last_known_info: Some(last_known_info),
                }],
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                requires_payment: None,
            });
        }
        PlayerActionKind::CastSpell { card, modes, targets, x_value } => {
            let object = state.object(card).ok_or(CommandError::InvalidTarget)?;
            if object.definition.card_types.contains(CardTypeSet::LAND) {
                return Err(CommandError::IllegalAction(
                    "lands aren't cast as spells - use PlayLand".to_string(),
                ));
            }
            if object.zone != ZoneKind::Hand || object.controller != command.actor {
                return Err(CommandError::IllegalAction(
                    "card is not in actor's hand".to_string(),
                ));
            }
            // TODO(CR 117.1a/602's flash, not modeled here yet): instants
            // may be cast any time the actor holds priority; anything else
            // castable as a spell needs the full sorcery-speed window
            // (is_sorcery_speed already covers "holds priority" as one of
            // its four conditions, so the instant branch below only adds
            // the priority check on its own rather than duplicating it).
            // Cards with flash (or effects granting flash) aren't
            // represented on CardDefinition/continuous_effects yet, so they
            // incorrectly fall into the non-instant branch below - same
            // class of gap is_sorcery_speed already flags for PlayLand.
            let is_instant = object.definition.card_types.contains(CardTypeSet::INSTANT);
            if is_instant {
                if state.priority_player != command.actor {
                    return Err(CommandError::IllegalAction(
                        "actor does not hold priority".to_string(),
                    ));
                }
            } else if !is_sorcery_speed(state, command.actor) {
                return Err(CommandError::IllegalAction(
                    "this spell can only be cast at sorcery speed: actor's turn, a main \
                     phase, actor holding priority, and an empty stack"
                        .to_string(),
                ));
            }

            // TODO(CR 601.2b-h): modes/targets/x_value are accepted and
            // stored on the resulting StackObject below, but none of them
            // are validated for legality yet - no mode-legality check, no
            // target-legality check (range/restriction/count per CR
            // 601.2c), no division/distribution step (CR 601.2d), and no
            // legality recheck (CR 601.2e). Also no splice (CR 601.2b/
            // 702.47) - a player revealing other cards in hand to splice
            // their text onto this spell isn't modeled at all; PlayerAction
            // has nowhere to carry "which cards I'm splicing," and
            // StackObject has nowhere to record it either.

            // CR 601.2f: total cost = the card's printed mana_cost only -
            // no additional/alternative costs, no reductions yet (see
            // CostSpec::mana_only's own doc, and PaymentPlan::validate's
            // "out of scope" list). x_value isn't folded into the generic
            // component yet either - same gap CostSpec/ManaCost flag.
            let cost = CostSpec::mana_only(object.definition.mana_cost.clone());

            // Snapshot before the move - CR 603.10's last_known_info (see
            // GameEventKind::ZoneChange's doc comment).
            let last_known_info = object.clone();

            // CR 400.7/601.2a: casting moves the card to the stack, which is
            // a zone change that mints a new ObjectId (same convention
            // PlayLand uses for hand->battlefield) - see ObjectLineage.
            let new_id = state.allocate_object_id();
            let mut new_object = last_known_info.clone();
            new_object.object_id = new_id;
            new_object.lineage = ObjectLineage { previous: Some(card) };
            new_object.zone = ZoneKind::Stack;
            new_object.timestamp = state.allocate_sequence_no();

            state
                .zone_mut(ZoneKind::Hand, Some(command.actor))
                .ok_or_else(|| CommandError::IllegalAction("actor has no hand zone".to_string()))?
                .remove(card);
            state.objects.remove(&card);
            state.objects.insert(new_id, new_object);

            // TODO(script bridge): `operation` should be the card's
            // registered cast/resolution callback (looked up via its
            // ScriptReference) - no such lookup exists yet, same gap as
            // every other ScriptHandle placeholder in this codebase.
            // ScriptHandle(0) here is a stand-in with nothing real bound to
            // it; resolving this StackObject can't actually run spell
            // effects until the script bridge fills this in for real.
            state.stack.push(StackObject::new(
                new_id,
                StackEntryKind::Spell,
                new_id,
                command.actor,
                modes,
                targets,
                x_value,
                ScriptHandle(0),
            ));

            return Ok(CommandOutcome {
                events: vec![
                    GameEventKind::ZoneChange {
                        object: new_id,
                        from: ZoneKind::Hand,
                        to: ZoneKind::Stack,
                        last_known_info: Some(last_known_info),
                    },
                    GameEventKind::StackObjectPushed { stack_id: new_id },
                ],
                // CR 601.2i: the controller already held priority before
                // casting (checked above), so they keep it - it doesn't move.
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                // CR 601.2f-h: cost isn't paid yet just by being on the
                // stack - engine.rs pauses on AwaitingPayment until
                // Engine::submit_payment answers, even for a {0} cost (CR
                // 118.5a still requires an explicit empty acknowledgment).
                requires_payment: Some(PendingPaymentRequest {
                    stack_id: new_id,
                    payer: command.actor,
                    cost,
                }),
            });
        }
        PlayerActionKind::ActivateAbility { source, ability_index, targets } => {
            // CR 602.2a/117.3c: activating an ability follows the same
            // process as casting a spell - the activator must hold priority
            // to begin with.
            if state.priority_player != command.actor {
                return Err(CommandError::IllegalAction(
                    "actor does not hold priority".to_string(),
                ));
            }

            let controller = state.object(source).ok_or(CommandError::InvalidTarget)?.controller;
            if controller != command.actor {
                return Err(CommandError::IllegalAction(
                    "actor does not control the ability's source".to_string(),
                ));
            }

            let ability = state
                .abilities
                .iter()
                .filter(|a| a.source == source)
                .nth(ability_index as usize)
                .ok_or_else(|| {
                    CommandError::IllegalAction("no such ability on source".to_string())
                })?;
            if ability.kind != AbilityKind::Activated {
                return Err(CommandError::IllegalAction(
                    "ability is not an activated ability".to_string(),
                ));
            }
            let effect = ability.effect.clone();

            // TODO(CR 602.5a): tap-cost abilities need a summoning-sickness
            // check (actor must have controlled source continuously since
            // their most recent turn began) - no "controlled since"
            // timestamp exists on GameObject yet, so this isn't checked.
            match effect {
                Some(AbilityEffect::AddMana(units)) => {
                    // CR 605.1a: a mana ability never has a target.
                    if !targets.is_empty() {
                        return Err(CommandError::IllegalAction(
                            "a mana ability cannot have targets".to_string(),
                        ));
                    }
                    let already_tapped =
                        state.object(source).ok_or(CommandError::InvalidTarget)?.tapped;
                    if already_tapped {
                        return Err(CommandError::IllegalAction(
                            "source is already tapped".to_string(),
                        ));
                    }

                    state.object_mut(source).ok_or(CommandError::InvalidTarget)?.tapped = true;
                    let pool = &mut state
                        .player_mut(command.actor)
                        .ok_or_else(|| {
                            CommandError::IllegalAction("actor has no player state".to_string())
                        })?
                        .mana_pool;
                    for unit in &units {
                        pool.add_unit(*unit);
                    }

                    // CR 605.3b: a mana ability resolves immediately as part
                    // of activation - it never uses the stack.
                    return Ok(CommandOutcome {
                        events: vec![
                            GameEventKind::Tapped { object: source },
                            GameEventKind::ManaAdded { player: command.actor, units },
                        ],
                        broke_pass_chain: true,
                        priority_handoff: PriorityHandoff::Unchanged,
                        requires_payment: None,
                    });
                }
                None => {
                    // TODO(script bridge): a non-mana activated ability
                    // needs cost/target/operation script callbacks to
                    // determine its real cost, validate targets, push a
                    // StackObject (CR 602.2a-b mirrors casting a spell), and
                    // eventually resolve via its operation - none of that
                    // exists yet (same gap every other ability/operation
                    // hits). Structural lookup/validation above (source
                    // exists, actor controls it, the ability exists and is
                    // Activated) is real; everything past here needs the
                    // script bridge.
                    todo!()
                }
            }
        }
        _ => {}
    }
    // TODO: match command.action (PlayerActionKind) and handle each variant:
    //   CastSpell is implemented above.
    //   ActivateAbility (mana-ability case) is implemented above; non-mana
    //     abilities fall through to todo!() pending the script bridge.
    //   PassPriority/PlayLand are implemented above (the match arms before
    //     this comment block) - everything else here is still unimplemented.
    //   DeclareAttackers/DeclareBlockers: validate legality (CR 508/509)
    //     and record the declarations into combat state - GameState doesn't
    //     have a dedicated combat-state field yet, may need one.
    //     broke_pass_chain = true, priority_handoff = Unchanged.
    //   SubmitPayment is NOT handled here, deliberately - see
    //     Engine::submit_payment (engine.rs) and CommandOutcome::
    //     requires_payment's doc comment above. Payment answers
    //     ProcessState::AwaitingPayment, a mid-pipeline pause, the same way
    //     submit_replacement_choice/submit_trigger_order answer their own
    //     pauses - never goes through submit_action/execute_command at all.
    //   MakeChoice: generic - depends entirely on what asked for the
    //     choice. broke_pass_chain = true, priority_handoff = Unchanged.
    // Every branch ends in returning
    // Ok(CommandOutcome { events, broke_pass_chain, priority_handoff }).
    todo!()
}
