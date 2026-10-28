//! CR 700.2d: resolving 2+ chosen modes in printed order. Each mode runs through the
//! normal dispatcher against a per-mode view of the stack object. See `DESIGN.md`.

use std::ops::{ControlFlow, Range};

use crate::commands;
use crate::types::ability::AbilityEffect;
use crate::types::event::GameEventKind;
use crate::types::ids::Target;
use crate::types::stack::StackObject;
use crate::types::target::TargetSpec;

use super::super::{Engine, ProcessState, StepStatus};

/// The modes still to run after the one in the pipeline. A mode needing the event
/// pipeline ends in `ReplacementCheck` and `CommitEvents`; `resolve_commit_events_state`
/// drops the `StackObjectResolved` marker and calls `resume_modal_resolution`. Nothing
/// between runs SBAs or collects triggers (CR 608.2c). `Chain` shares one legal set.
#[derive(Debug)]
pub(in crate::engine) struct ModalContinuation {
    /// The whole stack object, not a per-mode view.
    top: StackObject,
    effects: Vec<Option<AbilityEffect>>,
    words: Vec<usize>,
    /// One legal-target chunk per target word, flat across all modes.
    chunks: Vec<Vec<Target>>,
    /// `Some` for an `AbilityEffect::Chain`: the legal targets every entry sees.
    shared_legal: Option<Vec<Target>>,
    /// Index of the next mode to run.
    next: usize,
}

impl ModalContinuation {
    pub(in crate::engine) fn stack_id(&self) -> crate::types::ids::ObjectId {
        self.top.stack_id
    }
}

/// Whether `effect` finishes inside `resolve_ability_effect` without ever
/// touching `process_state`. Anything else may leave `ReplacementCheck` /
/// a choice pause behind, so pending events from earlier modes are committed
/// first (see `resolve_modal_from`) to keep printed-order semantics.
fn is_synchronous(effect: &AbilityEffect) -> bool {
    matches!(
        effect,
        AbilityEffect::GainLife(_)
            | AbilityEffect::Damage(_)
            | AbilityEffect::Counters { .. }
            | AbilityEffect::Pump { .. }
            | AbilityEffect::GainControl(_)
            | AbilityEffect::Shuffle
            | AbilityEffect::Regenerate
            | AbilityEffect::LoseLife(_)
            | AbilityEffect::Tap
            | AbilityEffect::Untap
            | AbilityEffect::TapSelf
            | AbilityEffect::UntapSelf
            | AbilityEffect::TapAll { .. }
            | AbilityEffect::UntapAll { .. }
            | AbilityEffect::Counter
            | AbilityEffect::Fight
            | AbilityEffect::Bite
            | AbilityEffect::ExchangeTextBoxes
    )
}

/// The slice of the flat word list that mode `index` owns.
fn word_range(words: &[usize], index: usize) -> Range<usize> {
    let start: usize = words[..index].iter().sum();
    start..start + words[index]
}

/// `top` as the single effect `effect` at mode `index` would have seen it:
/// that mode's own target requirement (none for the CR 700.2c placeholder,
/// a `Multiple` when it owns several words) and its own legal targets, so
/// `affected_player`, `Fight` and friends read the right shape.
fn mode_view(top: &StackObject, effect: &AbilityEffect, range: Range<usize>, legal: &[Target]) -> StackObject {
    let mut view = top.clone();
    view.effect = Some(effect.clone());
    view.target_requirement = match &top.target_requirement {
        // A lone word, owned by the entry whose range covers it (a chain
        // with per-entry words over a single-target ability).
        Some(TargetSpec::Primitive(req)) => (range == (0..1)).then(|| TargetSpec::Primitive(req.clone())),
        Some(TargetSpec::Multiple(reqs)) => match reqs.get(range) {
            Some([one]) if commands::is_modal_placeholder_requirement(one) => None,
            Some([one]) => Some(TargetSpec::Primitive(one.clone())),
            Some([]) | None => None,
            Some(many) => Some(TargetSpec::Multiple(many.to_vec())),
        },
        _ => None,
    };
    view.targets = legal.to_vec();
    view
}

impl Engine {
    /// CR 700.2d: 2+ chosen modes. `words[i]` target words belong to mode i, and the
    /// caller precomputed one legal chunk per word in `modal_chunk_legals`.
    pub(super) fn resolve_modal_effect(
        &mut self,
        top: &StackObject,
        effects: &[Option<AbilityEffect>],
        words: &[usize],
        modal_chunk_legals: &[Vec<Target>],
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        self.resolve_modal_from(top, effects, words, modal_chunk_legals, None, 0, events)
    }

    /// CR 608.2c — `AbilityEffect::Chain`: every entry runs in order.
    /// `words: None` hands each entry the ability's whole target requirement
    /// and `legal_targets`. `Some(words)` gives entry i the `words[i]`
    /// target words it owns, the same way a `Modal` mode owns its words.
    pub(super) fn resolve_chain_effect(
        &mut self,
        top: &StackObject,
        entries: &[AbilityEffect],
        words: Option<&[usize]>,
        legal_targets: &[Target],
        modal_chunk_legals: &[Vec<Target>],
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let effects: Vec<Option<AbilityEffect>> = entries.iter().cloned().map(Some).collect();
        match words {
            None => self.resolve_modal_from(top, &effects, &[], modal_chunk_legals, Some(legal_targets), 0, events),
            Some(words) => {
                // A lone `Primitive` word has no per-word chunks (they are
                // only derived for a `Multiple`), so its one chunk is the
                // whole legal set.
                let single;
                let chunks = match &top.target_requirement {
                    Some(TargetSpec::Primitive(_)) => {
                        single = vec![legal_targets.to_vec()];
                        &single[..]
                    }
                    _ => modal_chunk_legals,
                };
                self.resolve_modal_from(top, &effects, words, chunks, None, 0, events)
            }
        }
    }

    fn resolve_modal_from(
        &mut self,
        top: &StackObject,
        effects: &[Option<AbilityEffect>],
        words: &[usize],
        chunks: &[Vec<Target>],
        shared_legal: Option<&[Target]>,
        start: usize,
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        for index in start..effects.len() {
            let Some(effect) = &effects[index] else { continue };
            let (range, mode_chunks, legal): (Range<usize>, &[Vec<Target>], Vec<Target>) = match shared_legal {
                Some(legal) => (0..0, chunks, legal.to_vec()),
                None => {
                    let range = word_range(words, index);
                    let mode_chunks = chunks.get(range.clone()).unwrap_or(&[]);
                    let legal = mode_chunks.iter().flatten().copied().collect();
                    (range, mode_chunks, legal)
                }
            };
            let continuation = |next: usize| ModalContinuation {
                top: top.clone(),
                effects: effects.to_vec(),
                words: words.to_vec(),
                chunks: chunks.to_vec(),
                shared_legal: shared_legal.map(<[Target]>::to_vec),
                next,
            };

            // Earlier modes' proposed events (damage, counters) have to land
            // before a mode that moves cards or pauses runs, so commit them
            // through the pipeline first and pick this mode up afterwards.
            if !is_synchronous(effect) && !events.is_empty() {
                events.push(GameEventKind::StackObjectResolved { stack_id: top.stack_id });
                self.modal_continuation = Some(continuation(index));
                self.process_state = ProcessState::ReplacementCheck { remaining: events.into(), checked: Vec::new() };
                return ControlFlow::Break(StepStatus::Progressed);
            }

            let has_later_mode = effects[index + 1..].iter().any(Option::is_some);
            self.modal_continuation = (!is_synchronous(effect) && has_later_mode).then(|| continuation(index + 1));
            let view = match shared_legal {
                Some(_) => {
                    let mut view = top.clone();
                    view.effect = Some(effect.clone());
                    view.targets = legal.clone();
                    view
                }
                None => mode_view(top, effect, range, &legal),
            };
            match self.resolve_ability_effect(&view, &legal, mode_chunks, events) {
                ControlFlow::Break(status) => return ControlFlow::Break(status),
                ControlFlow::Continue(resumed) => events = resumed,
            }
            // The mode finished without pausing or reaching the pipeline.
            self.modal_continuation = None;
        }
        ControlFlow::Continue(events)
    }

    /// Picks the remaining modes up after the mode before them settled. Called
    /// from `resolve_commit_events_state` once it has committed the previous
    /// mode's events.
    pub(in crate::engine) fn resume_modal_resolution(&mut self, continuation: ModalContinuation) -> StepStatus {
        let ModalContinuation { top, effects, words, chunks, shared_legal, next } = continuation;
        match self.resolve_modal_from(&top, &effects, &words, &chunks, shared_legal.as_deref(), next, Vec::new()) {
            ControlFlow::Break(status) => status,
            ControlFlow::Continue(mut events) => {
                events.push(GameEventKind::StackObjectResolved { stack_id: top.stack_id });
                self.process_state = ProcessState::ReplacementCheck { remaining: events.into(), checked: Vec::new() };
                StepStatus::Progressed
            }
        }
    }
}
