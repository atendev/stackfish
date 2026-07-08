use std::collections::HashMap;
use std::collections::VecDeque;
use std::ops::Range;

use crate::commands::{self, CommandError, PendingPaymentRequest, PriorityHandoff};
use crate::engine_context::{PipelineContext, PipelineExit};
use crate::replacement::{self, ReplacementOutcome};
use crate::sba::{self, SbaOutcome};
use crate::script::{ScriptError, ScriptRuntime};
use crate::triggers::{self, PendingTrigger};
use crate::types::ability::{AbilityEffect, ConditionSpec};
use crate::types::card::CardTypeSet;
use crate::types::config::{GameConfig, GameSettings};
use crate::types::command::{EngineCommand, PlayerAction, PlayerActionKind};
use crate::types::cost::{CostSpec, PaymentPlan};
use crate::types::counter::CounterSet;
use crate::types::effect::EffectDuration;
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::{GameState, RngState};
use crate::types::ids::{CommandId, ObjectId, ObjectLineage, PlayerId, ScriptHandle, SequenceNo};
use crate::types::mana::ManaPool;
use crate::types::object::{GameObject, VisibilityPolicy};
use crate::types::player::PlayerState;
use crate::types::stack::{StackEntryKind, StackObject};
use crate::types::target::TargetSpec;
use crate::types::turn::{Phase, Step};
use crate::types::zone::{Zone, ZoneKind};

/// Why `submit_action` rejected a `PlayerAction` outright (wrong player has
/// priority, action doesn't match what's currently legal, etc). Distinct
/// from in-progress command execution failures, which surface through
/// `CommandError` instead once the action is accepted and being carried out.
#[derive(Debug)]
pub enum ActionError {
    NotYourTurn,
    IllegalAction(String),
}

/// Why `submit_replacement_choice` rejected an answer to a CR 616.1
/// ordering choice.
#[derive(Debug)]
pub enum ReplacementChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingReplacementChoice`.
    NotAwaitingChoice,
    /// `chosen` wasn't one of the offered `candidates`.
    InvalidCandidate,
}

/// Why `submit_trigger_order` rejected an answer to a CR 603.3b
/// intra-group ordering choice.
#[derive(Debug)]
pub enum TriggerOrderError {
    /// Engine isn't sitting in `ProcessState::AwaitTriggerOrderChoice`.
    NotAwaitingChoice,
    /// `chosen_order`'s length doesn't match the offered group's — caller
    /// must supply a reordering of the exact group, not a different set.
    WrongLength,
    /// Same length as the offered group, but not the same multiset of
    /// `PendingTrigger`s — length matching alone doesn't rule out the caller
    /// substituting/dropping/duplicating a trigger while keeping the count
    /// right, so this is checked separately (see `is_permutation`).
    NotAPermutation,
}

/// Why `submit_payment` rejected an answer to a CR 601.2f-h
/// `ProcessState::AwaitingPayment` pause.
#[derive(Debug)]
pub enum PaymentError {
    /// Engine isn't sitting in `ProcessState::AwaitingPayment`.
    NotAwaitingPayment,
    /// `plan` didn't validate against the pending cost (see
    /// `PaymentPlan::validate`) — wrong/missing/extra payments, or the
    /// payer's pool can't actually cover it. State is left untouched either
    /// way: no partial deduction ever happens (see `cost.rs`'s `PaymentPlan`
    /// doc).
    InvalidPlan,
}

/// Why `submit_discard` rejected an answer to a CR 514.1
/// `ProcessState::AwaitingDiscard` pause.
#[derive(Debug)]
pub enum DiscardError {
    /// Engine isn't sitting in `ProcessState::AwaitingDiscard`.
    NotAwaitingDiscard,
    /// Wrong number of cards — exactly `expected` must be discarded.
    WrongCount { expected: usize, got: usize },
    /// Same card id listed more than once.
    DuplicateCard,
    /// A card id isn't in the active player's hand under their control.
    CardNotInHand,
}

/// Why `submit_mulligan_decision` rejected a `KeepHand` or `TakeMulligan`.
#[derive(Debug)]
pub enum MulliganError {
    /// Engine isn't sitting in `ProcessState::AwaitingMulliganDecision`.
    NotAwaitingDecision,
    /// `action.player` isn't the player whose turn it is to decide this round.
    WrongPlayer,
    /// Player submitted `TakeMulligan` but has already reached
    /// `GameSettings::max_mulligans`. They must keep.
    MaxMulligansReached,
    /// `action.kind` wasn't `KeepHand` or `TakeMulligan` — only those two
    /// are valid during a mulligan decision window.
    InvalidAction,
}

/// Why `submit_bottom_cards` rejected a `BottomCards` submission.
#[derive(Debug)]
pub enum BottomCardsError {
    /// Engine isn't sitting in `ProcessState::AwaitingBottomCardChoice`.
    NotAwaitingChoice,
    /// `action.player` isn't the player who needs to bottom cards right now.
    WrongPlayer,
    /// `cards.len()` != the `count` in `AwaitingBottomCardChoice`. Must be
    /// exactly `count` — no more, no fewer.
    WrongCount { expected: usize, got: usize },
    /// A card id in `cards` isn't currently in the player's hand.
    CardNotInHand,
    /// Same card id appears more than once in `cards`.
    DuplicateCard,
}

/// Whether `chosen` is a reordering of `group` — same `PendingTrigger`s
/// (by value), each used exactly once, any order. Used by
/// `submit_trigger_order` to reject a same-length answer that isn't
/// actually a permutation (swapped-in trigger, duplicate, etc).
fn is_permutation(chosen: &[PendingTrigger], group: &[PendingTrigger]) -> bool {
    if chosen.len() != group.len() {
        return false;
    }
    let mut remaining: Vec<PendingTrigger> = group.to_vec();
    for item in chosen {
        match remaining.iter().position(|candidate| candidate == item) {
            Some(idx) => {
                remaining.remove(idx);
            }
            None => return false,
        }
    }
    true
}

/// What happened on one `step()` call, and what the engine needs next.
#[derive(Debug)]
pub enum StepStatus {
    /// Made progress; safe to call `step()` again immediately.
    Progressed,
    /// Needs a `PlayerAction` from `player` before it can continue — the
    /// caller should prompt and `submit_action`, then `step()` again.
    AwaitingAction { player: PlayerId },
    /// CR 616.1: more than one `ReplacementEffect` matched the same event
    /// mid-`ReplacementCheck` — the caller should prompt `player` to pick
    /// one of `candidates` (each a `ReplacementEffect::source`) and call
    /// `submit_replacement_choice`, then `step()` again. Distinct from
    /// `AwaitingAction`: this isn't a `PlayerAction`/priority decision, it's
    /// a sub-decision mid-pipeline that `submit_action` must keep rejecting
    /// while it's pending (see `submit_action`'s `PriorityWindow`-only
    /// check).
    AwaitingReplacementChoice { player: PlayerId, candidates: Vec<ObjectId> },
    /// CR 603.3b: `player` controls more than one trigger that matched the
    /// same batch of events and must choose the order to put them on the
    /// stack — the caller should prompt and call `submit_trigger_order`,
    /// then `step()` again.
    AwaitingTriggerOrderChoice { player: PlayerId, group: Vec<PendingTrigger> },
    /// CR 601.2f-h: `player` cast a spell that's now on the stack
    /// (`stack_id`) but hasn't paid its cost yet — the caller should look
    /// up the cost (e.g. via the card's `CardDefinition.mana_cost` off
    /// `stack_id`), prompt `player` to build a `PaymentPlan`, and call
    /// `submit_payment`, then `step()` again. Same "not a `PlayerAction`/
    /// priority decision" rationale as `AwaitingReplacementChoice` — it has
    /// its own entry point rather than going through `submit_action`.
    AwaitingPayment { player: PlayerId, stack_id: ObjectId },
    /// CR 514.1: `player`'s hand exceeds `max_hand_size` (normally 7) at the
    /// start of cleanup — the caller should prompt `player` to choose exactly
    /// `hand_size - max_hand_size` cards to discard and call `submit_discard`,
    /// then `step()` again. Separate entry point from `submit_action` for the
    /// same reason as `AwaitingPayment` — this is a mandatory cleanup action,
    /// not a priority decision.
    AwaitingDiscard { player: PlayerId, hand_size: usize, max_hand_size: usize },
    /// A command failed mid-pipeline. The engine resets to `PriorityWindow`
    /// so it doesn't get stuck — it does NOT retry or roll back whatever
    /// partial mutation happened before the error, since `execute_command`
    /// doesn't yet guarantee atomicity. Surfacing that gap honestly rather
    /// than hiding it behind a panic.
    Error(CommandError),
    GameOver { winner: Option<PlayerId> },
    /// CR 103.5 pregame: `player` must submit `KeepHand` or `TakeMulligan`
    /// via `Engine::submit_mulligan_decision` before `step()` can continue.
    AwaitingMulliganDecision { player: PlayerId },
    /// CR 103.5 pregame: `player` must put exactly `count` cards from their
    /// hand onto the bottom of their library via `Engine::submit_bottom_cards`.
    AwaitingBottomCardChoice { player: PlayerId, count: usize },
}

/// Result of one `step()` call. `events_added` indexes into
/// `Engine::state().ledger` rather than cloning events out, since the
/// ledger already holds them — this just tells the caller which slice is
/// new since the call.
pub struct StepResult {
    pub events_added: Range<usize>,
    pub status: StepStatus,
}

/// The pipeline stage `Engine` is currently sitting in — the explicit
/// state machine behind `step()`. One `step()` call generally advances by
/// one variant; `SbaLoop` is the exception, re-entering itself across
/// multiple `step()` calls until CR 704.3's fixed point is reached, so a
/// replay viewer can observe each individual SBA pass rather than the loop
/// being opaque.
///
/// `COLLECT_ACTIONS` and `PRIORITY_WINDOW` (in the human-design version
/// this came from) are merged into `PriorityWindow` here: there's no
/// `Engine::legal_actions()` yet to do a real "collect" step against, so
/// there's nothing to separate it from "waiting for a submit_action" yet.
/// Revisit if/when legal-action enumeration becomes a real feature.
///
/// Which pipeline a paused `ProcessState::AwaitingReplacementChoice` should
/// resume into once `Engine::submit_replacement_choice` answers — the pause
/// itself (event/candidates/excluded) is identical either way; only what
/// happens *after* the choice differs.
pub enum ReplacementResumeTarget {
    /// Reached from a command's own proposed events (`CastSpell`/
    /// `ActivateAbility`/combat damage's `DamageDealt`, ...) —
    /// `remaining`/`checked` carry the rest of the same `ReplacementCheck`
    /// run, same contract that struct's own fields already had.
    ReplacementCheck { remaining: VecDeque<GameEventKind>, checked: Vec<GameEventKind> },
    /// Reached from inside an SBA pass instead (`sba::resolve_deaths`, CR
    /// 614.13's "if this would die, instead ..."). `pending_deaths` is the
    /// rest of this pass's dying-creature list still to check; `then` says
    /// which state originally started this pass, since `SbaLoop`'s and
    /// `CleanupSbaCheck`'s own "nothing more to do" transitions differ.
    SbaDeath { pending_deaths: VecDeque<ObjectId>, then: SbaResumeThen },
}

/// See `ReplacementResumeTarget::SbaDeath`'s own doc comment.
pub enum SbaResumeThen {
    SbaLoop,
    CleanupSbaCheck,
}

pub enum ProcessState {
    PriorityWindow,
    /// Accepted by `submit_action`, waiting for the next `step()` to
    /// execute it via `commands::execute_command`.
    PendingAction(PlayerAction),
    /// All players passed in a row with a non-empty stack (CR 117.4) —
    /// resolve the top `StackObject` next.
    ResolveStackTop,
    /// Proposed events waiting on replacement/prevention checks (CR
    /// 614-616) before they're allowed to commit. `checked` accumulates
    /// events already resolved by earlier iterations of this same
    /// `ReplacementCheck` run; `remaining` is what's still unprocessed — a
    /// `VecDeque` so it can be drained front-to-back (`pop_front`) in the
    /// order it was built, rather than a `Vec`'s `pop()` silently reversing
    /// processing order.
    ReplacementCheck { remaining: VecDeque<GameEventKind>, checked: Vec<GameEventKind> },
    /// CR 616.1: paused because `pending_event` matched more than one
    /// `ReplacementEffect` (`candidates`, identified by `source` `ObjectId`)
    /// and `affected_player` must choose which applies first. `excluded` is
    /// `replacement::ReplacementOutcome::NeedsOrderingChoice`'s own
    /// per-event exclusion set (CR 616.1: an effect applies at most once to
    /// a given event) — carried here so resuming doesn't forget which
    /// effects already applied earlier in this same event's resolution.
    /// `resume` says what to do once `submit_replacement_choice` answers —
    /// this pause is reached from two different pipelines (a command's own
    /// events via `ReplacementCheck`, or a dying creature's zone-change via
    /// `sba::resolve_deaths`), and only `resume` distinguishes them; the
    /// pause itself is identical either way.
    AwaitingReplacementChoice {
        affected_player: PlayerId,
        pending_event: GameEventKind,
        candidates: Vec<ObjectId>,
        excluded: Vec<ObjectId>,
        resume: ReplacementResumeTarget,
    },
    /// Checked events ready to become real `GameEvent`s in the ledger.
    CommitEvents { checked: Vec<GameEventKind> },
    /// CR 704.3's fixed-point loop.
    SbaLoop,
    /// Collecting triggered abilities that matched ledger events from
    /// index `since` onward.
    CollectTriggers { since: usize },
    /// Triggers collected and grouped by controller in APNAP order
    /// (`triggers::group_apnap`) — waiting to be pushed to the stack.
    /// `ordered` accumulates groups already finalized (each in that
    /// controller's chosen intra-group order); `pending_groups` is what's
    /// left, still in APNAP order.
    /// `pending_groups` is a `VecDeque` for the same reason
    /// `ReplacementCheck::remaining` is: drained front-to-back in APNAP
    /// order, not reversed by a `Vec::pop()`.
    PutTriggersApnap {
        ordered: Vec<PendingTrigger>,
        pending_groups: VecDeque<(PlayerId, Vec<PendingTrigger>)>,
    },
    /// CR 603.3b: paused mid-`PutTriggersApnap` because `group` (all
    /// controlled by `player`) has more than one trigger and `player` must
    /// choose the order to put them on the stack. `ordered`/`pending_groups`
    /// carry the rest of the same `PutTriggersApnap` run so it can resume
    /// once `submit_trigger_order` answers.
    AwaitTriggerOrderChoice {
        player: PlayerId,
        group: Vec<PendingTrigger>,
        ordered: Vec<PendingTrigger>,
        pending_groups: VecDeque<(PlayerId, Vec<PendingTrigger>)>,
    },
    /// All players passed with an empty stack — advance turn/phase/step
    /// (CR 500-514) before priority reopens.
    AdvanceTurnStep,
    /// CR 103.5: pregame mulligan round — waiting for `player` to submit
    /// `KeepHand` or `TakeMulligan`. Players are polled in turn order; once
    /// all have kept the engine moves to bottoming (or to `Beginning` if no
    /// bottoming is needed).
    AwaitingMulliganDecision { player: PlayerId },
    /// CR 103.5: `player` kept their hand after taking `count` paid mulligans
    /// (i.e. mulligans beyond `GameSettings::free_mulligans`) and must now put
    /// exactly `count` cards from their hand on the bottom of their library.
    /// `remaining` holds the rest of the bottoming queue (same pattern as
    /// `PutTriggersApnap::pending_groups`).
    AwaitingBottomCardChoice { player: PlayerId, count: usize, remaining: VecDeque<(PlayerId, usize)> },
    /// CR 514.1: cleanup's discard-to-hand-size turn-based action found the
    /// active player's hand exceeds `max_hand_size` — waiting for
    /// `Engine::submit_discard` to answer before continuing.
    AwaitingDiscard { player: PlayerId, excess: usize },
    /// CR 514.3: cleanup's turn-based actions (discard, effect expiry) are
    /// done — run one SBA pass to decide whether priority opens (SBAs fired)
    /// or the turn advances silently (nothing triggered, normal case).
    CleanupSbaCheck,
    /// CR 601.2f-h: paused mid-pipeline because `CastSpell` pushed
    /// `stack_id` onto the stack but its cost (`cost`) hasn't been paid yet
    /// — `payer` must submit a `PaymentPlan` via `Engine::submit_payment`
    /// before this run can continue (CR 118.5a: even a `{0}` cost needs an
    /// explicit empty plan, no auto-pay). Set from `CommitEvents` reading
    /// `Engine::pending_payment` rather than carried in this variant's own
    /// construction path — see that field's doc comment. `source` is the
    /// object whose activation/cast this cost belongs to (see
    /// `commands::PendingPaymentRequest::source`'s doc comment) — needed by
    /// `PaymentPlan::validate` to check `AdditionalCost::TapSelf`.
    AwaitingPayment { stack_id: ObjectId, payer: PlayerId, cost: CostSpec, source: ObjectId },
}

/// The step-able rules processor (DESIGN.md's "step-able core"). Wraps
/// `GameState` (pure data) with the actual turn/phase/priority/stack
/// advancement logic. Live play drives `step()` in a tight loop between
/// player decisions; a replay viewer re-feeds the same seed + action stream
/// into a fresh `Engine` and calls `step()` one at a time to pause/scrub.
pub struct Engine {
    state: GameState,
    process_state: ProcessState,
    /// `None` while sitting in `PriorityWindow` with nothing in flight; set
    /// when a pipeline run begins (`PendingAction`/`ResolveStackTop`), read
    /// by `CommitEvents`/`CollectTriggers`, and MUST be reset back to
    /// `None` by `PutTriggersApnap` (the run's last stage) — otherwise the
    /// next run's `CommitEvents`/`CollectTriggers` would silently read this
    /// run's stale `since_ledger`/`caused_by_command`/`parent_event`.
    pipeline_context: Option<PipelineContext>,
    /// Decided when a pipeline run begins; consumed once that run's
    /// trigger-placement stage finishes to pick the next `ProcessState`.
    pipeline_exit: PipelineExit,
    /// Dedicated monotonic counter for `CommandId` allocation — see the
    /// same rationale as `GameState::allocate_event_id`; was previously
    /// derived from `ledger.len()`, which doesn't even make sense for
    /// command ids (commands aren't ledger entries).
    next_command_id: u64,
    /// Set by `PendingAction` when `execute_command`'s outcome carries a
    /// `CommandOutcome::requires_payment` (currently only `CastSpell`),
    /// consumed (`Option::take`) by `CommitEvents` once this run's events
    /// have committed — that's the point this run either continues straight
    /// into `SbaLoop` (nothing pending) or pauses on `ProcessState::
    /// AwaitingPayment` (something pending). Kept as its own field rather
    /// than folded into `pipeline_context`, since unlike that field's
    /// since_ledger/causal refs, this is specific to one command's outcome,
    /// not "this pipeline run" in general — `ResolveStackTop` runs never set
    /// it, and it's already `None` by the time they reach `CommitEvents`.
    pending_payment: Option<PendingPaymentRequest>,
    /// Settings that govern automated pregame behavior and format-level thresholds
    /// (life totals, poison threshold, max hand size). Stored on `Engine`, not
    /// `GameState`, because they're configuration for this session, not mutable
    /// game state that rules affect. `Engine::new` uses `GameSettings::default()`;
    /// `Engine::new_game` sets this from `GameConfig::settings`.
    pub settings: GameSettings,
    /// `true` once pregame setup (shuffle/draw/mulligan) is complete or was
    /// deliberately skipped. `Engine::new_game` initializes this to `false` so
    /// the first `step()` call triggers `run_pregame_setup`. `Engine::new`
    /// (legacy test constructor) initializes to `true` to preserve existing
    /// tests that build `GameState` directly with `phase: PreGame` and expect
    /// the old pass-through behavior where PreGame was a no-op.
    pregame_setup_done: bool,
    /// Embedded Lua VM + loaded callbacks (see `script.rs`). One per
    /// `Engine`, not per-game rule per DESIGN.md — but this is a per-`Engine`
    /// field rather than a global since there's currently no other place
    /// with the right lifetime to own it, and nothing yet shares one
    /// `ScriptRuntime` across multiple `Engine`s.
    script_runtime: ScriptRuntime,
}

impl Engine {
    pub fn new(state: GameState) -> Self {
        Engine {
            state,
            process_state: ProcessState::PriorityWindow,
            pipeline_context: None,
            pipeline_exit: PipelineExit::BackToPriority,
            next_command_id: 0,
            pending_payment: None,
            settings: GameSettings::default(),
            pregame_setup_done: true,
            script_runtime: ScriptRuntime::new(),
        }
    }

    pub fn state(&self) -> &GameState {
        &self.state
    }

    /// TODO(script bridge): `state.abilities` should be populated by each
    /// card's script registering itself (see `script_api.rs`'s
    /// `register_triggered_ability` TODO) as objects enter zones that grant
    /// abilities. That bridge doesn't exist yet, so callers building a game
    /// from `CardDefinition`s outside the manual scenario-fixture path (e.g.
    /// `new_game`) need a way to seed `RegisteredAbility` entries directly.
    /// Remove once the script bridge lands.
    pub fn state_mut(&mut self) -> &mut GameState {
        &mut self.state
    }

    /// Loads a standalone Lua callback (see `ScriptRuntime::load_function`)
    /// into this `Engine`'s script runtime, returning a `ScriptHandle` a
    /// caller can then put on a `RegisteredAbility`'s `condition`/`operation`
    /// field directly. Stand-in for real card-script self-registration
    /// (`Game.RegisterTriggeredAbility`, still `todo!()` in script_api.rs) —
    /// same "no producer exists yet, so the caller wires it up by hand"
    /// shape `state_mut`'s own doc comment already describes for
    /// `state.abilities`.
    pub fn load_script(&mut self, source: &str) -> Result<ScriptHandle, ScriptError> {
        self.script_runtime.load_function(source)
    }

    /// Loads a condition source string — either a `Game.Conditions.*`
    /// one-liner or a full custom function — into this `Engine`'s script
    /// runtime. See `ScriptRuntime::load_condition`.
    pub fn load_condition(&mut self, source: &str) -> Result<ConditionSpec, ScriptError> {
        self.script_runtime.load_condition(source)
    }

    /// Loads an operation source string — either a `Game.Ops.*` one-liner or
    /// a full custom function — into this `Engine`'s script runtime. See
    /// `ScriptRuntime::load_operation`.
    pub fn load_operation(
        &mut self,
        source: &str,
    ) -> Result<(Option<AbilityEffect>, ScriptHandle), ScriptError> {
        self.script_runtime.load_operation(source)
    }

    /// Diagnostic for tests/replay tooling: whether `pipeline_context` is
    /// currently set. Should be `false` whenever `process_state` is
    /// `PriorityWindow` - `end_pipeline_run` is the only place that clears
    /// it, and `PriorityWindow` is only ever reached through there (or
    /// `AdvanceTurnStep`, which also doesn't set it).
    pub fn pipeline_context_active(&self) -> bool {
        self.pipeline_context.is_some()
    }

    fn allocate_command_id(&mut self) -> CommandId {
        let id = CommandId(self.next_command_id);
        self.next_command_id += 1;
        id
    }

    /// Validates `action` is legal right now (right player, matches what's
    /// actually available) and queues it for the next `step()` to act on.
    /// Does not itself advance the ledger — `step()` does that.
    ///
    /// Only checks "are we even waiting for an action right now" and "is
    /// it this player's priority" (CR 117.3a). Cost/target/timing legality
    /// beyond that isn't designed yet — see `commands::execute_command`'s
    /// TODOs for where that has to land.
    ///
    /// CR 508.1/509.1/510.1 exception: `DeclareAttackers`/`DeclareBlockers`/
    /// `AssignCombatDamage` are turn-based actions that explicitly "don't
    /// use the stack" and aren't a priority decision at all - the defending
    /// player has no reason to hold priority when blockers are declared
    /// (508.2: priority stays with the active player through the declare-
    /// attackers step), and combat damage is dealt automatically as the
    /// combat damage step opens (510.1/510.2), not in response to a
    /// priority window. Skipping the priority check for these three and
    /// leaving the actor check entirely to `execute_command` (active player
    /// / defending player, per CR 506.2) is a deliberate simplification -
    /// the fully correct model would have these fire automatically on
    /// entering their step, before any priority window opens, which this
    /// engine doesn't have a pipeline stage for yet (`ProcessState`/
    /// `AdvanceTurnStep` only open a generic priority window on every step,
    /// combat steps included).
    pub fn submit_action(&mut self, action: PlayerAction) -> Result<(), ActionError> {
        match self.process_state {
            ProcessState::PriorityWindow => {}
            _ => {
                return Err(ActionError::IllegalAction(
                    "engine is mid-pipeline, not waiting for a player action".to_string(),
                ));
            }
        }
        let is_combat_turn_based_action = matches!(
            action.kind,
            PlayerActionKind::DeclareAttackers { .. }
                | PlayerActionKind::DeclareBlockers { .. }
                | PlayerActionKind::AssignCombatDamage { .. }
        );
        if !is_combat_turn_based_action && action.player != self.state.priority_player {
            return Err(ActionError::NotYourTurn);
        }
        if matches!(
            action.kind,
            PlayerActionKind::KeepHand
                | PlayerActionKind::TakeMulligan
                | PlayerActionKind::BottomCards { .. }
        ) {
            return Err(ActionError::IllegalAction(
                "pregame actions use submit_mulligan_decision / submit_bottom_cards".to_string(),
            ));
        }
        self.process_state = ProcessState::PendingAction(action);
        Ok(())
    }

    /// Answers a pending CR 616.1 ordering choice raised by
    /// `StepStatus::AwaitingReplacementChoice` — `chosen` must be one of
    /// that status's `candidates` (a `ReplacementEffect::source`).
    /// Deliberately a separate entry point from `submit_action`: this isn't
    /// a `PlayerAction`/priority decision, it's a sub-decision mid-pipeline,
    /// so it can't go through `submit_action`'s `PriorityWindow`-only gate.
    pub fn submit_replacement_choice(
        &mut self,
        chosen: ObjectId,
    ) -> Result<(), ReplacementChoiceError> {
        let ProcessState::AwaitingReplacementChoice { candidates, .. } = &self.process_state
        else {
            return Err(ReplacementChoiceError::NotAwaitingChoice);
        };
        if !candidates.contains(&chosen) {
            return Err(ReplacementChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingReplacementChoice { pending_event, mut excluded, resume, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        // Apply the chosen effect, exclude it from matching this same event
        // again (CR 616.1), then re-run resolution on the result (CR 616's
        // stabilization requirement - the rewritten form may match yet
        // another effect, including needing a further ordering choice of
        // its own) - carrying `excluded` forward rather than starting a
        // fresh exclusion set, so an effect already applied before this
        // pause can't reapply to the same event after it.
        let rewritten = replacement::apply_one_replacement(&mut self.state, chosen, pending_event);
        excluded.push(chosen);
        let outcome = replacement::apply_replacements_from(&mut self.state, rewritten, excluded);

        self.process_state = match resume {
            ReplacementResumeTarget::ReplacementCheck { remaining, mut checked } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    checked.push(event);
                    ProcessState::ReplacementCheck { remaining, checked }
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::ReplacementCheck { remaining, checked },
                    }
                }
            },
            ReplacementResumeTarget::SbaDeath { pending_deaths, then } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    let committed = sba::commit_death(&mut self.state, event);
                    self.resume_sba_death(pending_deaths, vec![committed], then)
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::SbaDeath { pending_deaths, then },
                    }
                }
            },
        };
        Ok(())
    }

    /// Answers a pending CR 603.3b intra-group ordering choice raised by
    /// `StepStatus::AwaitingTriggerOrderChoice` — `chosen_order` must be a
    /// reordering of that status's `group` (same triggers, player's chosen
    /// sequence). Separate entry point from `submit_action` for the same
    /// reason `submit_replacement_choice` is: this is a sub-decision
    /// mid-pipeline, not a `PlayerAction`/priority decision.
    pub fn submit_trigger_order(
        &mut self,
        chosen_order: Vec<PendingTrigger>,
    ) -> Result<(), TriggerOrderError> {
        let ProcessState::AwaitTriggerOrderChoice { group, .. } = &self.process_state else {
            return Err(TriggerOrderError::NotAwaitingChoice);
        };
        if chosen_order.len() != group.len() {
            return Err(TriggerOrderError::WrongLength);
        }
        if !is_permutation(&chosen_order, group) {
            return Err(TriggerOrderError::NotAPermutation);
        }
        let ProcessState::AwaitTriggerOrderChoice { mut ordered, pending_groups, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        ordered.extend(chosen_order);
        self.process_state = ProcessState::PutTriggersApnap { ordered, pending_groups };
        Ok(())
    }

    /// Answers a pending CR 601.2f-h cost payment raised by
    /// `StepStatus::AwaitingPayment` — commits `plan` (deducts mana,
    /// records `StackObject.costs_paid`) only if it validates against the
    /// pending `CostSpec`; otherwise rejects without touching state, same
    /// no-partial-payment guarantee `PaymentPlan`'s own doc comment
    /// describes. Separate entry point from `submit_action` for the same
    /// reason `submit_replacement_choice`/`submit_trigger_order` are: this
    /// is a sub-decision mid-pipeline, not a `PlayerAction`/priority
    /// decision.
    pub fn submit_payment(&mut self, plan: PaymentPlan) -> Result<(), PaymentError> {
        let ProcessState::AwaitingPayment { payer, source, .. } = &self.process_state else {
            return Err(PaymentError::NotAwaitingPayment);
        };
        let payer = *payer;
        let source = *source;
        let valid = {
            let ProcessState::AwaitingPayment { cost, .. } = &self.process_state else {
                unreachable!("checked above");
            };
            plan.validate(cost, &self.state, payer, source)
        };
        if !valid {
            return Err(PaymentError::InvalidPlan);
        }
        let ProcessState::AwaitingPayment { stack_id, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        // CR 118.3a/118.13a/602.2f-h: commit exactly what the plan declared
        // (mana deducted, tap/sacrifice/discard/life side effects applied) -
        // see commands::commit_payment's doc comment for why this trusts
        // validate's check rather than re-deriving anything.
        let mut events = commands::commit_payment(&mut self.state, payer, &plan.payments);
        if let Some(stack_object) = self.state.stack.iter_mut().find(|s| s.stack_id == stack_id) {
            stack_object.costs_paid = plan.payments;
        }
        events.push(GameEventKind::CostPaid { stack_id, payer });
        for kind in events {
            let event_id = self.state.allocate_event_id();
            let sequence_no = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no,
                kind,
                caused_by_command: None,
                parent_event: None,
            });
        }

        // Resume exactly where CommitEvents would otherwise have gone had
        // pending_payment been None (see that arm's doc comment) - the rest
        // of this pipeline run (SBA/triggers/exit) is unaffected by this
        // pause, since pipeline_context/pipeline_exit were already decided
        // before the pause happened.
        self.process_state = ProcessState::SbaLoop;
        Ok(())
    }

    /// Answers a pending CR 514.1 discard-to-hand-size choice raised by
    /// `StepStatus::AwaitingDiscard` — `cards` must be exactly the right
    /// number of cards from the active player's hand. Moves each hand →
    /// graveyard via the same CR 400.7 new-`ObjectId` convention every other
    /// zone move uses, then queues `ProcessState::CleanupSbaCheck` so the
    /// next `step()` decides whether CR 514.3's priority exception applies.
    ///
    /// TODO: discard can be replaced (e.g. Leyline of the Void redirects
    /// discarded cards to exile) but replacement effects aren't implemented
    /// yet — events committed directly to the ledger here, same shortcut
    /// `submit_payment` uses.
    pub fn submit_discard(&mut self, cards: Vec<ObjectId>) -> Result<(), DiscardError> {
        let ProcessState::AwaitingDiscard { player, excess } = self.process_state else {
            return Err(DiscardError::NotAwaitingDiscard);
        };

        if cards.len() != excess {
            return Err(DiscardError::WrongCount { expected: excess, got: cards.len() });
        }

        // Duplicate check before any mutation.
        let mut seen: Vec<ObjectId> = Vec::with_capacity(cards.len());
        for &card in &cards {
            if seen.contains(&card) {
                return Err(DiscardError::DuplicateCard);
            }
            seen.push(card);
        }

        // Validate each card is in the active player's hand under their control.
        for &card in &cards {
            let object = self.state.object(card).ok_or(DiscardError::CardNotInHand)?;
            if object.zone != ZoneKind::Hand || object.controller != player {
                return Err(DiscardError::CardNotInHand);
            }
        }

        // Move each card hand → graveyard (CR 400.7: new ObjectId per move).
        let mut events: Vec<GameEventKind> = Vec::with_capacity(cards.len());
        for card in cards {
            let last_known_info =
                self.state.object(card).expect("validated above").clone();
            let owner = last_known_info.owner;

            let new_id = self.state.allocate_object_id();
            let mut new_object = last_known_info.clone();
            new_object.object_id = new_id;
            new_object.lineage = ObjectLineage { previous: Some(card) };
            new_object.zone = ZoneKind::Graveyard;
            new_object.timestamp = self.state.allocate_sequence_no();

            self.state
                .zone_mut(ZoneKind::Hand, Some(player))
                .expect("hand zone must exist")
                .remove(card);
            // CR 400.7: discarded, no remap follows - purge any
            // RegisteredAbility still pointing at the hand-zone id.
            commands::retire_object(&mut self.state, card);
            self.state
                .zone_mut(ZoneKind::Graveyard, Some(owner))
                .expect("graveyard zone must exist")
                .push_bottom(new_id);
            self.state.objects.insert(new_id, new_object);

            events.push(GameEventKind::ZoneChange {
                object: new_id,
                from: ZoneKind::Hand,
                to: ZoneKind::Graveyard,
                last_known_info: Some(last_known_info),
            });
        }

        for kind in events {
            let event_id = self.state.allocate_event_id();
            let sequence_no = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no,
                kind,
                caused_by_command: None,
                parent_event: None,
            });
        }

        self.process_state = ProcessState::CleanupSbaCheck;
        Ok(())
    }

    /// Builds a fresh `Engine` from a `GameConfig`: creates zones and
    /// `GameObject`s for each player's library, sets starting life from
    /// `config.settings.starting_life` (or per-player `life_override`), and
    /// starts in `Phase::PreGame` / `ProcessState::PriorityWindow`. The first
    /// `step()` call will enter `run_pregame_setup`.
    pub fn new_game(config: GameConfig) -> Engine {
        let settings = config.settings;
        let starting_player = config.turn_order[0];

        let mut players: Vec<PlayerState> = Vec::new();
        let mut zones: Vec<Zone> = Vec::new();
        let mut objects: HashMap<ObjectId, GameObject> = HashMap::new();
        let mut next_object_id: u64 = 0;
        let mut next_sequence_no: u64 = 0;

        for player_cfg in &config.players {
            let pid = player_cfg.player_id;
            let life = player_cfg
                .life_override
                .unwrap_or(settings.starting_life) as i32;

            players.push(PlayerState {
                player_id: pid,
                life,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                mulligans_taken: 0,
                hand_kept: false,
            });

            let mut lib_zone = Zone::new(ZoneKind::Library, Some(pid));
            for card_def in &player_cfg.library {
                let obj_id = ObjectId(next_object_id);
                next_object_id += 1;
                let ts = SequenceNo(next_sequence_no);
                next_sequence_no += 1;
                let obj = GameObject {
                    object_id: obj_id,
                    lineage: ObjectLineage { previous: None },
                    definition: card_def.clone(),
                    owner: pid,
                    controller: pid,
                    zone: ZoneKind::Library,
                    tapped: false,
                    flipped: false,
                    face_down: false,
                    phased_out: false,
                    damage_marked: 0,
                    deathtouched: false,
                    counters: CounterSet::new(),
                    attachments: Vec::new(),
                    attached_to: None,
                    timestamp: ts,
                    controlled_since_turn: 1,
                    visibility: VisibilityPolicy { public: false, visible_to: Vec::new() },
                };
                lib_zone.contents.push_back(obj_id);
                objects.insert(obj_id, obj);
            }
            zones.push(lib_zone);
            zones.push(Zone::new(ZoneKind::Hand, Some(pid)));
            zones.push(Zone::new(ZoneKind::Graveyard, Some(pid)));
        }

        // Shared zones
        zones.push(Zone::new(ZoneKind::Battlefield, None));
        zones.push(Zone::new(ZoneKind::Exile, None));

        let state = GameState {
            players,
            turn_order: config.turn_order.clone(),
            active_player: starting_player,
            turn_number: 1,
            phase: Phase::PreGame,
            step: Step::Untap,
            priority_player: starting_player,
            consecutive_passes: 0,
            lands_played_this_turn: HashMap::new(),
            combat: None,
            last_active_turn_number: HashMap::new(),
            objects,
            zones,
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState { seed: config.rng_seed, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no,
            next_object_id,
            pending_step_skips: Vec::new(),
        };

        Engine {
            state,
            process_state: ProcessState::PriorityWindow,
            pipeline_context: None,
            pipeline_exit: PipelineExit::BackToPriority,
            next_command_id: 0,
            pending_payment: None,
            settings,
            pregame_setup_done: false,
            script_runtime: ScriptRuntime::new(),
        }
    }

    /// Returns the `GameSettings` stored on this engine instance.
    pub fn settings(&self) -> &GameSettings {
        &self.settings
    }

    /// Accepts a `KeepHand` or `TakeMulligan` action from the player whose
    /// turn it is to decide during pregame. Only valid when
    /// `process_state == AwaitingMulliganDecision { player }` and
    /// `action.player == player`. On `TakeMulligan`, increments
    /// `mulligans_taken`, shuffles the hand back into the library, and draws a
    /// new opening hand immediately. Advances to the next deciding player or
    /// (if all have kept) to the bottoming phase.
    pub fn submit_mulligan_decision(&mut self, action: PlayerAction) -> Result<(), MulliganError> {
        let ProcessState::AwaitingMulliganDecision { player } = self.process_state else {
            return Err(MulliganError::NotAwaitingDecision);
        };
        if action.player != player {
            return Err(MulliganError::WrongPlayer);
        }
        match action.kind {
            PlayerActionKind::KeepHand => {
                self.state.player_mut(player).expect("valid player").hand_kept = true;
                match self.next_mulligan_player() {
                    Some(next) => {
                        self.process_state = ProcessState::AwaitingMulliganDecision { player: next };
                    }
                    None => self.transition_to_bottoming(),
                }
            }
            PlayerActionKind::TakeMulligan => {
                let taken = self.state.player(player).expect("valid player").mulligans_taken;
                if let Some(max) = self.settings.max_mulligans {
                    if taken >= max {
                        return Err(MulliganError::MaxMulligansReached);
                    }
                }
                self.state.player_mut(player).expect("valid player").mulligans_taken += 1;
                self.return_hand_to_library(player);
                self.shuffle_library(player);
                self.draw_opening_hand(player);
                // Advance to the next undecided player after this one (circular).
                // Using next_after rather than next_mulligan_player because this
                // player is still !hand_kept (they mulliganed, not kept), so
                // next_mulligan_player would return them again immediately.
                let next = self.next_mulligan_player_after(player);
                self.process_state =
                    ProcessState::AwaitingMulliganDecision { player: next.expect("player must be in turn_order") };
            }
            _ => return Err(MulliganError::InvalidAction),
        }
        Ok(())
    }

    /// Accepts a `BottomCards` action from the player who must bottom cards
    /// after keeping with paid mulligans. Only valid when
    /// `process_state == AwaitingBottomCardChoice { player, .. }` and
    /// `action.player == player`. Validates count and card membership, moves
    /// the cards to the bottom of the player's library in submission order,
    /// then advances to the next bottoming player or to `AdvanceTurnStep`.
    pub fn submit_bottom_cards(&mut self, action: PlayerAction) -> Result<(), BottomCardsError> {
        let ProcessState::AwaitingBottomCardChoice { player, count, .. } = self.process_state else {
            return Err(BottomCardsError::NotAwaitingChoice);
        };
        if action.player != player {
            return Err(BottomCardsError::WrongPlayer);
        }
        let PlayerActionKind::BottomCards { cards } = action.kind else {
            return Err(BottomCardsError::WrongCount { expected: count, got: 0 });
        };
        if cards.len() != count {
            return Err(BottomCardsError::WrongCount { expected: count, got: cards.len() });
        }
        // Duplicate check
        let mut seen: Vec<ObjectId> = Vec::with_capacity(cards.len());
        for &card in &cards {
            if seen.contains(&card) {
                return Err(BottomCardsError::DuplicateCard);
            }
            seen.push(card);
        }
        // Validate each card is in the player's hand
        for &card in &cards {
            let obj = self.state.object(card).ok_or(BottomCardsError::CardNotInHand)?;
            if obj.zone != ZoneKind::Hand || obj.owner != player {
                return Err(BottomCardsError::CardNotInHand);
            }
        }
        // Move each card hand → library bottom (CR 400.7 new ObjectId per move)
        for card in &cards {
            let card = *card;
            let last_known = self.state.object(card).expect("validated above").clone();
            let owner = last_known.owner;
            let new_id = self.state.allocate_object_id();
            let mut new_obj = last_known.clone();
            new_obj.object_id = new_id;
            new_obj.lineage = ObjectLineage { previous: Some(card) };
            new_obj.zone = ZoneKind::Library;
            new_obj.timestamp = self.state.allocate_sequence_no();
            self.state.zone_mut(ZoneKind::Hand, Some(player)).expect("hand zone").remove(card);
            self.state.objects.remove(&card);
            self.state.zone_mut(ZoneKind::Library, Some(owner)).expect("library zone").push_bottom(new_id);
            self.state.objects.insert(new_id, new_obj);
            commands::remap_abilities(&mut self.state, card, new_id);
            let event_id = self.state.allocate_event_id();
            let seq = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no: seq,
                kind: GameEventKind::ZoneChange {
                    object: new_id,
                    from: ZoneKind::Hand,
                    to: ZoneKind::Library,
                    last_known_info: Some(last_known),
                },
                caused_by_command: None,
                parent_event: None,
            });
        }
        // Advance the bottoming queue
        let ProcessState::AwaitingBottomCardChoice { mut remaining, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };
        match remaining.pop_front() {
            Some((next_player, next_count)) => {
                self.process_state = ProcessState::AwaitingBottomCardChoice {
                    player: next_player,
                    count: next_count,
                    remaining,
                };
            }
            None => {
                self.process_state = ProcessState::AdvanceTurnStep;
            }
        }
        Ok(())
    }

    /// Called from `step()` when `phase == PreGame && process_state ==
    /// PriorityWindow`. Runs all automatic pregame actions: shuffles libraries
    /// (if `settings.shuffle_libraries`), draws opening hands for all players,
    /// then either transitions to `AwaitingMulliganDecision` for the first
    /// player in turn order (normal path) or directly to `AdvanceTurnStep`
    /// (if `settings.skip_mulligan`).
    fn run_pregame_setup(&mut self) {
        let turn_order: Vec<PlayerId> = self.state.turn_order.clone();
        if self.settings.shuffle_libraries {
            for &pid in &turn_order {
                self.shuffle_library(pid);
            }
        }
        for &pid in &turn_order {
            self.draw_opening_hand(pid);
        }
        if self.settings.skip_mulligan {
            for &pid in &turn_order {
                self.state.player_mut(pid).expect("valid player").hand_kept = true;
            }
            self.process_state = ProcessState::AdvanceTurnStep;
        } else {
            let first = turn_order[0];
            self.process_state = ProcessState::AwaitingMulliganDecision { player: first };
        }
    }

    /// Shuffles `player`'s library zone in-place using `state.rng`. Logs a
    /// `LibraryShuffled` event and an `RngDrawn` event per shuffle draw so
    /// the replay is fully deterministic. Uses Fisher-Yates over the zone's
    /// `VecDeque` contents.
    fn shuffle_library(&mut self, player: PlayerId) {
        let len = self.state.zone(ZoneKind::Library, Some(player)).map_or(0, |z| z.contents.len());
        // Generate all swap targets first — can't hold &mut zone while calling next_rng()
        let mut swaps: Vec<(usize, usize)> = Vec::with_capacity(len);
        for i in (1..len).rev() {
            let rng_val = self.next_rng();
            let j = (rng_val % (i as u64 + 1)) as usize;
            swaps.push((i, j));
            let event_id = self.state.allocate_event_id();
            let seq = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no: seq,
                kind: GameEventKind::RngDrawn { purpose: "shuffle".to_string(), value: rng_val },
                caused_by_command: None,
                parent_event: None,
            });
        }
        let lib = self.state.zone_mut(ZoneKind::Library, Some(player)).expect("library zone");
        for (i, j) in swaps {
            lib.contents.swap(i, j);
        }
        let event_id = self.state.allocate_event_id();
        let seq = self.state.allocate_sequence_no();
        self.state.ledger.push(GameEvent {
            event_id,
            sequence_no: seq,
            kind: GameEventKind::LibraryShuffled { player },
            caused_by_command: None,
            parent_event: None,
        });
    }

    /// Returns all cards from `player`'s hand to the top of their library
    /// (no particular order required by CR 103.5 — placed top for consistency).
    /// Logs a ZoneChange per card. Used before a mulligan redraw.
    fn return_hand_to_library(&mut self, player: PlayerId) {
        let hand_ids: Vec<ObjectId> = self.state
            .zone(ZoneKind::Hand, Some(player))
            .map_or(Vec::new(), |z| z.contents.iter().copied().collect());
        for card in hand_ids {
            let last_known = self.state.object(card).expect("hand card must exist").clone();
            let owner = last_known.owner;
            let new_id = self.state.allocate_object_id();
            let mut new_obj = last_known.clone();
            new_obj.object_id = new_id;
            new_obj.lineage = ObjectLineage { previous: Some(card) };
            new_obj.zone = ZoneKind::Library;
            new_obj.timestamp = self.state.allocate_sequence_no();
            self.state.zone_mut(ZoneKind::Hand, Some(player)).expect("hand zone").remove(card);
            self.state.objects.remove(&card);
            self.state.zone_mut(ZoneKind::Library, Some(owner)).expect("library zone").push_top(new_id);
            self.state.objects.insert(new_id, new_obj);
            commands::remap_abilities(&mut self.state, card, new_id);
            let event_id = self.state.allocate_event_id();
            let seq = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no: seq,
                kind: GameEventKind::ZoneChange {
                    object: new_id,
                    from: ZoneKind::Hand,
                    to: ZoneKind::Library,
                    last_known_info: Some(last_known),
                },
                caused_by_command: None,
                parent_event: None,
            });
        }
    }

    /// Moves `settings.starting_hand_size` cards from the top of `player`'s
    /// library to their hand zone, logging a `ZoneChange` per card.
    fn draw_opening_hand(&mut self, player: PlayerId) {
        for _ in 0..self.settings.starting_hand_size {
            let Some(card) = self.state.zone(ZoneKind::Library, Some(player))
                .and_then(|z| z.contents.front().copied())
            else {
                break;
            };
            let last_known = self.state.object(card).expect("library card must exist").clone();
            let new_id = self.state.allocate_object_id();
            let mut new_obj = last_known.clone();
            new_obj.object_id = new_id;
            new_obj.lineage = ObjectLineage { previous: Some(card) };
            new_obj.zone = ZoneKind::Hand;
            new_obj.timestamp = self.state.allocate_sequence_no();
            self.state.zone_mut(ZoneKind::Library, Some(player)).expect("library zone").remove(card);
            self.state.objects.remove(&card);
            self.state.zone_mut(ZoneKind::Hand, Some(player)).expect("hand zone").push_bottom(new_id);
            self.state.objects.insert(new_id, new_obj);
            commands::remap_abilities(&mut self.state, card, new_id);
            let event_id = self.state.allocate_event_id();
            let seq = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no: seq,
                kind: GameEventKind::ZoneChange {
                    object: new_id,
                    from: ZoneKind::Library,
                    to: ZoneKind::Hand,
                    last_known_info: Some(last_known),
                },
                caused_by_command: None,
                parent_event: None,
            });
        }
    }

    /// Returns the first player in `state.turn_order` whose `hand_kept` is
    /// `false`. Returns `None` when all players have kept (mulligan phase
    /// complete).
    fn next_mulligan_player(&self) -> Option<PlayerId> {
        self.state.turn_order.iter().copied()
            .find(|&pid| !self.state.player(pid).map_or(false, |p| p.hand_kept))
    }

    /// Returns the first `!hand_kept` player found in circular turn order
    /// starting AFTER `current`. Used after `TakeMulligan` to advance to the
    /// next player who hasn't kept, wrapping around to the current player if
    /// they're the only undecided one remaining.
    fn next_mulligan_player_after(&self, current: PlayerId) -> Option<PlayerId> {
        let order = &self.state.turn_order;
        let n = order.len();
        let cur_pos = order.iter().position(|&p| p == current)?;
        for i in 1..=n {
            let pid = order[(cur_pos + i) % n];
            if !self.state.player(pid).map_or(false, |p| p.hand_kept) {
                return Some(pid);
            }
        }
        None // all players have kept (shouldn't be reachable after a TakeMulligan)
    }

    /// Advances `state.rng` by one step and returns the raw u64. Uses
    /// splitmix64 so the sequence is deterministic from `seed` alone and
    /// every output is logged as `RngDrawn`.
    fn next_rng(&mut self) -> u64 {
        self.state.rng.draws_consumed += 1;
        let mut z = self.state.rng.seed.wrapping_add(
            0x9e3779b97f4a7c15u64.wrapping_mul(self.state.rng.draws_consumed),
        );
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    /// Builds the bottoming queue: collects every player (in turn order) whose
    /// paid mulligan count (`mulligans_taken` minus `settings.free_mulligans`,
    /// clamped to 0) is > 0. If the queue is empty, transitions directly to
    /// `AdvanceTurnStep`; otherwise sets `process_state` to
    /// `AwaitingBottomCardChoice` for the first player in the queue, with
    /// `remaining` holding the rest.
    fn transition_to_bottoming(&mut self) {
        let free = self.settings.free_mulligans;
        let mut queue: VecDeque<(PlayerId, usize)> = self.state.turn_order.iter().copied()
            .filter_map(|pid| {
                let taken = self.state.player(pid).map_or(0, |p| p.mulligans_taken);
                let paid = taken.saturating_sub(free) as usize;
                if paid > 0 { Some((pid, paid)) } else { None }
            })
            .collect();
        match queue.pop_front() {
            Some((first, count)) => {
                self.process_state = ProcessState::AwaitingBottomCardChoice {
                    player: first,
                    count,
                    remaining: queue,
                };
            }
            None => {
                self.process_state = ProcessState::AdvanceTurnStep;
            }
        }
    }

    /// CR 608.2m (normal resolution) and CR 608.2b (fizzle): moves a
    /// resolving spell's card off the stack into `destination`, minting a
    /// fresh `ObjectId` (CR 400.7) the same way every other zone change
    /// does. Pulled out so `ResolveStackTop`'s normal-resolution branch and
    /// its CR 608.2b fizzle branch (below) share one zone-move
    /// implementation instead of two copies that could drift apart -
    /// `destination`/`run_script` are the only two things that differ
    /// between them (battlefield-vs-graveyard, and whether a permanent's
    /// card script runs). Returns `None` only in the defensive case where
    /// `stack_id`'s `GameObject` is already gone (shouldn't happen - nothing
    /// else removes it while still on the stack).
    fn move_resolving_spell(
        &mut self,
        stack_id: ObjectId,
        destination: ZoneKind,
        run_script: bool,
    ) -> Option<GameEventKind> {
        let spell_object = self.state.object(stack_id)?;
        let last_known_info = spell_object.clone();
        // Battlefield and Exile are both shared zones (owner: None) - only
        // Graveyard (the other destination this function is ever called
        // with) is per-player.
        let destination_owner = match destination {
            ZoneKind::Battlefield | ZoneKind::Exile => None,
            _ => Some(last_known_info.owner),
        };

        let new_id = self.state.allocate_object_id();
        let mut new_object = last_known_info.clone();
        new_object.object_id = new_id;
        new_object.lineage = ObjectLineage { previous: Some(stack_id) };
        new_object.zone = destination;
        new_object.timestamp = self.state.allocate_sequence_no();
        // CR 302.6/602.5a: only a permanent actually entering the
        // battlefield is summoning sick - a fizzled/graveyard-bound card
        // was never in play.
        if destination == ZoneKind::Battlefield {
            new_object.controlled_since_turn = self.state.turn_number;
        }

        self.state.objects.remove(&stack_id);
        // Captured before `new_object` moves into `objects.insert` below -
        // `run_script` is `false` for a fizzle (CR 608.2b: it never
        // resolves, so no ETB self-registration either) and for any
        // non-battlefield destination.
        let definition_for_script = if run_script { Some(new_object.definition.clone()) } else { None };
        if let Some(zone) = self.state.zone_mut(destination, destination_owner) {
            zone.push_bottom(new_id);
            self.state.objects.insert(new_id, new_object);
        }
        commands::remap_abilities(&mut self.state, stack_id, new_id);
        if let Some(definition) = definition_for_script {
            // TODO: a script error here is silently dropped, same gap
            // ResolveStackTop's operation-invocation path already flags (no
            // error surface from ResolveStackTop back to a StepStatus yet).
            let _ = self.script_runtime.run_card_script(&mut self.state, &definition, new_id);
        }

        Some(GameEventKind::ZoneChange {
            object: new_id,
            from: ZoneKind::Stack,
            to: destination,
            last_known_info: Some(last_known_info),
        })
    }

    /// Pipeline run's final transition: consumes `self.pipeline_exit`,
    /// clears `self.pipeline_context` (the MUST documented on that field —
    /// the run that set it is finishing right here, in this call), and
    /// returns the `ProcessState` the engine should sit in next. Pulled out
    /// as its own function rather than left inline inside
    /// `PutTriggersApnap` so there's exactly one place that performs this
    /// transition, however many call sites eventually need it.
    fn end_pipeline_run(&mut self) -> ProcessState {
        self.pipeline_context = None;
        match std::mem::replace(&mut self.pipeline_exit, PipelineExit::BackToPriority) {
            PipelineExit::BackToPriority => ProcessState::PriorityWindow,
            PipelineExit::ResolveStackTop => ProcessState::ResolveStackTop,
            PipelineExit::AdvanceTurnStep => ProcessState::AdvanceTurnStep,
        }
    }

    /// Advances by one pipeline stage (see `ProcessState`) and returns.
    /// Callers that don't care about pausing (live play) just call this in
    /// a loop until `AwaitingAction`/`GameOver`; a replay viewer calls it
    /// once per UI "step".
    pub fn step(&mut self) -> StepResult {
        let events_start = self.state.ledger.len();
        let status = self.advance();
        StepResult { events_added: events_start..self.state.ledger.len(), status }
    }

    fn advance(&mut self) -> StepStatus {
        // `PriorityWindow` has no payload, so it's a cheap placeholder to
        // swap in while we take ownership of whatever state we're actually
        // in — every other variant gets explicitly replaced below before
        // this function returns.
        match std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow) {
            ProcessState::PriorityWindow => {
                if self.state.phase == Phase::PreGame && !self.pregame_setup_done {
                    self.pregame_setup_done = true;
                    self.run_pregame_setup();
                    return StepStatus::Progressed;
                }
                StepStatus::AwaitingAction { player: self.state.priority_player }
            }

            ProcessState::PendingAction(action) => {
                let command_id = self.allocate_command_id();
                let since_ledger = self.state.ledger.len();
                let command =
                    EngineCommand { command_id, actor: action.player, action: action.kind };

                let outcome =
                    match commands::execute_command(&mut self.state, &self.script_runtime, command) {
                        Ok(outcome) => outcome,
                        Err(err) => {
                            // Reset to PriorityWindow rather than leaving process_state
                            // in limbo - see StepStatus::Error's doc comment for why
                            // this doesn't attempt rollback.
                            self.process_state = ProcessState::PriorityWindow;
                            return StepStatus::Error(err);
                        }
                    };

                // CommandOutcome::battlefield_entries - execute_command
                // takes &ScriptRuntime as of the layer-engine-cda slice (for
                // effective_power/effective_keywords's read-only CDA
                // resolution), but that's an immutable borrow - it still
                // can't run a card's self-registering script itself
                // (run_card_script needs &mut ScriptRuntime to load into the
                // registry), so it just reports which ids became new
                // battlefield permanents; this is the "one place performs
                // the side effect" half, same split priority_handoff/broke_pass_chain
                // already use. TODO: a script error here is silently
                // dropped, same gap ResolveStackTop's own call flags.
                for object_id in &outcome.battlefield_entries {
                    if let Some(definition) =
                        self.state.object(*object_id).map(|o| o.definition.clone())
                    {
                        let _ =
                            self.script_runtime.run_card_script(&mut self.state, &definition, *object_id);
                    }
                }

                self.pipeline_context = Some(PipelineContext {
                    since_ledger,
                    caused_by_command: Some(command_id),
                    parent_event: None,
                });
                // See `pending_payment`'s doc comment - read here (this
                // command's own outcome) rather than inferred later, since
                // CommitEvents has no way to know "was this a CastSpell"
                // without it.
                self.pending_payment = outcome.requires_payment;

                // engine.rs owns state.priority_player entirely now -
                // execute_command just reports intent (CommandOutcome::
                // priority_handoff, commands.rs) instead of writing the
                // field itself.
                if let PriorityHandoff::To(player) = outcome.priority_handoff {
                    self.state.priority_player = player;
                }

                // engine.rs owns state.consecutive_passes entirely now -
                // execute_command just reports whether its variant broke
                // the chain (CommandOutcome::broke_pass_chain, commands.rs)
                // instead of mutating the counter itself, so there's one
                // place deciding what "everyone just passed" means rather
                // than relying on every match arm in execute_command
                // remembering to reset it.
                if outcome.broke_pass_chain {
                    self.state.consecutive_passes = 0;
                    self.pipeline_exit = PipelineExit::BackToPriority;
                } else {
                    self.state.consecutive_passes += 1;
                    self.pipeline_exit = if self.state.consecutive_passes as usize
                        >= self.state.turn_order.len()
                    {
                        self.state.consecutive_passes = 0;
                        if !self.state.stack.is_empty() {
                            PipelineExit::ResolveStackTop
                        } else {
                            PipelineExit::AdvanceTurnStep
                        }
                    } else {
                        PipelineExit::BackToPriority
                    };
                }

                self.process_state = ProcessState::ReplacementCheck {
                    remaining: outcome.events.into(),
                    checked: Vec::new(),
                };
                StepStatus::Progressed
            }

            ProcessState::ResolveStackTop => {
                let Some(top) = self.state.stack.pop() else {
                    // Shouldn't happen - pipeline_exit only routes here when
                    // the all-pass check saw a non-empty stack. Defensive
                    // fallback rather than a panic, since nothing downstream
                    // depends on this path being unreachable yet.
                    self.process_state = ProcessState::PriorityWindow;
                    return StepStatus::Progressed;
                };
                // CR 117.3b: once the stack object finishes resolving (after
                // SBA/triggers settle below), priority returns to the active
                // player. Set directly now rather than threading it through
                // PipelineExit - simpler for this minimal slice, and nothing
                // between here and PriorityWindow needs a different value.
                self.state.priority_player = self.state.active_player;
                let since_ledger = self.state.ledger.len();
                self.pipeline_context =
                    Some(PipelineContext { since_ledger, caused_by_command: None, parent_event: None });
                self.pipeline_exit = PipelineExit::BackToPriority;

                // CR 608.2b: before doing anything else, recheck every
                // target against current game state. Only fizzles if there
                // was a real (count > 0) requirement and every target now
                // fails it - a requirement of `None`/count 0 never fizzles,
                // same as if it had no targets to begin with.
                let fizzles = match &top.target_requirement {
                    Some(TargetSpec::Primitive(req)) if req.count > 0 => top.targets.iter().all(|t| {
                        !commands::target_is_legal(
                            &self.state,
                            &self.script_runtime,
                            *t,
                            &req.filter,
                            top.controller,
                        )
                    }),
                    // `None` (no targets required), count-0 requirement, or
                    // a `Script`-backed requirement (not evaluable this
                    // slice, see `TargetSpec`'s own doc comment) - never
                    // fizzles.
                    _ => false,
                };

                let mut events = Vec::new();
                if fizzles {
                    // CR 608.2b: doesn't resolve at all - no effect
                    // invocation, no permanent-entry script, and (unlike
                    // normal CR 608.2m resolution) a spell always lands in
                    // its owner's graveyard regardless of card type - unless
                    // it was cast via an alternative cost that exiles it
                    // instead (CR 702.34a: flashback's "exiled... as it
                    // resolves or as it leaves the stack otherwise" covers
                    // fizzling too).
                    if matches!(top.kind, StackEntryKind::Spell) {
                        let destination =
                            if top.exile_on_leaving_stack { ZoneKind::Exile } else { ZoneKind::Graveyard };
                        if let Some(event) = self.move_resolving_spell(top.stack_id, destination, false) {
                            events.push(event);
                        }
                    }
                    events.push(GameEventKind::Fizzled { stack_id: top.stack_id });
                } else {
                    // TODO: real resolution should look up top.operation
                    // (ScriptHandle) and invoke it via
                    // ScriptRuntime::invoke_operation to actually carry out
                    // whatever the spell/ability says - blocked on the script
                    // bridge like every other script-shaped gap. What follows is
                    // only CR 608.2m's zone-change half of resolution (a spell's
                    // card moves off the stack once it resolves) - it does not
                    // run any spell effect.
                    if matches!(top.kind, StackEntryKind::Spell) {
                        // CR 608.2m: a permanent type goes to the
                        // battlefield as a permanent; an instant or sorcery
                        // (the only other types castable as a spell - see
                        // CastSpell's land rejection in commands.rs) goes to
                        // its owner's graveyard instead - or exile, if this
                        // was cast via an alternative cost that exiles it on
                        // resolution (CR 702.34a). Peek the card's types
                        // before moving it - move_resolving_spell needs the
                        // destination decided up front.
                        let destination = self.state.object(top.stack_id).map(|obj| {
                            if obj
                                .definition
                                .card_types
                                .intersects(CardTypeSet::INSTANT | CardTypeSet::SORCERY)
                            {
                                if top.exile_on_leaving_stack {
                                    ZoneKind::Exile
                                } else {
                                    ZoneKind::Graveyard
                                }
                            } else {
                                ZoneKind::Battlefield
                            }
                        });
                        if let Some(destination) = destination {
                            let run_script = destination == ZoneKind::Battlefield;
                            if let Some(event) =
                                self.move_resolving_spell(top.stack_id, destination, run_script)
                            {
                                events.push(event);
                            }
                        }
                        // else: defensive - the spell's GameObject should
                        // always still be at top.stack_id (nothing else
                        // removes it while on the stack); None here would
                        // mean some other path already mutated it
                        // unexpectedly. Skip the zone move rather than
                        // panic, same defensive stance as the empty-stack
                        // branch above.
                    }
                    // ActivatedAbility/Copy: nothing on a zone moves when these
                    // resolve (CR 608's ability-resolution path doesn't involve
                    // a card) - only the Spell branch above produces a
                    // ZoneChange. Activated and triggered abilities resolve the
                    // same way: `top.effect`'s Rust-native fast path when it's a
                    // recognized primitive (no Lua call at all), falling back to
                    // the real script bridge (CR 602.2b/603.4/608.2m's
                    // resolution proper) only when it's None.
                    if matches!(
                        top.kind,
                        StackEntryKind::TriggeredAbility | StackEntryKind::ActivatedAbility
                    ) {
                        match &top.effect {
                            Some(AbilityEffect::GainLife(amount)) => {
                                if let Some(player) = self.state.player_mut(top.controller) {
                                    player.gain_life(*amount);
                                }
                            }
                            // AddMana is only ever set on a mana ability, which
                            // resolves immediately and never reaches the stack
                            // (CR 605.1a/605.3b) - no StackObject of either kind
                            // should ever carry this variant. Defensive no-op
                            // rather than a panic.
                            Some(AbilityEffect::AddMana(_)) => {}
                            None => {
                                // TODO: a script error here is silently dropped
                                // - no path from ResolveStackTop back to a
                                // StepStatus error exists for a mid-resolution
                                // script failure yet.
                                let _ = self.script_runtime.invoke_operation(
                                    top.operation,
                                    &mut self.state,
                                    top.source,
                                    top.controller,
                                );
                            }
                        }
                    }
                    events.push(GameEventKind::StackObjectResolved { stack_id: top.stack_id });
                }
                self.process_state =
                    ProcessState::ReplacementCheck { remaining: events.into(), checked: Vec::new() };
                StepStatus::Progressed
            }

            ProcessState::ReplacementCheck { mut remaining, mut checked } => {
                // Processes one event per step() call rather than mapping
                // the whole Vec in one shot — apply_replacements can return
                // NeedsOrderingChoice (CR 616.1) for any of them, and that
                // has to pause the whole pipeline run on a real
                // AwaitingReplacementChoice, not get silently dropped or
                // resolved without the player. `checked`/`remaining` carry
                // this run's progress across that pause.
                let Some(event) = remaining.pop_front() else {
                    self.process_state = ProcessState::CommitEvents { checked };
                    return StepStatus::Progressed;
                };
                match replacement::apply_replacements(&mut self.state, event) {
                    ReplacementOutcome::Resolved(event) => {
                        checked.push(event);
                        self.process_state = ProcessState::ReplacementCheck { remaining, checked };
                    }
                    ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded } => {
                        self.process_state = ProcessState::AwaitingReplacementChoice {
                            affected_player,
                            pending_event: event,
                            candidates,
                            excluded,
                            resume: ReplacementResumeTarget::ReplacementCheck { remaining, checked },
                        };
                    }
                }
                StepStatus::Progressed
            }

            ProcessState::AwaitingReplacementChoice { affected_player, pending_event, candidates, excluded, resume } => {
                // Nothing to do until submit_replacement_choice answers -
                // put the exact same state back and tell the caller who
                // needs to be prompted, mirroring PriorityWindow's
                // AwaitingAction pattern.
                let status = StepStatus::AwaitingReplacementChoice {
                    player: affected_player,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event,
                    candidates,
                    excluded,
                    resume,
                };
                status
            }

            ProcessState::CommitEvents { checked } => {
                let (caused_by_command, parent_event) = self
                    .pipeline_context
                    .as_ref()
                    .map(|ctx| (ctx.caused_by_command, ctx.parent_event))
                    .unwrap_or((None, None));

                for kind in checked {
                    // CR 510.2/614-616: a DamageDealt event describes an
                    // effect that hasn't happened to state yet - apply it
                    // now that it's in its final, post-replacement form
                    // (amount/target may have been prevented/redirected).
                    // See commands::apply_damage_event's own doc comment for
                    // why this can't happen any earlier than here.
                    if let GameEventKind::DamageDealt { source, target, amount } = &kind {
                        commands::apply_damage_event(&mut self.state, &self.script_runtime, *source, *target, *amount);
                    }
                    let event_id = self.state.allocate_event_id();
                    let sequence_no = self.state.allocate_sequence_no();
                    self.state.ledger.push(GameEvent {
                        event_id,
                        sequence_no,
                        kind,
                        caused_by_command,
                        parent_event,
                    });
                }
                // CR 601.2f-h: if this run's command was a CastSpell that
                // needs payment, pause here instead of continuing into
                // SbaLoop - the events above (card moved to stack) are
                // already committed regardless (CR 601.2a happens before
                // cost is paid), but the cast process itself isn't finished
                // until Engine::submit_payment answers. ResolveStackTop runs
                // never set pending_payment, so they fall straight through
                // to SbaLoop exactly as before this slice.
                self.process_state = match self.pending_payment.take() {
                    Some(PendingPaymentRequest { stack_id, payer, cost, source }) => {
                        ProcessState::AwaitingPayment { stack_id, payer, cost, source }
                    }
                    None => ProcessState::SbaLoop,
                };
                StepStatus::Progressed
            }

            ProcessState::AwaitingPayment { stack_id, payer, cost, source } => {
                // Nothing to do until submit_payment answers - put the
                // exact same state back and tell the caller who needs to be
                // prompted, mirroring AwaitingReplacementChoice/
                // AwaitTriggerOrderChoice.
                let status = StepStatus::AwaitingPayment { player: payer, stack_id };
                self.process_state = ProcessState::AwaitingPayment { stack_id, payer, cost, source };
                status
            }

            ProcessState::SbaLoop => {
                self.process_state = match sba::check_state_based_actions(&mut self.state, &self.script_runtime) {
                    SbaOutcome::Progressed(events) => self.finish_sba_loop(events),
                    SbaOutcome::NeedsOrderingChoice {
                        affected_player,
                        pending_event,
                        candidates,
                        excluded,
                        pending_deaths,
                        resolved_so_far,
                    } => {
                        // Unaffected by the pause below (player-loss events,
                        // plus any deaths already resolved earlier in this
                        // same pass) - commit them now rather than holding
                        // them hostage to a choice about a different death.
                        self.state.ledger.extend(resolved_so_far);
                        ProcessState::AwaitingReplacementChoice {
                            affected_player,
                            pending_event,
                            candidates,
                            excluded,
                            resume: ReplacementResumeTarget::SbaDeath { pending_deaths, then: SbaResumeThen::SbaLoop },
                        }
                    }
                };
                StepStatus::Progressed
            }

            ProcessState::CollectTriggers { since } => {
                let mut pending = Vec::new();
                for event in &self.state.ledger[since..] {
                    pending.extend(triggers::collect_triggers(&self.state, event, &self.script_runtime));
                }
                let groups = triggers::group_apnap(&self.state, pending);
                self.process_state =
                    ProcessState::PutTriggersApnap { ordered: Vec::new(), pending_groups: groups.into() };
                StepStatus::Progressed
            }

            ProcessState::PutTriggersApnap { mut ordered, mut pending_groups } => {
                // Walks pending_groups (already in APNAP order, per
                // triggers::group_apnap) one group per step() call rather
                // than ordering+pushing them all in one shot - a group with
                // more than one trigger needs its controller to choose the
                // intra-group order (CR 603.3b), which has to pause the
                // whole run on AwaitTriggerOrderChoice, not get resolved
                // without them.
                let Some((player, group)) = pending_groups.pop_front() else {
                    // Push each trigger in `ordered` onto self.state.stack
                    // as a real StackObject (CR 603.3b/603.4) - looked up by
                    // (source, ability_index) via commands::find_ability,
                    // same "nth ability on this source" convention
                    // ActivateAbility already relies on.
                    for trigger in ordered {
                        let Some(ability) =
                            commands::find_ability(&self.state, trigger.source, trigger.ability_index)
                        else {
                            // Ability no longer resolves (e.g. its source
                            // left play between being collected and now) -
                            // TODO(CR 603.3d): a leaves-the-battlefield
                            // triggered ability still goes on the stack
                            // using last-known information; not handled
                            // this slice (see collect_triggers's own note),
                            // so this trigger is silently dropped instead.
                            continue;
                        };
                        let operation = ability.operation;
                        let effect = ability.effect.clone();
                        let target_requirement = ability.target;
                        let stack_id = self.state.allocate_object_id();
                        // TODO(CR 603.3c): triggered abilities that require
                        // targets ("when this dies, destroy target
                        // creature") aren't choosable yet - targets is
                        // always empty here, same script-bridge-mid-
                        // pipeline gap invoke_target/select_targets already
                        // flag. target_requirement is still baked onto the
                        // StackObject (in case a future ability sets a
                        // fixed/no-choice target) so ResolveStackTop's CR
                        // 608.2b recheck applies uniformly regardless of
                        // StackEntryKind.
                        self.state.stack.push(StackObject::new(
                            stack_id,
                            StackEntryKind::TriggeredAbility,
                            trigger.source,
                            trigger.controller,
                            Vec::new(),
                            Vec::new(),
                            target_requirement,
                            None,
                            false,
                            operation,
                            effect,
                        ));
                        let event_id = self.state.allocate_event_id();
                        let sequence_no = self.state.allocate_sequence_no();
                        self.state.ledger.push(GameEvent {
                            event_id,
                            sequence_no,
                            kind: GameEventKind::StackObjectPushed { stack_id },
                            caused_by_command: None,
                            parent_event: None,
                        });
                    }
                    self.process_state = self.end_pipeline_run();
                    return StepStatus::Progressed;
                };
                if group.len() <= 1 {
                    ordered.extend(group);
                    self.process_state = ProcessState::PutTriggersApnap { ordered, pending_groups };
                } else {
                    self.process_state =
                        ProcessState::AwaitTriggerOrderChoice { player, group, ordered, pending_groups };
                }
                StepStatus::Progressed
            }

            ProcessState::AwaitTriggerOrderChoice { player, group, ordered, pending_groups } => {
                // Nothing to do until submit_trigger_order answers - put
                // the exact same state back and tell the caller who needs
                // to be prompted, mirroring AwaitingReplacementChoice.
                let status =
                    StepStatus::AwaitingTriggerOrderChoice { player, group: group.clone() };
                self.process_state =
                    ProcessState::AwaitTriggerOrderChoice { player, group, ordered, pending_groups };
                status
            }

            ProcessState::AdvanceTurnStep => {
                let events = self.advance_turn_step();
                self.log_events(events);

                if self.state.step == Step::Untap {
                    // CR 502.4: no priority window during untap - perform
                    // its turn-based action (CR 502.3) and loop straight
                    // back into AdvanceTurnStep instead of opening
                    // PriorityWindow. The next call into this same arm runs
                    // advance_turn_step() again (Untap -> Upkeep), which
                    // does open priority normally below.
                    let untap_events = self.untap_step();
                    self.log_events(untap_events);
                    self.process_state = ProcessState::AdvanceTurnStep;
                    return StepStatus::Progressed;
                }
                if self.state.step == Step::Draw {
                    // CR 500.7/500.8: consume any queued skip before doing
                    // anything — a skipped step has no turn-based actions and
                    // no priority window.
                    let active = self.state.active_player;
                    if self.state.consume_step_skip(active, Step::Draw) {
                        self.process_state = ProcessState::AdvanceTurnStep;
                        return StepStatus::Progressed;
                    }
                    // CR 504.1: draw is a turn-based action that happens
                    // before priority opens (504.2 still opens it normally
                    // below, unlike untap).
                    let draw_events = self.draw_step();
                    self.log_events(draw_events);
                }
                if self.state.step == Step::Cleanup {
                    // CR 514.3: normally no priority during cleanup - run
                    // cleanup's turn-based actions (514.1 discard, 514.2
                    // effect expiry) then check SBAs before deciding
                    // whether to open a priority window or advance the turn
                    // silently. begin_cleanup_step handles all of this.
                    return self.begin_cleanup_step();
                }

                // CR 117.3b: priority opens to the active player at the
                // start of each step/phase.
                self.state.priority_player = self.state.active_player;
                self.process_state = ProcessState::PriorityWindow;
                StepStatus::Progressed
            }

            ProcessState::AwaitingDiscard { player, excess } => {
                // Nothing to do until submit_discard answers - put the
                // exact same state back and tell the caller who needs to be
                // prompted, mirroring AwaitingPayment's pattern.
                let hand_size = self
                    .state
                    .zone(ZoneKind::Hand, Some(player))
                    .map(|z| z.contents.len())
                    .unwrap_or(0);
                let status =
                    StepStatus::AwaitingDiscard { player, hand_size, max_hand_size: 7 };
                self.process_state = ProcessState::AwaitingDiscard { player, excess };
                status
            }

            ProcessState::CleanupSbaCheck => {
                self.process_state = match sba::check_state_based_actions(&mut self.state, &self.script_runtime) {
                    SbaOutcome::Progressed(events) => self.finish_cleanup_sba_check(events),
                    SbaOutcome::NeedsOrderingChoice {
                        affected_player,
                        pending_event,
                        candidates,
                        excluded,
                        pending_deaths,
                        resolved_so_far,
                    } => {
                        self.state.ledger.extend(resolved_so_far);
                        ProcessState::AwaitingReplacementChoice {
                            affected_player,
                            pending_event,
                            candidates,
                            excluded,
                            resume: ReplacementResumeTarget::SbaDeath {
                                pending_deaths,
                                then: SbaResumeThen::CleanupSbaCheck,
                            },
                        }
                    }
                };
                StepStatus::Progressed
            }
            ProcessState::AwaitingMulliganDecision { player } => {
                self.process_state = ProcessState::AwaitingMulliganDecision { player };
                StepStatus::AwaitingMulliganDecision { player }
            }
            ProcessState::AwaitingBottomCardChoice { player, count, remaining } => {
                self.process_state = ProcessState::AwaitingBottomCardChoice { player, count, remaining };
                StepStatus::AwaitingBottomCardChoice { player, count }
            }
        }
    }

    /// `SbaLoop`'s own "what happens once this pass's events are known"
    /// transition — pulled out so a resumed CR 616.1 pause
    /// (`resume_sba_death`) and a fresh pass (`SbaLoop`'s own arm) agree on
    /// it instead of duplicating the branch.
    fn finish_sba_loop(&mut self, events: Vec<GameEvent>) -> ProcessState {
        if events.is_empty() {
            let since = self.pipeline_context.as_ref().map(|ctx| ctx.since_ledger).unwrap_or(0);
            ProcessState::CollectTriggers { since }
        } else {
            self.state.ledger.extend(events);
            // Re-enter SbaLoop next step() - CR 704.3's fixed point, not a
            // single pass.
            ProcessState::SbaLoop
        }
    }

    /// `CleanupSbaCheck`'s own "what happens once this pass's events are
    /// known" transition — pulled out for the same reason `finish_sba_loop`
    /// is.
    fn finish_cleanup_sba_check(&mut self, events: Vec<GameEvent>) -> ProcessState {
        if !events.is_empty() {
            // CR 514.3: SBAs fired during cleanup - open a priority window
            // so players can respond.
            // TODO: CR 514.3 says another cleanup step begins after this
            // priority window and its stack empties - currently
            // advance_turn_step's (Ending, Cleanup) arm rotates the turn
            // instead, skipping re-running discard/effect-expiry. Correct
            // once a case actually reaches this branch.
            self.state.ledger.extend(events);
            self.state.priority_player = self.state.active_player;
            ProcessState::PriorityWindow
        } else {
            // CR 514.3: no SBAs, no triggers (collect_triggers is todo!()
            // for any real ability) - skip priority entirely, advance the
            // turn.
            ProcessState::AdvanceTurnStep
        }
    }

    /// Continues `sba::resolve_deaths` after a paused CR 616.1 ordering
    /// choice was answered for one dying creature — `already_resolved` is
    /// that creature's own just-committed death event; `pending_deaths` is
    /// the rest of this SBA pass's dying-creature list. Finishes into
    /// whichever state originally started this pass (`then`), via
    /// `finish_sba_loop`/`finish_cleanup_sba_check`, so a resumed pause and
    /// a fresh pass reach the exact same "nothing more to do" transition.
    fn resume_sba_death(
        &mut self,
        pending_deaths: VecDeque<ObjectId>,
        mut already_resolved: Vec<GameEvent>,
        then: SbaResumeThen,
    ) -> ProcessState {
        match sba::resolve_deaths(&mut self.state, pending_deaths) {
            sba::DeathResolution::Done(mut events) => {
                already_resolved.append(&mut events);
                match then {
                    SbaResumeThen::SbaLoop => self.finish_sba_loop(already_resolved),
                    SbaResumeThen::CleanupSbaCheck => self.finish_cleanup_sba_check(already_resolved),
                }
            }
            sba::DeathResolution::NeedsOrderingChoice {
                affected_player,
                pending_event,
                candidates,
                excluded,
                pending_deaths,
            } => {
                // already_resolved (this pass's progress so far) commits now
                // - it's unaffected by this further pause.
                self.state.ledger.extend(already_resolved);
                ProcessState::AwaitingReplacementChoice {
                    affected_player,
                    pending_event,
                    candidates,
                    excluded,
                    resume: ReplacementResumeTarget::SbaDeath { pending_deaths, then },
                }
            }
        }
    }

    /// Wraps each `GameEventKind` into a `GameEvent` (allocating its
    /// `event_id`/`sequence_no`) and pushes it to the ledger. Pulled out of
    /// the `AdvanceTurnStep` arm so the same wrapping logic isn't repeated
    /// for the base step transition, `untap_step`, and `draw_step` alike.
    fn log_events(&mut self, events: Vec<GameEventKind>) {
        for kind in events {
            let event_id = self.state.allocate_event_id();
            let sequence_no = self.state.allocate_sequence_no();
            self.state.ledger.push(GameEvent {
                event_id,
                sequence_no,
                kind,
                caused_by_command: None,
                parent_event: None,
            });
        }
    }

    /// CR 502.3: untaps every battlefield permanent the active player
    /// controls, as the untap step's turn-based action. No phasing (CR
    /// 502.1 - `phased_out` stays untouched, out of scope this slice) and
    /// no "doesn't untap" query (no continuous-effects query API exists
    /// yet - same gap `PlayLand`'s land-limit TODO already flags).
    fn untap_step(&mut self) -> Vec<GameEventKind> {
        let active = self.state.active_player;
        let mut ids: Vec<ObjectId> = self
            .state
            .objects
            .iter()
            .filter(|(_, o)| o.zone == ZoneKind::Battlefield && o.controller == active && o.tapped)
            .map(|(id, _)| *id)
            .collect();
        // HashMap iteration order is unstable - sort so the ledger is
        // deterministic.
        ids.sort_by_key(|id| id.0);

        let mut events = Vec::with_capacity(ids.len());
        for id in ids {
            self.state.object_mut(id).expect("id just collected from state.objects").tapped = false;
            events.push(GameEventKind::Untapped { object: id });
        }
        events
    }

    /// CR 504.1: the active player draws one card as the draw step's
    /// turn-based action, following the same CR 400.7 new-`ObjectId` zone
    /// move `PlayLand`/`CastSpell`/SBA death already use. CR 103.8a: in a
    /// two-player game, the starting player skips the draw on their first
    /// turn - `turn_number == 1` always belongs to the starting player
    /// since `active_player` only rotates at Cleanup, so that check alone
    /// scopes this correctly without tracking who started separately.
    /// Drawing from an empty library is a silent no-op here, not a loss -
    /// CR 704.5b's empty-library SBA isn't modeled yet (TODO).
    fn draw_step(&mut self) -> Vec<GameEventKind> {
        let active = self.state.active_player;
        let Some(library) = self.state.zone(ZoneKind::Library, Some(active)) else {
            return Vec::new();
        };
        let Some(card) = library.contents.front().copied() else {
            return Vec::new();
        };

        // Snapshot before the move - CR 603.10's last_known_info (see
        // GameEventKind::ZoneChange's doc comment), same ordering PlayLand
        // uses.
        let last_known_info =
            self.state.object(card).expect("card id came from the library zone's own contents").clone();

        let new_id = self.state.allocate_object_id();
        let mut new_object = last_known_info.clone();
        new_object.object_id = new_id;
        new_object.lineage = ObjectLineage { previous: Some(card) };
        new_object.zone = ZoneKind::Hand;
        new_object.timestamp = self.state.allocate_sequence_no();

        self.state
            .zone_mut(ZoneKind::Library, Some(active))
            .expect("checked above")
            .remove(card);
        // CR 400.7: drawn, no remap follows - purge any RegisteredAbility
        // still pointing at the library-zone id.
        commands::retire_object(&mut self.state, card);
        self.state
            .zone_mut(ZoneKind::Hand, Some(active))
            .expect("actor has no hand zone")
            .push_bottom(new_id);
        self.state.objects.insert(new_id, new_object);

        vec![GameEventKind::ZoneChange {
            object: new_id,
            from: ZoneKind::Library,
            to: ZoneKind::Hand,
            last_known_info: Some(last_known_info),
        }]
    }

    /// CR 514.1/514.2/514.3: entry point for the cleanup step's turn-based
    /// actions. Called from `AdvanceTurnStep` when `advance_turn_step()`
    /// transitions into `Step::Cleanup`.
    ///
    /// Order: (1) drain "until end of turn" continuous effects (514.2),
    /// (2) check hand size and pause for discard if needed (514.1),
    /// (3) queue `CleanupSbaCheck` so the next `step()` decides whether
    /// priority opens (CR 514.3 exception) or the turn advances silently.
    fn begin_cleanup_step(&mut self) -> StepStatus {
        // CR 514.2: drain "until end of turn" continuous effects.
        // TODO: ReplacementEffect has no `duration` field yet - once added,
        // drain state.replacement_effects here with the same retain().
        self.state
            .continuous_effects
            .retain(|e| !matches!(e.duration, EffectDuration::UntilEndOfTurn));

        // CR 514.1: active player discards to max hand size if needed.
        let active = self.state.active_player;
        let hand_size = self
            .state
            .zone(ZoneKind::Hand, Some(active))
            .map(|z| z.contents.len())
            .unwrap_or(0);
        // TODO(CR 402.2): hardcoded to 7 - doesn't consult
        // state.continuous_effects for effects that modify max hand size
        // (e.g. "you have no maximum hand size"). Revisit once a continuous
        // effect that modifies this exists and has a query API.
        const MAX_HAND_SIZE: usize = 7;

        if hand_size > MAX_HAND_SIZE {
            let excess = hand_size - MAX_HAND_SIZE;
            self.process_state = ProcessState::AwaitingDiscard { player: active, excess };
            return StepStatus::AwaitingDiscard { player: active, hand_size, max_hand_size: MAX_HAND_SIZE };
        }

        // No discard needed - proceed to the SBA check.
        self.process_state = ProcessState::CleanupSbaCheck;
        StepStatus::Progressed
    }

    /// Deterministic CR 500-514 phase/step transition table for this slice:
    /// advances `state.phase`/`step`/`turn_number`/`active_player` and
    /// returns the log events for it. Does NOT carry out any of the actual
    /// rules work a step triggers (untapping permanents, drawing a card,
    /// etc.) - that's separate, larger work still gated on other `todo!()`s
    /// (layers.rs, zone manipulation); this only answers "what step comes
    /// next."
    ///
    /// CR 505.1's "Main happens twice per turn" ambiguity (`Phase` alone
    /// can't tell which - see turn.rs's `Phase` doc) is resolved by `Step`
    /// itself now (`PreCombatMain`/`PostCombatMain`), not a separate
    /// `postcombat_main: bool` this table has to remember to flip - see
    /// turn.rs's `Step` doc for why that field was folded in here instead.
    fn advance_turn_step(&mut self) -> Vec<GameEventKind> {
        let phase_before = self.state.phase;
        let starting_new_turn = match (self.state.phase, self.state.step) {
            (Phase::PreGame, _) => {
                self.state.phase = Phase::Beginning;
                self.state.step = Step::Untap;
                // CR 302.6/602.5a: seeds the active player's first
                // "most recent turn began at" mark - without this,
                // has_summoning_sickness's None branch would treat every
                // object as sick forever, since no rotation has happened
                // yet to set it.
                self.state
                    .last_active_turn_number
                    .insert(self.state.active_player, self.state.turn_number);
                // CR 103.8a: in a two-player game, the starting player skips
                // their draw step on turn 1. Seeded here so the general
                // consume_step_skip path handles it uniformly with any
                // card effect that also says "skip your next draw step".
                if self.state.turn_order.len() == 2 {
                    let starter = self.state.active_player;
                    self.state.pending_step_skips.push((starter, Step::Draw));
                }
                true
            }
            (Phase::Beginning, Step::Untap) => {
                self.state.step = Step::Upkeep;
                false
            }
            (Phase::Beginning, Step::Upkeep) => {
                self.state.step = Step::Draw;
                false
            }
            (Phase::Beginning, Step::Draw) => {
                self.state.phase = Phase::Main;
                self.state.step = Step::PreCombatMain;
                false
            }
            (Phase::Main, Step::PreCombatMain) => {
                self.state.phase = Phase::Combat;
                self.state.step = Step::BeginningOfCombat;
                false
            }
            (Phase::Combat, Step::BeginningOfCombat) => {
                self.state.step = Step::DeclareAttackers;
                false
            }
            (Phase::Combat, Step::DeclareAttackers) => {
              // CR 506.1: if no attackers were declared, skip to end of combat
              if self.state.combat.as_ref().map(|c| c.attackers.is_empty()).unwrap_or(true) {
                  self.state.phase = Phase::Combat;
                  self.state.step = Step::EndOfCombat;
              } else {
                self.state.step = Step::DeclareBlockers;
              }
              false
            }
            (Phase::Combat, Step::DeclareBlockers) => {
                self.state.step = Step::CombatDamage;
                false
            }
            (Phase::Combat, Step::CombatDamage) => {
                self.state.step = Step::EndOfCombat;
                false
            }
            (Phase::Combat, Step::EndOfCombat) => {
                self.state.phase = Phase::Main;
                self.state.step = Step::PostCombatMain;
                // CR 506.4: attacking/blocking status ends with the combat
                // phase - clear it so it doesn't leak into the next combat.
                self.state.combat = None;
                false
            }
            (Phase::Main, Step::PostCombatMain) => {
                self.state.phase = Phase::Ending;
                self.state.step = Step::End;
                false
            }
            (Phase::Ending, Step::End) => {
                self.state.step = Step::Cleanup;
                // CR 514.2: all damage marked on permanents is removed as a
                // turn-based action of the cleanup step. The combat-damage
                // slice is the first real damage_marked producer in this
                // engine - without this, marked damage would persist across
                // turns and falsely re-trigger the lethal-damage SBA next
                // turn.
                for object in self.state.objects.values_mut() {
                    if object.zone == ZoneKind::Battlefield {
                        object.damage_marked = 0;
                        object.deathtouched = false;
                    }
                }
                false
            }
            (Phase::Ending, Step::Cleanup) => {
                let current_idx = self
                    .state
                    .turn_order
                    .iter()
                    .position(|p| *p == self.state.active_player)
                    .unwrap_or(0);
                let next_idx = (current_idx + 1) % self.state.turn_order.len();
                self.state.active_player = self.state.turn_order[next_idx];
                self.state.turn_number += 1;
                self.state.phase = Phase::Beginning;
                self.state.step = Step::Untap;
                // CR 305.2's per-turn land-drop count resets with the turn.
                self.state.lands_played_this_turn.clear();
                // CR 302.6/602.5a: the new active player's most recent
                // turn now begins at this turn_number.
                self.state
                    .last_active_turn_number
                    .insert(self.state.active_player, self.state.turn_number);
                true
            }
            (phase, step) => unreachable!(
                "invalid Phase/Step combination {phase:?}/{step:?} - Step values are only \
                 meaningful within the Phase they belong to (CR 500-514); reaching this means \
                 state held a combination that doesn't correspond to any real turn position"
            ),
        };

        // Only re-emit PhaseStarted when the phase actually changed - e.g.
        // Untap -> Upkeep stays within Beginning and shouldn't double-log
        // it once Untap auto-chains into Upkeep without a player action
        // (CR 502.4) in between.
        let mut events = Vec::new();
        if self.state.phase != phase_before {
            events.push(GameEventKind::PhaseStarted { phase: self.state.phase });
        }
        events.push(GameEventKind::StepStarted { step: self.state.step });
        if starting_new_turn {
            events.push(GameEventKind::TurnStarted {
                player: self.state.active_player,
                turn_number: self.state.turn_number,
            });
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::counter::CounterSet;
    use crate::types::effect::ConsumptionPolicy;
    use crate::types::ids::Target;
    use crate::types::mana::ManaPool;
    use crate::types::player::PlayerState;
    use std::collections::HashMap;

    fn minimal_state() -> GameState {
        let p0 = PlayerId(0);
        GameState {
            players: vec![PlayerState {
                player_id: p0,
                life: 20,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                mulligans_taken: 0,
                hand_kept: false,
            }],
            turn_order: vec![p0],
            active_player: p0,
            turn_number: 1,
            phase: Phase::PreGame,
            step: Step::Untap,
            priority_player: p0,
            consecutive_passes: 0,
            lands_played_this_turn: HashMap::new(),
            combat: None,
            last_active_turn_number: HashMap::new(),
            objects: HashMap::new(),
            zones: Vec::new(),
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: crate::types::game::RngState { seed: 1, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 0,
            pending_step_skips: Vec::new(),
        }
    }

    fn trigger(id: u64, controller: PlayerId) -> PendingTrigger {
        PendingTrigger { source: ObjectId(id), controller, ability_index: 0 }
    }

    #[test]
    fn is_permutation_accepts_reorderings_and_rejects_substitutions() {
        let group = vec![trigger(1, PlayerId(0)), trigger(2, PlayerId(0))];
        // A true reordering (same elements, different sequence) must pass.
        assert!(is_permutation(&[group[1], group[0]], &group));
        // Same length but a substituted element must be rejected - length
        // alone can't catch this.
        assert!(!is_permutation(&[group[0], trigger(3, PlayerId(0))], &group));
        // A duplicate of one element instead of using both must be rejected.
        assert!(!is_permutation(&[group[0], group[0]], &group));
        // Wrong length, caught the same way as before.
        assert!(!is_permutation(&[group[0]], &group));
    }

    #[test]
    fn submit_trigger_order_rejects_non_permutation_and_accepts_real_reordering() {
        let player = PlayerId(0);
        let group = vec![trigger(1, player), trigger(2, player)];

        let mut engine = Engine::new(minimal_state());
        engine.process_state = ProcessState::AwaitTriggerOrderChoice {
            player,
            group: group.clone(),
            ordered: Vec::new(),
            pending_groups: VecDeque::new(),
        };
        let substituted = vec![group[0], trigger(99, player)];
        let err = engine.submit_trigger_order(substituted).unwrap_err();
        assert!(matches!(err, TriggerOrderError::NotAPermutation));

        // Engine must still be sitting in AwaitTriggerOrderChoice - a
        // rejected answer must not have consumed the pending state.
        assert!(matches!(engine.process_state, ProcessState::AwaitTriggerOrderChoice { .. }));

        let reordered = vec![group[1], group[0]];
        engine.submit_trigger_order(reordered).expect("a true reordering must be accepted");
        assert!(matches!(engine.process_state, ProcessState::PutTriggersApnap { .. }));
    }

    // ---------------------------------------------------------------------
    // Replacement/prevention (CR 614-616) - real ReplacementCheck/
    // AwaitingReplacementChoice pipeline wiring, driven directly (not
    // through combat/SBA - see docs/implementation.md's own note on why
    // those two producers can't exercise this for real yet).
    // ---------------------------------------------------------------------

    fn dummy_object(id: ObjectId, controller: PlayerId) -> crate::types::object::GameObject {
        use crate::types::card::{CardTypeSet, KeywordSet, PTValue, ScriptReference, SupertypeSet};
        use crate::types::object::VisibilityPolicy;
        crate::types::object::GameObject {
            object_id: id,
            lineage: crate::types::ids::ObjectLineage { previous: None },
            definition: crate::types::card::CardDefinition {
                definition_id: 1,
                name: "Test Object".to_string(),
                mana_cost: crate::types::mana::ManaCost { symbols: Vec::new() },
                color_indicator: None,
                supertypes: SupertypeSet::empty(),
                card_types: CardTypeSet::ARTIFACT,
                subtypes: Vec::new(),
                oracle_text: String::new(),
                power: Some(PTValue::Fixed(1)),
                toughness: Some(PTValue::Fixed(1)),
                loyalty: None,
                defense: None,
                script_ref: ScriptReference::ByKey("test".to_string()),
                keywords: KeywordSet::empty(),
                target_requirement: None,
                alternative_costs: Vec::new(),
            },
            owner: controller,
            controller,
            zone: crate::types::zone::ZoneKind::Battlefield,
            tapped: false,
            flipped: false,
            face_down: false,
            phased_out: false,
            damage_marked: 0,
            deathtouched: false,
            counters: CounterSet::new(),
            attachments: Vec::new(),
            attached_to: None,
            timestamp: crate::types::ids::SequenceNo(0),
            controlled_since_turn: 0,
            visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
        }
    }

    fn prevent_all_damage(source: ObjectId) -> crate::types::effect::ReplacementEffect {
        use crate::types::effect::{EffectFilter, EventFilter, ReplacementPayload, ReplacementSpec};
        crate::types::effect::ReplacementEffect {
            source,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt {
                    affected: EffectFilter { card_types: None, controller: None, self_only: false },
                },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::Repeatable,
        }
    }

    #[test]
    fn replacement_check_applies_single_match_and_commits_rewritten_event() {
        let p0 = PlayerId(0);
        let effect_source = ObjectId(0);
        let mut state = minimal_state();
        state.objects.insert(effect_source, dummy_object(effect_source, p0));
        state.replacement_effects.push(prevent_all_damage(effect_source));

        let mut engine = Engine::new(state);
        let event = GameEventKind::DamageDealt { source: ObjectId(1), target: Target::Player(p0), amount: 7 };
        engine.process_state =
            ProcessState::ReplacementCheck { remaining: VecDeque::from([event]), checked: Vec::new() };

        loop {
            match engine.step().status {
                StepStatus::Progressed => continue,
                _ => break,
            }
        }

        let last = engine.state().ledger.last().expect("CommitEvents must have appended the resolved event");
        assert!(
            matches!(last.kind, GameEventKind::DamageDealt { amount: 0, .. }),
            "the registered PreventDamage effect must have zeroed the committed event's amount"
        );
    }

    #[test]
    fn submit_replacement_choice_rejects_invalid_candidate_and_not_awaiting() {
        let mut engine = Engine::new(minimal_state());
        let err = engine.submit_replacement_choice(ObjectId(0)).unwrap_err();
        assert!(matches!(err, ReplacementChoiceError::NotAwaitingChoice));

        engine.process_state = ProcessState::AwaitingReplacementChoice {
            affected_player: PlayerId(0),
            pending_event: GameEventKind::DamageDealt {
                source: ObjectId(9),
                target: Target::Player(PlayerId(0)),
                amount: 1,
            },
            candidates: vec![ObjectId(1), ObjectId(2)],
            excluded: Vec::new(),
            resume: ReplacementResumeTarget::ReplacementCheck { remaining: VecDeque::new(), checked: Vec::new() },
        };
        let err = engine.submit_replacement_choice(ObjectId(99)).unwrap_err();
        assert!(matches!(err, ReplacementChoiceError::InvalidCandidate));
        // A rejected answer must not have consumed the pending state.
        assert!(matches!(engine.process_state, ProcessState::AwaitingReplacementChoice { .. }));
    }

    #[test]
    fn submit_replacement_choice_applies_chosen_then_resolves_the_other_and_commits() {
        let p0 = PlayerId(0);
        let a = ObjectId(0);
        let b = ObjectId(1);
        let mut state = minimal_state();
        state.objects.insert(a, dummy_object(a, p0));
        state.objects.insert(b, dummy_object(b, p0));
        state.replacement_effects.push(prevent_all_damage(a));
        state.replacement_effects.push(prevent_all_damage(b));

        let mut engine = Engine::new(state);
        let event = GameEventKind::DamageDealt { source: ObjectId(2), target: Target::Player(p0), amount: 7 };
        engine.process_state =
            ProcessState::ReplacementCheck { remaining: VecDeque::from([event]), checked: Vec::new() };

        // Both effects match unrestricted - one step() call should pause on
        // the CR 616.1 ordering choice rather than guessing an order.
        assert!(matches!(engine.step().status, StepStatus::Progressed));
        let ProcessState::AwaitingReplacementChoice { candidates, .. } = &engine.process_state else {
            panic!("expected AwaitingReplacementChoice - two effects matched the same event");
        };
        assert_eq!(candidates.len(), 2);

        engine.submit_replacement_choice(a).expect("a is a real candidate");
        // b still matches the (already-prevented) event since it isn't
        // excluded yet - this drains it too, then commits once nothing else
        // matches (CR 616.1: a is now excluded and can't reapply).
        loop {
            match engine.step().status {
                StepStatus::Progressed => continue,
                _ => break,
            }
        }

        let last = engine.state().ledger.last().expect("CommitEvents must have appended the resolved event");
        assert!(matches!(last.kind, GameEventKind::DamageDealt { amount: 0, .. }));
    }
}
