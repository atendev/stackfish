//! The step-able rules processor: the `Engine` struct, its pipeline-state
//! dispatcher (`advance`), and the pieces that don't belong to any one
//! functional cluster (error types, struct plumbing) — split out of the
//! original single-file `engine.rs`. Every non-trivial `ProcessState` match
//! arm below delegates to a `pub(super)`/`pub(in crate::engine)` method
//! defined in a sibling module; see each sibling's own module doc comment
//! for which cluster it owns. `ProcessState::ResolveStackTop` (CR 608.2m/
//! 601.2c/602.2b resolution, the single largest and most complex arm in the
//! whole engine) delegates into the `resolve/` subdirectory rather than a
//! single sibling file — see `resolve`'s own module doc comment.

use crate::commands::{CommandError, PendingPaymentRequest};
use crate::engine_context::{PipelineContext, PipelineExit};
use crate::script::{ScriptError, ScriptRuntime};
use crate::types::ability::{AbilityEffect, ConditionSpec, RegisteredAbility};
use crate::types::card::ScriptReference;
use crate::types::config::GameSettings;
use crate::types::cost::CostSpec;
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ScriptHandle};
use crate::types::turn::Phase;

mod action;
mod choice_answers;
mod event_pipeline;
mod library_choices;
mod payment;
mod pregame;
mod process_state;
mod replacement_resume;
mod resolve;
mod triggers_pipeline;
mod turn;
#[cfg(test)]
mod tests;

pub use process_state::*;

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

/// Why `submit_attach_choice` rejected an answer to a CR 303.4d "an Aura
/// can't enchant more than one object or player" choice. Dormant as of
/// this slice — no effect in this engine ever computes more than one
/// simultaneous attach candidate for one Aura (see `commands::propose_attach`'s
/// own doc comment), so this pause is only ever reached by a test
/// constructing `ProcessState::AwaitingAttachChoice` directly, not through
/// real gameplay.
#[derive(Debug)]
pub enum AttachChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingAttachChoice`.
    NotAwaitingChoice,
    /// `chosen` wasn't one of the offered `candidates`.
    InvalidCandidate,
}

/// Why `submit_discard_choice` rejected an answer to a pending CR
/// 701.9/701.9b targeted-discard choice (`AbilityEffect::Discard`).
#[derive(Debug)]
pub enum DiscardChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingDiscardChoice`.
    NotAwaitingChoice,
    /// The submitting player isn't the one this pause is actually asking -
    /// only the targeted player may answer their own discard choice.
    NotCurrentPlayer,
    /// `chosen` wasn't one of the offered `candidates` (that player's own
    /// current hand).
    InvalidCandidate,
}

/// Why `submit_devour_choice` rejected an answer to a CR 702.82a Devour
/// choice.
#[derive(Debug)]
pub enum DevourChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingDevourChoice`.
    NotAwaitingChoice,
    /// One of `sacrificed` wasn't one of the offered `candidates`.
    InvalidCandidate,
    /// The same id appeared twice in `sacrificed` — each creature can only
    /// be sacrificed once, same "no duplicate choice" stance the rest of
    /// this engine's multi-select choices take.
    DuplicateCandidate,
}

/// Why `submit_amplify_choice` rejected an answer to a CR 702.38a Amplify
/// choice. Same shape as `DevourChoiceError` — see that type's own doc
/// comment.
#[derive(Debug)]
pub enum AmplifyChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingAmplifyChoice`.
    NotAwaitingChoice,
    /// One of `revealed` wasn't one of the offered `candidates`.
    InvalidCandidate,
    /// The same id appeared twice in `revealed` — each card can only be
    /// revealed once.
    DuplicateCandidate,
}

/// Why `decline_madness_cast`/`submit_madness_cast` rejected an answer to a
/// CR 702.35a Madness choice.
#[derive(Debug)]
pub enum MadnessChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingMadnessChoice`.
    NotAwaitingChoice,
    /// `card` has no `AlternativeCost::Replacement { from_zone: Exile, .. }`
    /// entry — shouldn't happen for a real madness card (its own static
    /// ability is what put it in this pause in the first place), but a
    /// defensive check rather than an `unreachable!` since nothing actually
    /// enforces the two halves of Madness are always declared together.
    NoMadnessCost,
    /// `commands::execute_command` rejected the synthetic `CastSpell` (bad
    /// targets, unpayable cost, ...) — the pause is left untouched so the
    /// caller can retry with different targets/modes or call
    /// `decline_madness_cast` instead.
    IllegalCast(CommandError),
}

/// Why `decline_cascade_cast`/`submit_cascade_cast` rejected an answer to a
/// CR 702.85a Cascade choice. Same shape as `MadnessChoiceError` — see that
/// type's own doc comment.
#[derive(Debug)]
pub enum CascadeChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingCascadeChoice`.
    NotAwaitingChoice,
    /// `submit_cascade_cast` was called but the library walk found no
    /// qualifying candidate (`candidate: None`) — nothing to cast, only
    /// `decline_cascade_cast` is legal.
    NoCandidate,
    /// `commands::execute_command` rejected the synthetic `CastSpell` (bad
    /// targets, ...) — the pause is left untouched so the caller can retry
    /// with different targets/modes or call `decline_cascade_cast` instead.
    IllegalCast(CommandError),
}

/// Why `submit_unless_paid_payment`/`decline_unless_paid` rejected an answer
/// to a pending CR 118.12/118.12a "unless paid" pause
/// (`AbilityEffect::UnlessPaid` — Ward/Mana Leak/Echo-style).
#[derive(Debug)]
pub enum UnlessPaidError {
    /// Engine isn't sitting in `ProcessState::AwaitingUnlessPaidChoice`.
    NotAwaitingChoice,
    /// The submitting player isn't the current front of `payer_queue` — only
    /// whoever is currently being asked may answer.
    NotCurrentPayer,
    /// The submitted `PaymentPlan` doesn't validate against the pending
    /// `cost` — the pause is left untouched, same no-partial-payment stance
    /// `submit_payment`'s own `PaymentError::InvalidPlan` already takes, so
    /// the caller can retry with a different plan or call
    /// `decline_unless_paid` instead.
    InvalidPlan,
}

/// Why `submit_copy_choice` rejected an answer to a CR 707.6/614.12a "enter
/// as a copy" choice (Clone itself). No `DuplicateCandidate` variant, unlike
/// `DevourChoiceError`/`AmplifyChoiceError` — this is "which one, if any,"
/// an `Option<ObjectId>`, not a multi-select `Vec` that duplicates could
/// even appear in.
#[derive(Debug)]
pub enum CopyChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingCopyChoice`.
    NotAwaitingChoice,
    /// `Some(id)` wasn't one of the offered `candidates`.
    InvalidCandidate,
}

/// Why `submit_mode_choice` rejected an answer to a CR 700.2e "chosen by
/// opponent" modal choice.
#[derive(Debug)]
pub enum ModeChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingModeChoice`.
    NotAwaitingChoice,
    /// `modes.len()` fell outside the announced `min..=max` range.
    InvalidCount,
    /// The same mode index appeared twice - CR 700.2e doesn't grant CR
    /// 700.2d's "same mode twice" exception on its own; combining both stays
    /// unmodeled (see `ModeChoice`'s own doc comment).
    DuplicateMode,
    /// One of `modes` wasn't a valid index into the card's own mode list.
    NoSuchMode,
    /// 2+ modes chosen, but combining them hit the same synchronous-effect-
    /// kind narrowing `commands::is_modal_resolvable` already enforces for
    /// the caster-chosen variants (`ModeChoice::Exactly`/`UpTo`/
    /// `*Repeatable`).
    UnsupportedModeCombination,
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

/// Why `submit_trigger_targets` rejected an answer to a CR 603.3d
/// triggered-ability target choice.
#[derive(Debug)]
pub enum TriggerTargetsError {
    /// Engine isn't sitting in `ProcessState::AwaitingTriggerTargets`.
    NotAwaitingChoice,
    /// `chosen` failed `commands::validate_single_target_requirement` -
    /// wrong count, a duplicate, or an id/player not among the offered
    /// `candidates`.
    InvalidTargets,
}

/// Why `submit_trigger_mode_choice` rejected an answer to a CR 603.3c modal
/// triggered-ability mode choice — same shape `ModeChoiceError` already
/// takes for a spell's own CR 700.2e "chosen by opponent" pause.
#[derive(Debug)]
pub enum TriggerModeChoiceError {
    /// Engine isn't sitting in `ProcessState::AwaitingTriggerModeChoice`.
    NotAwaitingChoice,
    /// `modes.len()` fell outside the announced `min..=max` range.
    InvalidCount,
    /// The same mode index appeared twice and `mode_choice` isn't one of
    /// CR 700.2d's `*Repeatable` variants.
    DuplicateMode,
    /// One of `modes` wasn't a valid index into the ability's own mode list.
    NoSuchMode,
    /// 2+ modes chosen, but combining them hit `commands::
    /// combine_chosen_modes`'s own synchronous-effect-kind/multi-target
    /// narrowing (see that function's own doc comment).
    UnsupportedModeCombination,
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

/// One frame of `Engine::suspended_payments` — everything needed to resume
/// an outer `ProcessState::AwaitingPayment` once the nested mana-ability
/// activation that paused it (CR 601.2g/605.3a) fully settles. `request`
/// carries the same fields `AwaitingPayment` itself does; `pipeline_context`/
/// `pipeline_exit` are the outer run's own values, saved off because
/// `handle_pending_action` unconditionally overwrites both live fields the
/// moment the nested `PendingAction` runs (see `Engine::pipeline_context`'s
/// own doc comment) - without this, the outer run's trigger-collection
/// window and post-settle destination would silently pick up the nested
/// run's values instead of its own once resumed.
struct SuspendedPayment {
    request: PendingPaymentRequest,
    pipeline_context: Option<PipelineContext>,
    pipeline_exit: PipelineExit,
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
    /// CR 601.2g/605.3a: a mana ability may be activated "even if it's in
    /// the middle of casting or resolving a spell or activating or
    /// resolving an ability" that itself requires a mana payment - so
    /// `submit_action` (see its own doc comment) may accept a nested
    /// `ActivateAbility` for a mana ability while `process_state` is already
    /// `AwaitingPayment` for something else. Each entry is one suspended
    /// outer payment, LIFO - a real-cost (Treasure-style) mana ability
    /// activated mid-payment can itself pause on a fresh `AwaitingPayment`
    /// for its own cost, nesting one level deeper. Popped by
    /// `resume_suspended_payment_or` (the one place both `end_pipeline_run`'s
    /// `BackToPriority` arm and `handle_pending_action`'s error arm check
    /// before falling back to their old unconditional behavior) once the
    /// nested action's own pipeline run fully settles.
    suspended_payments: Vec<SuspendedPayment>,
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
            departed_triggered_abilities: Vec::new(),
            next_command_id: 0,
            pending_payment: None,
            suspended_payments: Vec::new(),
            settings: GameSettings::default(),
            pregame_setup_done: true,
            script_runtime: ScriptRuntime::new(),
        }
    }

    pub fn state(&self) -> &GameState {
        &self.state
    }

    /// CR 601.2f-h/605.3b: the cost currently awaiting `submit_payment`, and
    /// the object it belongs to (`PendingPaymentRequest::source`'s own
    /// doc comment - the `AdditionalCost::TapSelf` target, not necessarily
    /// `stack_id`) - `None` outside `ProcessState::AwaitingPayment`. Exposes
    /// exactly the `CostSpec` `PaymentPlan::validate` itself already checks
    /// a submitted plan against, so a caller building a plan on the fly (an
    /// auto-pay convenience, say) has one real source of truth instead of a
    /// second, potentially-drifting way to re-derive the pending cost.
    pub fn pending_payment_cost(&self) -> Option<(&CostSpec, ObjectId)> {
        match &self.process_state {
            ProcessState::AwaitingPayment { cost, source, .. } => Some((cost, *source)),
            _ => None,
        }
    }

    /// The real script bridge exists (`run_card_script_for` below, via
    /// `Game.CreateTriggeredAbility`/`Game.CreateActivatedAbility`/`Game.
    /// CreateManaAbility`) but only self-registers as an object's own script
    /// runs on zone entry - callers building a game from `CardDefinition`s
    /// outside the manual scenario-fixture path (e.g. `new_game`, before
    /// anything has entered a zone that triggers a script run) still need a
    /// way to seed `RegisteredAbility` entries directly.
    pub fn state_mut(&mut self) -> &mut GameState {
        &mut self.state
    }

    /// Runs `object_id`'s card script now that it has just entered the hand
    /// zone (CR 400.7 fresh id already minted by the caller), self-
    /// registering any ability that — like CR 702.29 Cycling — only
    /// functions from hand. The hand-zone counterpart to `action.rs`'s own
    /// `battlefield_entries` loop, which does the identical `run_card_script`
    /// call for a Hand->Battlefield move; this one covers every real
    /// Library->Hand producer instead (draw, opening hand, search-to-hand) —
    /// see each call site for why that producer needs it. Same "silently
    /// drop a script error" gap `battlefield_entries`' own call already
    /// carries, not new here.
    ///
    /// Deliberately narrow: only Library->Hand entry calls this. A card
    /// leaving hand for any other zone still purges its `RegisteredAbility`
    /// entries via the pre-existing `retire_object` convention, same as
    /// every other ability in this engine — CR 702.29b's "the ability
    /// continues to exist... in all other zones" has no real consumer today
    /// (no filter/effect in this engine ever queries "does this object have
    /// an activated ability"), so that half is a named, unobserved gap
    /// rather than something this method attempts.
    pub(in crate::engine) fn run_card_script_for(&mut self, object_id: ObjectId) {
        if let Some(definition) = self.state.object(object_id).map(|o| o.definition.clone()) {
            let result = self.script_runtime.run_card_script(&mut self.state, &definition, object_id);
            self.record_script_error(object_id, &definition.script_ref, result);
        }
    }

    /// Stores `request` as `pending_payment`, and - if it's a spell that was
    /// just put on the stack - runs its card script for the stack zone first.
    /// CR 601.2a: a spell's abilities (e.g. Cascade's "when you cast this
    /// spell") exist from the moment it's on the stack, before it ever
    /// resolves, so a script must be able to self-register them here. Safe to
    /// run in addition to the hand-entry and battlefield-entry runs: every
    /// script-registered ability is dropped again by
    /// `commands::remap_abilities` on the next zone change, and
    /// `run_card_script` never lets a non-battlefield run register a static
    /// effect, replacement effect or modifier.
    ///
    /// The one funnel for all four real `CastSpell` producers (the normal
    /// action, Madness, Cascade, and a script-requested cast). An activated
    /// ability shares `PaymentResolution::StackObject`, so the stack entry's
    /// own kind is what gates this.
    pub(in crate::engine) fn set_pending_payment(&mut self, request: Option<PendingPaymentRequest>) {
        if let Some(request) = &request {
            let is_spell = matches!(request.on_paid, crate::commands::PaymentResolution::StackObject)
                && self
                    .state
                    .stack
                    .iter()
                    .any(|s| s.stack_id == request.stack_id && matches!(s.kind, crate::StackEntryKind::Spell));
            if is_spell {
                self.run_card_script_for(request.stack_id);
            }
        }
        self.pending_payment = request;
    }

    /// Logs a `run_card_script` failure as a `GameEventKind::ScriptError`
    /// ledger event instead of silently dropping it (the "a script error
    /// here is silently dropped" gap named across `engine/action.rs`,
    /// `engine/choice_answers.rs`, `engine/payment.rs`, `engine/resolve/
    /// tokens.rs`, `engine/resolve/misc_effects.rs`, `engine/
    /// library_choices.rs`). Mirrors the exact message convention
    /// `triggers::collect_triggers`'s own `invoke_condition` call already
    /// established for a script error — the caller's existing conservative
    /// fallback (proceed as though nothing happened) is unchanged; this
    /// only makes a *real* failure observable in the ledger rather than
    /// invisible. No rollback of whatever the call already mutated is
    /// attempted — same non-atomicity `StepStatus::Error`'s own doc comment
    /// already admits for `execute_command` itself.
    ///
    /// Deliberately skips logging when `script_ref` is `ByKey` or an empty
    /// `Inline` source — `run_card_script`'s own doc comment documents
    /// `ByKey` as *always* erroring in this engine ("real integration is
    /// expected to always hand this engine an already-resolved `Inline`
    /// source"), and the scenario-TOML loader defaults an unset `script`
    /// field to `Inline("")`, which fails to eval to a table the same way.
    /// Both are the ordinary, deliberate shape of a "vanilla card, no real
    /// script" fixture used throughout this engine's own test suite (e.g.
    /// `stackfish_testutil::minimal_card_definition`'s `ByKey` default) —
    /// not a real script bug, and every permanent that resolves onto the
    /// battlefield runs through here (`payment.rs`'s `run_script` flag), so
    /// logging unconditionally would flood the ledger with a "failure" for
    /// nearly every existing fixture rather than surfacing the genuine
    /// "a real script threw" case this cluster is actually about.
    pub(in crate::engine) fn record_script_error<T>(
        &mut self,
        source: ObjectId,
        script_ref: &ScriptReference,
        result: Result<T, ScriptError>,
    ) {
        let has_real_script = matches!(script_ref, ScriptReference::Inline(s) if !s.is_empty());
        if has_real_script {
            if let Err(e) = result {
                self.log_events(vec![GameEventKind::ScriptError {
                    message: format!("card script error on source {source:?}: {e:?}"),
                }]);
            }
        }
    }

    /// Loads a standalone Lua callback (see `ScriptRuntime::load_function`)
    /// into this `Engine`'s script runtime, returning a `ScriptHandle` a
    /// caller can then put on a `RegisteredAbility`'s `condition`/`operation`
    /// field directly. For callers hand-building a `RegisteredAbility` via
    /// `state_mut` rather than through a real card script's own `Game.
    /// CreateTriggeredAbility`/etc. call — same "no self-registration
    /// happened, so the caller wires it up by hand" shape `state_mut`'s own
    /// doc comment already describes for `state.abilities`.
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

    /// A card's own base target requirement (an Aura's "Enchant [quality]"
    /// line, etc.), as declared by its script's optional `target_requirement()`
    /// function — see `ScriptRuntime::build_target_requirement`'s own doc
    /// comment for the full convention, including the scripted-predicate
    /// `filter` form. `&self`, not `&mut self`, despite that form needing to
    /// register a function in the script registry — same `Rc<RefCell<
    /// ScriptRegistry>>` interior mutability every other builder method
    /// already relies on. Whatever `ScriptHandle` this returns is only ever
    /// valid against *this* `Engine`'s own `script_runtime` — the same
    /// caller trap `docs/implementation/05-script-bridge.md` already names
    /// for a custom-function handle in general.
    pub fn build_target_requirement(
        &self,
        source: &str,
    ) -> Result<Option<crate::types::target::TargetSpec>, ScriptError> {
        self.script_runtime.build_target_requirement(source)
    }

    /// A plain spell's own resolution effect (Armageddon's "Destroy all
    /// lands," etc.), as declared by its script's optional `operation()`
    /// function — see `ScriptRuntime::build_operation`'s own doc comment for
    /// the full convention.
    pub fn build_operation(
        &self,
        source: &str,
    ) -> Result<Option<(Option<AbilityEffect>, ScriptHandle)>, ScriptError> {
        self.script_runtime.build_operation(source)
    }

    /// A card's own alternative costs (flashback/escape-style, CR 601.2b), as
    /// declared by its script's optional `alternative_costs()` function — see
    /// `ScriptRuntime::build_alternative_costs`'s own doc comment for the
    /// full convention.
    pub fn build_alternative_costs(
        &self,
        source: &str,
    ) -> Result<Vec<crate::types::cost::AlternativeCost>, ScriptError> {
        self.script_runtime.build_alternative_costs(source)
    }

    /// A card's own Buyback cost (CR 702.27a), as declared by its script's
    /// optional `buyback_cost()` function — see `ScriptRuntime::
    /// build_buyback_cost`'s own doc comment for the full convention.
    pub fn build_buyback_cost(&self, source: &str) -> Result<Option<crate::types::mana::ManaCost>, ScriptError> {
        self.script_runtime.build_buyback_cost(source)
    }

    /// Diagnostic for tests/replay tooling: whether `pipeline_context` is
    /// currently set. Should be `false` whenever `process_state` is
    /// `PriorityWindow` - `end_pipeline_run` is the only place that clears
    /// it, and `PriorityWindow` is only ever reached through there (or
    /// `AdvanceTurnStep`, which also doesn't set it).
    pub fn pipeline_context_active(&self) -> bool {
        self.pipeline_context.is_some()
    }

    pub fn settings(&self) -> &GameSettings {
        &self.settings
    }

    pub fn step(&mut self) -> StepResult {
        let events_start = self.state.ledger.len();
        let status = self.advance();
        StepResult { events_added: events_start..self.state.ledger.len(), status }
    }

    /// Wraps each `GameEventKind` into a `GameEvent` (allocating its
    /// `event_id`/`sequence_no`) and pushes it to the ledger. Pulled out of
    /// the `AdvanceTurnStep` arm so the same wrapping logic isn't repeated
    /// for the base step transition, `untap_step`, and `draw_step` alike.
    ///
    /// `pub(in crate::engine)`: also called from `turn.rs` and
    /// `triggers_pipeline.rs`.
    pub(in crate::engine) fn log_events(&mut self, events: Vec<GameEventKind>) {
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

            ProcessState::PendingAction(action) => self.handle_pending_action(action),

            ProcessState::ResolveStackTop => self.resolve_stack_top(),

            ProcessState::ReplacementCheck { remaining, checked } => {
                self.resolve_replacement_check_state(remaining, checked)
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

            ProcessState::AwaitingAttachChoice { aura, controller, candidates } => {
                // Nothing to do until submit_attach_choice answers - same
                // "put the exact same state back" pattern every other
                // choice-pause above already uses.
                let status =
                    StepStatus::AwaitingAttachChoice { player: controller, aura, candidates: candidates.clone() };
                self.process_state = ProcessState::AwaitingAttachChoice { aura, controller, candidates };
                status
            }

            ProcessState::AwaitingEntryAttachChoice {
                attaching, controller, candidates, stack_id, events_so_far, remaining, destination, shuffle_after,
            } => {
                // Nothing to do until submit_entry_attach_choice answers -
                // same "put the exact same state back" pattern every other
                // choice-pause above already uses.
                let status = StepStatus::AwaitingEntryAttachChoice {
                    player: controller,
                    attaching,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingEntryAttachChoice {
                    attaching, controller, candidates, stack_id, events_so_far, remaining, destination, shuffle_after,
                };
                status
            }

            ProcessState::AwaitingTokenAttachChoice {
                attaching, controller, candidates, stack_id, events_so_far, remaining_count, spec, copy_snapshot,
            } => {
                // Nothing to do until submit_token_attach_choice answers -
                // same "put the exact same state back" pattern every other
                // choice-pause above already uses.
                let status = StepStatus::AwaitingTokenAttachChoice {
                    player: controller,
                    attaching,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingTokenAttachChoice {
                    attaching, controller, candidates, stack_id, events_so_far, remaining_count, spec, copy_snapshot,
                };
                status
            }

            ProcessState::AwaitingDevourChoice { object, controller, per_creature, candidates, stack_id, events_so_far } => {
                // Nothing to do until submit_devour_choice answers - same
                // "put the exact same state back" pattern every other
                // choice-pause above already uses.
                let status = StepStatus::AwaitingDevourChoice {
                    player: controller,
                    object,
                    per_creature,
                    candidates: candidates.clone(),
                };
                self.process_state =
                    ProcessState::AwaitingDevourChoice { object, controller, per_creature, candidates, stack_id, events_so_far };
                status
            }

            ProcessState::AwaitingAmplifyChoice { object, controller, per_card, candidates, stack_id, events_so_far } => {
                // Nothing to do until submit_amplify_choice answers - same
                // "put the exact same state back" pattern every other
                // choice-pause above already uses.
                let status = StepStatus::AwaitingAmplifyChoice {
                    player: controller,
                    object,
                    per_card,
                    candidates: candidates.clone(),
                };
                self.process_state =
                    ProcessState::AwaitingAmplifyChoice { object, controller, per_card, candidates, stack_id, events_so_far };
                status
            }

            ProcessState::AwaitingMadnessChoice { player, card, stack_id, events_so_far } => {
                // Nothing to do until decline_madness_cast/submit_madness_cast
                // answers - same "put the exact same state back" pattern
                // every other choice-pause above already uses.
                let status = StepStatus::AwaitingMadnessChoice { player, card };
                self.process_state = ProcessState::AwaitingMadnessChoice { player, card, stack_id, events_so_far };
                status
            }

            ProcessState::AwaitingCascadeChoice { player, candidate, exiled, stack_id, events_so_far } => {
                // Nothing to do until decline_cascade_cast/submit_cascade_cast
                // answers - same "put the exact same state back" pattern
                // every other choice-pause above already uses.
                let status = StepStatus::AwaitingCascadeChoice { player, candidate };
                self.process_state =
                    ProcessState::AwaitingCascadeChoice { player, candidate, exiled, stack_id, events_so_far };
                status
            }

            ProcessState::AwaitingUnlessPaidChoice { payer_queue, cost, otherwise, target_id, source, stack_id, events_so_far } => {
                // Nothing to do until decline_unless_paid/
                // submit_unless_paid_payment answers - same "put the exact
                // same state back" pattern every other choice-pause above
                // already uses. `payer_queue` is never empty while this
                // pause is live - decline_unless_paid dispatches `otherwise`
                // and leaves ProcessState instead of re-raising once it
                // would go empty.
                let payer = *payer_queue.front().expect("payer_queue is never empty while paused");
                let status = StepStatus::AwaitingUnlessPaidChoice { payer, cost: cost.clone(), otherwise: otherwise.clone() };
                self.process_state = ProcessState::AwaitingUnlessPaidChoice {
                    payer_queue,
                    cost,
                    otherwise,
                    target_id,
                    source,
                    stack_id,
                    events_so_far,
                };
                status
            }

            ProcessState::AwaitingDiscardChoice { player, candidates, remaining_count, stack_id, events_so_far } => {
                // Nothing to do until submit_discard_choice answers - same
                // "put the exact same state back" pattern every other
                // choice-pause above already uses.
                let status = StepStatus::AwaitingDiscardChoice { player, candidates: candidates.clone(), remaining_count };
                self.process_state =
                    ProcessState::AwaitingDiscardChoice { player, candidates, remaining_count, stack_id, events_so_far };
                status
            }

            ProcessState::AwaitingSearchDevourChoice {
                object, controller, per_creature, candidates, stack_id, events_so_far, remaining, destination, shuffle_after,
            } => {
                // Nothing to do until submit_search_devour_choice answers -
                // same "put the exact same state back" pattern every other
                // choice-pause above already uses.
                let status = StepStatus::AwaitingSearchDevourChoice {
                    player: controller,
                    object,
                    per_creature,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingSearchDevourChoice {
                    object, controller, per_creature, candidates, stack_id, events_so_far, remaining, destination, shuffle_after,
                };
                status
            }

            ProcessState::AwaitingSearchAmplifyChoice {
                object, controller, per_card, candidates, stack_id, events_so_far, remaining, destination, shuffle_after,
            } => {
                // Nothing to do until submit_search_amplify_choice answers -
                // same "put the exact same state back" pattern every other
                // choice-pause above already uses.
                let status = StepStatus::AwaitingSearchAmplifyChoice {
                    player: controller,
                    object,
                    per_card,
                    candidates: candidates.clone(),
                };
                self.process_state = ProcessState::AwaitingSearchAmplifyChoice {
                    object, controller, per_card, candidates, stack_id, events_so_far, remaining, destination, shuffle_after,
                };
                status
            }

            ProcessState::AwaitingCopyChoice { object, controller, exceptions, candidates, stack_id, events_so_far, colors_spent, carry_forward_if_declined, x_value } => {
                // Nothing to do until submit_copy_choice answers - same
                // "put the exact same state back" pattern every other
                // choice-pause above already uses.
                let status = StepStatus::AwaitingCopyChoice { player: controller, object, candidates: candidates.clone() };
                self.process_state = ProcessState::AwaitingCopyChoice {
                    object, controller, exceptions, candidates, stack_id, events_so_far, colors_spent, carry_forward_if_declined, x_value,
                };
                status
            }

            ProcessState::AwaitingScriptChoice { thread, request, stack_id, source, controller, events_so_far, each_player } => {
                // Nothing to do until submit_script_choice answers - same
                // "put the exact same state back" pattern every other
                // choice-pause above already uses.
                let status = StepStatus::AwaitingScriptChoice { player: controller, request: request.clone() };
                self.process_state = ProcessState::AwaitingScriptChoice {
                    thread, request, stack_id, source, controller, events_so_far, each_player,
                };
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

            ProcessState::CommitEvents { checked } => self.resolve_commit_events_state(checked),

            ProcessState::AwaitingPayment { stack_id, payer, cost, source, on_paid } => {
                // Nothing to do until submit_payment answers - put the
                // exact same state back and tell the caller who needs to be
                // prompted, mirroring AwaitingReplacementChoice/
                // AwaitTriggerOrderChoice.
                let status = StepStatus::AwaitingPayment { player: payer, stack_id };
                self.process_state = ProcessState::AwaitingPayment { stack_id, payer, cost, source, on_paid };
                status
            }

            ProcessState::AwaitingModeChoice { stack_id, chooser, min, max } => {
                // Nothing to do until submit_mode_choice answers - same
                // "put the exact same state back" shape as AwaitingPayment
                // above.
                let status = StepStatus::AwaitingModeChoice { player: chooser, stack_id, min, max };
                self.process_state = ProcessState::AwaitingModeChoice { stack_id, chooser, min, max };
                status
            }

            ProcessState::SbaLoop => self.resolve_sba_loop_state(),

            ProcessState::CollectTriggers { since } => self.resolve_collect_triggers_state(since),

            ProcessState::PutTriggersApnap { ordered, pending_groups } => {
                self.resolve_put_triggers_apnap_state(ordered, pending_groups)
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

            ProcessState::AwaitingTriggerTargets { trigger, spec, candidates, remaining, modal_effect } => {
                // Nothing to do until submit_trigger_targets answers - same
                // "put the exact same state back" pattern every other
                // choice-pause above already uses.
                let status = StepStatus::AwaitingTriggerTargets {
                    player: trigger.controller,
                    spec: spec.clone(),
                    candidates: candidates.clone(),
                };
                self.process_state =
                    ProcessState::AwaitingTriggerTargets { trigger, spec, candidates, remaining, modal_effect };
                status
            }

            ProcessState::AwaitingTriggerModeChoice { trigger, modes, mode_choice, remaining, chooser } => {
                // Nothing to do until submit_trigger_mode_choice answers -
                // same "put the exact same state back" pattern every other
                // choice-pause above already uses. `chooser` (not
                // `trigger.controller`) is who's actually prompted - CR
                // 700.2e's own "chosen by opponent" case already resolved
                // this to the real other player, before this pause was ever
                // constructed.
                let (min, max, _repeatable) = mode_choice.bounds();
                let status = StepStatus::AwaitingTriggerModeChoice {
                    player: chooser,
                    modes: modes.clone(),
                    mode_choice,
                    min,
                    max,
                };
                self.process_state =
                    ProcessState::AwaitingTriggerModeChoice { trigger, modes, mode_choice, remaining, chooser };
                status
            }

            ProcessState::AdvanceTurnStep => self.resolve_advance_turn_step_state(),

            ProcessState::AwaitingDiscard { player, excess } => self.resolve_awaiting_discard_state(player, excess),

            ProcessState::CleanupSbaCheck => self.resolve_cleanup_sba_check_state(),

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
}
