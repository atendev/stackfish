//! The pipeline stage state machine behind `Engine::advance`. See `ProcessState`
//! and `DESIGN.md`.

use std::collections::VecDeque;
use std::ops::Range;

use crate::commands::{Internal, PaymentResolution};
use crate::replacement;
use crate::sba;
use crate::script::ScriptChoiceRequest;
use crate::triggers::PendingTrigger;
use crate::types::ability::{AbilityEffect, CopyExceptions, SearchDestination, TokenSpec, UnlessPaidConsequence};
use crate::types::card::{CardDefinition, ModeChoice, SpellMode};
use crate::types::command::PlayerAction;
use crate::types::cost::CostSpec;
use crate::types::event::GameEventKind;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::mana::ColorSet;
use crate::types::stack::StackObject;
use crate::types::target::TargetSpec;
use crate::types::zone::ZoneKind;

/// What happened on one `step()` call, and what the engine needs next.
#[derive(Debug)]
pub enum StepStatus {
    /// Made progress; safe to call `step()` again immediately.
    Progressed,
    /// Needs a `PlayerAction` from `player`; `submit_action`, then `step()`.
    AwaitingAction { player: PlayerId },
    /// CR 616.1: several replacement effects matched one event; `player` picks one
    /// of `candidates` via `submit_replacement_choice`. This is a sub-decision, so
    /// `submit_action` keeps rejecting while it is pending.
    AwaitingReplacementChoice { player: PlayerId, candidates: Vec<replacement::ReplacementCandidate> },
    /// CR 704.5j: `player` keeps one of the same-named legendary `candidates` via
    /// `submit_legend_rule_choice`.
    AwaitingLegendRuleChoice { player: PlayerId, candidates: Vec<ObjectId> },
    /// CR 903.9a: `player` may return `commander` to the command zone via
    /// `submit_commander_return_choice`.
    AwaitingCommanderReturnChoice { player: PlayerId, commander: ObjectId },
    /// CR 303.4d: several attach candidates for one Aura; `player` picks via
    /// `submit_attach_choice`. Only reachable by constructing the state directly,
    /// since no effect produces simultaneous candidates.
    AwaitingAttachChoice { player: PlayerId, aura: ObjectId, candidates: Vec<ObjectId> },
    /// CR 303.4f: 2+ legal attach targets for an Aura, Equipment or Fortification
    /// entering through a search; `player` picks via `submit_entry_attach_choice`.
    /// Candidates are `Target`s since an Aura can enchant a player (CR 303.4b).
    AwaitingEntryAttachChoice { player: PlayerId, attaching: ObjectId, candidates: Vec<Target> },
    /// CR 303.4f: the token-creation counterpart, answered by
    /// `submit_token_attach_choice`. Separate from `AwaitingEntryAttachChoice`
    /// because token creation resumes a count, not the search's library loop.
    AwaitingTokenAttachChoice { player: PlayerId, attaching: ObjectId, candidates: Vec<Target> },
    /// CR 702.82a: `player` sacrifices any subset of `candidates` for `object`'s
    /// devour (`per_creature` counters each) via `submit_devour_choice`.
    AwaitingDevourChoice { player: PlayerId, object: ObjectId, per_creature: i32, candidates: Vec<ObjectId> },
    /// CR 702.38a: `player` reveals any subset of `candidates` for `object`'s
    /// amplify (`per_card` counters each) via `submit_amplify_choice`.
    AwaitingAmplifyChoice { player: PlayerId, object: ObjectId, per_card: i32, candidates: Vec<ObjectId> },
    /// `AwaitingDevourChoice` raised by a search, answered by `submit_search_devour_choice`.
    AwaitingSearchDevourChoice { player: PlayerId, object: ObjectId, per_creature: i32, candidates: Vec<ObjectId> },
    /// `AwaitingAmplifyChoice` raised by a search, answered by `submit_search_amplify_choice`.
    AwaitingSearchAmplifyChoice { player: PlayerId, object: ObjectId, per_card: i32, candidates: Vec<ObjectId> },
    /// CR 702.35a: `player` casts `card` from exile via `submit_madness_cast` or
    /// declines via `decline_madness_cast`. A cast is a nested cast, not a candidate pick.
    AwaitingMadnessChoice { player: PlayerId, card: ObjectId },
    /// CR 702.85a: `player` casts `candidate` for free via `submit_cascade_cast`
    /// (only if it is `Some`) or declines via `decline_cascade_cast`.
    AwaitingCascadeChoice { player: PlayerId, candidate: Option<ObjectId> },
    /// CR 118.12: `payer` may pay `cost` via `submit_unless_paid_payment` to stop
    /// `otherwise`, or decline via `decline_unless_paid`.
    AwaitingUnlessPaidChoice { payer: PlayerId, cost: CostSpec, otherwise: UnlessPaidConsequence },
    /// CR 701.9: the targeted `player` discards one of `candidates` via
    /// `submit_discard_choice`; `remaining_count` more picks are owed after it.
    AwaitingDiscardChoice { player: PlayerId, candidates: Vec<ObjectId>, remaining_count: u32 },
    /// CR 707.9: `player` picks one of `candidates` for `object` to copy, or none,
    /// via `submit_copy_choice`.
    AwaitingCopyChoice { player: PlayerId, object: ObjectId, candidates: Vec<ObjectId> },
    /// A card script yielded mid-resolution asking `player` for a choice matching
    /// `request`; answered by `submit_script_choice`.
    AwaitingScriptChoice { player: PlayerId, request: ScriptChoiceRequest },
    /// CR 701.22: `player` orders the top `cards` between top and bottom via
    /// `submit_scry_choice`.
    AwaitingScryChoice { player: PlayerId, cards: Vec<ObjectId> },
    /// CR 701.25: like `AwaitingScryChoice`, but the other pile goes to the graveyard.
    AwaitingSurveilChoice { player: PlayerId, cards: Vec<ObjectId> },
    /// CR 701.23: `player` chooses up to `count` of `candidates` via
    /// `submit_search_choice`; fewer, including none, is legal (CR 701.23b).
    AwaitingSearchChoice { player: PlayerId, candidates: Vec<ObjectId>, count: u32 },
    /// CR 603.3b: `player` orders their simultaneous triggers via `submit_trigger_order`.
    AwaitingTriggerOrderChoice { player: PlayerId, group: Vec<PendingTrigger> },
    /// CR 603.3d: a trigger needs targets; `candidates` has one legal list per word
    /// of `spec`. `player` answers with one flat list via `submit_trigger_targets`.
    AwaitingTriggerTargets { player: PlayerId, spec: TargetSpec, candidates: Vec<Vec<Target>> },
    /// CR 603.3c: a modal trigger needs its mode choice, raised before the target
    /// choice. `player` is the chooser, which is another player under CR 700.2e.
    /// Answered by `submit_trigger_mode_choice`.
    AwaitingTriggerModeChoice { player: PlayerId, modes: Vec<SpellMode>, mode_choice: ModeChoice, min: u32, max: u32 },
    /// CR 601.2f: `player` cast a spell (`stack_id`) and must pay via
    /// `submit_payment`. A sub-decision with its own entry point.
    AwaitingPayment { player: PlayerId, stack_id: ObjectId },
    /// CR 700.2e: a modal spell (`stack_id`) reached resolution with its mode choice
    /// deferred to `player`, who picks between `min` and `max` modes via
    /// `submit_mode_choice`.
    AwaitingModeChoice { player: PlayerId, stack_id: ObjectId, min: u32, max: u32 },
    /// CR 514.1: `player`'s hand exceeds `max_hand_size`; they discard the excess via
    /// `submit_discard`.
    AwaitingDiscard { player: PlayerId, hand_size: usize, max_hand_size: usize },
    /// A broken engine invariant. Fatal: the state may be half-changed and the host
    /// must stop using the engine. A bad input is rejected up front and never lands
    /// here. A script error during resolution is logged as a `ScriptError` event instead.
    Error(Internal),
    GameOver { winner: Option<PlayerId> },
    /// CR 103.5: `player` keeps or mulligans via `submit_mulligan_decision`.
    AwaitingMulliganDecision { player: PlayerId },
    /// CR 103.5: `player` bottoms `count` cards via `submit_bottom_cards`.
    AwaitingBottomCardChoice { player: PlayerId, count: usize },
}

/// Result of one `step()`. `events_added` indexes into the ledger rather than
/// cloning events out.
pub struct StepResult {
    pub events_added: Range<usize>,
    pub status: StepStatus,
}

/// Which scripted zone-move op a `ReplacementResumeTarget::ScriptedZoneMove` is
/// resuming. Destroy and sacrifice commit through the SBA death helpers. Exile is
/// committed inline because its zone has no owner. Discard moves a card off a hand
/// rather than a permanent off the battlefield (CR 701.9), so `from_zone()` is read
/// instead of assuming the battlefield.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScriptedZoneMoveKind {
    Destroy,
    Sacrifice,
    Exile,
    Discard,
    /// CR 400.3: Battlefield -> owner's Hand (`AbilityEffect::ReturnToHand`).
    Bounce,
    /// CR 701.17a: library to graveyard (`AbilityEffect::Mill`). Committed through
    /// `commit_zone_move` so a redirect to exile lands in the right zone.
    Mill,
    /// CR 110.2a, 400.7: graveyard or exile to battlefield. `controller` is who it
    /// enters under (`None` is the owner); `tapped` enters it tapped.
    Reanimate { controller: Option<PlayerId>, tapped: bool },
    /// CR 610.3: `Exile` that also links each card landing in exile to
    /// `source`, so it returns once `source` leaves the battlefield
    /// (`GameState::linked_exiles`). Battlefield sources only.
    ExileUntilLeaves { source: ObjectId },
}

impl ScriptedZoneMoveKind {
    /// Whether an object in `zone` can be a source for this kind. Exile and bounce
    /// take the zone the target actually sits in.
    pub(in crate::engine) fn accepts_source_zone(self, zone: ZoneKind) -> bool {
        match self {
            ScriptedZoneMoveKind::Exile => matches!(zone, ZoneKind::Battlefield | ZoneKind::Graveyard),
            ScriptedZoneMoveKind::Bounce => {
                matches!(zone, ZoneKind::Battlefield | ZoneKind::Graveyard | ZoneKind::Exile)
            }
            ScriptedZoneMoveKind::Reanimate { .. } => matches!(zone, ZoneKind::Graveyard | ZoneKind::Exile),
            other => zone == other.from_zone(),
        }
    }

    /// The zone each queued id must sit in: the battlefield, or the hand for discard (CR 701.9a).
    pub(in crate::engine) fn from_zone(self) -> ZoneKind {
        match self {
            ScriptedZoneMoveKind::Discard => ZoneKind::Hand,
            ScriptedZoneMoveKind::Mill => ZoneKind::Library,
            ScriptedZoneMoveKind::Reanimate { .. } => ZoneKind::Graveyard,
            ScriptedZoneMoveKind::Destroy
            | ScriptedZoneMoveKind::Sacrifice
            | ScriptedZoneMoveKind::Exile
            | ScriptedZoneMoveKind::ExileUntilLeaves { .. }
            | ScriptedZoneMoveKind::Bounce => {
                ZoneKind::Battlefield
            }
        }
    }
}

/// What `continue_scripted_zone_move` does once its queue drains, so a devour
/// sacrifice raised mid-search can resume the search instead of finalizing.
#[derive(Debug)]
pub enum ScriptedZoneMoveThen {
    /// `stack_id` is a resolving object already popped off the stack; push
    /// `StackObjectResolved` and hand off to `ReplacementCheck`.
    StackResolution { stack_id: ObjectId },
    /// CR 614.12a: resume `continue_search_battlefield_entry` with these parameters
    /// once the devour sacrifices, and any nested ordering pause, settle.
    SearchResume {
        controller: PlayerId,
        remaining: VecDeque<ObjectId>,
        destination: SearchDestination,
        stack_id: ObjectId,
        shuffle_after: bool,
    },
    /// Chains a second batch of a different kind after the first fully settles,
    /// since `continue_scripted_zone_move` drains one kind per call.
    ThenScriptedZoneMove { kind: ScriptedZoneMoveKind, remaining: VecDeque<ObjectId>, then: Box<ScriptedZoneMoveThen> },
    /// CR 610.3: linked-exile returns end an `SbaLoop` pass, so the loop starts
    /// again afterwards. `trailing` events are logged first.
    // TODO: the cleanup-step SBA check (`finish_cleanup_sba_check`) doesn't return linked exiles
    SbaLoop,
}

/// One card to propose, replacement-check and commit, shared by cost payments and
/// the cleanup discard. `from` and `to` are the zones it is claimed to move
/// between, confirmed by the caller before queueing.
#[derive(Clone, Copy, Debug)]
pub struct PendingZoneMove {
    pub id: ObjectId,
    pub from: ZoneKind,
    pub to: ZoneKind,
}

/// Which pipeline a paused `AwaitingReplacementChoice` resumes into. The pause is
/// identical either way; only what happens after the choice differs.
#[derive(Debug)]
pub enum ReplacementResumeTarget {
    /// A command's proposed events; `remaining` and `checked` carry the run.
    ReplacementCheck { remaining: VecDeque<GameEventKind>, checked: Vec<GameEventKind> },
    /// Inside an SBA pass (CR 614.13). `pending_deaths` is the rest of the list and
    /// `then` names the pass that started it. `is_destroy` is the paused event's
    /// destroy tag (CR 614.8), already popped so it can't be re-derived.
    SbaDeath { pending_deaths: VecDeque<sba::DyingCandidate>, is_destroy: bool, then: SbaResumeThen },
    /// A scripted zone-move op inside `ResolveStackTop`, or a devour sacrifice raised
    /// mid-search. Each target proposes and commits independently, so `remaining` is
    /// the targets not yet proposed. `then` says what follows. `trailing` holds events
    /// still owed once every target is processed (CR 702.82a).
    ScriptedZoneMove { kind: ScriptedZoneMoveKind, remaining: VecDeque<ObjectId>, then: ScriptedZoneMoveThen, trailing: Vec<GameEventKind> },
    /// A sacrifice or discard cost payment. `remaining` is the rest of the queue,
    /// `payer` is for the final `CostPaid`, and `on_paid` rides through so a
    /// self-sacrificing mana ability still adds its mana afterwards.
    CostZoneMove {
        remaining: VecDeque<PendingZoneMove>,
        payer: PlayerId,
        stack_id: ObjectId,
        on_paid: PaymentResolution,
        /// CR 702.29c: keeps track of the cycling self-discard across the pause.
        cycling_source: Option<ObjectId>,
    },
    /// The cleanup hand-size discard. `since_ledger` predates the first committed
    /// card so the trigger window still sees the whole discard.
    CleanupDiscard { remaining: VecDeque<PendingZoneMove>, since_ledger: usize },
    /// The decline branch of madness (CR 702.35a): one proposal, not a queue.
    /// `stack_id` and `events_so_far` fold into the normal tail once it resolves.
    MadnessDecline { card: ObjectId, stack_id: ObjectId, events_so_far: Vec<GameEventKind> },
    /// The draw step's turn-based draw (CR 504.1): one proposal. `tail_events` are
    /// the step markers still unlogged. `since_ledger` predates the draw so the
    /// trigger window covers both.
    Draw { tail_events: Vec<GameEventKind>, since_ledger: usize },
    /// A scripted `DrawCards(n)` (CR 121): `remaining` draws are left after the
    /// paused one. The draws commit inline so they aren't re-checked; `events_so_far`
    /// reaches the normal pipeline afterwards.
    ScriptedDraw {
        remaining: u32,
        player: PlayerId,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    },
    /// A scripted discard after a choice was made (CR 701.9): `remaining_count` more
    /// are owed, and `events_so_far` is the resolving ability's tail. The choice
    /// itself lives on `AwaitingDiscardChoice`.
    TargetedDiscard {
        player: PlayerId,
        remaining_count: u32,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    },
    /// CR 614, 616.1: two or more `ReplaceZoneDestination` effects matched an
    /// entering permanent. Nothing has moved yet, so the whole popped `top` is
    /// carried. `legal_targets` is its rechecked list (CR 608.2b), for the Aura
    /// attach step. Resumed by `continue_permanent_entry`.
    PermanentEntry { top: StackObject, legal_targets: Vec<Target>, events: Vec<GameEventKind> },
}

/// CR 101.4: the rest of a `ForEachPlayer` loop once the current player's choice is
/// answered. `source` and `operation` re-invoke the function for the next player,
/// since the stack object is already popped. `sacrifice_ids` and `discard_ids`
/// accumulate and apply only when `remaining_players` empties, so the actions
/// happen simultaneously.
#[derive(Debug)]
pub struct EachPlayerLoop {
    pub source: ObjectId,
    pub operation: ScriptHandle,
    pub remaining_players: VecDeque<PlayerId>,
    pub sacrifice_ids: Vec<ObjectId>,
    pub discard_ids: Vec<ObjectId>,
}

/// See `ReplacementResumeTarget::SbaDeath`. `CleanupSbaCheck` carries
/// `since_ledger` through the pause for `finish_cleanup_sba_check`.
#[derive(Debug)]
pub enum SbaResumeThen {
    SbaLoop,
    CleanupSbaCheck { since_ledger: usize },
}

/// The pipeline stage the engine is in, behind `step()`. One `step()` advances one
/// variant; `SbaLoop` re-enters itself so a replay viewer sees each SBA pass
/// (CR 704.3). `legal_actions()` reads state on demand, so it has no pause of its own.
#[derive(Debug)]
pub enum ProcessState {
    PriorityWindow,
    /// Accepted by `submit_action`; the next `step()` runs it.
    PendingAction(PlayerAction),
    /// CR 117.4: all players passed with a non-empty stack; resolve the top object.
    ResolveStackTop,
    /// Proposed events awaiting replacement checks (CR 614-616). `checked` holds
    /// those already resolved; `remaining` is a queue so processing keeps its order.
    ReplacementCheck { remaining: VecDeque<GameEventKind>, checked: Vec<GameEventKind> },
    /// CR 616.1: `pending_event` matched several effects and `affected_player` picks
    /// which applies first. `excluded` carries the effects already applied to this
    /// event. `resume` says what follows the answer, the only difference between the
    /// pipelines that reach this pause.
    AwaitingReplacementChoice {
        affected_player: PlayerId,
        pending_event: GameEventKind,
        candidates: Vec<replacement::ReplacementCandidate>,
        excluded: Vec<replacement::ReplacementCandidate>,
        resume: ReplacementResumeTarget,
    },
    /// CR 704.5j: `player` keeps one of `candidates`. `then` names the SBA pass that
    /// raised it; the losers resolve through `resume_sba_death`.
    AwaitingLegendRuleChoice { player: PlayerId, candidates: Vec<ObjectId>, then: SbaResumeThen },
    /// CR 903.9a: `commander` sits in a graveyard or exile and `player` hasn't been
    /// offered the return. `then` is as above.
    AwaitingCommanderReturnChoice { player: PlayerId, commander: ObjectId, then: SbaResumeThen },
    /// CR 303.4d: several candidates for one Aura. It has no `then` because no
    /// producer dictates a pipeline, so the answer resumes `PriorityWindow`.
    AwaitingAttachChoice { aura: ObjectId, controller: PlayerId, candidates: Vec<ObjectId> },
    /// CR 303.4f: a found Aura, Equipment or Fortification entering through a
    /// search has 2+ legal attach candidates; `controller` picks. The search's loop
    /// state rides along for the resume, and `attaching` is already on the
    /// battlefield, unattached. Candidates are `Target`s since an Aura can enchant a
    /// player (CR 303.4b); Equipment and Fortification candidates are objects.
    AwaitingEntryAttachChoice {
        attaching: ObjectId,
        controller: PlayerId,
        candidates: Vec<Target>,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
        remaining: std::collections::VecDeque<ObjectId>,
        destination: SearchDestination,
        shuffle_after: bool,
    },
    /// CR 303.4f: the token-creation counterpart. `spec` and `copy_snapshot` build
    /// the further tokens owed; `remaining_count` is how many follow `attaching`.
    /// There is no library queue or shuffle to resume, only a count.
    AwaitingTokenAttachChoice {
        attaching: ObjectId,
        controller: PlayerId,
        candidates: Vec<Target>,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
        remaining_count: u32,
        spec: TokenSpec,
        copy_snapshot: Option<CardDefinition>,
    },
    /// CR 702.82a: raised where the enters-the-battlefield modifiers would run for
    /// `object`, with 1+ creature candidates. `stack_id` and `events_so_far` are the
    /// resolution's uncommitted progress. The answer sacrifices through
    /// `continue_scripted_zone_move` and folds a `CounterAdded` into its tail.
    AwaitingDevourChoice {
        object: ObjectId,
        controller: PlayerId,
        per_creature: i32,
        candidates: Vec<ObjectId>,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    },
    /// CR 702.38a: like devour with hand-card candidates. Revealing moves nothing, so
    /// the answer folds `events_so_far` and a `CounterAdded` straight into the
    /// resolved tail.
    AwaitingAmplifyChoice {
        object: ObjectId,
        controller: PlayerId,
        per_card: i32,
        candidates: Vec<ObjectId>,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    },
    /// CR 702.82a: devour raised by a search. A separate variant because the answer
    /// must resume the search's `remaining`, `destination`, `controller` and
    /// `shuffle_after` instead of finalizing; `stack_id` and `events_so_far` are as above.
    AwaitingSearchDevourChoice {
        object: ObjectId,
        controller: PlayerId,
        per_creature: i32,
        candidates: Vec<ObjectId>,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
        remaining: VecDeque<ObjectId>,
        destination: SearchDestination,
        shuffle_after: bool,
    },
    /// CR 702.38a: amplify raised by a search. Resumes `continue_search_battlefield_entry`
    /// directly since revealing moves nothing.
    AwaitingSearchAmplifyChoice {
        object: ObjectId,
        controller: PlayerId,
        per_card: i32,
        candidates: Vec<ObjectId>,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
        remaining: VecDeque<ObjectId>,
        destination: SearchDestination,
        shuffle_after: bool,
    },
    /// CR 702.35a: the madness trigger found its card in exile. `card` is the live
    /// exile object, not the original hand object, and its `alternative_costs` are
    /// read from this id. Both answers push `StackObjectResolved` into `events_so_far`.
    AwaitingMadnessChoice {
        player: PlayerId,
        card: ObjectId,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    },
    /// CR 702.85a: the cascade library walk finished. `exiled` is every card moved
    /// in walk order; `candidate` is the first nonland card of lower mana value.
    AwaitingCascadeChoice {
        player: PlayerId,
        candidate: Option<ObjectId>,
        exiled: Vec<ObjectId>,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    },
    /// CR 118.12: `payer_queue` is built in APNAP order (CR 101.4), and only its
    /// front may answer; a decline pops it and dispatches `otherwise` once empty.
    /// `target_id` (the targeted stack object, for `Counter`) and `source` (for
    /// `SacrificeSelf`) are resolved once up front.
    AwaitingUnlessPaidChoice {
        payer_queue: VecDeque<PlayerId>,
        cost: CostSpec,
        otherwise: UnlessPaidConsequence,
        target_id: Option<ObjectId>,
        source: ObjectId,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    },
    /// CR 701.9: the targeted `player` discards one of `candidates`, re-derived from
    /// their hand before every pick. `remaining_count` more picks follow.
    AwaitingDiscardChoice {
        player: PlayerId,
        candidates: Vec<ObjectId>,
        remaining_count: u32,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    },
    /// CR 707.9: raised where the enters-the-battlefield modifiers would run for
    /// `object`, with 1+ creature candidates. `exceptions` apply if a copy is chosen;
    /// `colors_spent` comes from the spell's `costs_paid` so a copied sunburst
    /// creature still gets a count.
    ///
    /// `carry_forward_if_declined` is `None` on a first pause. When
    /// `submit_copy_choice` detects a chain (the copied definition is itself an
    /// `EntersAsCopy`), it re-enters this variant holding the outer round's deferred
    /// exceptions: applied if this pause is declined, superseded if a copy is
    /// chosen (CR 707.9e).
    AwaitingCopyChoice {
        object: ObjectId,
        controller: PlayerId,
        exceptions: CopyExceptions,
        candidates: Vec<ObjectId>,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
        colors_spent: ColorSet,
        carry_forward_if_declined: Option<CopyExceptions>,
        /// CR 707.9e: `object`'s paid X, resolving `CounterAmount::X`. It is
        /// captured once because it never changes across a chain.
        x_value: Option<u32>,
    },
    /// A script operation yielded mid-resolution (`NeedsChoice`). `thread` and
    /// `request` are the suspended coroutine and its ask. `stack_id`, `controller` and
    /// `events_so_far` are the popped resolution's progress. `each_player` is set only
    /// inside a `ForEachPlayer` invocation. `source` is kept because a resumed
    /// coroutine needs it to build `deal_damage` and `add_counters` events.
    AwaitingScriptChoice {
        thread: ScriptHandle,
        request: ScriptChoiceRequest,
        stack_id: ObjectId,
        source: ObjectId,
        controller: PlayerId,
        events_so_far: Vec<GameEventKind>,
        each_player: Option<EachPlayerLoop>,
    },
    /// An `AbilityEffect::Scry(n)` pause. `cards` is the top `n` library ids,
    /// clamped to the library size. `stack_id`, `controller` and `events_so_far`
    /// are carried as for `AwaitingScriptChoice`.
    AwaitingScryChoice {
        stack_id: ObjectId,
        controller: PlayerId,
        events_so_far: Vec<GameEventKind>,
        cards: Vec<ObjectId>,
    },
    /// As `AwaitingScryChoice`, for `AbilityEffect::Surveil(n)`.
    AwaitingSurveilChoice {
        stack_id: ObjectId,
        controller: PlayerId,
        events_so_far: Vec<GameEventKind>,
        cards: Vec<ObjectId>,
    },
    /// An `AbilityEffect::Search` pause. `candidates` are pre-filtered, so
    /// `submit_search_choice` doesn't need the `FilterSpec`.
    AwaitingSearchChoice {
        stack_id: ObjectId,
        controller: PlayerId,
        events_so_far: Vec<GameEventKind>,
        candidates: Vec<ObjectId>,
        count: u32,
        destination: SearchDestination,
        shuffle_after: bool,
    },
    CommitEvents { checked: Vec<GameEventKind> },
    /// CR 704.3
    SbaLoop,
    CollectTriggers { since: usize },
    /// Triggers grouped in APNAP order (`group_apnap`), waiting for the stack.
    /// `ordered` holds finalized groups; `pending_groups` is a queue so APNAP order holds.
    PutTriggersApnap {
        ordered: Vec<PendingTrigger>,
        pending_groups: VecDeque<(PlayerId, Vec<PendingTrigger>)>,
    },
    /// CR 603.3b: `group` has several triggers and `player` orders them.
    /// `ordered` and `pending_groups` carry the rest of the run.
    AwaitTriggerOrderChoice {
        player: PlayerId,
        group: Vec<PendingTrigger>,
        ordered: Vec<PendingTrigger>,
        pending_groups: VecDeque<(PlayerId, Vec<PendingTrigger>)>,
    },
    /// CR 603.3d: `trigger` needs targets before it can be pushed. `spec` and
    /// `candidates` mirror `StepStatus::AwaitingTriggerTargets`; `remaining` is the
    /// rest of the push run.
    AwaitingTriggerTargets {
        trigger: PendingTrigger,
        spec: TargetSpec,
        candidates: Vec<Vec<Target>>,
        remaining: VecDeque<PendingTrigger>,
        /// `None` for a non-modal trigger, which uses the ability's own `effect`.
        /// `Some(effect)` comes from a mode choice (CR 603.3c) and is used verbatim,
        /// since a modal ability's own `effect` is a placeholder. The inner `Option`
        /// is real: a chosen mode can combine to no effect.
        modal_effect: Option<Option<AbilityEffect>>,
    },
    /// CR 603.3c: `trigger` is modal and `chooser` hasn't announced modes yet.
    /// Checked before the target pause. `modes` and `mode_choice` are cloned from the
    /// ability; `remaining` is the rest of the push run. `chooser` is the controller
    /// unless the choice is opponent-chosen (CR 700.2e), resolved before this pause
    /// is raised.
    AwaitingTriggerModeChoice {
        trigger: PendingTrigger,
        modes: Vec<SpellMode>,
        mode_choice: ModeChoice,
        remaining: VecDeque<PendingTrigger>,
        chooser: PlayerId,
    },
    /// CR 500-514: all players passed with an empty stack; advance the turn position.
    AdvanceTurnStep,
    /// CR 103.5: pregame mulligan round. Players are polled in turn order.
    AwaitingMulliganDecision { player: PlayerId },
    /// CR 103.5: `player` bottoms `count` cards (paid mulligans beyond
    /// `free_mulligans`). `remaining` is the rest of the queue.
    AwaitingBottomCardChoice { player: PlayerId, count: usize, remaining: VecDeque<(PlayerId, usize)> },
    /// CR 514.1: awaiting `submit_discard` for `excess` cards.
    AwaitingDiscard { player: PlayerId, excess: usize },
    /// CR 514.3: cleanup's turn-based actions are done. Runs one SBA pass, then
    /// routes through trigger collection (via `CleanupCheck`) to decide whether
    /// priority opens or the turn advances silently.
    CleanupSbaCheck,
    /// CR 601.2f: `stack_id` is on the stack with `cost` unpaid; `payer` submits a
    /// plan via `submit_payment`. Even a `{0}` cost needs an explicit empty plan
    /// (CR 118.5a). `source` is needed by `PaymentPlan::validate` for `TapSelf`.
    AwaitingPayment {
        stack_id: ObjectId,
        payer: PlayerId,
        cost: CostSpec,
        source: ObjectId,
        on_paid: PaymentResolution,
    },
    /// CR 700.2e: the top stack object's `pending_mode_choice` is still set;
    /// `chooser` submits between `min` and `max` modes via `submit_mode_choice`.
    AwaitingModeChoice { stack_id: ObjectId, chooser: PlayerId, min: u32, max: u32 },
    /// CR 104.2a: at most one player remains; `winner` is `None` if the last
    /// players were eliminated together (CR 104.4a). Terminal: `step()` returns
    /// `GameOver` without mutating anything.
    GameOver { winner: Option<PlayerId> },
}
