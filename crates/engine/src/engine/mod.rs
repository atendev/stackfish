//! The step-able rules processor: the `Engine` struct, its pipeline-state
//! dispatcher (`advance`), and the pieces that don't belong to any one
//! functional cluster (error types, struct plumbing, token construction) —
//! split out of the original single-file `engine.rs`. Every non-trivial
//! `ProcessState` match arm below delegates to a `pub(super)`/
//! `pub(in crate::engine)` method defined in a sibling module; see each
//! sibling's own module doc comment for which cluster it owns.
//!
//! `ProcessState::ResolveStackTop` is the one exception, left fully inlined
//! here rather than delegated — it's the single largest arm (CR 608.2m/
//! 601.2c/602.2b resolution, ~1300 lines) and splitting it needs its own
//! dedicated pass (a `resolve/` subdirectory) rather than a one-line
//! delegation like every other arm gets.

use std::collections::VecDeque;

use crate::commands::{self, PendingPaymentRequest};
use crate::engine_context::{PipelineContext, PipelineExit};
use crate::replacement::{self, ReplacementOutcome};
use crate::script::{OperationOutcome, ScriptError, ScriptRuntime};
use crate::types::ability::{
    apply_text_box, AbilityEffect, AbilityKind, ConditionSpec, InterveningIfSpec, NamedToken, RegisteredAbility,
    TokenSpec,
};
use crate::types::card::{CardDefinition, CardTypeSet, KeywordSet, ModeChoice, PTValue, ScriptReference, Subtype, SupertypeSet};
use crate::types::config::GameSettings;
use crate::types::cost::{colors_spent, AdditionalCost, CostSpec, FilterController, FilterSpec};
use crate::types::counter::CounterSet;
use crate::types::effect::{
    matches_effect_filter, AffectedFilter, ConsumptionPolicy, ContinuousEffect, EffectFilter, EffectPayload,
    EffectPrimitive, EventFilter, Layer, ReplacementEffect, ReplacementPayload, ReplacementSpec,
};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId, ScriptHandle, Target};
use crate::types::mana::{ColorSet, ManaCost};
use crate::types::object::{GameObject, VisibilityPolicy};
use crate::types::stack::StackEntryKind;
use crate::types::target::{TargetRequirement, TargetSpec};
use crate::types::turn::Phase;
use crate::types::zone::ZoneKind;

mod action;
mod choice_answers;
mod event_pipeline;
mod library_choices;
mod payment;
mod pregame;
mod process_state;
mod replacement_resume;
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
        TokenSpec::CopyOf(_, _) => unreachable!(
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
            mode_choice: ModeChoice::Exactly(1),
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
            mode_choice: ModeChoice::Exactly(1),
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
                mode_choice: ModeChoice::Exactly(1),
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
            let id = state.allocate_ability_id();
            state.abilities.push(RegisteredAbility {
                id,
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
            let id = state.allocate_ability_id();
            state.abilities.push(RegisteredAbility {
                id,
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
        TokenSpec::CopyOf(_, _) => {}
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

    /// A card's own base target requirement (an Aura's "Enchant [quality]"
    /// line, etc.), as declared by its script's optional `target_requirement()`
    /// function — see `ScriptRuntime::build_target_requirement`'s own doc
    /// comment for the full convention. `&self`, not `&mut self`: unlike
    /// `load_script`/`load_condition`/`load_operation`, this doesn't touch
    /// the script registry at all.
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

            ProcessState::ResolveStackTop => {
                // CR 700.2e: peek before popping - a modal spell whose mode
                // is chosen by another player reaches this point with
                // nothing resolvable yet (`chosen_modes`/`target_requirement`
                // /`effect` are still empty/`None` placeholders). Pause here
                // instead of popping it off the stack; `Engine::
                // submit_mode_choice` patches it in place and re-enters this
                // same `ProcessState` once the choice arrives, at which
                // point `pending_mode_choice` is `None` and this check falls
                // through exactly as before this slice.
                if let Some(top) = self.state.stack.last() {
                    if let Some(pending) = &top.pending_mode_choice {
                        let status = StepStatus::AwaitingModeChoice {
                            player: pending.chooser,
                            stack_id: top.stack_id,
                            min: pending.min,
                            max: pending.max,
                        };
                        self.process_state = ProcessState::AwaitingModeChoice {
                            stack_id: top.stack_id,
                            chooser: pending.chooser,
                            min: pending.min,
                            max: pending.max,
                        };
                        return status;
                    }
                }
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
                // `top.source`, not `top.stack_id` - they only coincide for
                // a Spell (see `StackObject`'s own construction sites); for
                // an ActivatedAbility/TriggeredAbility, `top.stack_id` is a
                // bare, never-a-real-GameObject id, and passing it here as
                // "source" silently broke both protection-from-source-
                // quality checks (`target_has_protection_from`'s
                // `state.object(source)` lookup would just miss and return
                // `false`) and `exclude_source` below - every other
                // resolution site already uses `top.source` for "the real
                // thing this effect comes from" (e.g. the Damage
                // `AbilityEffect` arm further down).
                let is_legal_now = |t: &Target, req: &TargetRequirement| {
                    commands::target_is_legal(
                        &self.state,
                        &self.script_runtime,
                        *t,
                        &req.filter,
                        top.controller,
                        top.source,
                        req.exclude_source,
                    )
                };
                let legal_targets: Vec<Target> = match &top.target_requirement {
                    Some(TargetSpec::Primitive(req)) => {
                        top.targets.iter().copied().filter(|t| is_legal_now(t, req)).collect()
                    }
                    // CR 601.2c/608.2b: each word's own legal subset,
                    // concatenated - downstream effect-resolution arms
                    // (Damage/Destroy/etc) consume this same flattened list
                    // exactly as they already do for a single-word
                    // requirement, which is correct for a uniform-verb
                    // multi-word spell (CR 608.2b's own Plague Spores
                    // example) - see `TargetSpec::Multiple`'s own doc
                    // comment. `split_targets_by_requirement` returning
                    // `None` here would mean `top.targets` was never valid
                    // in the first place (validated at cast/activation
                    // time) - falls back to empty rather than panicking,
                    // same "recheck, don't assume" posture this whole block
                    // already takes.
                    Some(TargetSpec::Multiple(reqs)) => commands::split_targets_by_requirement(reqs, &top.targets)
                        .map(|chunks| {
                            reqs.iter()
                                .zip(chunks)
                                .flat_map(|(req, chunk)| chunk.iter().copied().filter(|t| is_legal_now(t, req)).collect::<Vec<_>>())
                                .collect()
                        })
                        .unwrap_or_default(),
                    // `None` (no targets required) or a `Script`-backed
                    // requirement (not evaluable this slice, see
                    // `TargetSpec`'s own doc comment) - every submitted
                    // target passes through unfiltered, same as before this
                    // slice.
                    _ => top.targets.clone(),
                };
                // CR 700.2d: for a Modal effect, legal_targets above is
                // deliberately flattened (losing chunk boundaries, correct
                // for a uniform-verb multi-word spell but wrong once each
                // chunk belongs to a *different* mode's own effect) -
                // precompute per-mode legal chunks here instead, right next
                // to is_legal_now's other uses, so its borrow of
                // &self.state ends here rather than having to extend all
                // the way into the match block below (which mutates
                // self.state per effect kind - see AbilityEffect::Modal's
                // own arm there for why calling is_legal_now again from
                // inside that match wouldn't borrow-check).
                let modal_chunk_legals: Vec<Vec<Target>> = match &top.target_requirement {
                    Some(TargetSpec::Multiple(reqs)) => commands::split_targets_by_requirement(reqs, &top.targets)
                        .map(|chunks| {
                            reqs.iter()
                                .zip(chunks)
                                .map(|(req, chunk)| chunk.iter().copied().filter(|t| is_legal_now(t, req)).collect())
                                .collect()
                        })
                        .unwrap_or_default(),
                    _ => Vec::new(),
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
                    Some(TargetSpec::Primitive(_) | TargetSpec::Multiple(_)) if !top.targets.is_empty() => {
                        legal_targets.is_empty()
                    }
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
                            // CR 614's general "instead" framing: a
                            // registered ReplaceZoneDestination effect can
                            // redirect a would-be battlefield entry to a
                            // different zone entirely (e.g. "if a creature
                            // would enter the battlefield under your
                            // control, exile it instead") - proposed and
                            // checked *before* move_resolving_spell ever
                            // places anything, mirroring
                            // sba::resolve_deaths's own propose-then-commit
                            // shape for the graveyard side. Only meaningful
                            // for a would-be Battlefield destination - the
                            // instant/sorcery "remains in its previous
                            // zone" redirect above is CR 400.4a's own
                            // unconditional rule, untouched by this.
                            //
                            // Narrow, named gap: only an *externally*
                            // granted redirect effect can ever fire here -
                            // the entering permanent's own self-registered
                            // effects (if any) haven't been registered yet
                            // at this point (run_card_script only runs
                            // inside move_resolving_spell, below, once the
                            // destination is already decided), so a card's
                            // own "as this would enter, redirect it
                            // instead" text isn't reachable via this path.
                            let destination = if destination == ZoneKind::Battlefield {
                                let proposed = GameEventKind::ZoneChange {
                                    object: top.stack_id,
                                    from: ZoneKind::Stack,
                                    to: ZoneKind::Battlefield,
                                    last_known_info: None,
                                };
                                match replacement::apply_replacements(&mut self.state, &self.script_runtime, proposed, false) {
                                    ReplacementOutcome::Resolved(GameEventKind::ZoneChange { to, .. }) => to,
                                    // Defensive - a WouldMoveZone candidate's
                                    // own matching payloads only ever
                                    // rewrite it back into a ZoneChange;
                                    // falls back to the original destination
                                    // rather than guessing at an
                                    // unreachable shape.
                                    ReplacementOutcome::Resolved(_) => destination,
                                    // CR 616.1: two or more simultaneous
                                    // "instead" redirects for the same
                                    // entering permanent is a genuinely rare
                                    // combination (two different sources
                                    // each independently hating on the same
                                    // entry at once) - narrowed to "no
                                    // redirect happens" rather than a real
                                    // player-choice pause, same posture this
                                    // codebase already takes for comparably
                                    // rare nested-pause interactions (e.g.
                                    // pending_devour_choice's own
                                    // sacrifice-triggers-another-pause gap).
                                    // TODO: a real AwaitingReplacementChoice
                                    // pause here, if a real card ever needs
                                    // it - would need its own
                                    // ReplacementResumeTarget variant plus
                                    // extracting this whole Spell-resolution
                                    // tail into a shared continuation, since
                                    // nothing has been placed yet at this
                                    // point for submit_replacement_choice to
                                    // resume into.
                                    ReplacementOutcome::NeedsOrderingChoice { .. } => destination,
                                }
                            } else {
                                destination
                            };
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
                                    let object_id = *object;
                                    // CR 707.6/614.12a: checked before even
                                    // Devour/Amplify below - becoming a copy
                                    // is the most identity-fundamental of
                                    // the three mid-ETB choices, and no real
                                    // Clone-family card carries Devour/
                                    // Amplify anyway (see EntersAsCopy's own
                                    // doc comment for why the post-choice
                                    // resume doesn't chain back into those
                                    // two). Only this call site (a resolving
                                    // spell) can raise this pause, same
                                    // "PlayLand/Search never check this"
                                    // posture pending_devour_choice already
                                    // established - see pending_copy_choice's
                                    // own doc comment.
                                    if let Some((exceptions, candidates)) = replacement::pending_copy_choice(
                                        &self.state,
                                        &self.script_runtime,
                                        object_id,
                                        ZoneKind::Stack,
                                    ) {
                                        events.push(event);
                                        self.process_state = ProcessState::AwaitingCopyChoice {
                                            object: object_id,
                                            controller: top.controller,
                                            exceptions,
                                            candidates,
                                            stack_id: top.stack_id,
                                            events_so_far: events,
                                            colors_spent: colors_spent(&top.costs_paid),
                                        };
                                        return StepStatus::Progressed;
                                    }
                                    // CR 702.82a: checked *before*
                                    // apply_enters_battlefield_modifiers -
                                    // only this call site (a resolving
                                    // spell) can raise this pause; see
                                    // pending_devour_choice's own doc
                                    // comment for why PlayLand/Search never
                                    // check this at all.
                                    if let Some((per_creature, candidates)) = replacement::pending_devour_choice(
                                        &self.state,
                                        &self.script_runtime,
                                        object_id,
                                        ZoneKind::Stack,
                                        top.controller,
                                    ) {
                                        events.push(event);
                                        self.process_state = ProcessState::AwaitingDevourChoice {
                                            object: object_id,
                                            controller: top.controller,
                                            per_creature,
                                            candidates,
                                            stack_id: top.stack_id,
                                            events_so_far: events,
                                        };
                                        return StepStatus::Progressed;
                                    }
                                    // CR 702.38a: same "checked before
                                    // apply_enters_battlefield_modifiers,
                                    // only this call site can pause" posture
                                    // as Devour above - see
                                    // pending_amplify_choice's own doc
                                    // comment.
                                    if let Some((per_card, candidates)) = replacement::pending_amplify_choice(
                                        &self.state,
                                        &self.script_runtime,
                                        object_id,
                                        ZoneKind::Stack,
                                        top.controller,
                                    ) {
                                        events.push(event);
                                        self.process_state = ProcessState::AwaitingAmplifyChoice {
                                            object: object_id,
                                            controller: top.controller,
                                            per_card,
                                            candidates,
                                            stack_id: top.stack_id,
                                            events_so_far: events,
                                        };
                                        return StepStatus::Progressed;
                                    }
                                    // CR 702.44b: only a resolving spell can
                                    // ever have a nonempty `costs_paid` to
                                    // read colors from - `top` is this same
                                    // spell's own `StackObject`, captured
                                    // above before its zone move.
                                    let counter_events = replacement::apply_enters_battlefield_modifiers(
                                        &mut self.state,
                                        &self.script_runtime,
                                        object_id,
                                        ZoneKind::Stack,
                                        colors_spent(&top.costs_paid),
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
                            // CR 700.2d: 2+ chosen modes, combined - see
                            // AbilityEffect::Modal's own doc comment for
                            // the shape invariant this relies on (one entry per
                            // chosen mode, top.target_requirement always
                            // TargetSpec::Multiple with a matching per-mode
                            // TargetRequirement, count: 0 synthesized for a
                            // mode with no target_requirement of its own).
                            // legal_targets above is deliberately flattened
                            // (losing chunk boundaries - correct for a
                            // uniform-verb multi-word spell, wrong here
                            // where each chunk belongs to a *different*
                            // effect), so this re-derives per-mode chunks
                            // instead and re-filters each for current
                            // legality (CR 608.2b), same is_legal_now
                            // closure legal_targets itself uses. Narrowed to
                            // the synchronous subset that never pauses
                            // mid-resolution (GainLife/Damage/Counters/Pump/
                            // GainControl/Shuffle) - commands::execute_command's
                            // CastSpell arm already rejects combining any
                            // other effect kind into a Modal at cast time,
                            // so an unsupported kind reaching the fallback
                            // arm below would mean that check has a gap;
                            // skip rather than guess/panic, same stance
                            // every other unreachable shape in this engine
                            // already takes.
                            Some(AbilityEffect::Modal(effects)) => {
                                // modal_chunk_legals is precomputed above,
                                // next to is_legal_now's other uses - see
                                // its own doc comment for why it can't be
                                // (re)computed from inside this arm.
                                for (effect, legal) in effects.iter().zip(modal_chunk_legals.iter()) {
                                    let Some(effect) = effect else { continue };
                                    match effect {
                                        AbilityEffect::GainLife(amount) => {
                                            if let Some(player) = self.state.player_mut(top.controller) {
                                                player.gain_life(*amount);
                                            }
                                        }
                                        AbilityEffect::Damage(amount) => {
                                            for target in legal {
                                                events.push(GameEventKind::DamageDealt {
                                                    source: top.source,
                                                    target: *target,
                                                    amount: *amount,
                                                    is_combat_damage: false,
                                                });
                                            }
                                        }
                                        AbilityEffect::Counters { kind, amount } => {
                                            for target in legal {
                                                let Target::Object(id) = target else { continue };
                                                events.push(GameEventKind::CounterAdded {
                                                    object: *id,
                                                    kind: kind.clone(),
                                                    amount: *amount,
                                                });
                                            }
                                        }
                                        AbilityEffect::Pump { power, toughness, duration } => {
                                            let timestamp = self.state.allocate_sequence_no();
                                            for target in legal {
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
                                        AbilityEffect::GainControl(duration) => {
                                            let timestamp = self.state.allocate_sequence_no();
                                            let new_controller = top.controller;
                                            for target in legal {
                                                let Target::Object(id) = target else { continue };
                                                self.state.continuous_effects.push(ContinuousEffect {
                                                    source: *id,
                                                    layer: Layer::ControlChangingEffects,
                                                    affected: AffectedFilter::Primitive(EffectFilter {
                                                        controller: None,
                                                        card_types: None,
                                                        self_only: true,
                                                    }),
                                                    effect: EffectPayload::Primitive(EffectPrimitive::GainControl(
                                                        new_controller,
                                                    )),
                                                    timestamp,
                                                    duration: duration.clone(),
                                                    condition: None,
                                                });
                                            }
                                        }
                                        AbilityEffect::Shuffle => {
                                            self.shuffle_library(top.controller);
                                        }
                                        // Unsupported within Modal (see this
                                        // arm's own doc comment) - skip
                                        // rather than guess.
                                        _ => {}
                                    }
                                }
                            }
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
                                    Vec::new(),
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
                                    Vec::new(),
                                );
                                return StepStatus::Progressed;
                            }
                            // CR 701.8/601.2c: "destroy all X" doesn't target
                            // at all - every battlefield object matching
                            // `filter` (relative to this effect's own
                            // source/controller, same as a continuous
                            // effect's own affected-set check) is collected
                            // directly, no `legal_targets`/`target_requirement`
                            // involved. Handed to the exact same
                            // `continue_scripted_zone_move(Destroy, ...)`
                            // pipeline the single-target `Destroy` arm just
                            // above uses, so CR 702.12b indestructibility, CR
                            // 616.1 ordering-choice pauses, and the CR 614.13
                            // `WouldDie` replacement check all apply
                            // identically - no new destruction logic here.
                            Some(AbilityEffect::DestroyAll(filter)) => {
                                let targets: VecDeque<ObjectId> = self
                                    .state
                                    .zone(ZoneKind::Battlefield, None)
                                    .map(|zone| {
                                        zone.contents
                                            .iter()
                                            .copied()
                                            .filter(|id| {
                                                self.state.object(*id).is_some_and(|object| {
                                                    matches_effect_filter(filter, object, top.source, top.controller)
                                                })
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                self.process_state = self.continue_scripted_zone_move(
                                    ScriptedZoneMoveKind::Destroy,
                                    targets,
                                    top.stack_id,
                                    Vec::new(),
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
                                    Vec::new(),
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
                            Some(AbilityEffect::BecomeCopy(exceptions)) => {
                                if let Some(Target::Object(target_id)) = top.targets.first().copied() {
                                    let source_id = top.source;
                                    // CR 707.9c/d: the copying object's own
                                    // true pre-copy definition, for
                                    // retain_name/retain_color to fall back
                                    // to - captured before anything mutates.
                                    let original_definition = self.state.object(source_id).map(|o| o.definition.clone());
                                    let snapshot = self.state.object(target_id).map(|obj| {
                                        obj.copiable_override.clone().unwrap_or_else(|| obj.definition.clone())
                                    });
                                    if let Some(mut snapshot) = snapshot {
                                        exceptions.apply(&mut snapshot, original_definition.as_ref());
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
                            // CR 612.5: inherently pairwise, unlike a
                            // uniform multi-word spell (Plague Spores'
                            // "destroy target creature and target land,"
                            // where each surviving legal target is still
                            // independently destroyed) - rechecks both
                            // targets directly against their own
                            // TargetRequirement rather than consuming the
                            // flattened legal_targets list, which loses the
                            // positional pairing this role-differentiated
                            // effect needs (see ExchangeTextBoxes's own doc
                            // comment). Anything short of two still-legal
                            // Target::Object targets is a silent no-op - no
                            // partner to exchange with.
                            Some(AbilityEffect::ExchangeTextBoxes) => {
                                // Not `is_legal_now` (the closure defined
                                // above) - reusing it here would extend its
                                // captured `&self.state` borrow across every
                                // `&mut self.state` use in every other arm
                                // of this same match, since the borrow
                                // checker can't see that match arms are
                                // mutually exclusive at runtime. Same
                                // `target_is_legal` call, inlined instead.
                                let is_legal_now = |t: &Target, req: &TargetRequirement| {
                                    commands::target_is_legal(
                                        &self.state,
                                        &self.script_runtime,
                                        *t,
                                        &req.filter,
                                        top.controller,
                                        top.source,
                                        req.exclude_source,
                                    )
                                };
                                if let (Some(TargetSpec::Multiple(reqs)), [Target::Object(a_id), Target::Object(b_id)]) =
                                    (&top.target_requirement, top.targets.as_slice())
                                {
                                    let (a_id, b_id) = (*a_id, *b_id);
                                    if reqs.len() == 2
                                        && is_legal_now(&Target::Object(a_id), &reqs[0])
                                        && is_legal_now(&Target::Object(b_id), &reqs[1])
                                    {
                                        let a_snapshot = self.state.object(a_id).map(|obj| {
                                            obj.copiable_override.clone().unwrap_or_else(|| obj.definition.clone())
                                        });
                                        let b_snapshot = self.state.object(b_id).map(|obj| {
                                            obj.copiable_override.clone().unwrap_or_else(|| obj.definition.clone())
                                        });
                                        if let (Some(a_snapshot), Some(b_snapshot)) = (a_snapshot, b_snapshot) {
                                            let mut a_new = a_snapshot.clone();
                                            apply_text_box(&mut a_new, &b_snapshot);
                                            let mut b_new = b_snapshot.clone();
                                            apply_text_box(&mut b_new, &a_snapshot);

                                            if let Some(obj) = self.state.object_mut(a_id) {
                                                obj.copiable_override = Some(a_new.clone());
                                            }
                                            if let Some(obj) = self.state.object_mut(b_id) {
                                                obj.copiable_override = Some(b_new.clone());
                                            }
                                            commands::purge_granted_abilities(&mut self.state, a_id);
                                            commands::purge_granted_abilities(&mut self.state, b_id);
                                            // TODO: a script error here is
                                            // silently dropped, same gap
                                            // every other run_card_script
                                            // call site in this file already
                                            // flags.
                                            let _ = self.script_runtime.run_card_script(&mut self.state, &a_new, a_id);
                                            let _ = self.script_runtime.run_card_script(&mut self.state, &b_new, b_id);

                                            events.push(GameEventKind::TextBoxesExchanged { a: a_id, b: b_id });
                                        }
                                    }
                                }
                                // else: fewer than two Target::Object
                                // targets, or the requirement isn't
                                // Multiple([_, _]) - silent no-op, same
                                // stance BecomeCopy's own arm takes above.
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
                            //
                            // `apply_enters_battlefield_modifiers` is real
                            // now too, the 4th of the four real to-
                            // Battlefield producers (`PlayLand`, the
                            // resolving-spell path above, `Search`) - closes
                            // a real, previously-silent gap: EntersTapped/
                            // EntersWithCounters/Bloodthirst/Sunburst all
                            // already worked for every other permanent-entry
                            // shape but never applied to a token, even one
                            // externally granted "tokens you control enter
                            // tapped"-style modifier (matched by controller,
                            // not `self_only`, so a plain `Named`/`Creature`
                            // token with no script of its own can still be
                            // affected) or a `CopyOf` token whose freshly
                            // re-run `run_card_script` self-registered one.
                            // Devour/Amplify stay unwired here on purpose -
                            // both need a genuine mid-ETB player pause this
                            // function structurally can't raise (it only
                            // ever mutates state/proposes events
                            // synchronously) - same narrow "no real producer
                            // reaches this yet" posture `pending_devour_
                            // choice`'s own doc comment already takes for
                            // `Search`. `colors_spent` is always
                            // `ColorSet::empty()` - CR 702.44b, a token
                            // creation is never a cast, same convention
                            // `PlayLand`/`Search` already use for Sunburst.
                            Some(AbilityEffect::CreateToken { spec, count }) => {
                                let controller = top.controller;
                                let copy_snapshot = match spec {
                                    TokenSpec::CopyOf(source_id, exceptions) => self.state.object(*source_id).map(|obj| {
                                        let mut snapshot =
                                            obj.copiable_override.clone().unwrap_or_else(|| obj.definition.clone());
                                        // CR 707.9c/d: no `original` here - a
                                        // freshly created token has no prior
                                        // "own identity" to retain a name/
                                        // color from, unlike BecomeCopy/
                                        // submit_copy_choice's own already-
                                        // existing object. retain_name/
                                        // retain_color are a deliberate,
                                        // tested no-op for this producer.
                                        exceptions.apply(&mut snapshot, None);
                                        snapshot
                                    }),
                                    _ => None,
                                };
                                for _ in 0..*count {
                                    let token_def = match (&copy_snapshot, spec) {
                                        (Some(def), _) => def.clone(),
                                        (None, TokenSpec::CopyOf(_, _)) => {
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
                                    if matches!(spec, TokenSpec::CopyOf(_, _)) {
                                        // TODO: a script error here is silently dropped,
                                        // same gap every other run_card_script call site
                                        // in this file already flags.
                                        let _ = self.script_runtime.run_card_script(&mut self.state, &token_def, token_id);
                                    }
                                    // Checked after self-registration above
                                    // so a `CopyOf` token's freshly re-run
                                    // script is already in `state.
                                    // replacement_effects` by this point,
                                    // same ordering `PlayLand`/`Search`/the
                                    // resolving-spell path all already use.
                                    let counter_events = replacement::apply_enters_battlefield_modifiers(
                                        &mut self.state,
                                        &self.script_runtime,
                                        token_id,
                                        ZoneKind::Stack,
                                        ColorSet::empty(),
                                    );
                                    events.push(GameEventKind::ZoneChange {
                                        object: token_id,
                                        from: ZoneKind::Stack,
                                        to: ZoneKind::Battlefield,
                                        last_known_info: None,
                                    });
                                    events.extend(counter_events);
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
                            // `CastSpell` always unpacks this into
                            // `operation`/`effect: None` at push time (see
                            // `AbilityEffect::Script`'s own doc comment) - no
                            // `StackObject` should ever actually carry this
                            // variant on its `effect` field. Defensive
                            // no-op, same stance `AddMana`/`AddManaChoice`
                            // just above take for their own unreachable case.
                            Some(AbilityEffect::Script(_)) => {}
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

            ProcessState::AwaitingCopyChoice { object, controller, exceptions, candidates, stack_id, events_so_far, colors_spent } => {
                // Nothing to do until submit_copy_choice answers - same
                // "put the exact same state back" pattern every other
                // choice-pause above already uses.
                let status = StepStatus::AwaitingCopyChoice { player: controller, object, candidates: candidates.clone() };
                self.process_state = ProcessState::AwaitingCopyChoice {
                    object, controller, exceptions, candidates, stack_id, events_so_far, colors_spent,
                };
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
