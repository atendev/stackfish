//! The pipeline stage state machine driving `Engine::advance` — split out of
//! `engine.rs` for readability. See `ProcessState`'s own doc comment for the
//! overview; every doc comment below is preserved verbatim from the original
//! single-file layout.

use std::collections::VecDeque;
use std::ops::Range;

use crate::commands::{CommandError, PaymentResolution};
use crate::replacement;
use crate::sba;
use crate::script::ScriptChoiceRequest;
use crate::triggers::PendingTrigger;
use crate::types::ability::{AbilityEffect, CopyExceptions, SearchDestination, TokenSpec};
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
    /// Needs a `PlayerAction` from `player` before it can continue — the
    /// caller should prompt and `submit_action`, then `step()` again.
    AwaitingAction { player: PlayerId },
    /// CR 616.1: more than one replacement/prevention effect matched the
    /// same event mid-`ReplacementCheck` — the caller should prompt `player`
    /// to pick one of `candidates` (each a `replacement::ReplacementCandidate`
    /// — a real registered effect or CR 702.16e protection, see that type's
    /// own doc comment) and call `submit_replacement_choice`, then `step()`
    /// again. Distinct from `AwaitingAction`: this isn't a `PlayerAction`/
    /// priority decision, it's a sub-decision mid-pipeline that
    /// `submit_action` must keep rejecting while it's pending (see
    /// `submit_action`'s `PriorityWindow`-only check).
    AwaitingReplacementChoice { player: PlayerId, candidates: Vec<replacement::ReplacementCandidate> },
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
    /// CR 303.4d: `commands::propose_attach` found more than one candidate
    /// for a single Aura's attachment at once — `aura`'s controller
    /// (`player`) must choose which of `candidates` it becomes attached to,
    /// then call `submit_attach_choice`. Dormant as of this slice (see
    /// `AttachChoiceError`'s own doc comment) — no real effect in this
    /// engine ever produces more than one simultaneous candidate, so this
    /// is only ever reached by directly constructing
    /// `ProcessState::AwaitingAttachChoice`.
    AwaitingAttachChoice { player: PlayerId, aura: ObjectId, candidates: Vec<ObjectId> },
    /// CR 303.4f/301.5e/301.6: `commands::legal_entry_attach_candidates`
    /// found 2+ legal attach targets for an Aura/Equipment/Fortification
    /// entering the battlefield via
    /// `SearchDestination::Battlefield::enters_attached` — `player` must
    /// choose which of `candidates` `attaching` becomes attached to, then
    /// call `submit_entry_attach_choice`. Real (not dormant) — the search
    /// pathway is a genuine producer. `candidates` is `Target`-shaped (not
    /// `ObjectId`) since `phase5/aura-enters-attached-player` — an Aura's
    /// own enchant ability can name a player (CR 303.4f/b); Equipment/
    /// Fortification candidates are always `Target::Object` in practice (CR
    /// 301.5a/301.6 forbid attaching either to a player), same as before.
    AwaitingEntryAttachChoice { player: PlayerId, attaching: ObjectId, candidates: Vec<Target> },
    /// CR 303.4f/301.5e/301.6, extended to token creation
    /// (`phase5/token-enters-attached`): `commands::legal_entry_attach_
    /// candidates` found 2+ legal attach targets for a freshly created
    /// token that's an Aura/Equipment/Fortification (realistically only
    /// reachable via `TokenSpec::CopyOf` — a plain `Named`/`Creature` token
    /// spec can never resolve to one) — `player` must choose which of
    /// `candidates` `attaching` (already a real battlefield token by this
    /// point) becomes attached to, then call `submit_token_attach_choice`.
    /// A separate variant from `AwaitingEntryAttachChoice` rather than a
    /// shared one — that pause's own resume always re-enters Search's
    /// `VecDeque<ObjectId>`-based library loop, which token creation's own
    /// `count`-driven loop has no equivalent of.
    AwaitingTokenAttachChoice { player: PlayerId, attaching: ObjectId, candidates: Vec<Target> },
    /// CR 702.82a (Devour): `replacement::pending_devour_choice` found a
    /// registered Devour effect on `object` (already a real battlefield
    /// permanent by this point) with 1+ creatures `player` controls to
    /// choose from — `player` picks any subset of `candidates` (including
    /// none) to sacrifice, then calls `submit_devour_choice`. `object`
    /// enters with `per_creature` counters times however many were chosen.
    AwaitingDevourChoice { player: PlayerId, object: ObjectId, per_creature: i32, candidates: Vec<ObjectId> },
    /// CR 702.38a (Amplify): `replacement::pending_amplify_choice` found a
    /// registered Amplify effect on `object` (already a real battlefield
    /// permanent by this point) with 1+ hand cards `player` could reveal —
    /// `player` picks any subset of `candidates` (including none) to
    /// reveal, then calls `submit_amplify_choice`. `object` enters with
    /// `per_card` counters times however many were revealed; revealed cards
    /// stay in hand untouched.
    AwaitingAmplifyChoice { player: PlayerId, object: ObjectId, per_card: i32, candidates: Vec<ObjectId> },
    /// Same as `AwaitingDevourChoice` above, but raised by a Devour creature
    /// fetched onto the battlefield via `Search` rather than resolved off
    /// the stack — call `submit_search_devour_choice` to answer instead of
    /// `submit_devour_choice` (`phase5/devour-amplify-search-entry`).
    AwaitingSearchDevourChoice { player: PlayerId, object: ObjectId, per_creature: i32, candidates: Vec<ObjectId> },
    /// Same as `AwaitingAmplifyChoice` above, but raised by an Amplify
    /// creature fetched onto the battlefield via `Search` — call
    /// `submit_search_amplify_choice` to answer instead of
    /// `submit_amplify_choice` (`phase5/devour-amplify-search-entry`).
    AwaitingSearchAmplifyChoice { player: PlayerId, object: ObjectId, per_card: i32, candidates: Vec<ObjectId> },
    /// CR 702.35a (Madness): the madness triggered ability resolved and
    /// found `card` still sitting in exile (its own live descendant of the
    /// departed hand object, located via `ObjectLineage`) — `player` (its
    /// controller) must either call `Engine::submit_madness_cast` to cast it
    /// paying its madness cost, or `Engine::decline_madness_cast` to put it
    /// into their graveyard instead. Unlike `AwaitingDevourChoice`/
    /// `AwaitingAmplifyChoice`, this isn't a `Vec<ObjectId>` candidate
    /// choice — the "yes" branch is a full nested cast (its own targets/
    /// modes/payment), so `card`/`player` are all the caller needs to decide
    /// which of the two entry points to call.
    AwaitingMadnessChoice { player: PlayerId, card: ObjectId },
    /// CR 702.85a (Cascade): the cascade trigger's own library walk
    /// finished — `candidate` is the first nonland card found whose mana
    /// value was less than the cascade spell's own (`None` if the library
    /// ran out first). `player` (the cascade spell's own controller) must
    /// either call `Engine::submit_cascade_cast` (only legal if `candidate`
    /// is `Some`) to cast it for free, or `Engine::decline_cascade_cast` to
    /// put every exiled card on the bottom of their library in a random
    /// order. Same "not a `Vec<ObjectId>` candidate choice, the 'yes'
    /// branch is a full nested cast" shape `AwaitingMadnessChoice` above
    /// already established.
    AwaitingCascadeChoice { player: PlayerId, candidate: Option<ObjectId> },
    /// CR 707.6/614.12a (Clone itself): `replacement::pending_copy_choice`
    /// found a registered `EntersAsCopy` effect on `object` (already a real
    /// battlefield permanent by this point) with 1+ creatures on the
    /// battlefield `player` could copy — `player` picks at most one of
    /// `candidates` (or none, CR 707.6's "may") to become a copy of, then
    /// calls `submit_copy_choice`.
    AwaitingCopyChoice { player: PlayerId, object: ObjectId, candidates: Vec<ObjectId> },
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
    /// CR 603.3d: a triggered ability about to go on the stack has a real
    /// target requirement (`spec` - any `TargetSpec`, single- or multi-word)
    /// and every word has at least its own minimum legal candidates —
    /// `player` (the trigger's own controller) must choose from `candidates`
    /// (one legal-candidate list per word, positionally paired with `spec`'s
    /// own words) and call `submit_trigger_targets` with one flat list
    /// (split internally the same way `CastSpell`/`ActivateAbility`'s own
    /// submitted `targets` already are), then `step()` again. Unlike
    /// `CastSpell`/`ActivateAbility`, nothing initiated this ability with
    /// player-submitted targets already attached — it was detected and
    /// queued by the trigger pipeline itself, so the choice has to happen
    /// here, mid-pipeline, the first time this engine asks for one this way.
    AwaitingTriggerTargets { player: PlayerId, spec: TargetSpec, candidates: Vec<Vec<Target>> },
    /// CR 603.3c: a triggered ability about to go on the stack is itself
    /// modal (`RegisteredAbility.modes` non-empty) — `player` (normally the
    /// trigger's own controller, but CR 700.2e's "chosen by opponent" is
    /// real for a trigger's own modes too, so `player` is the resolved
    /// other-player chooser in that case — see `ProcessState::
    /// AwaitingTriggerModeChoice`'s own `chooser` field) must choose between
    /// `min` and `max` of `modes` and call `Engine::submit_trigger_mode_choice`,
    /// then `step()` again. Raised *before* `AwaitingTriggerTargets` in
    /// `push_ordered_triggers` — CR 603.3c's own mode choice happens before
    /// 603.3d's target choice (mirrors 601.2b before 601.2c for a cast
    /// spell) — so a chosen mode needing a target chains into that same
    /// pause afterward rather than needing its own separate target-selection
    /// machinery.
    AwaitingTriggerModeChoice { player: PlayerId, modes: Vec<SpellMode>, mode_choice: ModeChoice, min: u32, max: u32 },
    /// CR 601.2f-h: `player` cast a spell that's now on the stack
    /// (`stack_id`) but hasn't paid its cost yet — the caller should look
    /// up the cost (e.g. via the card's `CardDefinition.mana_cost` off
    /// `stack_id`), prompt `player` to build a `PaymentPlan`, and call
    /// `submit_payment`, then `step()` again. Same "not a `PlayerAction`/
    /// priority decision" rationale as `AwaitingReplacementChoice` — it has
    /// its own entry point rather than going through `submit_action`.
    AwaitingPayment { player: PlayerId, stack_id: ObjectId },
    /// CR 700.2e: a modal spell (`stack_id`) whose `ModeChoice` defers the
    /// mode choice to a player other than its controller
    /// (`ExactlyChosenByOpponent`/`UpToChosenByOpponent`) reached resolution
    /// with no mode chosen yet — `ResolveStackTop` peeked at the top of the
    /// stack (without popping it) and found `StackObject::pending_mode_choice`
    /// still `Some`. The caller should prompt `player` (the "other player"
    /// CR 700.2e names, resolved at cast time) to choose between `min` and
    /// `max` modes and call `submit_mode_choice`, then `step()` again. Same
    /// "not a `PlayerAction`/priority decision" rationale as
    /// `AwaitingReplacementChoice`.
    AwaitingModeChoice { player: PlayerId, stack_id: ObjectId, min: u32, max: u32 },
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

/// What `Engine::continue_scripted_zone_move` does once its own `remaining`
/// queue drains — added by `phase5/devour-amplify-search-entry` so a CR
/// 702.82a Devour sacrifice raised mid-`Search` (not just mid-`ResolveStackTop`)
/// can finish by resuming the search's own battlefield-entry loop instead of
/// unconditionally finalizing into `ReplacementCheck`. Same "generalize the
/// resume target" shape `SbaResumeThen` already established for
/// `SbaDeath`/`resume_sba_death`.
pub enum ScriptedZoneMoveThen {
    /// The pre-existing behavior: `stack_id` is a resolving spell/ability
    /// (`ResolveStackTop`'s own `top.stack_id`, already popped off
    /// `state.stack` by the time any of this runs — see `continue_scripted_
    /// zone_move`'s own doc comment) — once the queue drains, push
    /// `StackObjectResolved { stack_id }` and hand off to `ReplacementCheck`.
    StackResolution { stack_id: ObjectId },
    /// CR 614.12a wired into `Search` (`phase5/devour-amplify-search-entry`):
    /// a Devour choice raised mid-`continue_search_battlefield_entry` needs
    /// its chosen sacrifices performed via the shared scripted-zone-move
    /// pipeline (so CR 616.1 ordering and dies-triggers apply exactly as
    /// they do for the resolving-spell path), then control must return to
    /// the search's own still-pending candidates instead of finalizing —
    /// `controller`/`remaining`/`destination`/`stack_id`/`shuffle_after` are
    /// exactly `continue_search_battlefield_entry`'s own parameters, carried
    /// through the sacrifice (and any nested CR 616.1 pause it itself
    /// raises) so it can be called again once the sacrifices settle.
    SearchResume {
        controller: PlayerId,
        remaining: VecDeque<ObjectId>,
        destination: SearchDestination,
        stack_id: ObjectId,
        shuffle_after: bool,
    },
}

/// One card still to propose+replacement-check+commit — shared queue entry
/// for `Engine::continue_cost_zone_moves` (a `Sacrifice`/`Discard`/`Exile`
/// `CostPayment`, built by `commands::collect_cost_zone_moves` from an
/// already-validated `PaymentPlan`) and `Engine::continue_discard_moves`
/// (CR 514.1's cleanup hand-size discard, `phase5/replaceable-cleanup-
/// discard`). `from`/`to` are the zones the card is claimed to move between
/// (`Battlefield`->`Graveyard` for `Sacrifice`, `Hand`->`Graveyard` for
/// `Discard`/cleanup discard, `Graveyard`->`Exile` for `Exile`) — the caller
/// already confirmed `id` is really sitting in `from` before queuing it.
#[derive(Clone, Copy, Debug)]
pub struct PendingZoneMove {
    pub id: ObjectId,
    pub from: ZoneKind,
    pub to: ZoneKind,
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
    /// `ResolveStackTop` (`Engine::continue_scripted_zone_move`), or (since
    /// `phase5/devour-amplify-search-entry`) from a Devour sacrifice raised
    /// mid-`Search`. Unlike `ReplacementCheck`, there's no pre-built batch of
    /// proposed events to resume into — each target proposes and
    /// inline-commits its own `ZoneChange` independently (the "already
    /// happened" convention every scripted zone-move op here uses), so
    /// `remaining` is just the rest of this same op's own target list, not
    /// yet proposed. `then` says what to do once every target's been
    /// processed — see `ScriptedZoneMoveThen`'s own doc comment; carried
    /// through this pause unchanged, same as `remaining`. `trailing` (CR
    /// 702.82a, Devour): extra events still owed once every target's
    /// processed — e.g. `submit_devour_choice`'s own `events_so_far` plus a
    /// `CounterAdded`, which must survive a CR 616.1 ordering-choice pause
    /// raised mid-sacrifice just as intact as `remaining`/`then` do. Empty
    /// for every other caller (`Destroy`/`Exile`/the plain
    /// `AbilityEffect::Sacrifice` path), which have nothing to carry
    /// forward.
    ScriptedZoneMove { kind: ScriptedZoneMoveKind, remaining: VecDeque<ObjectId>, then: ScriptedZoneMoveThen, trailing: Vec<GameEventKind> },
    /// Reached from `Engine::continue_cost_zone_moves` (`Engine::
    /// submit_payment`'s own `Sacrifice`/`Discard` handling). Same "each
    /// target proposes+commits independently" shape `ScriptedZoneMove`
    /// already has - `remaining` is the rest of this payment's own
    /// `PendingZoneMove` queue. `payer` is needed for the final
    /// `CostPaid` event once every zone-moving payment's been processed
    /// (`stack_id`'s `costs_paid` was already set before this pause could
    /// first fire, so it doesn't need to be threaded through here too).
    /// `on_paid` carries `PendingPaymentRequest::on_paid` through the pause
    /// unchanged - a cost-bearing mana ability (`PaymentResolution::
    /// ManaAbility`) sacrificing itself as part of its own cost, say, still
    /// needs to know to add its mana once every zone move (including any
    /// nested CR 616.1 choice this pause itself represents) finally settles.
    CostZoneMove {
        remaining: VecDeque<PendingZoneMove>,
        payer: PlayerId,
        stack_id: ObjectId,
        on_paid: PaymentResolution,
        /// CR 702.29c/f: threaded through unchanged from `Engine::
        /// submit_payment`'s own `AdditionalCost::DiscardSelf` check, so a
        /// CR 616.1 ordering choice raised mid-cycling-cost-payment doesn't
        /// lose track of which payment (if any) is the Cycling ability's
        /// own self-discard once this pause resolves.
        cycling_source: Option<ObjectId>,
    },
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
    /// Reached from `Engine::decline_madness_cast`'s own Exile -> Graveyard
    /// proposal (CR 702.35a's "if you don't, put it into your graveyard").
    /// A single proposal, not a queue - same "nothing left to loop over"
    /// shape `Draw` above has. `stack_id`/`events_so_far` are the madness
    /// trigger's own paused-resolution state (this trigger's own
    /// `StackObjectResolved` hasn't been pushed yet) - once this resolves,
    /// both are folded into the normal `ReplacementCheck` tail, same as the
    /// synchronous (no CR 616.1 collision) path already does.
    MadnessDecline { card: ObjectId, stack_id: ObjectId, events_so_far: Vec<GameEventKind> },
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
    /// CR 614/616.1: reached from `permanent_entry::resolve_permanent_entry`'s
    /// own would-be-Battlefield redirect proposal — two or more registered
    /// `ReplaceZoneDestination` effects both matching the same entering
    /// permanent's destination. Unlike every other resume target above,
    /// nothing has been placed on the battlefield (or anywhere else) yet at
    /// this pause point — the popped `StackObject` (`top`) hasn't even had
    /// its zone move committed, so the *entire* object, not just a scalar id,
    /// has to be carried through the pause. `legal_targets` is `top`'s own
    /// CR 608.2b-rechecked target list (needed for the CR 303.4b Aura-attach
    /// step once the destination is finally settled); `events` is whatever
    /// this resolution had already accumulated before the redirect proposal
    /// (empty for every real caller today, carried anyway so the shape
    /// matches every other resume target's own "this run's progress so far"
    /// field). Resumed by `Engine::continue_permanent_entry`, the same
    /// continuation the non-paused path already calls once its own
    /// destination is decided synchronously.
    PermanentEntry { top: StackObject, legal_targets: Vec<Target>, events: Vec<GameEventKind> },
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
    /// replacement/prevention effect (`candidates`, each a `replacement::
    /// ReplacementCandidate`) and `affected_player` must choose which
    /// applies first. `excluded` is `replacement::ReplacementOutcome::
    /// NeedsOrderingChoice`'s own per-event exclusion set (CR 616.1: an
    /// effect applies at most once to a given event) — carried here so
    /// resuming doesn't forget which effects already applied earlier in this
    /// same event's resolution. `resume` says what to do once
    /// `submit_replacement_choice` answers — this pause is reached from two
    /// different pipelines (a command's own events via `ReplacementCheck`,
    /// or a dying creature's zone-change via `sba::resolve_deaths`), and
    /// only `resume` distinguishes them; the pause itself is identical
    /// either way.
    AwaitingReplacementChoice {
        affected_player: PlayerId,
        pending_event: GameEventKind,
        candidates: Vec<replacement::ReplacementCandidate>,
        excluded: Vec<replacement::ReplacementCandidate>,
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
    /// CR 303.4d: paused because `commands::propose_attach` found more than
    /// one simultaneous candidate for a single Aura's attachment. No `then`
    /// continuation, unlike `AwaitingLegendRuleChoice`/
    /// `AwaitingCommanderReturnChoice` above — there's no real producer to
    /// dictate which pipeline this resumes into (see `AttachChoiceError`'s
    /// own doc comment), so `submit_attach_choice` resumes to the generic
    /// `PriorityWindow` once the chosen candidate is attached.
    AwaitingAttachChoice { aura: ObjectId, controller: PlayerId, candidates: Vec<ObjectId> },
    /// CR 303.4f/301.5e/301.6: paused mid-`submit_search_choice` because a
    /// found Aura/Equipment/Fortification card entering the battlefield with
    /// `SearchDestination::Battlefield::enters_attached` set found 2+ legal
    /// attach candidates (`commands::legal_entry_attach_candidates`) —
    /// `controller` must choose which of `candidates` `attaching` becomes
    /// attached to, then call `submit_entry_attach_choice`. Unlike
    /// `AwaitingAttachChoice` above, this *does* have a real continuation to
    /// resume: `stack_id`/`events_so_far`/`remaining`/`destination`/
    /// `shuffle_after` are exactly what `submit_search_choice`'s own
    /// battlefield-entry loop had already built before pausing on this card
    /// - `attaching` itself has already been moved onto the battlefield
    /// (unattached) by the time this pause is raised, same "already a real
    /// permanent, then gets attached" order the cast-time Aura-ETB-attach
    /// path (`ResolveStackTop`) already uses. A separate variant from
    /// `AwaitingAttachChoice` rather than reusing it — that one's
    /// `submit_attach_choice` deliberately has no continuation (nothing
    /// produces it in real gameplay yet, per its own doc comment); giving it
    /// one here would mean threading this search-specific continuation
    /// through a pause type named for a different CR clause. Named for the
    /// generic "entering already attached" event rather than "Aura" once
    /// Equipment/Fortification joined it (`phase5/attach-entry-equipment-
    /// fortification`) — keeping the old Aura-specific name would have
    /// misled the next reader into thinking it was still Aura-only.
    /// `candidates` is `Target`-shaped since `phase5/aura-enters-attached-
    /// player` (an Aura's own candidates can be players, CR 303.4f/b) —
    /// Equipment/Fortification candidates are always `Target::Object`.
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
    /// CR 303.4f/301.5e/301.6, extended to token creation
    /// (`phase5/token-enters-attached`): paused mid-`continue_create_token`
    /// (the resumable loop body `resolve_create_token_effect`'s own
    /// `for`-style loop was factored into) — `commands::legal_entry_attach_
    /// candidates` found 2+ legal attach targets for `attaching` (already a
    /// real battlefield token, unattached, by this point). `spec`/
    /// `copy_snapshot` are exactly what `continue_create_token` needs to
    /// keep building any further tokens still owed by the original
    /// `count`; `remaining_count` is how many of those are left *after*
    /// `attaching` itself (which `submit_token_attach_choice` finishes).
    /// No `remaining: VecDeque<ObjectId>`/`destination`/`shuffle_after`
    /// unlike `AwaitingEntryAttachChoice` above — token creation has no
    /// library queue or shuffle step to resume, just a plain count.
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
    /// CR 702.82a (Devour): paused mid-`ResolveStackTop`, right where
    /// `apply_enters_battlefield_modifiers` would otherwise run for `object`
    /// — `replacement::pending_devour_choice` found a registered Devour
    /// effect on it with 1+ legal creature candidates. `stack_id`/
    /// `events_so_far` are exactly what this resolution had already built
    /// before reaching this point (same "not yet committed, still a local
    /// `Vec`" state `AwaitingScriptChoice` below carries for the identical
    /// reason) — `submit_devour_choice` resumes by performing the chosen
    /// sacrifices (`Engine::continue_scripted_zone_move`, so CR 616.1's
    /// ordering choice and dies-triggers all apply for free), then folding
    /// `events_so_far` plus a `CounterAdded` (if any creatures were
    /// sacrificed) into that same continuation's own tail.
    AwaitingDevourChoice {
        object: ObjectId,
        controller: PlayerId,
        per_creature: i32,
        candidates: Vec<ObjectId>,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    },
    /// CR 702.38a (Amplify): paused mid-`ResolveStackTop`, right where
    /// `apply_enters_battlefield_modifiers` would otherwise run for `object`
    /// — `replacement::pending_amplify_choice` found a registered Amplify
    /// effect on it with 1+ legal hand-card candidates. `stack_id`/
    /// `events_so_far` are exactly what this resolution had already built
    /// before reaching this point, same "not yet committed, still a local
    /// `Vec`" state `AwaitingDevourChoice` above carries. Unlike Devour,
    /// `submit_amplify_choice` needs no `continue_scripted_zone_move` at
    /// all — revealing changes nothing about the chosen cards' zone — it
    /// folds `events_so_far` plus a `CounterAdded` (if any cards were
    /// revealed) straight into the same `StackObjectResolved`/
    /// `ReplacementCheck` tail every non-pausing resolution already ends on.
    AwaitingAmplifyChoice {
        object: ObjectId,
        controller: PlayerId,
        per_card: i32,
        candidates: Vec<ObjectId>,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    },
    /// CR 702.82a (Devour), wired into `Search` (`phase5/devour-amplify-
    /// search-entry`): paused mid-`continue_search_battlefield_entry`, right
    /// where `apply_enters_battlefield_modifiers` would otherwise run for
    /// `object` (a card just fetched onto the battlefield) — same trigger
    /// condition `AwaitingDevourChoice` above checks, just from the second
    /// real producer. A separate variant rather than reusing
    /// `AwaitingDevourChoice` — same "the resume differs, so the pause type
    /// does too" precedent `AwaitingTokenAttachChoice`'s own doc comment
    /// already established relative to `AwaitingEntryAttachChoice`:
    /// `submit_search_devour_choice` must resume back into the search's own
    /// `remaining`/`destination`/`shuffle_after` loop state once the chosen
    /// sacrifices settle, which `AwaitingDevourChoice`'s own resume (straight
    /// to `ReplacementCheck` once `continue_scripted_zone_move`'s queue
    /// drains) has no way to do. `stack_id`/`events_so_far` are exactly what
    /// `continue_search_battlefield_entry` had already built before reaching
    /// this point, same role they play on `AwaitingDevourChoice`;
    /// `remaining`/`destination`/`controller`/`shuffle_after` are that same
    /// function's own parameters, needed to call it again on resume.
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
    /// CR 702.38a (Amplify), wired into `Search`
    /// (`phase5/devour-amplify-search-entry`): same relationship to
    /// `AwaitingAmplifyChoice` that `AwaitingSearchDevourChoice` above has to
    /// `AwaitingDevourChoice` — a separate variant since
    /// `submit_search_amplify_choice` must resume the search's own loop
    /// state instead of finalizing straight to `ReplacementCheck`. Unlike
    /// Devour, no zone move is involved (revealing doesn't move anything),
    /// so the resume is a direct call into `continue_search_battlefield_
    /// entry`, not a `continue_scripted_zone_move` round trip.
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
    /// CR 702.35a (Madness): paused mid-`ResolveStackTop` once the madness
    /// triggered ability's own `MadnessCastOrDiscard` effect located its
    /// live exile-zone descendant — `stack_id`/`events_so_far` are exactly
    /// what this resolution had already built before reaching this point,
    /// same "not yet committed, still a local `Vec`" state its Devour/
    /// Amplify siblings above carry (both `Engine::decline_madness_cast`/
    /// `Engine::submit_madness_cast` push this trigger's own
    /// `StackObjectResolved { stack_id }` into `events_so_far` on resume,
    /// same as every other pause here does for its own triggering
    /// resolution). `card` is the live exile object (not `player`'s
    /// original hand object, which no longer resolves) — `submit_madness_
    /// cast` reads its own `alternative_costs` off this id directly.
    AwaitingMadnessChoice {
        player: PlayerId,
        card: ObjectId,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    },
    /// CR 702.85a (Cascade): paused once `resolve_cascade_effect`'s own
    /// synchronous library walk finished — `exiled` is every card the walk
    /// moved to exile (in walk order), `candidate` the first nonland one
    /// with mana value less than the cascade spell's own (`None` if the
    /// library ran dry first). `stack_id`/`events_so_far` are this trigger's
    /// own paused-resolution state, same role they play on
    /// `AwaitingMadnessChoice` above.
    AwaitingCascadeChoice {
        player: PlayerId,
        candidate: Option<ObjectId>,
        exiled: Vec<ObjectId>,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
    },
    /// CR 707.6/614.12a (Clone itself): paused mid-`ResolveStackTop`, right
    /// where `apply_enters_battlefield_modifiers` would otherwise run for
    /// `object` — `replacement::pending_copy_choice` found a registered
    /// `EntersAsCopy` effect on it with 1+ legal battlefield-creature
    /// candidates. `stack_id`/`events_so_far` are exactly what this
    /// resolution had already built before reaching this point, same
    /// "not yet committed, still a local `Vec`" state its siblings above
    /// carry. `exceptions` and `colors_spent` are carried through the pause
    /// too — `exceptions` for `submit_copy_choice` to apply if a copy is
    /// chosen, `colors_spent` (derived from the entering spell's own real
    /// `costs_paid` at the point this pause is raised) so a copied Sunburst
    /// creature still gets a real count once `apply_enters_battlefield_
    /// modifiers` runs on resume.
    ///
    /// `carry_forward_if_declined` (CR 707.9e, `phase5/chained-copy-effects`)
    /// is `None` for every *first* pause (the object hasn't copied anything
    /// yet, so there's nothing to supersede). It's only ever `Some` when
    /// `submit_copy_choice` itself detects a *chain* — the just-copied
    /// definition has its own `EntersAsCopy` — and re-enters this same
    /// variant instead of finishing: it holds the outer round's own
    /// already-applied-copy-but-not-yet-applied-additional-effect
    /// `CopyExceptions`, to be applied after all if this new pause's choice
    /// is declined (CR 707.9e's own condition, "another copy effect is
    /// applied... after," was never actually met in that case). If instead
    /// this pause's choice picks a real target, the outer round's exception
    /// is conclusively superseded and `submit_copy_choice` discards this
    /// field for good, keeping only its own newly-applied round's exceptions
    /// as the next `carry_forward_if_declined` candidate.
    AwaitingCopyChoice {
        object: ObjectId,
        controller: PlayerId,
        exceptions: CopyExceptions,
        candidates: Vec<ObjectId>,
        stack_id: ObjectId,
        events_so_far: Vec<GameEventKind>,
        colors_spent: ColorSet,
        carry_forward_if_declined: Option<CopyExceptions>,
        /// CR 107.3b/707.9e (`phase5/altered-ego-x-counters`): `object`'s own
        /// paid X (`StackObject.x_value` at the point this pause was first
        /// raised) - resolves `CounterAmount::X` in whichever round's
        /// exceptions end up applied. `object` never changes across a
        /// chained re-pause (only the *candidate* being copied does), so
        /// this is captured once and carried through unchanged, same
        /// invariant-through-the-chain treatment `colors_spent` already
        /// gets.
        x_value: Option<u32>,
    },
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
    /// CR 603.3d: paused mid-`PutTriggersApnap`'s own final push loop
    /// (`Engine::push_ordered_triggers`) because `trigger`'s own ability
    /// needs a real target chosen before it can be pushed to the stack —
    /// `spec`/`candidates` mirror `StepStatus::AwaitingTriggerTargets`
    /// exactly (see that variant's own doc comment); `remaining` is every
    /// other trigger in this same push run still to process afterward, same
    /// "queue survives the pause" convention `AwaitTriggerOrderChoice`'s own
    /// `pending_groups` already established.
    AwaitingTriggerTargets {
        trigger: PendingTrigger,
        spec: TargetSpec,
        candidates: Vec<Vec<Target>>,
        remaining: VecDeque<PendingTrigger>,
        /// `None` for the plain (non-modal) path this variant originally
        /// shipped with - `push_trigger_stack_object` falls back to the
        /// ability's own `effect` field, unchanged. `Some(effect)` means
        /// this pause was reached via `Engine::submit_trigger_mode_choice`
        /// (CR 603.3c) instead - `effect` is the already-combined modal
        /// effect (via `commands::combine_chosen_modes`), which must be
        /// used verbatim instead of re-deriving from the ability (a modal
        /// `RegisteredAbility`'s own `effect` field is a meaningless
        /// placeholder - see `RegisteredAbility::modes`'s own doc comment).
        /// The inner `Option` is real, not a second "was this set" flag - a
        /// single script-driven chosen mode can legitimately combine to
        /// `effect: None` (CR 603.3c has no "no effect at all" restriction,
        /// same as `SpellMode::effect`/`RegisteredAbility::effect` already
        /// allow).
        modal_effect: Option<Option<AbilityEffect>>,
    },
    /// CR 603.3c: paused mid-`PutTriggersApnap`'s own final push loop
    /// (`Engine::push_ordered_triggers`) because `trigger`'s own ability is
    /// modal (`modes` — its `RegisteredAbility::modes`, non-empty) and
    /// `chooser` hasn't announced a mode choice yet. Checked *before* the
    /// `AwaitingTriggerTargets` check in the same loop — `modes`/
    /// `mode_choice` are the ability's own fields, cloned out the same way
    /// `AwaitingTriggerTargets`'s own `spec` is; `remaining` is every other
    /// trigger in this same push run still to process afterward, same
    /// "queue survives the pause" convention every other trigger-pipeline
    /// pause already established.
    ///
    /// `chooser`: `trigger.controller` for `ModeChoice::Exactly`/`UpTo`/
    /// `*Repeatable` — but CR 700.2e's "a player other than their controller
    /// chooses" (`ExactlyChosenByOpponent`/`UpToChosenByOpponent`) is real
    /// for a trigger's own modes now too (`phase5/trigger-mode-chosen-by-
    /// opponent`), so `push_ordered_triggers` resolves the real chooser
    /// *before* constructing this pause rather than the dispatch side
    /// assuming `trigger.controller` unconditionally — same "resolved once,
    /// at the point the choice is raised" shape `CastSpell`'s own
    /// `PendingModeChoice::chooser` already established for a spell.
    AwaitingTriggerModeChoice {
        trigger: PendingTrigger,
        modes: Vec<SpellMode>,
        mode_choice: ModeChoice,
        remaining: VecDeque<PendingTrigger>,
        chooser: PlayerId,
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
    AwaitingPayment {
        stack_id: ObjectId,
        payer: PlayerId,
        cost: CostSpec,
        source: ObjectId,
        on_paid: PaymentResolution,
    },
    /// CR 700.2e: paused because `ProcessState::ResolveStackTop` peeked at
    /// the top of the stack (without popping it) and found a `StackObject`
    /// whose `pending_mode_choice` is still `Some` — `chooser` must submit
    /// between `min` and `max` mode indices via `Engine::submit_mode_choice`
    /// before this spell can actually resolve. See `StackObject::
    /// pending_mode_choice`'s own doc comment for how the pause is raised
    /// and answered.
    AwaitingModeChoice { stack_id: ObjectId, chooser: PlayerId, min: u32, max: u32 },
    /// CR 104.2a/104.4a: at most one non-eliminated player remains —
    /// `winner` is that player, or `None` if the last remaining players were
    /// eliminated simultaneously (a draw). Terminal: `step()` keeps
    /// returning `StepStatus::GameOver` without mutating anything further,
    /// same "frozen once reached" contract `Error` already has, and
    /// `submit_action` already rejects new actions outside `PriorityWindow`
    /// so nothing more can be submitted either.
    GameOver { winner: Option<PlayerId> },
}
