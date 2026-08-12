//! CR 608.2m/601.2c/602.2b spell/ability resolution — `ProcessState::
//! ResolveStackTop`'s full dispatcher, split out of the original fully-
//! inlined arm in `engine/mod.rs` (the single largest and most complex arm
//! in the whole engine, ~1300 lines) into this subdirectory.
//!
//! `resolve_stack_top` is the outer shell: the CR 700.2e mode-choice peek,
//! the pop + `priority_player`/`pipeline_context`/`pipeline_exit`
//! bookkeeping, the CR 608.2b target-legality/intervening-if fizzle checks,
//! the fizzle branch, dispatch to `permanent_entry::resolve_permanent_entry`
//! when the popped `StackObject` is a `Spell` (its zone move — to the
//! battlefield as a permanent, or back to graveyard/exile as an instant/
//! sorcery — happens the same way either way, see that function's own doc
//! comment), dispatch to `resolve_ability_effect` for a `TriggeredAbility`/
//! `ActivatedAbility`/instant-or-sorcery's own effect, and the shared tail
//! (`StackObjectResolved` push + `ReplacementCheck` transition).
//!
//! `resolve_ability_effect` is a thin `match &top.effect { ... }` dispatcher,
//! one line per arm, delegating the real body of each `AbilityEffect`
//! variant to a themed helper in a sibling file — see each sibling's own
//! module doc comment for which cluster it owns:
//! - `direct_effects`: the synchronous-only arms (never pause mid-
//!   resolution) — `Modal`, `GainLife`, `Damage`, `Counters`, `Pump`,
//!   `GainControl`, `Regenerate`.
//! - `destruction`: the always-pausing zone-move arms — `Sacrifice`,
//!   `Destroy`, `DestroyAll`, `Exile`, `SacrificeRandom`.
//! - `discard`: `DiscardRandom` (`phase5/mass-choice-effects`) — CR 701.9's
//!   discard verb had no resolution-time producer at all before this slice.
//! - `each_player`: `ForEachPlayer` (`phase5/mass-choice-effects`, CR 101.4)
//!   — re-invokes a script function once per player in APNAP order.
//! - `library_effects`: the library-touching arms — `DrawCards`, `Shuffle`,
//!   `Scry`, `Surveil`, `Search`.
//! - `tokens`: `CreateToken` (plus the token-definition-building free
//!   functions only it calls).
//! - `misc_effects`: `Attach`, `BecomeCopy`, `ExchangeTextBoxes`, and the
//!   `AddMana`/`AddManaChoice`/`Script` no-ops plus the `None`-effect script-
//!   bridge fallback.
//! - `reanimation`: `ReturnSelfWithCounter` (CR 702.79a Persist/702.93a
//!   Undying).
//! - `madness`: `MadnessCastOrDiscard` (CR 702.35a Madness's own "cast or
//!   discard" decision).
//! - `cascade`: `Cascade` (CR 702.85a).
//! - `counter`: `Counter` (CR 701.6a) — removes a *different*, still-present
//!   `StackObject` from `state.stack` by matching `stack_id`, not a
//!   zone-move arm on `top` itself like `destruction`'s cluster.
//!
//! Every extracted effect-resolver returns `std::ops::ControlFlow<StepStatus,
//! Vec<GameEventKind>>` — `Break(status)` for "this resolution is pausing,
//! hand this `StepStatus` straight back up," `Continue(events)` for "here's
//! the accumulated event list so far, resolution keeps going toward the
//! shared tail." Only this file's own `resolve_stack_top`/
//! `resolve_ability_effect` ever unwind a `ControlFlow` — a plain `match` at
//! each call site, no nightly `Try`/`?`-on-`ControlFlow` needed.

use std::ops::ControlFlow;

use crate::commands;
use crate::engine_context::{PipelineContext, PipelineExit};
use crate::types::ability::{AbilityEffect, InterveningIfSpec};
use crate::types::card::CardTypeSet;
use crate::types::event::GameEventKind;
use crate::types::ids::Target;
use crate::types::stack::{StackEntryKind, StackObject};
use crate::types::target::{TargetRequirement, TargetSpec};
use crate::types::zone::ZoneKind;

mod cascade;
mod counter;
mod destruction;
mod direct_effects;
mod discard;
mod each_player;
mod library_effects;
mod madness;
mod misc_effects;
mod permanent_entry;
mod reanimation;
mod tokens;

use super::{Engine, ProcessState, StepStatus};

impl Engine {
    /// `ProcessState::ResolveStackTop` — CR 608.2m/601.2c/602.2b resolution
    /// of the top object on the stack. See this module's own doc comment for
    /// the overall shape.
    pub(super) fn resolve_stack_top(&mut self) -> StepStatus {
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
        let Some(mut top) = self.state.stack.pop() else {
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
                // CR 202.3: this resolving spell/ability's own announced X
                // (already resolved at cast time) - same value a
                // `mana_value` filter's own `X` bound was checked against
                // when this was cast/activated.
                top.x_value,
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
            Some(TargetSpec::Multiple(reqs)) => {
                commands::split_targets_by_requirement(&self.state, &self.script_runtime, reqs, &top.targets, top.controller, top.source, top.x_value)
                    .map(|chunks| {
                        reqs.iter()
                            .zip(chunks)
                            .flat_map(|(req, chunk)| chunk.into_iter().filter(|t| is_legal_now(t, req)).collect::<Vec<_>>())
                            .collect()
                    })
                    .unwrap_or_default()
            }
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
            Some(TargetSpec::Multiple(reqs)) => {
                commands::split_targets_by_requirement(&self.state, &self.script_runtime, reqs, &top.targets, top.controller, top.source, top.x_value)
                    .map(|chunks| {
                        reqs.iter()
                            .zip(chunks)
                            .map(|(req, chunk)| chunk.into_iter().filter(|t| is_legal_now(t, req)).collect())
                            .collect()
                    })
                    .unwrap_or_default()
            }
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
            Some(InterveningIfSpec::Primitive(p)) => {
                !p.holds(&self.state, top.controller, top.source, top.source_lki.as_ref())
            }
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
                if let Some(event) = self.move_resolving_spell(top.stack_id, destination, false, false) {
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
                match self.resolve_permanent_entry(top, &legal_targets, events) {
                    ControlFlow::Break(status) => return status,
                    ControlFlow::Continue((resumed_top, resumed_events)) => {
                        top = resumed_top;
                        events = resumed_events;
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
                match self.resolve_ability_effect(&top, &legal_targets, &modal_chunk_legals, events) {
                    ControlFlow::Break(status) => return status,
                    ControlFlow::Continue(resumed) => events = resumed,
                }
            }
            events.push(GameEventKind::StackObjectResolved { stack_id: top.stack_id });
        }
        self.process_state =
            ProcessState::ReplacementCheck { remaining: events.into(), checked: Vec::new() };
        StepStatus::Progressed
    }

    /// The `top.effect` master dispatch — one line per `AbilityEffect`
    /// variant, delegating the real body to a themed helper in a sibling
    /// file. See this module's own doc comment for which file owns which
    /// cluster.
    fn resolve_ability_effect(
        &mut self,
        top: &StackObject,
        legal_targets: &[Target],
        modal_chunk_legals: &[Vec<Target>],
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
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
                self.resolve_modal_effect(top, effects, modal_chunk_legals, events)
            }
            Some(AbilityEffect::GainLife(amount)) => self.resolve_gain_life_effect(top, *amount, events),
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
                self.resolve_damage_effect(top, legal_targets, *amount, events)
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
            Some(AbilityEffect::Sacrifice) => self.resolve_sacrifice_effect(top, legal_targets),
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
            Some(AbilityEffect::Destroy) => self.resolve_destroy_effect(top, legal_targets),
            // CR 509.1h/702.22i: direct CombatState mutation via
            // commands::mark_effect_blocked, same "already
            // happened, no CommitEvents needed" convention Pump/
            // GainControl already use - see AbilityEffect::
            // BecomeBlocked's own doc comment for the full shape.
            Some(AbilityEffect::BecomeBlocked) => self.resolve_become_blocked_effect(legal_targets, events),
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
            Some(AbilityEffect::DestroyAll(filter)) => self.resolve_destroy_all_effect(top, filter),
            // CR 701.21/CR 701.something "at random": count candidates
            // drawn without replacement via next_rng, same deterministic
            // ledger-logged source shuffle_library already uses - see
            // AbilityEffect::SacrificeRandom's own doc comment.
            Some(AbilityEffect::SacrificeRandom { filter, count }) => {
                self.resolve_sacrifice_random_effect(top, filter, *count)
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
            Some(AbilityEffect::Exile) => self.resolve_exile_effect(top, legal_targets),
            // CR 701.6a: removes the *targeted* StackObject (a different
            // stack_id than top.stack_id) from state.stack directly - see
            // AbilityEffect::Counter's own doc comment and
            // counter::resolve_counter_effect for the full shape.
            Some(AbilityEffect::Counter) => self.resolve_counter_effect(legal_targets, events),
            // CR 701.9b: same "count candidates drawn without replacement
            // via next_rng" shape as SacrificeRandom above, from the
            // resolving StackObject's own controller's hand instead of the
            // battlefield - see AbilityEffect::DiscardRandom's own doc
            // comment.
            Some(AbilityEffect::DiscardRandom { filter, count }) => self.resolve_discard_random_effect(top, filter, *count),
            // CR 101.4: invokes top.operation (a real script function
            // handle, not the usual ScriptHandle(0) placeholder - see
            // AbilityEffect::ForEachPlayer's own doc comment) once per
            // player in APNAP order, accumulating sacrifice_chosen/
            // discard_chosen ids across every player before performing any
            // real zone move, so CR 101.4's "then the actions happen
            // simultaneously" holds.
            Some(AbilityEffect::ForEachPlayer(wrapped)) => self.resolve_for_each_player_effect(top, wrapped, events),
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
                self.resolve_counters_effect(legal_targets, kind, *amount, events)
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
                self.resolve_pump_effect(legal_targets, *power, *toughness, duration, events)
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
                self.resolve_gain_control_effect(top, legal_targets, duration, events)
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
            Some(AbilityEffect::Regenerate) => self.resolve_regenerate_effect(legal_targets, events),
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
                self.resolve_draw_cards_effect(top, *amount, events)
            }
            // CR 701.19b: no target, always top.controller
            // (same "controller only" shape DrawCards/
            // GainLife use). shuffle_library already mutates
            // state and logs its own LibraryShuffled/RngDrawn
            // events - no events pushed here, same "already
            // happened" convention DrawCards established.
            Some(AbilityEffect::Shuffle) => self.resolve_shuffle_effect(top, events),
            // CR 701.22a: peek the top n library ids
            // read-only (no mutation - the actual reorder
            // needs the controller's per-card decision) and
            // pause the whole pipeline run right here,
            // exactly like the NeedsChoice branch below does
            // for Game.SelectTargets. CR 701.22b: n == 0 (or
            // an empty library) falls straight through -
            // no pause, no event.
            Some(AbilityEffect::Scry(n)) => self.resolve_scry_effect(top, *n, events),
            // CR 701.25a: same pause shape as Scry above -
            // submit_surveil_choice is what actually differs
            // (graveyard destination instead of bottom).
            Some(AbilityEffect::Surveil(n)) => self.resolve_surveil_effect(top, *n, events),
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
            Some(AbilityEffect::Search { filter, count, destination, shuffle_after }) => self
                .resolve_search_effect(top, filter, *count, *destination, *shuffle_after, events),
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
                self.resolve_become_copy_effect(top, exceptions, events)
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
            Some(AbilityEffect::ExchangeTextBoxes) => self.resolve_exchange_text_boxes_effect(top, events),
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
                self.resolve_create_token_effect(top, spec, *count, events)
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
            Some(AbilityEffect::Attach) => self.resolve_attach_effect(top, legal_targets, events),
            // CR 702.79a/702.93a Persist/Undying - see this variant's own
            // doc comment in ability.rs for the full shape (locates the
            // graveyard object via lineage, mints a new CR 400.7 object on
            // the battlefield, self-registers its abilities, and pushes
            // the ability's own counter as a plain CounterAdded event).
            Some(AbilityEffect::ReturnSelfWithCounter(kind)) => {
                self.resolve_return_self_with_counter_effect(top, kind.clone(), events)
            }
            // CR 702.35a Madness - see this variant's own doc comment in
            // ability.rs and `madness::resolve_madness_effect`'s own doc
            // comment for the full shape (locates the exiled object via
            // lineage, always pauses for a real cast-or-decline choice).
            Some(AbilityEffect::MadnessCastOrDiscard) => self.resolve_madness_effect(top, events),
            // CR 702.85a Cascade - see this variant's own doc comment in
            // ability.rs and `cascade::resolve_cascade_effect`'s own doc
            // comment for the full shape (synchronous library walk, always
            // pauses for a real cast-or-decline choice once it finds a
            // candidate or exhausts the library).
            Some(AbilityEffect::Cascade) => self.resolve_cascade_effect(top, events),
            // AddMana/AddManaChoice are only ever set on a mana
            // ability, which resolves immediately and never
            // reaches the stack (CR 605.1a/605.3b) - no
            // StackObject of any kind should ever carry either
            // variant. Defensive no-op rather than a panic.
            //
            // `CastSpell` always unpacks this into
            // `operation`/`effect: None` at push time (see
            // `AbilityEffect::Script`'s own doc comment) - no
            // `StackObject` should ever actually carry this
            // variant on its `effect` field. Defensive
            // no-op, same stance `AddMana`/`AddManaChoice`
            // just above take for their own unreachable case.
            //
            // `None`: the script-bridge fallback - see
            // `misc_effects::resolve_misc_or_script_effect`'s own
            // doc comment for the real body.
            Some(AbilityEffect::AddMana(_))
            | Some(AbilityEffect::AddManaChoice { .. })
            | Some(AbilityEffect::Script(_))
            | None => self.resolve_misc_or_script_effect(top, events),
        }
    }
}
