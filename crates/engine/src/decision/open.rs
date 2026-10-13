//! Builds the open Decision from the engine's current `ProcessState`. The
//! match below has no wildcard arm on purpose: a new pause variant fails to
//! compile here until it is mapped to a reply shape or marked as not waiting
//! on a player.

use crate::commands;
use crate::engine::{Engine, ProcessState};
use crate::script::ScriptChoiceRequest;
use crate::triggers::PendingTrigger;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::target::TargetSpec;
use crate::types::zone::ZoneKind;

use super::{Candidate, PartitionRest, ReplyShape};

/// Which engine entry point an Answer to this pause goes to.
pub(crate) enum Route {
    ReplacementChoice,
    LegendRule,
    CommanderReturn,
    AttachChoice,
    EntryAttachChoice,
    TokenAttachChoice,
    Devour,
    Amplify,
    SearchDevour,
    SearchAmplify,
    Madness,
    Cascade,
    UnlessPaid,
    DiscardChoice,
    CopyChoice,
    ScriptSelect,
    ScriptMay,
    Scry,
    Surveil,
    Search,
    /// The offered group, so index replies can be turned back into triggers.
    TriggerOrder(Vec<PendingTrigger>),
    TriggerTargets,
    TriggerMode,
    Payment,
    SpellMode,
    CleanupDiscard,
    Mulligan,
    BottomCards,
    Priority,
}

pub(super) struct Described {
    pub player: PlayerId,
    pub mechanic: &'static str,
    pub shape: ReplyShape,
    pub route: Route,
}

fn objects(ids: &[ObjectId]) -> Vec<Candidate> {
    ids.iter().map(|&id| Candidate::Target(Target::Object(id))).collect()
}

fn targets(list: &[Target]) -> Vec<Candidate> {
    list.iter().map(|&t| Candidate::Target(t)).collect()
}

/// `player`'s hand, as Pick candidates.
fn hand(engine: &Engine, player: PlayerId) -> Vec<Candidate> {
    engine
        .state()
        .zone(ZoneKind::Hand, Some(player))
        .map(|zone| zone.contents.iter().map(|&id| Candidate::Target(Target::Object(id))).collect())
        .unwrap_or_default()
}

fn pick(candidates: Vec<Candidate>, min: usize, max: usize) -> ReplyShape {
    ReplyShape::Pick { candidates, min, max }
}

/// Every object and player a `Game.SelectTargets` ask may legally pick,
/// objects first in id order. `ScriptChoiceRequest::SelectTarget` carries a
/// filter rather than a list, so the facade evaluates it here.
/// TODO: abilities on the stack are legal `Target::Ability` candidates for a
/// filter that asks for them; they are not enumerated yet.
fn script_select_candidates(
    engine: &Engine,
    filter: &crate::types::target::TargetFilter,
    exclude_ids: &[ObjectId],
    controller: PlayerId,
    source: ObjectId,
) -> Vec<Candidate> {
    let state = engine.state();
    let mut ids: Vec<ObjectId> = state.objects.keys().copied().filter(|id| !exclude_ids.contains(id)).collect();
    ids.sort();
    let objects = ids.into_iter().map(Target::Object);
    let players = state.players.iter().map(|p| Target::Player(p.player_id));
    objects
        .chain(players)
        .filter(|&t| commands::target_is_legal(state, engine.script_runtime(), t, filter, controller, source, false, None))
        .map(Candidate::Target)
        .collect()
}

/// The open Decision's contents, or `None` when the engine is running, over,
/// or has not finished pregame setup.
pub(super) fn describe(engine: &Engine) -> Option<Described> {
    use ReplyShape::*;
    let (player, mechanic, shape, route) = match engine.process_state() {
        ProcessState::PriorityWindow => {
            if engine.awaiting_pregame_setup() {
                return None;
            }
            (engine.state().priority_player, "Priority", Action, Route::Priority)
        }

        ProcessState::AwaitingReplacementChoice { affected_player, candidates, .. } => (
            *affected_player,
            "Replacement order",
            pick(candidates.iter().map(|&c| Candidate::Replacement(c)).collect(), 1, 1),
            Route::ReplacementChoice,
        ),
        ProcessState::AwaitingLegendRuleChoice { player, candidates, .. } => {
            (*player, "Legend rule", pick(objects(candidates), 1, 1), Route::LegendRule)
        }
        ProcessState::AwaitingCommanderReturnChoice { player, .. } => {
            (*player, "Commander return", YesNo, Route::CommanderReturn)
        }
        ProcessState::AwaitingAttachChoice { controller, candidates, .. } => {
            (*controller, "Attach", pick(objects(candidates), 1, 1), Route::AttachChoice)
        }
        ProcessState::AwaitingEntryAttachChoice { controller, candidates, .. } => {
            (*controller, "Attach on entry", pick(targets(candidates), 1, 1), Route::EntryAttachChoice)
        }
        ProcessState::AwaitingTokenAttachChoice { controller, candidates, .. } => {
            (*controller, "Attach on entry", pick(targets(candidates), 1, 1), Route::TokenAttachChoice)
        }
        ProcessState::AwaitingDevourChoice { controller, candidates, .. } => {
            (*controller, "Devour", pick(objects(candidates), 0, candidates.len()), Route::Devour)
        }
        ProcessState::AwaitingAmplifyChoice { controller, candidates, .. } => {
            (*controller, "Amplify", pick(objects(candidates), 0, candidates.len()), Route::Amplify)
        }
        ProcessState::AwaitingSearchDevourChoice { controller, candidates, .. } => {
            (*controller, "Devour", pick(objects(candidates), 0, candidates.len()), Route::SearchDevour)
        }
        ProcessState::AwaitingSearchAmplifyChoice { controller, candidates, .. } => {
            (*controller, "Amplify", pick(objects(candidates), 0, candidates.len()), Route::SearchAmplify)
        }
        ProcessState::AwaitingMadnessChoice { player, card, .. } => {
            (*player, "Madness", MayCast { card: Some(*card) }, Route::Madness)
        }
        ProcessState::AwaitingCascadeChoice { player, candidate, .. } => {
            (*player, "Cascade", MayCast { card: *candidate }, Route::Cascade)
        }
        ProcessState::AwaitingUnlessPaidChoice { payer_queue, cost, .. } => {
            let payer = *payer_queue.front().expect("payer_queue is never empty while paused");
            (payer, "Unless paid", Payment { cost: cost.clone() }, Route::UnlessPaid)
        }
        ProcessState::AwaitingDiscardChoice { player, candidates, .. } => {
            (*player, "Discard", pick(objects(candidates), 1, 1), Route::DiscardChoice)
        }
        ProcessState::AwaitingCopyChoice { controller, candidates, .. } => {
            (*controller, "Copy", pick(objects(candidates), 0, 1), Route::CopyChoice)
        }
        ProcessState::AwaitingScriptChoice { request, controller, source, .. } => match request {
            ScriptChoiceRequest::SelectTarget { filter, exclude_ids } => (
                *controller,
                "Choose target",
                pick(script_select_candidates(engine, filter, exclude_ids, *controller, *source), 1, 1),
                Route::ScriptSelect,
            ),
            ScriptChoiceRequest::MayChoose => (*controller, "May", YesNo, Route::ScriptMay),
        },
        ProcessState::AwaitingScryChoice { controller, cards, .. } => (
            *controller,
            "Scry",
            Partition { cards: cards.clone(), rest: PartitionRest::LibraryBottom },
            Route::Scry,
        ),
        ProcessState::AwaitingSurveilChoice { controller, cards, .. } => (
            *controller,
            "Surveil",
            Partition { cards: cards.clone(), rest: PartitionRest::Graveyard },
            Route::Surveil,
        ),
        ProcessState::AwaitingSearchChoice { controller, candidates, count, .. } => {
            (*controller, "Search", pick(objects(candidates), 0, *count as usize), Route::Search)
        }
        ProcessState::AwaitTriggerOrderChoice { player, group, .. } => {
            (*player, "Trigger order", Order { items: group.clone() }, Route::TriggerOrder(group.clone()))
        }
        ProcessState::AwaitingTriggerTargets { trigger, spec, candidates, .. } => (
            trigger.controller,
            "Trigger targets",
            targets_shape(spec, candidates),
            Route::TriggerTargets,
        ),
        ProcessState::AwaitingTriggerModeChoice { modes, mode_choice, chooser, .. } => {
            let (min, max, repeatable) = mode_choice.bounds();
            (*chooser, "Trigger mode", Modes { mode_count: modes.len(), min, max, repeatable }, Route::TriggerMode)
        }
        ProcessState::AwaitingPayment { payer, cost, .. } => {
            (*payer, "Payment", Payment { cost: cost.clone() }, Route::Payment)
        }
        ProcessState::AwaitingModeChoice { stack_id, chooser, min, max } => {
            let mode_count = engine.state().object(*stack_id).map_or(0, |o| o.definition.modes.len());
            (*chooser, "Spell mode", Modes { mode_count, min: *min, max: *max, repeatable: false }, Route::SpellMode)
        }
        ProcessState::AwaitingDiscard { player, excess } => {
            (*player, "Cleanup discard", pick(hand(engine, *player), *excess, *excess), Route::CleanupDiscard)
        }
        ProcessState::AwaitingMulliganDecision { player } => (*player, "Mulligan", YesNo, Route::Mulligan),
        ProcessState::AwaitingBottomCardChoice { player, count, .. } => {
            (*player, "Mulligan bottom", pick(hand(engine, *player), *count, *count), Route::BottomCards)
        }

        // The engine is running or finished: no player has to answer.
        ProcessState::PendingAction(_)
        | ProcessState::ResolveStackTop
        | ProcessState::ReplacementCheck { .. }
        | ProcessState::CommitEvents { .. }
        | ProcessState::SbaLoop
        | ProcessState::CollectTriggers { .. }
        | ProcessState::PutTriggersApnap { .. }
        | ProcessState::AdvanceTurnStep
        | ProcessState::CleanupSbaCheck
        | ProcessState::GameOver { .. } => return None,
    };
    Some(Described { player, mechanic, shape, route })
}

fn targets_shape(spec: &TargetSpec, candidates: &[Vec<Target>]) -> ReplyShape {
    ReplyShape::Targets { spec: spec.clone(), candidates: candidates.to_vec() }
}
