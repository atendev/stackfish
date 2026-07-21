use std::collections::HashMap;
use std::collections::VecDeque;
use std::ops::Range;

use crate::commands::{self, CommandError, PendingPaymentRequest, PriorityHandoff};
use crate::engine_context::{PipelineContext, PipelineExit};
use crate::replacement::{self, ReplacementOutcome};
use crate::sba::{self, SbaOutcome};
use crate::script::{OperationOutcome, ScriptChoiceRequest, ScriptError, ScriptRuntime};
use crate::triggers::{self, PendingTrigger};
use crate::types::ability::{
    AbilityEffect, AbilityKind, ConditionSpec, InterveningIfSpec, NamedToken, RegisteredAbility,
    SearchDestination, TokenSpec,
};
use crate::types::card::{CardDefinition, CardTypeSet, KeywordSet, PTValue, ScriptReference, Subtype, SupertypeSet};
use crate::types::config::{GameConfig, GameSettings};
use crate::types::command::{EngineCommand, PlayerAction, PlayerActionKind};
use crate::types::cost::{AdditionalCost, CostSpec, FilterController, FilterSpec, PaymentPlan};
use crate::types::counter::CounterSet;
use crate::types::effect::{
    AffectedFilter, ConsumptionPolicy, ContinuousEffect, EffectDuration, EffectFilter, EffectPayload,
    EffectPrimitive, EventFilter, Layer, ReplacementEffect, ReplacementPayload, ReplacementSpec,
};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::{GameState, RngState};
use crate::types::ids::{CommandId, ObjectId, ObjectLineage, PlayerId, ScriptHandle, SequenceNo, Target};
use crate::types::mana::{ManaCost, ManaPool};
use crate::types::object::{GameObject, VisibilityPolicy};
use crate::types::player::{effective_max_hand_size, PlayerState};
use crate::types::stack::{StackEntryKind, StackObject};
use crate::types::target::{TargetFilter, TargetSpec};
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

/// Why `submit_legend_rule_choice` rejected an answer to a CR 704.5j
/// legend-rule choice.
#[derive(Debug)]
pub enum LegendRuleChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingLegendRuleChoice`.
    NotAwaitingChoice,
    /// `kept` wasn't one of the offered `candidates`.
    InvalidCandidate,
}

/// Why `submit_commander_return_choice` rejected an answer to a CR 903.9a
/// commander-return choice.
#[derive(Debug)]
pub enum CommanderReturnChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingCommanderReturnChoice`.
    NotAwaitingChoice,
}

/// Why `submit_script_choice` rejected an answer to a backlog-item-9
/// `Game.SelectTargets` pause.
#[derive(Debug)]
pub enum ScriptChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingScriptChoice`.
    NotAwaitingChoice,
    /// `chosen` doesn't satisfy the request's own `TargetFilter` (CR 115) —
    /// same legality check `target_is_legal` already performs for a spell's
    /// static target requirement, reused here rather than a second bespoke
    /// check.
    InvalidCandidate,
    /// The pending request doesn't match the answer shape submitted — e.g.
    /// `submit_script_choice` (a `Target` answer) called while the pause is
    /// actually a `ScriptChoiceRequest::MayChoose` (backlog item 9a), or
    /// vice versa via `submit_script_may_choice`.
    WrongRequestKind,
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

/// Why `submit_scry_choice` rejected an answer to a CR 701.22
/// `ProcessState::AwaitingScryChoice` pause.
#[derive(Debug)]
pub enum ScryChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingScryChoice`.
    NotAwaitingChoice,
    /// `kept_on_top`/`put_bottom` together don't contain exactly the pending
    /// `cards` set once each — missing a card, duplicating one, or
    /// including an id that wasn't offered.
    WrongCards,
}

/// Why `submit_surveil_choice` rejected an answer to a CR 701.25
/// `ProcessState::AwaitingSurveilChoice` pause. Same shape as
/// `ScryChoiceError` — kept as its own type rather than shared, same "one
/// error type per pause shape" precedent `ScriptChoiceError` vs
/// `LegendRuleChoiceError` already set.
#[derive(Debug)]
pub enum SurveilChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingSurveilChoice`.
    NotAwaitingChoice,
    /// `kept_on_top`/`put_graveyard` together don't contain exactly the
    /// pending `cards` set once each.
    WrongCards,
}

/// Why `submit_search_choice` rejected an answer to a CR 701.23
/// `ProcessState::AwaitingSearchChoice` pause.
#[derive(Debug)]
pub enum SearchChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingSearchChoice`.
    NotAwaitingChoice,
    /// `chosen.len()` exceeds the pending `count` — CR 701.23b allows
    /// choosing fewer (including zero), never more.
    TooMany,
    /// A chosen id isn't one of the pending `candidates`, or the same id
    /// was chosen more than once.
    NotACandidate,
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

/// Whether `a` and `b` together contain exactly `whole`'s ids, each exactly
/// once (any split, any order) — used by `submit_scry_choice`/
/// `submit_surveil_choice` to validate the two answered piles against the
/// pending `cards` set. Same "manual multiset comparison" shape
/// `is_permutation` above already uses, kept separate since this checks two
/// lists against one reference set rather than one list being a reordering
/// of another.
fn is_exact_partition(a: &[ObjectId], b: &[ObjectId], whole: &[ObjectId]) -> bool {
    if a.len() + b.len() != whole.len() {
        return false;
    }
    let mut remaining: Vec<ObjectId> = whole.to_vec();
    for &id in a.iter().chain(b.iter()) {
        match remaining.iter().position(|&candidate| candidate == id) {
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
    /// CR 704.5j: `player` controls 2+ legendary permanents sharing a name
    /// (`candidates`) and must choose which one to keep — the caller should
    /// prompt and call `submit_legend_rule_choice`, then `step()` again. Same
    /// "not a `PlayerAction`/priority decision, its own entry point" rationale
    /// as `AwaitingReplacementChoice`.
    AwaitingLegendRuleChoice { player: PlayerId, candidates: Vec<ObjectId> },
    /// CR 903.9a: `commander` (owned by `player`) is in a graveyard or exile
    /// and `player` may put it into the command zone — the caller should
    /// prompt and call `submit_commander_return_choice`, then `step()`
    /// again. Same "not a `PlayerAction`/priority decision, its own entry
    /// point" rationale as `AwaitingReplacementChoice`/
    /// `AwaitingLegendRuleChoice`.
    AwaitingCommanderReturnChoice { player: PlayerId, commander: ObjectId },
    /// Backlog item 9: a card script (via `Game.SelectTargets`) yielded
    /// mid-resolution, asking `player` to choose a real target matching
    /// `request` before the operation can finish — the caller should prompt
    /// and call `submit_script_choice`, then `step()` again. Same "not a
    /// `PlayerAction`/priority decision, its own entry point" rationale as
    /// `AwaitingReplacementChoice`/`AwaitingLegendRuleChoice`.
    AwaitingScriptChoice { player: PlayerId, request: ScriptChoiceRequest },
    /// Backlog item 14a, CR 701.22: a `Game.Ops.Scry(n)` primitive is
    /// resolving and needs `player` to decide which of `cards` (the top `n`
    /// of their library, front = top) go on the bottom versus back on top,
    /// and in what order — the caller should prompt and call
    /// `submit_scry_choice`, then `step()` again. Same "not a `PlayerAction`/
    /// priority decision, its own entry point" rationale as
    /// `AwaitingReplacementChoice`/`AwaitingLegendRuleChoice`.
    AwaitingScryChoice { player: PlayerId, cards: Vec<ObjectId> },
    /// Same as `AwaitingScryChoice`, for CR 701.25's `Game.Ops.Surveil(n)` —
    /// the only difference is `submit_surveil_choice` sends the "not kept on
    /// top" pile to the graveyard instead of the bottom of the library.
    AwaitingSurveilChoice { player: PlayerId, cards: Vec<ObjectId> },
    /// Backlog item 14b, CR 701.23: a `Game.Ops.Search(...)` primitive is
    /// resolving and needs `player` to choose up to `count` of `candidates`
    /// (already pre-filtered to ids matching the search's own `FilterSpec`)
    /// — the caller should prompt and call `submit_search_choice`, then
    /// `step()` again. CR 701.23b: choosing fewer than `count` (including
    /// zero) is always legal.
    AwaitingSearchChoice { player: PlayerId, candidates: Vec<ObjectId>, count: u32 },
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
/// Which scripted zone-move op a `ReplacementResumeTarget::ScriptedZoneMove`
/// pause is resuming — each commits its resolved event differently:
/// `Destroy`/`Sacrifice` reuse `sba::commit_death_or_regeneration`/
/// `commit_death` (the same helpers every SBA-driven death already goes
/// through), `Exile` can't reuse either (its destination is the shared
/// Exile zone, `owner: None`, not a per-player zone `commit_death`'s
/// `zone_mut(to, Some(owner))` call assumes) so it's committed inline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScriptedZoneMoveKind {
    Destroy,
    Sacrifice,
    Exile,
}

/// One card still to propose+replacement-check+commit into a graveyard —
/// shared queue entry for `Engine::continue_cost_zone_moves` (a `Sacrifice`/
/// `Discard` `CostPayment`, built by `commands::collect_cost_zone_moves` from
/// an already-validated `PaymentPlan`) and `Engine::continue_discard_moves`
/// (CR 514.1's cleanup hand-size discard, `phase5/replaceable-cleanup-
/// discard`). `from` is the zone the card is claimed to move out of
/// (`Battlefield` for `Sacrifice`, `Hand` for `Discard`/cleanup discard) —
/// the caller already confirmed `id` is really there before queuing it.
#[derive(Clone, Copy, Debug)]
pub struct PendingZoneMove {
    pub id: ObjectId,
    pub from: ZoneKind,
}

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
    /// `is_destroy` is the currently-paused event's own destroy-tag (CR
    /// 614.8's regeneration only intercepts genuine destruction - see
    /// `sba::DyingCandidate`'s own doc comment) - already popped off
    /// `pending_deaths` by the time this pause fires, so it can't be
    /// re-derived from the queue.
    SbaDeath { pending_deaths: VecDeque<sba::DyingCandidate>, is_destroy: bool, then: SbaResumeThen },
    /// Reached from the scripted `Destroy`/`Sacrifice`/`Exile` ops inside
    /// `ResolveStackTop` (`Engine::continue_scripted_zone_move`). Unlike
    /// `ReplacementCheck`, there's no pre-built batch of proposed events to
    /// resume into — each target proposes and inline-commits its own
    /// `ZoneChange` independently (the "already happened" convention every
    /// scripted zone-move op here uses), so `remaining` is just the rest of
    /// this same op's own target list, not yet proposed. `stack_id` is
    /// needed for the final `StackObjectResolved` event once every target's
    /// been processed.
    ScriptedZoneMove { kind: ScriptedZoneMoveKind, remaining: VecDeque<ObjectId>, stack_id: ObjectId },
    /// Reached from `Engine::continue_cost_zone_moves` (`Engine::
    /// submit_payment`'s own `Sacrifice`/`Discard` handling). Same "each
    /// target proposes+commits independently" shape `ScriptedZoneMove`
    /// already has - `remaining` is the rest of this payment's own
    /// `PendingZoneMove` queue. `payer` is needed for the final
    /// `CostPaid` event once every zone-moving payment's been processed
    /// (`stack_id`'s `costs_paid` was already set before this pause could
    /// first fire, so it doesn't need to be threaded through here too).
    CostZoneMove { remaining: VecDeque<PendingZoneMove>, payer: PlayerId, stack_id: ObjectId },
    /// Reached from `Engine::continue_discard_moves` (`Engine::
    /// submit_discard`'s own CR 514.1 cleanup hand-size discard). Same
    /// shape `CostZoneMove` has - `remaining` is the rest of this
    /// discard's own `PendingZoneMove` queue. `since_ledger` is the ledger
    /// length from *before* the first card in this discard committed -
    /// carried through the pause so the eventual `ProcessState::
    /// CollectTriggers { since }` transition (once the queue empties) still
    /// sees this discard's own earlier-committed `ZoneChange`s, not just
    /// whatever comes after the choice.
    CleanupDiscard { remaining: VecDeque<PendingZoneMove>, since_ledger: usize },
    /// Reached from `ProcessState::AdvanceTurnStep`'s own `Step::Draw`
    /// handling (CR 504.1's turn-based draw, `phase5/replaceable-draw`) -
    /// a single proposal, not a queue (`draw_step` is always exactly one
    /// card), so there's nothing left to loop over once this resolves -
    /// resuming just means committing and continuing on to
    /// `Engine::enter_priority_via_replacement_check`. `tail_events` is
    /// this same `step()` call's own `TurnStarted`/`PhaseStarted`/
    /// `StepStarted` markers, still unlogged - carried through so they
    /// still reach the real pipeline (and, downstream, trigger collection)
    /// once the choice is answered. `since_ledger` was captured *before*
    /// the draw's own proposal, so the eventual trigger-collection window
    /// covers both the draw and these markers regardless of which commits
    /// (or logs) first - same reasoning `CleanupDiscard`'s own
    /// `since_ledger` field documents.
    Draw { tail_events: Vec<GameEventKind>, since_ledger: usize },
    /// Reached from `ResolveStackTop`'s `AbilityEffect::DrawCards(n)` arm
    /// (CR 120.3/121, `Game.Ops.DrawCards`). Unlike `Draw` above, this is
    /// a count-based queue inside a resolving spell/ability - each draw
    /// re-peeks the library's current top, and a CR 616.1 pause can fire
    /// on any individual card. `remaining` is the number of cards still to
    /// draw *after* the currently paused one; `events_so_far` carries the
    /// spell's own Stack->Graveyard `ZoneChange` (if any) so it reaches the
    /// normal `ReplacementCheck`/`CommitEvents` pipeline once the draws are
    /// done. The draw events themselves are committed inline (like every
    /// other replacement-checked zone move) so they are not re-checked a
    /// second time against an empty `excluded` set.
    ScriptedDraw {
        remaining: u32,
        player: PlayerId,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    },
}

/// See `ReplacementResumeTarget::SbaDeath`'s own doc comment. `CleanupSbaCheck`
/// carries `since_ledger` (the value `ProcessState::CleanupSbaCheck` captured
/// before this SBA pass began) through the pause, so `resume_sba_death` can
/// still hand it to `finish_cleanup_sba_check`/`PipelineExit::CleanupCheck`
/// once the paused choice is answered.
pub enum SbaResumeThen {
    SbaLoop,
    CleanupSbaCheck { since_ledger: usize },
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
    /// CR 704.5j: paused because `sba::find_legend_rule_group` found `player`
    /// controlling 2+ legendary permanents sharing a name (`candidates`) —
    /// `player` must choose which to keep. `then` says which SBA pass
    /// originally raised this (same role it plays on `SbaDeath` above) so
    /// `submit_legend_rule_choice` knows whether to resume `SbaLoop` or
    /// `CleanupSbaCheck` once the losers are resolved via the same
    /// `resume_sba_death` helper `submit_replacement_choice` already uses.
    AwaitingLegendRuleChoice { player: PlayerId, candidates: Vec<ObjectId>, then: SbaResumeThen },
    /// CR 903.9a: paused because `sba::find_commander_return_candidate`
    /// found `commander` (owned by `player`) sitting in a graveyard or
    /// exile, not yet offered the "may return" choice. `then` plays the
    /// same role it does on `AwaitingLegendRuleChoice` above.
    AwaitingCommanderReturnChoice { player: PlayerId, commander: ObjectId, then: SbaResumeThen },
    /// Backlog item 9: paused because `ScriptRuntime::invoke_operation`
    /// yielded (`OperationOutcome::NeedsChoice`) mid-`ResolveStackTop` —
    /// `thread`/`request` are the suspended coroutine's own handle and its
    /// decoded ask; `stack_id`/`controller`/`events_so_far` are exactly what
    /// `ResolveStackTop` had already built for this same resolution before
    /// hitting the ask (the stack object is already popped off `state.stack`
    /// by this point, same as any other in-progress resolution), carried
    /// here so `submit_script_choice` can pick up in the same spot — push
    /// the final `StackObjectResolved` and continue into `ReplacementCheck`
    /// — rather than re-deriving them.
    AwaitingScriptChoice {
        thread: ScriptHandle,
        request: ScriptChoiceRequest,
        stack_id: ObjectId,
        controller: PlayerId,
        events_so_far: Vec<GameEventKind>,
    },
    /// Backlog item 14a: paused mid-`ResolveStackTop` because the resolving
    /// `StackObject`'s effect was `AbilityEffect::Scry(n)` — `cards` is the
    /// top `n` library ids peeked read-only (front = top; `n` already
    /// clamped to the library's actual size). `stack_id`/`controller`/
    /// `events_so_far` are carried through exactly like
    /// `AwaitingScriptChoice`'s own fields, for the same reason: this
    /// resolution already popped its `StackObject` and built some events
    /// before hitting the pause, so `submit_scry_choice` can resume in the
    /// same spot rather than re-deriving them.
    AwaitingScryChoice {
        stack_id: ObjectId,
        controller: PlayerId,
        events_so_far: Vec<GameEventKind>,
        cards: Vec<ObjectId>,
    },
    /// Same as `AwaitingScryChoice`, for `AbilityEffect::Surveil(n)`.
    AwaitingSurveilChoice {
        stack_id: ObjectId,
        controller: PlayerId,
        events_so_far: Vec<GameEventKind>,
        cards: Vec<ObjectId>,
    },
    /// Backlog item 14b: paused mid-`ResolveStackTop` because the resolving
    /// `StackObject`'s effect was `AbilityEffect::Search { .. }` —
    /// `candidates` is every library id already pre-filtered against the
    /// search's own `FilterSpec` (evaluated with the controller as
    /// `viewer`); `count`/`destination`/`shuffle_after` are copied straight
    /// off the effect so `submit_search_choice` doesn't need the original
    /// `FilterSpec` at all (the pre-filtering already happened). `stack_id`/
    /// `controller`/`events_so_far` carried through for the same reason
    /// `AwaitingScryChoice` carries them.
    AwaitingSearchChoice {
        stack_id: ObjectId,
        controller: PlayerId,
        events_so_far: Vec<GameEventKind>,
        candidates: Vec<ObjectId>,
        count: u32,
        destination: SearchDestination,
        shuffle_after: bool,
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
    /// done — run one SBA pass, then (via `PipelineExit::CleanupCheck`) route
    /// through the real `CollectTriggers`/`PutTriggersApnap` stages so any
    /// CR 603.10 dies-trigger etc raised by that pass actually gets a chance
    /// to be placed, before deciding whether priority opens (something
    /// happened) or the turn advances silently (normal case, nothing did).
    /// Previously bypassed `CollectTriggers` entirely — backlog item 7a's
    /// cleanup half, closed here.
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
    /// CR 104.2a/104.4a: at most one non-eliminated player remains —
    /// `winner` is that player, or `None` if the last remaining players were
    /// eliminated simultaneously (a draw). Terminal: `step()` keeps
    /// returning `StepStatus::GameOver` without mutating anything further,
    /// same "frozen once reached" contract `Error` already has, and
    /// `submit_action` already rejects new actions outside `PriorityWindow`
    /// so nothing more can be submitted either.
    GameOver { winner: Option<PlayerId> },
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
    /// CR 603.10 last-known-information stash: every `Triggered`-kind
    /// ability captured off an object that died during this pipeline run's
    /// `SbaLoop`, *after* `commands::retire_object` already purged it from
    /// `state.abilities` — same "transient process state, not
    /// `GameState`" contract `pipeline_context` establishes. `collect_
    /// triggers`/`PutTriggersApnap`'s ability lookup both fall back to this
    /// list when a live `state.abilities` lookup misses. Populated by
    /// `finish_sba_loop`/`resume_sba_death` as deaths are committed; MUST be
    /// cleared by `end_pipeline_run` (the run's last stage) for the same
    /// reason `pipeline_context` must be — otherwise a later pipeline run
    /// would see this run's stale entries piled up alongside its own.
    departed_triggered_abilities: Vec<RegisteredAbility>,
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

/// Builds a `CardDefinition` for a token from its spec. Tokens have no mana
/// cost, no script, and no alternative costs; their color comes from the
/// color indicator field rather than a mana cost symbol.
///
/// Never called with `TokenSpec::CopyOf` — the `CreateToken` resolution site
/// builds that variant's `CardDefinition` from the copied object's own
/// current copiable values instead (see `TokenSpec::CopyOf`'s own doc
/// comment), before this function would ever be reached.
fn build_token_definition(spec: &TokenSpec) -> CardDefinition {
    match spec {
        TokenSpec::CopyOf(_) => unreachable!(
            "TokenSpec::CopyOf is built from the copied object's snapshot, not build_token_definition"
        ),
        TokenSpec::Named(NamedToken::Clue) => CardDefinition {
            definition_id: 0,
            name: "Clue".to_string(),
            mana_cost: ManaCost { symbols: Vec::new() },
            color_indicator: None,
            supertypes: SupertypeSet::empty(),
            card_types: CardTypeSet::ARTIFACT,
            subtypes: vec![Subtype("Clue".to_string())],
            oracle_text: "{2}, Sacrifice this artifact: Draw a card.".to_string(),
            power: None,
            toughness: None,
            loyalty: None,
            defense: None,
            script_ref: ScriptReference::Inline(String::new()),
            keywords: KeywordSet::empty(),
            target_requirement: None,
            alternative_costs: Vec::new(),
            effect: None,
            modes: Vec::new(),
        },
        TokenSpec::Named(NamedToken::Food) => CardDefinition {
            definition_id: 0,
            name: "Food".to_string(),
            mana_cost: ManaCost { symbols: Vec::new() },
            color_indicator: None,
            supertypes: SupertypeSet::empty(),
            card_types: CardTypeSet::ARTIFACT,
            subtypes: vec![Subtype("Food".to_string())],
            oracle_text: "{2}, Sacrifice this artifact: You gain 3 life.".to_string(),
            power: None,
            toughness: None,
            loyalty: None,
            defense: None,
            script_ref: ScriptReference::Inline(String::new()),
            keywords: KeywordSet::empty(),
            target_requirement: None,
            alternative_costs: Vec::new(),
            effect: None,
            modes: Vec::new(),
        },
        TokenSpec::Creature(spec) => {
            let name = spec.name.clone().unwrap_or_else(|| spec.subtypes.join(" "));
            let color_indicator = if spec.colors.is_empty() { None } else { Some(spec.colors) };
            CardDefinition {
                definition_id: 0,
                name,
                mana_cost: ManaCost { symbols: Vec::new() },
                color_indicator,
                supertypes: spec.additional_supertypes,
                card_types: CardTypeSet::CREATURE | spec.additional_types,
                subtypes: spec.subtypes.iter().map(|s| Subtype(s.clone())).collect(),
                oracle_text: String::new(),
                power: Some(PTValue::Fixed(spec.power)),
                toughness: Some(PTValue::Fixed(spec.toughness)),
                loyalty: None,
                defense: None,
                script_ref: ScriptReference::Inline(String::new()),
                keywords: spec.keywords,
                target_requirement: None,
                alternative_costs: Vec::new(),
                effect: None,
                modes: Vec::new(),
            }
        }
    }
}

/// Pushes any activated abilities that a named token carries into
/// `state.abilities`. Creature tokens have no script-side abilities to
/// register here (keywords live on the definition; richer effects are a
/// future slice). Named tokens (Clue, Food) get their sacrifice ability
/// directly as a Rust-native `RegisteredAbility`.
fn register_token_abilities(
    state: &mut GameState,
    token_id: ObjectId,
    spec: &TokenSpec,
    _controller: PlayerId,
) {
    let artifact_sacrifice_filter = FilterSpec {
        card_types: Some(CardTypeSet::ARTIFACT),
        supertypes: None,
        subtypes: None,
        controller: Some(FilterController::You),
        random: false,
        extra: None,
    };
    match spec {
        TokenSpec::Named(NamedToken::Clue) => {
            state.abilities.push(RegisteredAbility {
                source: token_id,
                kind: AbilityKind::Activated,
                condition: None,
                // TODO: "sacrifice THIS artifact" -- the filter doesn't enforce
                // identity, only type+controller. Same pre-existing gap as every
                // other sacrifice-cost site. Blocked on backlog item 9.
                cost: Some(CostSpec {
                    mana: ManaCost::parse("{2}").ok(),
                    additional: vec![AdditionalCost::Sacrifice { filter: artifact_sacrifice_filter }],
                    alternative: Vec::new(),
                }),
                target: None,
                operation: ScriptHandle(0),
                effect: Some(AbilityEffect::DrawCards(1)),
                intervening_if: None,
            });
        }
        TokenSpec::Named(NamedToken::Food) => {
            state.abilities.push(RegisteredAbility {
                source: token_id,
                kind: AbilityKind::Activated,
                condition: None,
                cost: Some(CostSpec {
                    mana: ManaCost::parse("{2}").ok(),
                    additional: vec![AdditionalCost::Sacrifice { filter: artifact_sacrifice_filter }],
                    alternative: Vec::new(),
                }),
                target: None,
                operation: ScriptHandle(0),
                effect: Some(AbilityEffect::GainLife(3)),
                intervening_if: None,
            });
        }
        TokenSpec::Creature(_) => {}
        // A copy token's abilities come from run_card_script against its
        // copied CardDefinition (see the CreateToken resolution site), not
        // from this hand-built-Rust-native path.
        TokenSpec::CopyOf(_) => {}
    }
}

impl Engine {
    pub fn new(state: GameState) -> Self {
        Engine {
            state,
            process_state: ProcessState::PriorityWindow,
            pipeline_context: None,
            pipeline_exit: PipelineExit::BackToPriority,
            departed_triggered_abilities: Vec::new(),
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
                | PlayerActionKind::DeclareBands { .. }
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
        // pause can't reapply to the same event after it. `chosen` may name
        // a real registered effect or the CR 702.16e protection sentinel
        // (the protected object's own id) - apply_one_replacement_or_
        // protection dispatches either way, same as apply_replacements_
        // from's own single-match branch.
        let rewritten = replacement::apply_one_replacement_or_protection(&mut self.state, chosen, pending_event);
        excluded.push(chosen);
        // is_destroy is per-resume-target: a ReplacementCheck-originated
        // event is never a destroy proposal (see replacement.rs's own doc
        // comment on why); an SbaDeath resume carries its own paused-event
        // tag forward (sba::DyingCandidate's own doc comment).
        let is_destroy = match &resume {
            ReplacementResumeTarget::ReplacementCheck { .. } => false,
            ReplacementResumeTarget::SbaDeath { is_destroy, .. } => *is_destroy,
            ReplacementResumeTarget::ScriptedZoneMove { kind, .. } => *kind == ScriptedZoneMoveKind::Destroy,
            // CR 614.8: a Sacrifice/Discard cost payment is never genuine
            // destruction (Sacrifice/Discard are each their own zone-change
            // verb) - a regeneration shield never intercepts one.
            ReplacementResumeTarget::CostZoneMove { .. } => false,
            // CR 614.8: cleanup discard isn't destruction either.
            ReplacementResumeTarget::CleanupDiscard { .. } => false,
            // CR 614.8: drawing a card isn't destruction either.
            ReplacementResumeTarget::Draw { .. } => false,
            // CR 614.8: a scripted DrawCards(n) ability is also just a draw,
            // not destruction.
            ReplacementResumeTarget::ScriptedDraw { .. } => false,
        };
        let outcome =
            replacement::apply_replacements_from(&mut self.state, &self.script_runtime, rewritten, is_destroy, excluded);

        self.process_state = match resume {
            ReplacementResumeTarget::ReplacementCheck { remaining, mut checked } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    checked.push(event);
                    ProcessState::ReplacementCheck { remaining, checked }
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::ReplacementCheck { remaining, checked },
                    }
                }
            },
            ReplacementResumeTarget::SbaDeath { pending_deaths, then, .. } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    let (committed, departed) = sba::commit_death_or_regeneration(&mut self.state, event);
                    self.resume_sba_death(pending_deaths, vec![committed], departed, then)
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::SbaDeath { pending_deaths, is_destroy, then },
                    }
                }
            },
            ReplacementResumeTarget::ScriptedZoneMove { kind, remaining, stack_id } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    self.commit_scripted_zone_move(kind, event);
                    self.continue_scripted_zone_move(kind, remaining, stack_id)
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::ScriptedZoneMove { kind, remaining, stack_id },
                    }
                }
            },
            ReplacementResumeTarget::CostZoneMove { remaining, payer, stack_id } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                    self.departed_triggered_abilities.extend(departed);
                    self.state.ledger.push(committed);
                    self.continue_cost_zone_moves(remaining, payer, stack_id)
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::CostZoneMove { remaining, payer, stack_id },
                    }
                }
            },
            ReplacementResumeTarget::CleanupDiscard { remaining, since_ledger } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                    self.departed_triggered_abilities.extend(departed);
                    self.state.ledger.push(committed);
                    self.continue_discard_moves(remaining, since_ledger)
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::CleanupDiscard { remaining, since_ledger },
                    }
                }
            },
            ReplacementResumeTarget::Draw { tail_events, since_ledger } => match outcome {
                ReplacementOutcome::Resolved(event) => {
                    let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                    self.departed_triggered_abilities.extend(departed);
                    self.state.ledger.push(committed);
                    self.enter_priority_via_replacement_check(tail_events, since_ledger)
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::Draw { tail_events, since_ledger },
                    }
                }
            },
            ReplacementResumeTarget::ScriptedDraw { remaining, player, stack_id, events_so_far } => {
                match outcome {
                    ReplacementOutcome::Resolved(event) => {
                        let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                        self.departed_triggered_abilities.extend(departed);
                        self.state.ledger.push(committed);
                        self.continue_draw_cards(remaining, player, stack_id, events_so_far)
                    }
                    ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                        ProcessState::AwaitingReplacementChoice {
                            affected_player,
                            pending_event: event,
                            candidates,
                            excluded,
                            resume: ReplacementResumeTarget::ScriptedDraw {
                                remaining,
                                player,
                                stack_id,
                                events_so_far,
                            },
                        }
                    }
                }
            }
        };
        Ok(())
    }

    /// Answers a pending CR 704.5j legend-rule choice raised by
    /// `StepStatus::AwaitingLegendRuleChoice` — `kept` must be one of that
    /// status's `candidates` (the legendary permanent this player keeps).
    /// Deliberately a separate entry point from `submit_action`, same reason
    /// `submit_replacement_choice` is.
    ///
    /// Every other candidate is put into `pending_deaths` and resolved via
    /// the same `resume_sba_death` helper `submit_replacement_choice` already
    /// uses for `ReplacementResumeTarget::SbaDeath` — the losers' actual
    /// destruction is ordinary SBA-driven death (real replacement checking,
    /// CR 400.7 new object id, `retire_object`), not bespoke logic. A
    /// replacement effect on one of the losers (e.g. "if this would die,
    /// exile it instead") correctly re-pauses on `AwaitingReplacementChoice`
    /// via that same helper, rather than being silently skipped.
    pub fn submit_legend_rule_choice(&mut self, kept: ObjectId) -> Result<(), LegendRuleChoiceError> {
        let ProcessState::AwaitingLegendRuleChoice { candidates, .. } = &self.process_state else {
            return Err(LegendRuleChoiceError::NotAwaitingChoice);
        };
        if !candidates.contains(&kept) {
            return Err(LegendRuleChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingLegendRuleChoice { candidates, then, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        // CR 704.5j is "put into its owner's graveyard," not "destroy" -
        // never interceptable by a regeneration shield, same as 704.5f/k
        // (see sba::DyingCandidate's own doc comment).
        let losers: VecDeque<sba::DyingCandidate> = candidates
            .into_iter()
            .filter(|id| *id != kept)
            .map(|id| sba::DyingCandidate { id, is_destroy: false })
            .collect();
        self.process_state = self.resume_sba_death(losers, Vec::new(), Vec::new(), then);
        Ok(())
    }

    /// Answers a pending CR 903.9a commander-return choice raised by
    /// `StepStatus::AwaitingCommanderReturnChoice`. `returned = true`
    /// performs the zone move (`sba::move_commander_to_command_zone`);
    /// `false` declines, leaving the card exactly where it is. Either way,
    /// the commander is marked in `GameState::commander_return_offered` so
    /// this exact arrival isn't re-asked about on a later pass.
    ///
    /// Deliberately a separate entry point from `submit_action`, same
    /// reason `submit_legend_rule_choice` is. Doesn't route through
    /// `resume_sba_death`/`resolve_deaths` like that method does — there's
    /// no "death" here to replacement-check, just a direct zone move (see
    /// `move_commander_to_command_zone`'s own doc comment for why that move
    /// itself skips CR 614-616 replacement checking).
    pub fn submit_commander_return_choice(
        &mut self,
        returned: bool,
    ) -> Result<(), CommanderReturnChoiceError> {
        if !matches!(self.process_state, ProcessState::AwaitingCommanderReturnChoice { .. }) {
            return Err(CommanderReturnChoiceError::NotAwaitingChoice);
        }
        let ProcessState::AwaitingCommanderReturnChoice { commander, then, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        self.state.commander_return_offered.insert(commander);
        let mut events = Vec::new();
        if returned {
            if let Some(event) = sba::move_commander_to_command_zone(&mut self.state, commander) {
                events.push(event);
            }
        }

        self.process_state = match then {
            SbaResumeThen::SbaLoop => self.finish_sba_loop(events, Vec::new()),
            SbaResumeThen::CleanupSbaCheck { since_ledger } => {
                self.finish_cleanup_sba_check(events, Vec::new(), since_ledger)
            }
        };
        Ok(())
    }

    /// Answers a pending backlog-item-9 `Game.SelectTargets` pause raised by
    /// `StepStatus::AwaitingScriptChoice` — `chosen` must satisfy the
    /// request's own `TargetFilter` (checked via the same `target_is_legal`
    /// a spell's static target requirement already uses; that check already
    /// branches correctly on `Target::Player`/`Target::Object` for
    /// `TargetFilter::Player`/`AnyTarget`, so widening `chosen` from
    /// `ObjectId` to `Target` here needed no change to `target_is_legal`
    /// itself). Deliberately a separate entry point from `submit_action`,
    /// same reason `submit_replacement_choice`/`submit_legend_rule_choice`
    /// are.
    ///
    /// Resumes the suspended coroutine (`ScriptRuntime::resume_operation`)
    /// with `chosen`. A script error on resume is silently dropped, same
    /// pre-existing "no path from `ResolveStackTop` back to a `StepStatus`
    /// error for a mid-resolution script failure" stance the original
    /// (non-yielding) `invoke_operation` call already had — not a new gap
    /// this method introduces.
    pub fn submit_script_choice(&mut self, chosen: Target) -> Result<(), ScriptChoiceError> {
        let ProcessState::AwaitingScriptChoice { request, controller, stack_id, .. } = &self.process_state else {
            return Err(ScriptChoiceError::NotAwaitingChoice);
        };
        let ScriptChoiceRequest::SelectTarget { filter } = request else {
            return Err(ScriptChoiceError::WrongRequestKind);
        };
        // CR 702.16c: the resolving StackObject's own id is the "source"
        // protection checks against, not the controller - same distinction
        // validate_targets' own callers already make for a static target
        // requirement.
        // TODO(CR 115.4/"another target"): ScriptChoiceRequest::SelectTarget
        // carries a bare TargetFilter, not a full TargetRequirement, so
        // there's no exclude_source to read here - a Game.SelectTargets
        // mid-resolution choice can't express "another target" yet, only a
        // static card-level target_requirement can (see validate_targets'
        // own call site).
        if !commands::target_is_legal(&self.state, &self.script_runtime, chosen, filter, *controller, *stack_id, false)
        {
            return Err(ScriptChoiceError::InvalidCandidate);
        }
        let ProcessState::AwaitingScriptChoice { thread, stack_id, controller, mut events_so_far, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        let outcome = self
            .script_runtime
            .resume_operation(thread, &mut self.state, controller, chosen)
            .unwrap_or(OperationOutcome::Finished);
        self.process_state = match outcome {
            OperationOutcome::Finished => {
                events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
                ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() }
            }
            OperationOutcome::NeedsChoice { thread, request } => {
                ProcessState::AwaitingScriptChoice { thread, request, stack_id, controller, events_so_far }
            }
        };
        Ok(())
    }

    /// Answers a pending backlog-item-9a `Game.MayChoose` pause (CR 608.2d)
    /// raised by `StepStatus::AwaitingScriptChoice` — sibling of
    /// `submit_script_choice` for the boolean-answer request shape instead
    /// of the target-answer one. No legality check: unlike `SelectTarget`,
    /// any `bool` is a legal answer to a "may" choice. Deliberately a
    /// separate entry point rather than widening `submit_script_choice`'s
    /// own `chosen: Target` parameter — same "one method per pause shape"
    /// precedent `submit_replacement_choice`/`submit_legend_rule_choice`
    /// already set.
    pub fn submit_script_may_choice(&mut self, chosen: bool) -> Result<(), ScriptChoiceError> {
        let ProcessState::AwaitingScriptChoice { request, .. } = &self.process_state else {
            return Err(ScriptChoiceError::NotAwaitingChoice);
        };
        let ScriptChoiceRequest::MayChoose = request else {
            return Err(ScriptChoiceError::WrongRequestKind);
        };
        let ProcessState::AwaitingScriptChoice { thread, stack_id, controller, mut events_so_far, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        let outcome = self
            .script_runtime
            .resume_operation_bool(thread, &mut self.state, controller, chosen)
            .unwrap_or(OperationOutcome::Finished);
        self.process_state = match outcome {
            OperationOutcome::Finished => {
                events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
                ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() }
            }
            OperationOutcome::NeedsChoice { thread, request } => {
                ProcessState::AwaitingScriptChoice { thread, request, stack_id, controller, events_so_far }
            }
        };
        Ok(())
    }

    /// Answers a pending CR 701.22 `Game.Ops.Scry(n)` pause raised by
    /// `StepStatus::AwaitingScryChoice` — `kept_on_top`/`put_bottom`
    /// together must contain exactly the pending `cards` set once each (see
    /// `is_exact_partition`), each in the caller's chosen order.
    /// `kept_on_top[0]` becomes the new top of the library; `put_bottom`'s
    /// order stacks upward from the library's existing bottom (so
    /// `put_bottom`'s last entry ends up the new absolute bottom card).
    /// Deliberately a separate entry point from `submit_action`, same reason
    /// `submit_replacement_choice`/`submit_legend_rule_choice` are.
    ///
    /// Pure reorder within the library — no zone change, no new `ObjectId`s
    /// (same "mutate `Zone.contents` directly" convention `shuffle_library`
    /// already uses), so replacement effects never see this at all (there's
    /// nothing in real Magic that replaces a scry anyway).
    pub fn submit_scry_choice(
        &mut self,
        kept_on_top: Vec<ObjectId>,
        put_bottom: Vec<ObjectId>,
    ) -> Result<(), ScryChoiceError> {
        let ProcessState::AwaitingScryChoice { cards, .. } = &self.process_state else {
            return Err(ScryChoiceError::NotAwaitingChoice);
        };
        if !is_exact_partition(&kept_on_top, &put_bottom, cards) {
            return Err(ScryChoiceError::WrongCards);
        }
        let ProcessState::AwaitingScryChoice { stack_id, controller, mut events_so_far, cards } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        if let Some(lib) = self.state.zone_mut(ZoneKind::Library, Some(controller)) {
            for id in &cards {
                lib.remove(*id);
            }
            for id in kept_on_top.iter().rev() {
                lib.push_top(*id);
            }
            for id in &put_bottom {
                lib.push_bottom(*id);
            }
        }

        events_so_far.push(GameEventKind::CardsScried {
            player: controller,
            kept_on_top,
            put_bottom,
        });
        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        self.process_state =
            ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
        Ok(())
    }

    /// Answers a pending CR 701.25 `Game.Ops.Surveil(n)` pause raised by
    /// `StepStatus::AwaitingSurveilChoice` — same shape as
    /// `submit_scry_choice` except `put_graveyard` is a real library ->
    /// graveyard move (CR 400.7 new `ObjectId` per card), same convention
    /// `submit_discard` already established, rather than a bottom-of-library
    /// reorder. Departed triggered abilities are stashed for consistency
    /// (same "captured but practically dormant, no Hand/Library-departure
    /// LKI condition exists yet" stance `retire_object`'s own doc comment
    /// already takes for `draw_step`'s library-side retirement).
    pub fn submit_surveil_choice(
        &mut self,
        kept_on_top: Vec<ObjectId>,
        put_graveyard: Vec<ObjectId>,
    ) -> Result<(), SurveilChoiceError> {
        let ProcessState::AwaitingSurveilChoice { cards, .. } = &self.process_state else {
            return Err(SurveilChoiceError::NotAwaitingChoice);
        };
        if !is_exact_partition(&kept_on_top, &put_graveyard, cards) {
            return Err(SurveilChoiceError::WrongCards);
        }
        let ProcessState::AwaitingSurveilChoice { stack_id, controller, mut events_so_far, cards } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        // Remove every offered card from the library up front (both
        // piles), then put the kept-on-top pile straight back - a pure
        // reorder, no zone change, no new ObjectIds (same convention
        // submit_scry_choice's own top pile uses).
        if let Some(lib) = self.state.zone_mut(ZoneKind::Library, Some(controller)) {
            for id in &cards {
                lib.remove(*id);
            }
            for id in kept_on_top.iter().rev() {
                lib.push_top(*id);
            }
        }

        let mut new_ids = Vec::with_capacity(put_graveyard.len());
        for card in &put_graveyard {
            let last_known_info =
                self.state.object(*card).expect("validated by is_exact_partition above").clone();
            let owner = last_known_info.owner;

            let new_id = self.state.allocate_object_id();
            let mut new_object = last_known_info.clone();
            new_object.object_id = new_id;
            new_object.lineage = ObjectLineage { previous: Some(*card) };
            new_object.zone = ZoneKind::Graveyard;
            new_object.timestamp = self.state.allocate_sequence_no();

            let departed = commands::retire_object(&mut self.state, *card);
            self.departed_triggered_abilities.extend(departed);
            self.state
                .zone_mut(ZoneKind::Graveyard, Some(owner))
                .expect("graveyard zone must exist")
                .push_bottom(new_id);
            self.state.objects.insert(new_id, new_object);

            events_so_far.push(GameEventKind::ZoneChange {
                object: new_id,
                from: ZoneKind::Library,
                to: ZoneKind::Graveyard,
                last_known_info: Some(last_known_info),
            });
            new_ids.push(new_id);
        }

        events_so_far.push(GameEventKind::CardsSurveiled {
            player: controller,
            kept_on_top,
            put_graveyard: new_ids,
        });
        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        self.process_state =
            ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
        Ok(())
    }

    /// Answers a pending CR 701.23 `Game.Ops.Search(...)` pause raised by
    /// `StepStatus::AwaitingSearchChoice` — `chosen` must be a subset of the
    /// pending `candidates` (already pre-filtered against the search's own
    /// `FilterSpec`, so no re-filtering happens here), no duplicates, and no
    /// more than `count` entries (CR 701.23b: fewer, including zero, is
    /// always legal). Moves each found card Library -> the pause's own
    /// `destination` (CR 400.7 new `ObjectId` per card): `Hand` mirrors
    /// `submit_surveil_choice`'s graveyard-move convention (`retire_object`,
    /// no script); `Battlefield` mirrors `move_resolving_spell`'s own
    /// battlefield-entry convention instead (`remap_abilities`, a real ETB
    /// `run_card_script`, `controlled_since_turn` set to the current turn,
    /// `tapped` applied as requested). Shuffles the library afterward iff
    /// `shuffle_after` (CR 701.24a), via the existing `shuffle_library`
    /// helper — same "already logs its own events" convention
    /// `AbilityEffect::Shuffle` itself already relies on.
    pub fn submit_search_choice(&mut self, chosen: Vec<ObjectId>) -> Result<(), SearchChoiceError> {
        let ProcessState::AwaitingSearchChoice { candidates, count, .. } = &self.process_state else {
            return Err(SearchChoiceError::NotAwaitingChoice);
        };
        if chosen.len() > *count as usize {
            return Err(SearchChoiceError::TooMany);
        }
        let mut seen: Vec<ObjectId> = Vec::with_capacity(chosen.len());
        for &id in &chosen {
            if seen.contains(&id) || !candidates.contains(&id) {
                return Err(SearchChoiceError::NotACandidate);
            }
            seen.push(id);
        }
        let ProcessState::AwaitingSearchChoice {
            stack_id, controller, mut events_so_far, destination, shuffle_after, ..
        } = std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        for card in &chosen {
            let last_known_info =
                self.state.object(*card).expect("validated against candidates above").clone();
            let owner = last_known_info.owner;
            let new_id = self.state.allocate_object_id();
            let mut new_object = last_known_info.clone();
            new_object.object_id = new_id;
            new_object.lineage = ObjectLineage { previous: Some(*card) };
            new_object.timestamp = self.state.allocate_sequence_no();

            match destination {
                SearchDestination::Hand => {
                    new_object.zone = ZoneKind::Hand;
                    self.state
                        .zone_mut(ZoneKind::Library, Some(owner))
                        .expect("library zone must exist")
                        .remove(*card);
                    let departed = commands::retire_object(&mut self.state, *card);
                    self.departed_triggered_abilities.extend(departed);
                    self.state
                        .zone_mut(ZoneKind::Hand, Some(owner))
                        .expect("hand zone must exist")
                        .push_bottom(new_id);
                    self.state.objects.insert(new_id, new_object);
                    events_so_far.push(GameEventKind::ZoneChange {
                        object: new_id,
                        from: ZoneKind::Library,
                        to: ZoneKind::Hand,
                        last_known_info: Some(last_known_info),
                    });
                }
                SearchDestination::Battlefield { tapped } => {
                    new_object.zone = ZoneKind::Battlefield;
                    new_object.tapped = tapped;
                    new_object.controlled_since_turn = self.state.turn_number;
                    self.state
                        .zone_mut(ZoneKind::Library, Some(owner))
                        .expect("library zone must exist")
                        .remove(*card);
                    let definition_for_script = new_object.definition.clone();
                    self.state.objects.remove(card);
                    self.state
                        .zone_mut(ZoneKind::Battlefield, None)
                        .expect("battlefield zone must exist")
                        .push_bottom(new_id);
                    self.state.objects.insert(new_id, new_object);
                    commands::remap_abilities(&mut self.state, *card, new_id);
                    let _ = self.script_runtime.run_card_script(&mut self.state, &definition_for_script, new_id);
                    events_so_far.push(GameEventKind::ZoneChange {
                        object: new_id,
                        from: ZoneKind::Library,
                        to: ZoneKind::Battlefield,
                        last_known_info: Some(last_known_info),
                    });
                    // CR 603.6d/614.1c: same post-script apply step
                    // `move_resolving_spell`'s caller and the PlayLand
                    // battlefield_entries loop both take - `tapped` above is
                    // this search's own explicit destination flag (CR
                    // 701.23), unrelated to and independent of a card's own
                    // "enters tapped" ability, so both can stack (either
                    // already tapping it is harmless - EntersTapped is
                    // idempotent).
                    let counter_events = replacement::apply_enters_battlefield_modifiers(
                        &mut self.state,
                        &self.script_runtime,
                        new_id,
                        ZoneKind::Library,
                    );
                    events_so_far.extend(counter_events);
                }
            }
        }

        if shuffle_after {
            self.shuffle_library(controller);
        }

        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        self.process_state =
            ProcessState::ReplacementCheck { remaining: events_so_far.into(), checked: Vec::new() };
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
            plan.validate(cost, &self.state, &self.script_runtime, payer, source)
        };
        if !valid {
            return Err(PaymentError::InvalidPlan);
        }
        let ProcessState::AwaitingPayment { stack_id, .. } =
            std::mem::replace(&mut self.process_state, ProcessState::PriorityWindow)
        else {
            unreachable!("checked above");
        };

        // CR 118.3a/118.13a/602.2f-h: commit exactly what the plan declared.
        // Mana/tap/life are immediate (never replaceable in this engine) -
        // see commands::commit_payment's doc comment for why this trusts
        // validate's check rather than re-deriving anything. Sacrifice/
        // Discard are collected separately, *not* committed yet - CR
        // 601.2h/616.1 makes each one its own replaceable zone-change
        // proposal (see commands::propose_zone_move's doc comment),
        // driven below by continue_cost_zone_moves.
        let pending_zone_moves: VecDeque<PendingZoneMove> = commands::collect_cost_zone_moves(&plan.payments)
            .into_iter()
            .map(|(id, from)| PendingZoneMove { id, from })
            .collect();
        let immediate_events = commands::commit_payment(&mut self.state, payer, &plan.payments);
        if let Some(stack_object) = self.state.stack.iter_mut().find(|s| s.stack_id == stack_id) {
            stack_object.costs_paid = plan.payments;
        }
        for kind in immediate_events {
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
        // before the pause happened. continue_cost_zone_moves logs CostPaid
        // itself once every zone-moving payment (if any) has committed, and
        // returns SbaLoop the same way this used to unconditionally.
        self.process_state = self.continue_cost_zone_moves(pending_zone_moves, payer, stack_id);
        Ok(())
    }

    /// Drains `remaining` `Sacrifice`/`Discard` cost-payment targets one at a
    /// time - CR 616.1's ordering-choice pause can be raised by any of them,
    /// not just the first, since each independently proposes+replacement-
    /// checks+commits its own `ZoneChange` (`commands::propose_zone_move`/
    /// `commit_zone_move`). Same shape `continue_scripted_zone_move`
    /// already established for the scripted Destroy/Sacrifice/Exile ops -
    /// `departed_triggered_abilities`/`state.ledger` are updated inline as
    /// each target commits, rather than accumulated and flushed at the end
    /// (CR 603.10: a departed "when this dies" ability must be capturable
    /// before `CollectTriggers` ever runs, same reasoning that function's own
    /// doc comment already gives).
    ///
    /// Once the queue empties (including the trivial case of no zone-moving
    /// payments at all), logs the final `CostPaid` event and returns
    /// `ProcessState::SbaLoop` - exactly what `submit_payment` used to do
    /// unconditionally before this split existed.
    fn continue_cost_zone_moves(
        &mut self,
        mut remaining: VecDeque<PendingZoneMove>,
        payer: PlayerId,
        stack_id: ObjectId,
    ) -> ProcessState {
        while let Some(pending) = remaining.pop_front() {
            let Some(proposed) = commands::propose_zone_move(&self.state, pending.id, pending.from) else {
                // Already gone somehow (shouldn't happen - PaymentPlan::
                // validate confirmed it) - skip rather than guess, same
                // stance continue_scripted_zone_move's own analogous check
                // takes.
                continue;
            };
            // CR 614.8: neither Sacrifice nor Discard is genuine destruction
            // - a regeneration shield never intercepts either.
            match replacement::apply_replacements(&mut self.state, &self.script_runtime, proposed, false) {
                ReplacementOutcome::Resolved(event) => {
                    let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                    self.departed_triggered_abilities.extend(departed);
                    self.state.ledger.push(committed);
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    return ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::CostZoneMove { remaining, payer, stack_id },
                    };
                }
            }
        }
        let event_id = self.state.allocate_event_id();
        let sequence_no = self.state.allocate_sequence_no();
        self.state.ledger.push(GameEvent {
            event_id,
            sequence_no,
            kind: GameEventKind::CostPaid { stack_id, payer },
            caused_by_command: None,
            parent_event: None,
        });
        ProcessState::SbaLoop
    }

    /// Answers a pending CR 601.2f-h payment pause by abandoning the cast/
    /// activation entirely (CR 601.6: "the game returns to the moment before
    /// the casting of that spell was proposed") — the caller's explicit
    /// declaration that they cannot or will not pay, distinct from
    /// resubmitting a different `PaymentPlan` via `submit_payment` (which
    /// stays valid and keeps the pause open on `PaymentError::InvalidPlan`).
    /// Without this, a cost with no valid plan at all leaves the engine
    /// stuck in `AwaitingPayment` forever — nothing else returns the card to
    /// hand or un-pushes the stack object.
    ///
    /// Not a true CR 733 state rewind (this engine's ledger is append-only,
    /// no undo primitive exists anywhere) — reuses `move_resolving_spell`
    /// (the same Stack-departure helper CR 608.2b's fizzle path already
    /// uses) to return a `Spell`'s card to hand under a fresh CR 400.7 id;
    /// an `ActivatedAbility`'s stack entry is just removed, since no card
    /// left any zone to activate it — mirrors `Fizzled`'s own "a fizzled
    /// ability just stops existing, no `ZoneChange`" stance exactly.
    /// `TriggeredAbility`/`Copy` never reach `AwaitingPayment` (the former
    /// never carries a cost, the latter isn't modeled) — not handled
    /// specially, same as `ActivatedAbility`. Either way,
    /// `GameEventKind::CastingUnwound` marks the ledger so a replay viewer
    /// can see this happened even when no `ZoneChange` accompanies it.
    pub fn unwind_payment(&mut self) -> Result<(), PaymentError> {
        let ProcessState::AwaitingPayment { stack_id, .. } = &self.process_state else {
            return Err(PaymentError::NotAwaitingPayment);
        };
        let stack_id = *stack_id;

        let Some(idx) = self.state.stack.iter().position(|s| s.stack_id == stack_id) else {
            // Defensive: shouldn't happen - nothing else removes a
            // StackObject while AwaitingPayment is pending for it.
            self.process_state = self.end_pipeline_run();
            return Ok(());
        };
        let stack_object = self.state.stack.remove(idx);

        let mut events = Vec::new();
        if matches!(stack_object.kind, StackEntryKind::Spell) {
            if let Some(event) = self.move_resolving_spell(stack_id, ZoneKind::Hand, false) {
                events.push(event);
            }
        }
        events.push(GameEventKind::CastingUnwound { stack_id });

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

        self.process_state = self.end_pipeline_run();
        Ok(())
    }

    /// Answers a pending CR 514.1 discard-to-hand-size choice raised by
    /// `StepStatus::AwaitingDiscard` — `cards` must be exactly the right
    /// number of cards from the active player's hand. Each card is now a
    /// real replaceable zone-change proposal (CR 616.1's "if a card would be
    /// discarded, exile it instead" pattern - e.g. Leyline of the Void) -
    /// `continue_discard_moves` drives the queue, same propose/replacement-
    /// check/commit split `Engine::continue_cost_zone_moves` established for
    /// Sacrifice/Discard cost payments (`phase5/replaceable-cleanup-
    /// discard`). Once every card's committed, `ProcessState::
    /// CollectTriggers` still runs (`PipelineExit::CleanupSbaCheck`
    /// unconditionally continues on to `ProcessState::CleanupSbaCheck` once
    /// any `DiscardsCard`/`EachDiscardsCard` trigger has had a chance to be
    /// placed) so the next `step()` decides whether CR 514.3's priority
    /// exception applies - unchanged from before this split.
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

        let since_ledger = self.state.ledger.len();
        let pending: VecDeque<PendingZoneMove> =
            cards.into_iter().map(|id| PendingZoneMove { id, from: ZoneKind::Hand }).collect();
        self.process_state = self.continue_discard_moves(pending, since_ledger);
        Ok(())
    }

    /// Drains `remaining` cleanup-discard targets one at a time - same
    /// propose/replacement-check/commit loop shape `continue_cost_zone_moves`
    /// already established for Sacrifice/Discard cost payments, reused here
    /// for `Engine::submit_discard`'s own CR 514.1 targets. `since_ledger`
    /// (captured before the first card in this discard committed) is what
    /// the eventual `ProcessState::CollectTriggers { since }` transition
    /// needs once the queue empties - carried through any CR 616.1 pause via
    /// `ReplacementResumeTarget::CleanupDiscard` so it isn't lost.
    fn continue_discard_moves(&mut self, mut remaining: VecDeque<PendingZoneMove>, since_ledger: usize) -> ProcessState {
        while let Some(pending) = remaining.pop_front() {
            let Some(proposed) = commands::propose_zone_move(&self.state, pending.id, pending.from) else {
                continue;
            };
            // CR 614.8: a cleanup discard is never genuine destruction - a
            // regeneration shield never intercepts one.
            match replacement::apply_replacements(&mut self.state, &self.script_runtime, proposed, false) {
                ReplacementOutcome::Resolved(event) => {
                    let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                    self.departed_triggered_abilities.extend(departed);
                    self.state.ledger.push(committed);
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    return ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::CleanupDiscard { remaining, since_ledger },
                    };
                }
            }
        }
        // Route through CollectTriggers/PutTriggersApnap instead of ending
        // the pipeline here - this discard's own ZoneChange events (and any
        // DiscardsCard/EachDiscardsCard trigger they raise) need real
        // trigger collection, same as every other real zone move.
        // PipelineExit::CleanupSbaCheck (unconditional, unlike
        // PipelineExit::CleanupCheck) is what still gets to the SBA check
        // afterward regardless of whether anything triggered, matching
        // begin_cleanup_step's own discard-then-SBA-check sequencing.
        self.pipeline_exit = PipelineExit::CleanupSbaCheck;
        ProcessState::CollectTriggers { since: since_ledger }
    }

    /// The shared tail every `ProcessState::AdvanceTurnStep` step reaches
    /// once its own turn-based action (if any) is done and it's ready to
    /// open a priority window (CR 603.3/117.3b: replacement check -> SBA ->
    /// trigger collection first, same "settle everything before priority"
    /// contract `PendingAction`/`ResolveStackTop` already follow). Factored
    /// out (`phase5/replaceable-draw`) so `Step::Draw`'s own handling can
    /// reach it both on the normal path and after a CR 616.1 pause resumes
    /// (`ReplacementResumeTarget::Draw`) - `since_ledger` is a parameter
    /// rather than captured fresh here specifically so `Step::Draw` can
    /// supply one taken *before* its own draw committed (see that call
    /// site's own comment for why the ordering matters).
    fn enter_priority_via_replacement_check(&mut self, events: Vec<GameEventKind>, since_ledger: usize) -> ProcessState {
        self.state.priority_player = self.state.active_player;
        self.pipeline_context = Some(PipelineContext { since_ledger, caused_by_command: None, parent_event: None });
        self.pipeline_exit = PipelineExit::BackToPriority;
        ProcessState::ReplacementCheck { remaining: events.into(), checked: Vec::new() }
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
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
                commander_casts: std::collections::HashMap::new(),
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
                    copiable_override: None,
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
                    is_token: false,
                    is_commander: false,
                    visibility: VisibilityPolicy { public: false, visible_to: Vec::new() },
                };
                lib_zone.contents.push_back(obj_id);
                objects.insert(obj_id, obj);
            }
            zones.push(lib_zone);
            zones.push(Zone::new(ZoneKind::Hand, Some(pid)));
            zones.push(Zone::new(ZoneKind::Graveyard, Some(pid)));

            // CR 903.6: commander(s) go to the command zone, not the
            // library - minted the same way library cards just were above,
            // just with is_commander: true and a different destination.
            let mut command_zone = Zone::new(ZoneKind::Command, Some(pid));
            for card_def in &player_cfg.commanders {
                let obj_id = ObjectId(next_object_id);
                next_object_id += 1;
                let ts = SequenceNo(next_sequence_no);
                next_sequence_no += 1;
                let obj = GameObject {
                    object_id: obj_id,
                    lineage: ObjectLineage { previous: None },
                    definition: card_def.clone(),
                    copiable_override: None,
                    owner: pid,
                    controller: pid,
                    zone: ZoneKind::Command,
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
                    is_token: false,
                    is_commander: true,
                    // CR 408: the command zone's contents are public, same
                    // default as the battlefield/graveyard/exile.
                    visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
                };
                command_zone.contents.push_back(obj_id);
                objects.insert(obj_id, obj);
            }
            zones.push(command_zone);
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
            cost_modifiers: Vec::new(),
            hand_size_modifiers: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState { seed: config.rng_seed, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no,
            next_object_id,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
        };

        Engine {
            state,
            process_state: ProcessState::PriorityWindow,
            pipeline_context: None,
            pipeline_exit: PipelineExit::BackToPriority,
            departed_triggered_abilities: Vec::new(),
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
        self.departed_triggered_abilities.clear();
        match std::mem::replace(&mut self.pipeline_exit, PipelineExit::BackToPriority) {
            PipelineExit::BackToPriority => ProcessState::PriorityWindow,
            PipelineExit::ResolveStackTop => ProcessState::ResolveStackTop,
            PipelineExit::AdvanceTurnStep => ProcessState::AdvanceTurnStep,
            PipelineExit::CleanupCheck { since_ledger } => {
                // Ledger growth catches an SBA event or a trigger placed
                // during *this* pass; `!stack.is_empty()` catches a trigger
                // placed earlier in the same cleanup sequence (e.g.
                // Engine::submit_discard's own DiscardsCard trigger,
                // reached via PipelineExit::CleanupSbaCheck just before
                // this SBA pass ran) that this pass's own since_ledger
                // marker can't see, since it was captured after that
                // trigger already landed. Either one means CR 514.3 says
                // open a priority window; only both being false means
                // nothing at all is waiting and the turn advances silently.
                if self.state.ledger.len() > since_ledger || !self.state.stack.is_empty() {
                    self.state.priority_player = self.state.active_player;
                    ProcessState::PriorityWindow
                } else {
                    ProcessState::AdvanceTurnStep
                }
            }
            PipelineExit::CleanupSbaCheck => ProcessState::CleanupSbaCheck,
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

                let mut outcome =
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
                //
                // CR 603.6d/614.1c: `battlefield_entries` is only ever
                // populated by `PlayLand` (a hand->battlefield move) as of
                // this writing - once the script above has (possibly)
                // self-registered an EntersTapped/EntersWithCounters effect
                // targeting this same object, apply it (and any externally
                // granted one already sitting in state.replacement_effects)
                // right here, same call `move_resolving_spell`'s own
                // ResolveStackTop caller makes.
                for object_id in &outcome.battlefield_entries {
                    if let Some(definition) =
                        self.state.object(*object_id).map(|o| o.definition.clone())
                    {
                        let _ =
                            self.script_runtime.run_card_script(&mut self.state, &definition, *object_id);
                    }
                    let counter_events = replacement::apply_enters_battlefield_modifiers(
                        &mut self.state,
                        &self.script_runtime,
                        *object_id,
                        ZoneKind::Hand,
                    );
                    outcome.events.extend(counter_events);
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
                    // CR 800.4b: an eliminated player is never asked for
                    // priority, so "everyone passed" only needs a pass from
                    // each player still in the game.
                    self.pipeline_exit = if self.state.consecutive_passes as usize
                        >= commands::non_eliminated_players(&self.state).len()
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
                // target against current game state. `legal_targets` is the
                // subset of `top.targets` that's still legal right now -
                // used both for the fizzle decision below (every target
                // illegal -> `legal_targets` empty -> full fizzle, CR
                // 608.2b's first sentence) and, when it's not empty, by the
                // effect-resolution arms further down to skip only the
                // individually-illegal targets rather than guessing which
                // one the original single-target-only code path meant (CR
                // 608.2b's second sentence - "still resolves, but it won't
                // affect those targets that are illegal, if any").
                let legal_targets: Vec<Target> = match &top.target_requirement {
                    Some(TargetSpec::Primitive(req)) => top
                        .targets
                        .iter()
                        .copied()
                        .filter(|t| {
                            commands::target_is_legal(
                                &self.state,
                                &self.script_runtime,
                                *t,
                                &req.filter,
                                top.controller,
                                // top.source, not top.stack_id - they only
                                // coincide for a Spell (see StackObject's
                                // own construction sites); for an
                                // ActivatedAbility/TriggeredAbility,
                                // top.stack_id is a bare, never-a-real-
                                // GameObject id, and passing it here as
                                // "source" silently broke both protection-
                                // from-source-quality checks (target_has_
                                // protection_from's state.object(source)
                                // lookup would just miss and return false)
                                // and this slice's own exclude_source below
                                // - every other resolution site already uses
                                // top.source for "the real thing this
                                // effect comes from" (e.g. the Damage
                                // AbilityEffect arm further down).
                                top.source,
                                req.exclude_source,
                            )
                        })
                        .collect(),
                    // `None` (no targets required) or a `Script`-backed
                    // requirement (not evaluable this slice, see
                    // `TargetSpec`'s own doc comment) - every submitted
                    // target passes through unfiltered, same as before this
                    // slice.
                    _ => top.targets.clone(),
                };
                let fizzles = match &top.target_requirement {
                    // Gated on whether any target was actually *chosen*
                    // (`top.targets`), not on the requirement's own `count`
                    // schema field - those coincided before CR 601.2c/115.6
                    // "up to N" existed (an exact-count requirement always
                    // had `targets.len() == count`), but "up to N" can
                    // legally choose zero targets, which must never fizzle
                    // (same as a `None`/count-0 requirement) even though
                    // `req.count` itself is still nonzero.
                    Some(TargetSpec::Primitive(_)) if !top.targets.is_empty() => legal_targets.is_empty(),
                    _ => false,
                };

                // CR 603.4/608.2a: an intervening-if clause is rechecked
                // fresh against current state at resolution too - if it's
                // now false, the ability is removed from the stack and does
                // nothing, same observable "no effect, Fizzled logged"
                // outcome as the target-legality fizzle above (reused
                // rather than a second bespoke branch). `None` (every
                // `Spell`/`ActivatedAbility`, and any `TriggeredAbility`
                // without "if" text) never fizzles this way.
                let intervening_if_fizzles = match &top.intervening_if {
                    Some(InterveningIfSpec::Primitive(p)) => !p.holds(&self.state, top.controller),
                    // A script error falls back to the same conservative
                    // "doesn't hold" default as before, but is now logged as
                    // a real ScriptError event instead of silently
                    // disappearing (`phase5/script-error-surfacing`).
                    Some(InterveningIfSpec::Script(handle)) => {
                        let holds = match self.script_runtime.invoke_intervening_if(
                            *handle,
                            &self.state,
                            top.source,
                            top.controller,
                        ) {
                            Ok(holds) => holds,
                            Err(e) => {
                                self.log_events(vec![GameEventKind::ScriptError {
                                    message: format!(
                                        "intervening-if script error resolving source {:?}: {e:?}",
                                        top.source
                                    ),
                                }]);
                                false
                            }
                        };
                        !holds
                    }
                    None => false,
                };
                let fizzles = fizzles || intervening_if_fizzles;

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
                    // bridge like every other script-shaped gap. `top.effect`'s
                    // Rust-native fast path (below) covers a growing set of
                    // recognized primitives without a Lua call at all.

                    // CR 608.2m: a permanent type goes to the battlefield as a
                    // permanent; an instant or sorcery (the only other types
                    // castable as a spell - see CastSpell's land rejection in
                    // commands.rs) goes to its owner's graveyard/exile and its
                    // own effect resolves (see below) - a permanent has no
                    // effect of its own to resolve here; any ETB behavior is a
                    // separate Triggered-kind RegisteredAbility, not
                    // `CardDefinition.effect`. Computed once, up front, before
                    // the zone move below removes `top.stack_id` from
                    // `state.objects` (CR 400.7 mints a new id for the
                    // graveyard/exile-side object).
                    let is_instant_or_sorcery = matches!(top.kind, StackEntryKind::Spell)
                        && self.state.object(top.stack_id).is_some_and(|obj| {
                            obj.definition.card_types.intersects(CardTypeSet::INSTANT | CardTypeSet::SORCERY)
                        });

                    if matches!(top.kind, StackEntryKind::Spell) {
                        // Peek the card's types before moving it -
                        // move_resolving_spell needs the destination decided
                        // up front.
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
                                // CR 303.4b: an Aura entering the battlefield
                                // this way attaches to its (already
                                // legality-checked, CR 608.2b, above)
                                // sole target instead of entering bare -
                                // the only permanent-entry path this engine
                                // builds attach for; CR 303.4f/h's other
                                // entry shapes need an effect that puts a
                                // permanent onto the battlefield already
                                // attached, which doesn't exist here yet.
                                let new_battlefield_aura = match &event {
                                    GameEventKind::ZoneChange { object, to: ZoneKind::Battlefield, .. }
                                        if self
                                            .state
                                            .object(*object)
                                            .is_some_and(|o| commands::is_aura(&o.definition)) =>
                                    {
                                        Some(*object)
                                    }
                                    _ => None,
                                };
                                // CR 603.6d/614.1c: `move_resolving_spell`
                                // already ran this object's own self-
                                // registration script (run_script above),
                                // so any self-registered EntersTapped/
                                // EntersWithCounters effect is already in
                                // state.replacement_effects by now - apply
                                // it (and any externally granted one)
                                // directly, same as the PlayLand and
                                // search-to-battlefield call sites below.
                                if let GameEventKind::ZoneChange { object, to: ZoneKind::Battlefield, .. } = &event {
                                    let counter_events = replacement::apply_enters_battlefield_modifiers(
                                        &mut self.state,
                                        &self.script_runtime,
                                        *object,
                                        ZoneKind::Stack,
                                    );
                                    events.push(event);
                                    events.extend(counter_events);
                                } else {
                                    events.push(event);
                                }
                                if let Some(aura_id) = new_battlefield_aura {
                                    if let Some(Target::Object(target_id)) = legal_targets.first().copied() {
                                        commands::attach_permanent(&mut self.state, aura_id, target_id);
                                        events.push(GameEventKind::Attached {
                                            object: aura_id,
                                            attached_to: target_id,
                                        });
                                    }
                                }
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
                    // ZoneChange. Activated/triggered abilities and an instant/
                    // sorcery spell all resolve their effect the same way:
                    // `top.effect`'s Rust-native fast path when it's a
                    // recognized primitive (no Lua call at all), falling back
                    // to the real script bridge (CR 602.2b/603.4/608.2m's
                    // resolution proper) only when it's None. A permanent
                    // spell is deliberately excluded (`is_instant_or_sorcery`
                    // above) - see this block's own doc comment.
                    if matches!(
                        top.kind,
                        StackEntryKind::TriggeredAbility | StackEntryKind::ActivatedAbility
                    ) || is_instant_or_sorcery
                    {
                        match &top.effect {
                            Some(AbilityEffect::GainLife(amount)) => {
                                if let Some(player) = self.state.player_mut(top.controller) {
                                    player.gain_life(*amount);
                                }
                            }
                            // CR 608.2m/614-616: unlike GainLife above, Damage
                            // doesn't mutate GameState directly - it emits a
                            // real DamageDealt event and lets it flow through
                            // the same ReplacementCheck->CommitEvents pipeline
                            // combat damage already uses (commands::
                            // apply_damage_event is the sole consumer of that
                            // event kind), so prevention/replacement effects
                            // and Lifelink/Wither/Infect/Deathtouch apply for
                            // free. `is_combat_damage: false` - CR 903.10a's
                            // commander-damage tracking only counts CR 510
                            // combat damage, never a spell/ability's own
                            // damage effect, even from a commander source.
                            // CR 601.2d: every *legal* target takes the full
                            // `amount` independently - "divide as you
                            // choose" (a player-chosen split among targets)
                            // isn't modeled, still a real, named gap, not
                            // silently guessed at. CR 608.2b: a target that's
                            // individually illegal by now is skipped (not a
                            // fizzle of the whole spell/ability - that
                            // decision already happened above), which is
                            // exactly what `legal_targets` already is. Empty
                            // targets is a silent no-op (shouldn't be
                            // reachable given cast-time target validation)
                            // rather than a panic.
                            Some(AbilityEffect::Damage(amount)) => {
                                for target in &legal_targets {
                                    events.push(GameEventKind::DamageDealt {
                                        source: top.source,
                                        target: *target,
                                        amount: *amount,
                                        is_combat_damage: false,
                                    });
                                }
                            }
                            // CR 701.17/614.13: sacrifice every legal target
                            // independently — identical to the Destroy arm
                            // below except indestructibility is never checked
                            // (CR 702.12b is Destroy-specific; sacrifice
                            // bypasses it) and CR 701.20/614.8: sacrifice is
                            // never destruction, so a regeneration shield
                            // can't (and per the real rules, shouldn't)
                            // intercept it. Dies/LeavesBattlefield triggers
                            // fire the same way as SBA-driven death. A
                            // non-Object legal target (a `Target::Player`, if
                            // this were ever paired with a filter that
                            // allowed one) is skipped, same "silent no-op"
                            // stance as before. CR 616.1: 2+ replacement
                            // effects matching one target's proposed
                            // `ZoneChange` pauses via
                            // `continue_scripted_zone_move`/
                            // `AwaitingReplacementChoice`, same as every other
                            // SBA-driven death - the remaining independent
                            // targets in this same op resume afterward.
                            // TODO: "sacrifice a permanent you control"
                            // (player chooses at resolution) needs coroutines
                            // — blocked on backlog item 9.
                            Some(AbilityEffect::Sacrifice) => {
                                let targets: VecDeque<ObjectId> = legal_targets
                                    .iter()
                                    .filter_map(|t| match t {
                                        Target::Object(id) => Some(*id),
                                        _ => None,
                                    })
                                    .collect();
                                self.process_state = self.continue_scripted_zone_move(
                                    ScriptedZoneMoveKind::Sacrifice,
                                    targets,
                                    top.stack_id,
                                );
                                return StepStatus::Progressed;
                            }
                            // CR 701.8/614.13/702.12b: unlike Damage, this
                            // can't be modeled as "push an unapplied event
                            // and let CommitEvents retroactively apply it" -
                            // every other ZoneChange in this codebase already
                            // describes a move that happened. Every legal
                            // target, independently - a Target::Player
                            // (Destroy only ever affects a permanent) is
                            // skipped, shouldn't be reachable given cast-time
                            // target validation against a real card's filter.
                            // CR 614.8: Destroy is genuine destruction, so a
                            // regeneration shield can intercept it (CR
                            // 701.19c: only if nothing already said this
                            // Destroy "can't be regenerated" - that qualifier
                            // isn't modeled, see the regeneration slice's own
                            // UNRESOLVED notes). CR 616.1: 2+ replacement
                            // effects matching one target's proposed
                            // `ZoneChange` pauses via
                            // `continue_scripted_zone_move`/
                            // `AwaitingReplacementChoice`, same as every other
                            // SBA-driven death - the remaining independent
                            // targets in this same op resume afterward.
                            Some(AbilityEffect::Destroy) => {
                                let targets: VecDeque<ObjectId> = legal_targets
                                    .iter()
                                    .filter_map(|t| match t {
                                        Target::Object(id) => Some(*id),
                                        _ => None,
                                    })
                                    .collect();
                                self.process_state = self.continue_scripted_zone_move(
                                    ScriptedZoneMoveKind::Destroy,
                                    targets,
                                    top.stack_id,
                                );
                                return StepStatus::Progressed;
                            }
                            // CR 701.18: exile the first chosen target from
                            // the battlefield. Unlike Destroy, indestructibility
                            // is irrelevant - exile bypasses it entirely (CR
                            // 702.12b is Destroy-specific). Like Destroy, a
                            // ZoneChange always describes an already-happened
                            // move. Every legal target, independently - a
                            // Target::Player is skipped, same "silent no-op"
                            // stance as before. Battlefield only - TODO:
                            // exiling from graveyard/hand requires knowing
                            // which zone to remove from at resolution time;
                            // deferred as a named follow-up. CR 616.1: same
                            // ordering-choice handling as Destroy/Sacrifice
                            // above.
                            Some(AbilityEffect::Exile) => {
                                let targets: VecDeque<ObjectId> = legal_targets
                                    .iter()
                                    .filter_map(|t| match t {
                                        Target::Object(id) => Some(*id),
                                        _ => None,
                                    })
                                    .collect();
                                self.process_state = self.continue_scripted_zone_move(
                                    ScriptedZoneMoveKind::Exile,
                                    targets,
                                    top.stack_id,
                                );
                                return StepStatus::Progressed;
                            }
                            // CR 122.1a: same "push an unapplied event, let
                            // CommitEvents retroactively apply it" pattern as
                            // Damage above - GameEventKind::CounterAdded
                            // already existed with this exact shape. Every
                            // legal target gets the full `amount`
                            // independently, same CR 601.2d "no division
                            // modeled" stance as Damage. Object targets only
                            // - a Target::Player is skipped, same stance
                            // every other arm here takes.
                            Some(AbilityEffect::Counters { kind, amount }) => {
                                for target in &legal_targets {
                                    let Target::Object(id) = target else { continue };
                                    events.push(GameEventKind::CounterAdded {
                                        object: *id,
                                        kind: kind.clone(),
                                        amount: *amount,
                                    });
                                }
                            }
                            // CR 611.2a's own "duration stated by the spell
                            // or ability creating it": every legal target
                            // independently gets its own ContinuousEffect
                            // pushed straight into state.continuous_effects
                            // (mutates immediately - no event kind exists
                            // for "a continuous effect was added", same as
                            // static-ability registration). `source` is set
                            // to the *target's* own id with `self_only:
                            // true`, reusing EffectFilter's existing
                            // "self_only pins to ContinuousEffect.source"
                            // mechanism to target a spell's chosen object
                            // instead of an aura's attachment (see this
                            // variant's own doc comment in ability.rs).
                            // Object targets only, same "silent no-op"
                            // stance as Damage/Counters above.
                            Some(AbilityEffect::Pump { power, toughness, duration }) => {
                                let timestamp = self.state.allocate_sequence_no();
                                for target in &legal_targets {
                                    let Target::Object(id) = target else { continue };
                                    self.state.continuous_effects.push(ContinuousEffect {
                                        source: *id,
                                        layer: Layer::PowerToughnessModifying,
                                        affected: AffectedFilter::Primitive(EffectFilter {
                                            controller: None,
                                            card_types: None,
                                            self_only: true,
                                        }),
                                        effect: EffectPayload::Primitive(EffectPrimitive::ModifyPT {
                                            power: *power,
                                            toughness: *toughness,
                                        }),
                                        timestamp,
                                        duration: duration.clone(),
                                        condition: None,
                                    });
                                }
                            }
                            // CR 108.4/613.1 layer 2: same per-target
                            // ContinuousEffect construction as Pump above,
                            // just layer 2/GainControl instead of layer 7c/
                            // ModifyPT. The new controller is always
                            // top.controller (the resolving spell/ability's
                            // own controller) - see AbilityEffect::
                            // GainControl's own doc comment for why no
                            // separate player argument exists. Object
                            // targets only, same "silent no-op" stance as
                            // Pump/Damage/Counters above.
                            Some(AbilityEffect::GainControl(duration)) => {
                                let timestamp = self.state.allocate_sequence_no();
                                let new_controller = top.controller;
                                for target in &legal_targets {
                                    let Target::Object(id) = target else { continue };
                                    self.state.continuous_effects.push(ContinuousEffect {
                                        source: *id,
                                        layer: Layer::ControlChangingEffects,
                                        affected: AffectedFilter::Primitive(EffectFilter {
                                            controller: None,
                                            card_types: None,
                                            self_only: true,
                                        }),
                                        effect: EffectPayload::Primitive(EffectPrimitive::GainControl(new_controller)),
                                        timestamp,
                                        duration: duration.clone(),
                                        condition: None,
                                    });
                                }
                            }
                            // CR 701.19a: every legal target independently
                            // gets its own one-shot regeneration shield,
                            // pinned via self_only: true exactly like Pump
                            // above pins its own ContinuousEffect. CR
                            // 701.19c: a target that currently can't be
                            // regenerated gets no shield at all - checked
                            // here, at creation time, not as a property of
                            // the ReplacementEffect itself (see this
                            // variant's own doc comment in ability.rs).
                            // Object targets only, same "silent no-op"
                            // stance as Pump/Damage/Counters above.
                            Some(AbilityEffect::Regenerate) => {
                                for target in &legal_targets {
                                    let Target::Object(id) = target else { continue };
                                    if commands::effective_keywords(&self.state, &self.script_runtime, *id)
                                        .contains(KeywordSet::CANT_BE_REGENERATED)
                                    {
                                        continue;
                                    }
                                    self.state.replacement_effects.push(ReplacementEffect {
                                        source: *id,
                                        spec: ReplacementSpec::Primitive {
                                            filter: EventFilter::WouldDie {
                                                affected: EffectFilter {
                                                    controller: None,
                                                    card_types: None,
                                                    self_only: true,
                                                },
                                            },
                                            payload: ReplacementPayload::Regenerate,
                                        },
                                        consumption: ConsumptionPolicy::OneShot,
                                    });
                                }
                            }
                            // CR 120.3/121/616.1: no target, always
                            // top.controller (same "controller only" shape
                            // GainLife uses). Routed through the real
                            // propose/apply_replacements/commit pipeline so
                            // "would draw a card, instead..." effects intercept
                            // each individual draw. N draws is a count-based
                            // queue (`ReplacementResumeTarget::ScriptedDraw`)
                            // because each draw re-peeks the library's current
                            // top. Stops rather than looping past an empty
                            // library (CR 704.5b's empty-library-loss SBA
                            // isn't modeled - TODO, same gap draw_step
                            // itself already carries).
                            Some(AbilityEffect::DrawCards(amount)) => {
                                self.process_state = self.continue_draw_cards(
                                    *amount,
                                    top.controller,
                                    top.stack_id,
                                    events,
                                );
                                return StepStatus::Progressed;
                            }
                            // CR 701.19b: no target, always top.controller
                            // (same "controller only" shape DrawCards/
                            // GainLife use). shuffle_library already mutates
                            // state and logs its own LibraryShuffled/RngDrawn
                            // events - no events pushed here, same "already
                            // happened" convention DrawCards established.
                            Some(AbilityEffect::Shuffle) => {
                                self.shuffle_library(top.controller);
                            }
                            // CR 701.22a: peek the top n library ids
                            // read-only (no mutation - the actual reorder
                            // needs the controller's per-card decision) and
                            // pause the whole pipeline run right here,
                            // exactly like the NeedsChoice branch below does
                            // for Game.SelectTargets. CR 701.22b: n == 0 (or
                            // an empty library) falls straight through -
                            // no pause, no event.
                            Some(AbilityEffect::Scry(n)) => {
                                let cards: Vec<ObjectId> = self
                                    .state
                                    .zone(ZoneKind::Library, Some(top.controller))
                                    .map_or(Vec::new(), |z| {
                                        z.contents.iter().take(*n as usize).copied().collect()
                                    });
                                if !cards.is_empty() {
                                    self.process_state = ProcessState::AwaitingScryChoice {
                                        stack_id: top.stack_id,
                                        controller: top.controller,
                                        events_so_far: events,
                                        cards,
                                    };
                                    return StepStatus::Progressed;
                                }
                            }
                            // CR 701.25a: same pause shape as Scry above -
                            // submit_surveil_choice is what actually differs
                            // (graveyard destination instead of bottom).
                            Some(AbilityEffect::Surveil(n)) => {
                                let cards: Vec<ObjectId> = self
                                    .state
                                    .zone(ZoneKind::Library, Some(top.controller))
                                    .map_or(Vec::new(), |z| {
                                        z.contents.iter().take(*n as usize).copied().collect()
                                    });
                                if !cards.is_empty() {
                                    self.process_state = ProcessState::AwaitingSurveilChoice {
                                        stack_id: top.stack_id,
                                        controller: top.controller,
                                        events_so_far: events,
                                        cards,
                                    };
                                    return StepStatus::Progressed;
                                }
                            }
                            // CR 701.23a: search the controller's *entire*
                            // library (not a fixed top-N peek like
                            // Scry/Surveil) and pre-filter to just the ids
                            // matching `filter` - submit_search_choice only
                            // has to check the answer's a subset of this
                            // list, never re-running the filter itself.
                            // Empty candidates: nothing to choose, falls
                            // straight through with no pause and no event -
                            // same "no real decision" stance Scry/Surveil's
                            // own n == 0 case takes.
                            Some(AbilityEffect::Search { filter, count, destination, shuffle_after }) => {
                                let candidates: Vec<ObjectId> = self
                                    .state
                                    .zone(ZoneKind::Library, Some(top.controller))
                                    .map(|z| z.contents.iter().copied().collect::<Vec<_>>())
                                    .unwrap_or_default()
                                    .into_iter()
                                    .filter(|id| {
                                        // A library card's `.controller`
                                        // always equals its owner in this
                                        // engine (no GainControl effect
                                        // targets a non-battlefield card,
                                        // CR 108.4) - the raw field is
                                        // exactly correct here.
                                        self.state
                                            .object(*id)
                                            .is_some_and(|obj| filter.matches(obj, obj.controller, top.controller))
                                    })
                                    .collect();
                                if !candidates.is_empty() {
                                    self.process_state = ProcessState::AwaitingSearchChoice {
                                        stack_id: top.stack_id,
                                        controller: top.controller,
                                        events_so_far: events,
                                        candidates,
                                        count: *count,
                                        destination: *destination,
                                        shuffle_after: *shuffle_after,
                                    };
                                    return StepStatus::Progressed;
                                }
                            }
                            // CR 707.2/707.3/613.1a (layer 1): top.source
                            // becomes a copy of its first chosen target's
                            // current copiable values - a real mutation
                            // written once (GameObject.copiable_override),
                            // not a live-queried ContinuousEffect (see that
                            // field's own doc comment for why layer 1 can't
                            // fit the other layers' shape). CR 707.4: this
                            // never triggers ETB/LTB (top.source stays on
                            // the battlefield, same ObjectId) - but CR
                            // 707.2a's abilities are copiable, so whatever
                            // top.source had self-registered under its
                            // pre-copy identity is purged first and
                            // run_card_script re-runs against the new
                            // snapshot right after.
                            Some(AbilityEffect::BecomeCopy) => {
                                if let Some(Target::Object(target_id)) = top.targets.first().copied() {
                                    let snapshot = self.state.object(target_id).map(|obj| {
                                        obj.copiable_override.clone().unwrap_or_else(|| obj.definition.clone())
                                    });
                                    if let Some(snapshot) = snapshot {
                                        let source_id = top.source;
                                        let became_copy =
                                            self.state.objects.get_mut(&source_id).is_some_and(|source_obj| {
                                                if source_obj.zone == ZoneKind::Battlefield {
                                                    source_obj.copiable_override = Some(snapshot.clone());
                                                    true
                                                } else {
                                                    false
                                                }
                                            });
                                        if became_copy {
                                            commands::purge_granted_abilities(&mut self.state, source_id);
                                            // TODO: a script error here is silently
                                            // dropped, same gap every other
                                            // run_card_script call site in this file
                                            // already flags.
                                            let _ = self.script_runtime.run_card_script(
                                                &mut self.state,
                                                &snapshot,
                                                source_id,
                                            );
                                            events.push(GameEventKind::BecameCopy {
                                                object: source_id,
                                                copied: target_id,
                                            });
                                        }
                                    }
                                }
                                // else: no target, or a Target::Player
                                // (BecomeCopy only ever targets a permanent)
                                // - silent no-op, same stance every other
                                // arm here takes for an unreachable target
                                // shape.
                            }
                            // CR 111: create one or more token permanents on
                            // the battlefield under the controller's control.
                            // Named tokens (Clue, Food) get their activated
                            // abilities registered here. Tokens have empty
                            // scripts so run_card_script is deliberately
                            // skipped - except `TokenSpec::CopyOf` (CR
                            // 707.10f), whose "definition" is a real
                            // snapshot of another object's copiable values
                            // (see `TokenSpec::CopyOf`'s own doc comment),
                            // abilities and all, so its self-registration
                            // does need to run. TODO: CR 111.7 cessation
                            // (tokens leaving the battlefield should cease
                            // to exist) is not enforced — blocked on a
                            // zone-watch SBA hook (future slice).
                            Some(AbilityEffect::CreateToken { spec, count }) => {
                                let controller = top.controller;
                                let copy_snapshot = match spec {
                                    TokenSpec::CopyOf(source_id) => self.state.object(*source_id).map(|obj| {
                                        obj.copiable_override.clone().unwrap_or_else(|| obj.definition.clone())
                                    }),
                                    _ => None,
                                };
                                for _ in 0..*count {
                                    let token_def = match (&copy_snapshot, spec) {
                                        (Some(def), _) => def.clone(),
                                        (None, TokenSpec::CopyOf(_)) => {
                                            // Source no longer resolves - skip rather
                                            // than guess, same stance every other
                                            // unresolvable reference in this engine
                                            // already takes.
                                            continue;
                                        }
                                        (None, _) => build_token_definition(spec),
                                    };
                                    let token_id = self.state.allocate_object_id();
                                    let ts = self.state.allocate_sequence_no();
                                    let token_obj = GameObject {
                                        object_id: token_id,
                                        lineage: ObjectLineage { previous: None },
                                        definition: token_def.clone(),
                                        copiable_override: None,
                                        owner: controller,
                                        controller,
                                        zone: ZoneKind::Battlefield,
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
                                        controlled_since_turn: self.state.turn_number,
                                        is_token: true,
                                        is_commander: false,
                                        visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
                                    };
                                    if let Some(zone) = self.state.zone_mut(ZoneKind::Battlefield, None) {
                                        zone.push_bottom(token_id);
                                        self.state.objects.insert(token_id, token_obj);
                                    }
                                    register_token_abilities(&mut self.state, token_id, spec, controller);
                                    if matches!(spec, TokenSpec::CopyOf(_)) {
                                        // TODO: a script error here is silently dropped,
                                        // same gap every other run_card_script call site
                                        // in this file already flags.
                                        let _ = self.script_runtime.run_card_script(&mut self.state, &token_def, token_id);
                                    }
                                    events.push(GameEventKind::ZoneChange {
                                        object: token_id,
                                        from: ZoneKind::Stack,
                                        to: ZoneKind::Battlefield,
                                        last_known_info: None,
                                    });
                                }
                            }
                            // CR 702.6a: Equip's own resolution - attaches
                            // top.source (the Equipment) to its first (only,
                            // per CR 702.6a's singular "target creature")
                            // already legality-checked (CR 608.2b, above)
                            // target. Same commands::attach_permanent +
                            // GameEventKind::Attached convention the Aura
                            // ETB-attach path (Spell branch, above) already
                            // established - attach_permanent's own
                            // reattach-safety fix (phase5/equip-ability)
                            // means retargeting an already-equipped
                            // Equipment cleans up the old target's
                            // attachments list for free. Object targets
                            // only, same "silent no-op" stance every other
                            // arm here takes for an unreachable target shape.
                            Some(AbilityEffect::Attach) => {
                                if let Some(Target::Object(target_id)) = legal_targets.first().copied() {
                                    commands::attach_permanent(&mut self.state, top.source, target_id);
                                    events.push(GameEventKind::Attached {
                                        object: top.source,
                                        attached_to: target_id,
                                    });
                                }
                            }
                            // AddMana/AddManaChoice are only ever set on a mana
                            // ability, which resolves immediately and never
                            // reaches the stack (CR 605.1a/605.3b) - no
                            // StackObject of any kind should ever carry either
                            // variant. Defensive no-op rather than a panic.
                            Some(AbilityEffect::AddMana(_)) | Some(AbilityEffect::AddManaChoice { .. }) => {}
                            None => {
                                // TODO: a script error here is silently
                                // dropped - no path from ResolveStackTop
                                // back to a StepStatus error exists for a
                                // mid-resolution script failure yet.
                                match self.script_runtime.invoke_operation(
                                    top.operation,
                                    &mut self.state,
                                    top.source,
                                    top.controller,
                                ) {
                                    Ok(OperationOutcome::Finished) | Err(_) => {}
                                    // Backlog item 9: the script called
                                    // Game.SelectTargets and yielded mid-
                                    // resolution - pause the whole pipeline
                                    // run right here rather than falling
                                    // through to StackObjectResolved/
                                    // ReplacementCheck below. events (this
                                    // resolution's own progress so far, e.g.
                                    // a permanent's ETB ZoneChange) moves
                                    // into events_so_far untouched -
                                    // submit_script_choice pushes the real
                                    // StackObjectResolved and continues into
                                    // ReplacementCheck once answered.
                                    Ok(OperationOutcome::NeedsChoice { thread, request }) => {
                                        self.process_state = ProcessState::AwaitingScriptChoice {
                                            thread,
                                            request,
                                            stack_id: top.stack_id,
                                            controller: top.controller,
                                            events_so_far: events,
                                        };
                                        return StepStatus::Progressed;
                                    }
                                }
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
                // Never a destroy proposal - every event reaching this
                // generic command-pipeline check (damage, sacrifice/discard
                // cost payments, ...) is unrelated to CR 704.5g/614.8; the
                // real destroy producers (SBA lethal damage, the scripted
                // Destroy op) both run their own inline replacement check
                // instead of this one, same as before this slice.
                match replacement::apply_replacements(&mut self.state, &self.script_runtime, event, false) {
                    ReplacementOutcome::Resolved(event) => {
                        checked.push(event);
                        self.process_state = ProcessState::ReplacementCheck { remaining, checked };
                    }
                    ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
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

            ProcessState::AwaitingLegendRuleChoice { player, candidates, then } => {
                // Nothing to do until submit_legend_rule_choice answers -
                // same "put the exact same state back" pattern
                // AwaitingReplacementChoice above already uses.
                let status = StepStatus::AwaitingLegendRuleChoice { player, candidates: candidates.clone() };
                self.process_state = ProcessState::AwaitingLegendRuleChoice { player, candidates, then };
                status
            }

            ProcessState::AwaitingCommanderReturnChoice { player, commander, then } => {
                // Nothing to do until submit_commander_return_choice
                // answers - same "put the exact same state back" pattern.
                let status = StepStatus::AwaitingCommanderReturnChoice { player, commander };
                self.process_state = ProcessState::AwaitingCommanderReturnChoice { player, commander, then };
                status
            }

            ProcessState::AwaitingScriptChoice { thread, request, stack_id, controller, events_so_far } => {
                // Nothing to do until submit_script_choice answers - same
                // "put the exact same state back" pattern every other
                // choice-pause above already uses.
                let status = StepStatus::AwaitingScriptChoice { player: controller, request: request.clone() };
                self.process_state =
                    ProcessState::AwaitingScriptChoice { thread, request, stack_id, controller, events_so_far };
                status
            }

            ProcessState::AwaitingScryChoice { stack_id, controller, events_so_far, cards } => {
                // Nothing to do until submit_scry_choice answers - same
                // "put the exact same state back" pattern every other
                // choice-pause above already uses.
                let status = StepStatus::AwaitingScryChoice { player: controller, cards: cards.clone() };
                self.process_state =
                    ProcessState::AwaitingScryChoice { stack_id, controller, events_so_far, cards };
                status
            }

            ProcessState::AwaitingSurveilChoice { stack_id, controller, events_so_far, cards } => {
                let status = StepStatus::AwaitingSurveilChoice { player: controller, cards: cards.clone() };
                self.process_state =
                    ProcessState::AwaitingSurveilChoice { stack_id, controller, events_so_far, cards };
                status
            }

            ProcessState::AwaitingSearchChoice { stack_id, controller, events_so_far, candidates, count, destination, shuffle_after } => {
                let status =
                    StepStatus::AwaitingSearchChoice { player: controller, candidates: candidates.clone(), count };
                self.process_state = ProcessState::AwaitingSearchChoice {
                    stack_id,
                    controller,
                    events_so_far,
                    candidates,
                    count,
                    destination,
                    shuffle_after,
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
                    if let GameEventKind::DamageDealt { source, target, amount, is_combat_damage } = &kind {
                        commands::apply_damage_event(
                            &mut self.state,
                            &self.script_runtime,
                            *source,
                            *target,
                            *amount,
                            *is_combat_damage,
                        );
                    }
                    // CR 122.1a: same "describes an effect that hasn't
                    // happened to state yet" shape as DamageDealt above -
                    // apply it now that it's in final, post-replacement form.
                    // No replacement effect actually targets CounterAdded
                    // yet (no EventFilter::CounterAdded), so this never
                    // changes from what ResolveStackTop proposed - still
                    // applied here rather than at push time, matching
                    // Damage's convention rather than GainLife's shortcut.
                    if let GameEventKind::CounterAdded { object, kind, amount } = &kind {
                        if let Some(obj) = self.state.object_mut(*object) {
                            obj.counters.add(kind.clone(), *amount);
                        }
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
                    SbaOutcome::Progressed(events, departed) => self.finish_sba_loop(events, departed),
                    SbaOutcome::NeedsOrderingChoice {
                        affected_player,
                        pending_event,
                        candidates,
                        excluded,
                        is_destroy,
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
                            resume: ReplacementResumeTarget::SbaDeath {
                                pending_deaths,
                                is_destroy,
                                then: SbaResumeThen::SbaLoop,
                            },
                        }
                    }
                    SbaOutcome::NeedsLegendRuleChoice { player, candidates, resolved_so_far } => {
                        // Unaffected by the pause below, same reasoning as
                        // NeedsOrderingChoice's own resolved_so_far above.
                        self.state.ledger.extend(resolved_so_far);
                        ProcessState::AwaitingLegendRuleChoice { player, candidates, then: SbaResumeThen::SbaLoop }
                    }
                    SbaOutcome::NeedsCommanderReturnChoice { player, commander, resolved_so_far } => {
                        self.state.ledger.extend(resolved_so_far);
                        ProcessState::AwaitingCommanderReturnChoice {
                            player,
                            commander,
                            then: SbaResumeThen::SbaLoop,
                        }
                    }
                };
                StepStatus::Progressed
            }

            ProcessState::CollectTriggers { since } => {
                let mut pending = Vec::new();
                let mut script_errors = Vec::new();
                for event in &self.state.ledger[since..] {
                    let (matches, errors) = triggers::collect_triggers(
                        &self.state,
                        event,
                        &self.script_runtime,
                        &self.departed_triggered_abilities,
                    );
                    // CR 603.2h: record a once-per-turn match now, before
                    // the next event in this same batch is checked -
                    // collect_triggers's own cap check reads this same set,
                    // so a second qualifying event later in this batch (not
                    // just a later turn) is correctly excluded too.
                    for trigger in &matches {
                        if trigger.once_per_turn {
                            self.state.once_per_turn_triggered.insert((trigger.source, trigger.ability_index));
                        }
                    }
                    pending.extend(matches);
                    script_errors.extend(errors);
                }
                // Logged once the loop above (which holds an immutable
                // borrow of self.state.ledger) has fully released it -
                // log_events needs &mut self (`phase5/script-error-
                // surfacing`).
                if !script_errors.is_empty() {
                    self.log_events(
                        script_errors.into_iter().map(|message| GameEventKind::ScriptError { message }).collect(),
                    );
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
                        // CR 603.10: a Dies/LeavesBattlefield trigger's
                        // source never resolves live in state.abilities
                        // (retire_object already purged it) - fall back to
                        // the same departed_triggered_abilities stash
                        // collect_triggers itself consulted to produce this
                        // PendingTrigger in the first place. Same "nth
                        // ability for this source, in that list's own
                        // order" convention triggers::collect_triggers's
                        // departed-abilities loop counts by.
                        let ability = commands::find_ability(&self.state, trigger.source, trigger.ability_index)
                            .or_else(|| {
                                self.departed_triggered_abilities
                                    .iter()
                                    .filter(|a| a.source == trigger.source)
                                    .nth(trigger.ability_index as usize)
                            });
                        let Some(ability) = ability else {
                            // Neither state.abilities nor the departed stash
                            // resolved this trigger - TODO(CR 603.3d):
                            // sacrifice/discard-as-cost/other zone
                            // departures aren't wired to the departed stash
                            // yet (see retire_object's own doc comment), so
                            // a dies-trigger from one of those still lands
                            // here and gets silently dropped.
                            continue;
                        };
                        let operation = ability.operation;
                        let effect = ability.effect.clone();
                        let target_requirement = ability.target.clone();
                        let intervening_if = ability.intervening_if.clone();
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
                        // CR 800.4a: the source's own current owner when
                        // it's still live; `PendingTrigger` has no
                        // last-known-owner of its own for the departed-
                        // source case (a Dies/LeavesBattlefield trigger),
                        // so that narrow case falls back to `controller` -
                        // same proxy this field replaces everywhere else,
                        // now scoped to just this one gap.
                        let source_owner =
                            self.state.object(trigger.source).map(|o| o.owner).unwrap_or(trigger.controller);
                        self.state.stack.push(StackObject::new(
                            stack_id,
                            StackEntryKind::TriggeredAbility,
                            trigger.source,
                            source_owner,
                            trigger.controller,
                            Vec::new(),
                            Vec::new(),
                            target_requirement,
                            None,
                            false,
                            operation,
                            effect,
                            intervening_if,
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
                let mut events = self.advance_turn_step();

                if self.state.step == Step::Untap {
                    // CR 502.4: no priority window during untap - perform
                    // its turn-based action (CR 502.3) and loop straight
                    // back into AdvanceTurnStep instead of opening
                    // PriorityWindow. The next call into this same arm runs
                    // advance_turn_step() again (Untap -> Upkeep), which
                    // does open priority normally below.
                    //
                    // Still routed through the real pipeline (ReplacementCheck
                    // -> CommitEvents -> SbaLoop -> CollectTriggers ->
                    // PutTriggersApnap), same as every other step - CR
                    // 603.3's last sentence means an untap-step trigger
                    // (BeginningOfYourUntap/EachUntap - rare on real cards,
                    // but real) just waits to go on the stack until the next
                    // time a player would receive priority, rather than
                    // never triggering at all. Placing it onto state.stack
                    // right here (instead of literally deferring the "put on
                    // stack" action until Upkeep's priority window) is
                    // functionally equivalent once that window opens - the
                    // trigger is already sitting there either way.
                    // PipelineExit::AdvanceTurnStep (not BackToPriority) is
                    // what keeps this from opening a priority window Untap
                    // isn't allowed to grant.
                    let untap_events = self.untap_step();
                    events.extend(untap_events);
                    let since_ledger = self.state.ledger.len();
                    self.pipeline_context =
                        Some(PipelineContext { since_ledger, caused_by_command: None, parent_event: None });
                    self.pipeline_exit = PipelineExit::AdvanceTurnStep;
                    self.process_state =
                        ProcessState::ReplacementCheck { remaining: events.into(), checked: Vec::new() };
                    return StepStatus::Progressed;
                }
                if self.state.step == Step::Draw {
                    // CR 500.7/500.8: consume any queued skip before doing
                    // anything — a skipped step has no turn-based actions and
                    // no priority window.
                    let active = self.state.active_player;
                    if self.state.consume_step_skip(active, Step::Draw) {
                        // CR 500.8: a skipped step doesn't happen at all -
                        // still log its own StepStarted marker (unchanged
                        // pre-existing behavior), but no turn-based action,
                        // no priority window, and no trigger collection
                        // either.
                        self.log_events(events);
                        self.process_state = ProcessState::AdvanceTurnStep;
                        return StepStatus::Progressed;
                    }
                    // CR 504.1/616.1: drawing is a real replaceable zone-
                    // change proposal now (phase5/replaceable-draw) - same
                    // propose/apply_replacements/commit split every other
                    // zone-move producer in this engine uses. since_ledger
                    // is captured *before* the draw's own event is logged
                    // (directly, via commit_zone_move + a plain
                    // state.ledger.push - bypassing the generic
                    // ReplacementCheck pipeline below entirely, so the
                    // already-resolved event never gets re-checked a second
                    // time against a fresh, empty `excluded` set) so the
                    // eventual trigger-collection window still covers both
                    // the draw and this call's own TurnStarted/PhaseStarted/
                    // StepStarted markers (`events`, still unlogged at this
                    // point) - same property `DrawsCard`/`EachDrawsCard`
                    // already depend on.
                    let since_ledger = self.state.ledger.len();
                    if let Some(proposed) = commands::propose_draw(&mut self.state, active) {
                        match replacement::apply_replacements(&mut self.state, &self.script_runtime, proposed, false) {
                            ReplacementOutcome::Resolved(event) => {
                                let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                                self.departed_triggered_abilities.extend(departed);
                                self.state.ledger.push(committed);
                            }
                            ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                                self.process_state = ProcessState::AwaitingReplacementChoice {
                                    affected_player,
                                    pending_event: event,
                                    candidates,
                                    excluded,
                                    resume: ReplacementResumeTarget::Draw { tail_events: events, since_ledger },
                                };
                                return StepStatus::Progressed;
                            }
                        }
                    }
                    self.process_state = self.enter_priority_via_replacement_check(events, since_ledger);
                    return StepStatus::Progressed;
                }
                if self.state.step == Step::Cleanup {
                    // CR 514.3: normally no priority during cleanup - run
                    // cleanup's turn-based actions (514.1 discard, 514.2
                    // effect expiry) then check SBAs before deciding
                    // whether to open a priority window or advance the turn
                    // silently. begin_cleanup_step handles all of this via
                    // its own CleanupSbaCheck flow (which still doesn't run
                    // collect_triggers either - a second, differently-shaped
                    // gap, left for its own slice per docs/implementation.md's
                    // backlog item 7a).
                    self.log_events(events);
                    return self.begin_cleanup_step();
                }

                // CR 603.3/117.3b: route through the real pipeline
                // (replacement check -> SBA -> trigger collection) before
                // opening the priority window that follows every step/phase
                // transition except Untap/Draw/Cleanup above - same "settle
                // everything before priority" contract PendingAction/
                // ResolveStackTop already follow. This is the fix for the
                // gap ConditionPrimitive::BeginningOfYourUpkeep's own doc
                // comment (ability.rs) names: AdvanceTurnStep used to push
                // these events via log_events, which never ran
                // collect_triggers at all.
                let since_ledger = self.state.ledger.len();
                self.process_state = self.enter_priority_via_replacement_check(events, since_ledger);
                StepStatus::Progressed
            }

            ProcessState::AwaitingDiscard { player, excess } => {
                // Nothing to do until submit_discard answers - put the
                // exact same state back and tell the caller who needs to be
                // prompted, mirroring AwaitingPayment's pattern. CR 402.2:
                // recomputed the same way begin_cleanup_step derived it -
                // this arm only re-displays it, `excess` (fixed when this
                // pause first started) is what submit_discard actually
                // validates against.
                let hand_size = self
                    .state
                    .zone(ZoneKind::Hand, Some(player))
                    .map(|z| z.contents.len())
                    .unwrap_or(0);
                let max_hand_size = effective_max_hand_size(&self.state.hand_size_modifiers, player, 7)
                    .unwrap_or(u32::MAX) as usize;
                let status = StepStatus::AwaitingDiscard { player, hand_size, max_hand_size };
                self.process_state = ProcessState::AwaitingDiscard { player, excess };
                status
            }

            ProcessState::CleanupSbaCheck => {
                let since_ledger = self.state.ledger.len();
                self.process_state = match sba::check_state_based_actions(&mut self.state, &self.script_runtime) {
                    SbaOutcome::Progressed(events, departed) => {
                        self.finish_cleanup_sba_check(events, departed, since_ledger)
                    }
                    SbaOutcome::NeedsOrderingChoice {
                        affected_player,
                        pending_event,
                        candidates,
                        excluded,
                        is_destroy,
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
                                is_destroy,
                                then: SbaResumeThen::CleanupSbaCheck { since_ledger },
                            },
                        }
                    }
                    SbaOutcome::NeedsLegendRuleChoice { player, candidates, resolved_so_far } => {
                        self.state.ledger.extend(resolved_so_far);
                        ProcessState::AwaitingLegendRuleChoice {
                            player,
                            candidates,
                            then: SbaResumeThen::CleanupSbaCheck { since_ledger },
                        }
                    }
                    SbaOutcome::NeedsCommanderReturnChoice { player, commander, resolved_so_far } => {
                        self.state.ledger.extend(resolved_so_far);
                        ProcessState::AwaitingCommanderReturnChoice {
                            player,
                            commander,
                            then: SbaResumeThen::CleanupSbaCheck { since_ledger },
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
            ProcessState::GameOver { winner } => {
                self.process_state = ProcessState::GameOver { winner };
                StepStatus::GameOver { winner }
            }
        }
    }

    /// `SbaLoop`'s own "what happens once this pass's events are known"
    /// transition — pulled out so a resumed CR 616.1 pause
    /// (`resume_sba_death`) and a fresh pass (`SbaLoop`'s own arm) agree on
    /// it instead of duplicating the branch.
    fn finish_sba_loop(&mut self, events: Vec<GameEvent>, departed: Vec<RegisteredAbility>) -> ProcessState {
        // Accumulate across every SbaLoop pass in this pipeline run (CR
        // 704.3's fixed point may take several passes, each killing more
        // creatures) - cleared by end_pipeline_run once CollectTriggers/
        // PutTriggersApnap have both had a chance to consult it.
        self.departed_triggered_abilities.extend(departed);
        let next = if events.is_empty() {
            let since = self.pipeline_context.as_ref().map(|ctx| ctx.since_ledger).unwrap_or(0);
            ProcessState::CollectTriggers { since }
        } else {
            self.state.ledger.extend(events);
            // Re-enter SbaLoop next step() - CR 704.3's fixed point, not a
            // single pass.
            ProcessState::SbaLoop
        };
        self.game_over_check(next)
    }

    /// CR 104.2a/104.4a: overrides `next` with `GameOver` if at most one
    /// player remains in a game that started with more than one — checked
    /// after every SBA pass commits its events, since `sba::
    /// apply_player_loss` (the only place `PlayerState.eliminated` flips)
    /// always runs inside one of those passes. `winner` is the sole
    /// remainder, or `None` if the last players standing were eliminated
    /// simultaneously (a draw).
    ///
    /// The `turn_order.len() < 2` guard matters: a single-player test
    /// fixture (many exist, unrelated to this feature) has exactly one
    /// never-eliminated player, which would otherwise read as "game over"
    /// after the very first SBA pass — CR 104.2a presupposes opponents to
    /// win against, so there's nothing to end here.
    fn game_over_check(&self, next: ProcessState) -> ProcessState {
        if self.state.turn_order.len() < 2 {
            return next;
        }
        let remaining = commands::non_eliminated_players(&self.state);
        if remaining.len() <= 1 {
            ProcessState::GameOver { winner: remaining.first().copied() }
        } else {
            next
        }
    }

    /// `CleanupSbaCheck`'s own "what happens once this pass's events (and
    /// any dying-creature choice) are known" transition — pulled out for the
    /// same reason `finish_sba_loop` is, and shared the same way between a
    /// fresh pass (the `CleanupSbaCheck` arm) and one resumed after a paused
    /// CR 616.1/704.5j choice (`resume_sba_death`).
    ///
    /// Stashes `departed` (CR 603.10 dies-trigger abilities) into
    /// `departed_triggered_abilities` and routes through the real
    /// `CollectTriggers`/`PutTriggersApnap` stages via
    /// `PipelineExit::CleanupCheck` instead of deciding priority-vs-advance
    /// right here — this cleanup path used to bypass `CollectTriggers`
    /// entirely (backlog item 7a's cleanup half), silently dropping any
    /// trigger a cleanup-step SBA raised. `since_ledger` (captured before
    /// this pass began) is what `PipelineExit::CleanupCheck` later compares
    /// the ledger against to decide CR 514.3's priority-vs-advance question.
    ///
    /// TODO: CR 514.3 says another cleanup step begins after a priority
    /// window opens here and its stack empties - currently
    /// advance_turn_step's (Ending, Cleanup) arm rotates the turn instead,
    /// skipping re-running discard/effect-expiry. Correct once a case
    /// actually reaches this branch.
    fn finish_cleanup_sba_check(
        &mut self,
        events: Vec<GameEvent>,
        departed: Vec<RegisteredAbility>,
        since_ledger: usize,
    ) -> ProcessState {
        self.departed_triggered_abilities.extend(departed);
        self.state.ledger.extend(events);
        self.pipeline_exit = PipelineExit::CleanupCheck { since_ledger };
        self.game_over_check(ProcessState::CollectTriggers { since: since_ledger })
    }

    /// Continues `sba::resolve_deaths` after a paused CR 616.1 ordering
    /// choice was answered for one dying creature — `already_resolved` is
    /// that creature's own just-committed death event; `pending_deaths` is
    /// the rest of this SBA pass's dying-creature list. Finishes into
    /// whichever state originally started this pass (`then`), via
    /// `finish_sba_loop`/`finish_cleanup_sba_check`, so a resumed pause and
    /// a fresh pass reach the exact same "nothing more to do" transition.
    /// Commits one scripted zone-move target's already-replacement-checked
    /// `final_event` — `Destroy`/`Sacrifice` reuse the same
    /// `commit_death_or_regeneration`/`commit_death` helpers every SBA-driven
    /// death already goes through; `Exile` is inlined (unchanged from the
    /// pre-`ScriptedZoneMove` Exile arm's own body) since its destination is
    /// the shared Exile zone (`owner: None`), not a per-player zone
    /// `commit_death`'s `zone_mut(to, Some(owner))` call assumes.
    fn commit_scripted_zone_move(&mut self, kind: ScriptedZoneMoveKind, final_event: GameEventKind) {
        match kind {
            ScriptedZoneMoveKind::Destroy => {
                let (event, departed) = sba::commit_death_or_regeneration(&mut self.state, final_event);
                self.departed_triggered_abilities.extend(departed);
                self.state.ledger.push(event);
            }
            ScriptedZoneMoveKind::Sacrifice => {
                let (event, departed) = sba::commit_death(&mut self.state, final_event);
                self.departed_triggered_abilities.extend(departed);
                self.state.ledger.push(event);
            }
            ScriptedZoneMoveKind::Exile => {
                let GameEventKind::ZoneChange { object: old_id, to, last_known_info, .. } = final_event else {
                    unreachable!("apply_replacements only rewrites ZoneChange fields, never the kind");
                };
                let last_known_info = last_known_info.expect(
                    "continue_scripted_zone_move always proposes with last_known_info populated",
                );
                let owner = last_known_info.owner;
                let new_id = self.state.allocate_object_id();
                let mut new_object = last_known_info.clone();
                new_object.object_id = new_id;
                new_object.lineage = ObjectLineage { previous: Some(old_id) };
                new_object.zone = to;
                new_object.timestamp = self.state.allocate_sequence_no();

                if let Some(zone) = self.state.zone_mut(ZoneKind::Battlefield, None) {
                    zone.remove(old_id);
                }
                let departed = commands::retire_object(&mut self.state, old_id);
                self.departed_triggered_abilities.extend(departed);
                // Exile is a shared zone (owner = None); per-player
                // graveyard/hand need Some(owner) - this lookup must use
                // None or the object silently disappears.
                let exile_owner = match to {
                    ZoneKind::Exile => None,
                    _ => Some(owner),
                };
                if let Some(zone) = self.state.zone_mut(to, exile_owner) {
                    zone.push_bottom(new_id);
                }
                self.state.objects.insert(new_id, new_object);

                let event_id = self.state.allocate_event_id();
                let sequence_no = self.state.allocate_sequence_no();
                let committed = GameEvent {
                    event_id,
                    sequence_no,
                    kind: GameEventKind::ZoneChange {
                        object: new_id,
                        from: ZoneKind::Battlefield,
                        to,
                        last_known_info: Some(last_known_info),
                    },
                    caused_by_command: None,
                    parent_event: None,
                };
                self.state.ledger.push(committed);
            }
        }
    }

    /// Drains `remaining` scripted zone-move targets one at a time — CR
    /// 616.1's ordering-choice pause can be raised by any of them, not just
    /// the first, since each target independently proposes+replacement-
    /// checks+commits its own `ZoneChange` (`commit_scripted_zone_move`).
    /// Returns the same `ReplacementCheck { remaining: [StackObjectResolved] }`
    /// transition the pre-`ScriptedZoneMove` non-pausing path already used
    /// once the queue empties, or pauses immediately on
    /// `AwaitingReplacementChoice` the instant one target needs an ordering
    /// choice, carrying the rest of the queue in `ReplacementResumeTarget::
    /// ScriptedZoneMove` for `submit_replacement_choice` to continue later.
    fn continue_scripted_zone_move(
        &mut self,
        kind: ScriptedZoneMoveKind,
        mut remaining: VecDeque<ObjectId>,
        stack_id: ObjectId,
    ) -> ProcessState {
        while let Some(id) = remaining.pop_front() {
            let Some(last_known_info) =
                self.state.object(id).filter(|obj| obj.zone == ZoneKind::Battlefield).cloned()
            else {
                // Target already left the battlefield (or never resolved) -
                // nothing to move, same "skip rather than guess" stance
                // every other arm here takes.
                continue;
            };
            if kind == ScriptedZoneMoveKind::Destroy {
                // CR 702.12b: an indestructible permanent can't be destroyed
                // at all - not a replacement effect, checked up front.
                let indestructible =
                    commands::effective_keywords(&self.state, &self.script_runtime, id)
                        .contains(KeywordSet::INDESTRUCTIBLE);
                if indestructible {
                    continue;
                }
            }
            let to = if kind == ScriptedZoneMoveKind::Exile { ZoneKind::Exile } else { ZoneKind::Graveyard };
            let proposed = GameEventKind::ZoneChange {
                object: id,
                from: ZoneKind::Battlefield,
                to,
                last_known_info: Some(last_known_info),
            };
            // CR 614.8: only Destroy is genuine destruction - a regeneration
            // shield can intercept it and only it (Sacrifice/Exile are each
            // their own real zone-change verb, neither is destruction).
            let is_destroy = kind == ScriptedZoneMoveKind::Destroy;
            match replacement::apply_replacements(&mut self.state, &self.script_runtime, proposed, is_destroy) {
                ReplacementOutcome::Resolved(final_event) => {
                    self.commit_scripted_zone_move(kind, final_event);
                }
                ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                    return ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::ScriptedZoneMove { kind, remaining, stack_id },
                    };
                }
            }
        }
        ProcessState::ReplacementCheck {
            remaining: VecDeque::from([GameEventKind::StackObjectResolved { stack_id }]),
            checked: Vec::new(),
        }
    }

    /// CR 120.3/121/616.1: drains `remaining` scripted draws one at a time,
    /// each proposed via `commands::propose_draw`, replacement-checked, and
    /// committed inline. A CR 616.1 ordering choice pauses on any individual
    /// draw, carrying the rest of the count in `ReplacementResumeTarget::
    /// ScriptedDraw` so `submit_replacement_choice` can resume. The draw
    /// events themselves are pushed directly to the ledger (not queued in
    /// `events_so_far`) so they are never re-checked a second time against a
    /// fresh, empty `excluded` set; `events_so_far` carries only the rest of
    /// this resolution's already-produced events (e.g. an instant/sorcery
    /// spell's Stack->Graveyard move) so they still reach the normal
    /// `ReplacementCheck`->`CommitEvents` pipeline once the draws finish.
    fn continue_draw_cards(
        &mut self,
        mut remaining: u32,
        player: PlayerId,
        stack_id: ObjectId,
        mut events_so_far: Vec<GameEventKind>,
    ) -> ProcessState {
        while remaining > 0 {
            remaining -= 1;
            let Some(proposed) = commands::propose_draw(&mut self.state, player) else {
                break;
            };
            match replacement::apply_replacements(
                &mut self.state,
                &self.script_runtime,
                proposed,
                false,
            ) {
                ReplacementOutcome::Resolved(event) => {
                    let (committed, departed) = commands::commit_zone_move(&mut self.state, event);
                    self.departed_triggered_abilities.extend(departed);
                    self.state.ledger.push(committed);
                }
                ReplacementOutcome::NeedsOrderingChoice {
                    affected_player,
                    event,
                    candidates,
                    excluded,
                    ..
                } => {
                    return ProcessState::AwaitingReplacementChoice {
                        affected_player,
                        pending_event: event,
                        candidates,
                        excluded,
                        resume: ReplacementResumeTarget::ScriptedDraw {
                            remaining,
                            player,
                            stack_id,
                            events_so_far,
                        },
                    };
                }
            }
        }
        events_so_far.push(GameEventKind::StackObjectResolved { stack_id });
        ProcessState::ReplacementCheck {
            remaining: events_so_far.into(),
            checked: Vec::new(),
        }
    }

    fn resume_sba_death(
        &mut self,
        pending_deaths: VecDeque<sba::DyingCandidate>,
        mut already_resolved: Vec<GameEvent>,
        mut already_departed: Vec<RegisteredAbility>,
        then: SbaResumeThen,
    ) -> ProcessState {
        match sba::resolve_deaths(&mut self.state, &self.script_runtime, pending_deaths) {
            sba::DeathResolution::Done(mut events, mut departed) => {
                already_resolved.append(&mut events);
                already_departed.append(&mut departed);
                match then {
                    SbaResumeThen::SbaLoop => self.finish_sba_loop(already_resolved, already_departed),
                    SbaResumeThen::CleanupSbaCheck { since_ledger } => {
                        self.finish_cleanup_sba_check(already_resolved, already_departed, since_ledger)
                    }
                }
            }
            sba::DeathResolution::NeedsOrderingChoice {
                affected_player,
                pending_event,
                candidates,
                excluded,
                is_destroy,
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
                    resume: ReplacementResumeTarget::SbaDeath { pending_deaths, is_destroy, then },
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
        // CR 502.3: untaps what the active player *currently* controls
        // (layers::effective_controller), not the raw field.
        let mut ids: Vec<ObjectId> = self
            .state
            .objects
            .iter()
            .filter(|(_, o)| {
                o.zone == ZoneKind::Battlefield
                    && o.tapped
                    && crate::layers::effective_controller(&self.state, &self.script_runtime, o.object_id)
                        == active
            })
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
        // CR 402.2: `None` means "no maximum hand size" - skip the discard
        // check entirely, same as if hand_size could never exceed it.
        let Some(max_hand_size) = effective_max_hand_size(&self.state.hand_size_modifiers, active, 7) else {
            self.process_state = ProcessState::CleanupSbaCheck;
            return StepStatus::Progressed;
        };
        let max_hand_size = max_hand_size as usize;

        if hand_size > max_hand_size {
            let excess = hand_size - max_hand_size;
            self.process_state = ProcessState::AwaitingDiscard { player: active, excess };
            return StepStatus::AwaitingDiscard { player: active, hand_size, max_hand_size };
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
                // CR 510.1a: two combat damage steps instead of one iff some
                // attacker or blocker currently has first strike or double
                // strike - read through effective_keywords (CR 613 layer 6)
                // so a continuous effect granting the keyword counts too,
                // not just the printed KeywordSet.
                let has_first_or_double_strike = self
                    .state
                    .combat
                    .as_ref()
                    .map(|combat| {
                        combat
                            .attackers
                            .iter()
                            .map(|(attacker, _)| *attacker)
                            .chain(combat.blocks.iter().map(|(blocker, _)| *blocker))
                            .any(|id| {
                                commands::effective_keywords(&self.state, &self.script_runtime, id)
                                    .intersects(KeywordSet::FIRST_STRIKE | KeywordSet::DOUBLE_STRIKE)
                            })
                    })
                    .unwrap_or(false);
                self.state.step = if has_first_or_double_strike {
                    Step::FirstStrikeDamage
                } else {
                    Step::CombatDamage
                };
                false
            }
            (Phase::Combat, Step::FirstStrikeDamage) => {
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
                // CR 511.2/500.5a: "until end of combat" effects expire at
                // the end of the combat phase itself (this transition out of
                // it), not at the beginning of the End of Combat step.
                self.state
                    .continuous_effects
                    .retain(|e| !matches!(e.duration, EffectDuration::UntilEndOfCombat));
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
                // CR 800.4b: the next turn skips any eliminated player.
                self.state.active_player =
                    commands::next_active_player(&self.state, self.state.active_player);
                // CR 723.1-style "until your next turn": the effect lasts
                // until the beginning of its controller's own next turn, not
                // just any next turn - drain exactly the effects whose
                // source's current controller is the player who is about to
                // become active. An effect whose source no longer resolves
                // (e.g. a spell that already left the stack) can never match
                // here and is left inert rather than guessed at - same
                // stance `applicable_effects_in_layer` already takes for an
                // unresolvable source (TODO: known limitation, not fixed by
                // this slice).
                let new_active_player = self.state.active_player;
                let objects = &self.state.objects;
                self.state.continuous_effects.retain(|e| {
                    if !matches!(e.duration, EffectDuration::UntilYourNextTurn) {
                        return true;
                    }
                    match objects.get(&e.source) {
                        Some(source_obj) => source_obj.controller != new_active_player,
                        None => true,
                    }
                });
                self.state.turn_number += 1;
                self.state.phase = Phase::Beginning;
                self.state.step = Step::Untap;
                // CR 305.2's per-turn land-drop count resets with the turn.
                self.state.lands_played_this_turn.clear();
                // CR 603.2h's "only once each turn" cap resets with the turn
                // too - same reasoning, a new turn is a fresh "turn" for
                // this restriction.
                self.state.once_per_turn_triggered.clear();
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
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
                commander_casts: std::collections::HashMap::new(),
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
            cost_modifiers: Vec::new(),
            hand_size_modifiers: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: crate::types::game::RngState { seed: 1, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 0,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
        }
    }

    fn trigger(id: u64, controller: PlayerId) -> PendingTrigger {
        PendingTrigger { source: ObjectId(id), controller, ability_index: 0, once_per_turn: false }
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
                effect: None,
                modes: Vec::new(),
            },
            copiable_override: None,
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
            is_commander: false,
            is_token: false,
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
        let event = GameEventKind::DamageDealt { source: ObjectId(1), target: Target::Player(p0), amount: 7, is_combat_damage: false };
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
                is_combat_damage: false,
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
        let event = GameEventKind::DamageDealt { source: ObjectId(2), target: Target::Player(p0), amount: 7, is_combat_damage: false };
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
