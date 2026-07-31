use crate::script::ScriptRuntime;
use crate::types::ability::CopyExceptions;
use crate::types::card::CardTypeSet;
use crate::types::cost::FilterController;
use crate::types::counter::CounterKind;
use crate::types::effect::{ConsumptionPolicy, ReplacementPayload, ReplacementSpec};
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::mana::ColorSet;
use crate::types::zone::ZoneKind;

/// One CR 616.1 ordering-choice candidate. Two kinds can match the same
/// event: a real registered `ReplacementEffect` (`Registered`, identified by
/// its own `source`), or CR 702.16e protection's damage prevention
/// (`Protection`, identified by the *protected object's own id* — protection
/// isn't a `state.replacement_effects` entry at all, see `protection_prevents`'s
/// own doc comment). Previously both were represented as a bare `ObjectId`,
/// which silently collided whenever a protected object also had its own
/// registered effect with itself as `source` matching the same event —
/// `apply_replacements_from`'s candidate-list builder would then treat them
/// as "the same candidate" and never offer the real 2-way choice, and
/// `apply_one_replacement_or_protection`'s dispatch always resolved the
/// collision toward the registered effect. This enum makes the two kinds
/// distinguishable through the whole pause/resume pipeline
/// (`ReplacementOutcome::NeedsOrderingChoice`, `ProcessState`/
/// `StepStatus::AwaitingReplacementChoice`, `Engine::submit_replacement_choice`)
/// so they can never be silently conflated again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplacementCandidate {
    Registered(ObjectId),
    Protection(ObjectId),
}

/// What `apply_replacements` produced for one proposed event. Not just a
/// `GameEventKind`: CR 616.1 has the *affected player* choose which
/// replacement applies first when more than one matches the same event —
/// a player decision, not something a pure transform can resolve on its
/// own — so this type makes that case a value the caller (`engine.rs`)
/// must handle rather than something `apply_replacements` could silently
/// resolve "for" the player or panic on.
pub enum ReplacementOutcome {
    /// Zero or exactly one `ReplacementEffect` matched (after however many
    /// rewrite/re-match passes CR 616's stabilization requirement needed) —
    /// this is the event's final form, ready for `CommitEvents`.
    Resolved(GameEventKind),
    /// More than one replacement/prevention effect matches `event` and
    /// `affected_player` must choose which applies first (CR 616.1) before
    /// resolution can continue. `candidates` are `ReplacementCandidate`
    /// values, not bare `ObjectId`s — see that type's own doc comment for
    /// why (a registered effect and protection can share the same
    /// identifying object id without being the same candidate). The engine
    /// must surface this as an actual pause — see `engine::ProcessState::
    /// AwaitingReplacementChoice` — not fold it back into a synchronous
    /// transform. `excluded` carries forward every effect already applied to
    /// this same event earlier in this same resolution (see `apply_
    /// replacements`'s own doc comment for why) — the caller must thread it
    /// back into whatever continues this resolution after the choice is
    /// made, not start a fresh empty exclusion set.
    NeedsOrderingChoice {
        affected_player: PlayerId,
        event: GameEventKind,
        candidates: Vec<ReplacementCandidate>,
        excluded: Vec<ReplacementCandidate>,
        /// Carried forward so a resumed `apply_replacements_from` call (once
        /// the player answers) rechecks the rewritten event with the same
        /// "is this genuinely a destroy" meaning it started with — see
        /// `ReplacementEffect::matches`'s own doc comment.
        is_destroy: bool,
    },
}

/// CR 616.1's "affected player" for a matched-more-than-once event — the
/// permanent's controller if the event's affected slot is an object, the
/// player directly if it's a player. Only ever called against an event
/// `apply_replacements` already confirmed at least one `ReplacementEffect`
/// matched, so the two constructible `EventFilter` shapes (`DamageDealt`,
/// `WouldDie` -> `ZoneChange`) cover every real call; the fallback exists
/// only so this stays a total function rather than something that could
/// panic if a future `Script`-backed filter ever matches a different event
/// shape.
fn affected_player_for(event: &GameEventKind, state: &GameState, script_runtime: &ScriptRuntime) -> PlayerId {
    let fallback = || state.turn_order.first().copied().expect("a game always has players");
    match event {
        GameEventKind::DamageDealt { target, .. } => match target {
            Target::Player(pid) => *pid,
            Target::Object(id) => state
                .object(*id)
                .map(|_| crate::layers::effective_controller(state, script_runtime, *id))
                .unwrap_or_else(fallback),
        },
        GameEventKind::ZoneChange { object, .. } => state
            .object(*object)
            .map(|_| crate::layers::effective_controller(state, script_runtime, *object))
            .unwrap_or_else(fallback),
        _ => fallback(),
    }
}

/// Applies `source`'s `ReplacementEffect` to `event` (assumes `matches`
/// already confirmed it applies) and consumes it per its `ConsumptionPolicy`
/// (CR 614.13-style shields with a counter budget) — `OneShot`/an exhausted
/// `LimitedUses` removes the entry from `state.replacement_effects`
/// entirely, `LimitedUses(n)` with `n > 1` decrements in place, `Repeatable`
/// is untouched. Returns `event` unchanged if `source` doesn't actually name
/// a registered effect (shouldn't happen given the callers below, but this
/// keeps the function total rather than panicking on a stale id).
pub(crate) fn apply_one_replacement(
    state: &mut GameState,
    source: ObjectId,
    event: GameEventKind,
) -> GameEventKind {
    let Some(idx) = state.replacement_effects.iter().position(|r| r.source == source) else {
        return event;
    };
    apply_replacement_at_index(state, idx, event)
}

/// Index-based real body of `apply_one_replacement` above — operates
/// directly on `state.replacement_effects[idx]` rather than re-deriving the
/// index from a `source: ObjectId`, which is only safe when `source`
/// uniquely identifies one registered effect. `apply_enters_battlefield_
/// modifiers`'s own loop is the one caller that can't assume that (a single
/// re-registered source can now carry more than one matching
/// battlefield-entry-modifier payload at once — see
/// `is_battlefield_entry_modifier`'s own doc comment for the bug this
/// closes), so it tracks real indices itself instead.
fn apply_replacement_at_index(state: &mut GameState, idx: usize, event: GameEventKind) -> GameEventKind {
    let rewritten = state.replacement_effects[idx].apply(event);
    match state.replacement_effects[idx].consumption {
        ConsumptionPolicy::OneShot => {
            state.replacement_effects.remove(idx);
        }
        ConsumptionPolicy::LimitedUses(n) => {
            if n <= 1 {
                state.replacement_effects.remove(idx);
            } else {
                state.replacement_effects[idx].consumption = ConsumptionPolicy::LimitedUses(n - 1);
            }
        }
        ConsumptionPolicy::Repeatable => {}
    }
    rewritten
}

/// Dispatch for a chosen `ReplacementCandidate` out of `apply_replacements_from`'s
/// `matching` list (either the loop's own single-match branch, or a resumed
/// `NeedsOrderingChoice` pause via `Engine::submit_replacement_choice`) — a
/// direct match on which kind was chosen, not an `ObjectId` lookup. CR
/// 702.16e protection's damage prevention (`protection_prevents`) has no
/// `state.replacement_effects` entry of its own to look up (it's a live
/// characteristics query, not a registered effect — see that function's own
/// doc comment for why); `ReplacementCandidate::Protection` zeroes the
/// damage directly instead. Not rechecked against `protection_prevents`
/// again here: exactly like `apply_one_replacement`'s own "trusts `matches`
/// already confirmed this" contract, whoever built `matching` already
/// established eligibility, and `Engine::submit_replacement_choice`
/// independently validates `chosen` against `candidates` before this is ever
/// called — a stale/invalid candidate can't reach this function at all.
///
/// `Registered` and `Protection` naming the *same* underlying object id no
/// longer collide (previously both were a bare `ObjectId`, and the
/// registered effect always won the dispatch when they shared an id — see
/// `ReplacementCandidate`'s own doc comment for the bug this fixes) — each
/// is now its own distinct variant, matched independently.
pub(crate) fn apply_one_replacement_or_protection(
    state: &mut GameState,
    chosen: ReplacementCandidate,
    event: GameEventKind,
) -> GameEventKind {
    match chosen {
        ReplacementCandidate::Registered(source) => apply_one_replacement(state, source, event),
        ReplacementCandidate::Protection(_) => match event {
            GameEventKind::DamageDealt { source, target, is_combat_damage, .. } => {
                GameEventKind::DamageDealt { source, target, amount: 0, is_combat_damage }
            }
            other => other,
        },
    }
}

/// CR 702.16a/615.1/616.1: whether protection prevents `event`'s damage —
/// checked as an OR of all six protected qualities (color, card type,
/// subtype, name, "everything," or a specific player), same as before. A
/// *predicate*, not a mutation, unlike every other payload in this file:
/// protection isn't a `state.replacement_effects` entry a card registers
/// (`layers::effective_characteristics()`'s `protection_colors`/
/// `protection_card_types`/`protection_subtypes`/`protection_names`/
/// `protection_everything`/`protection_players` are a live characteristics
/// query, re-derived every call, not a registered struct with a `source`
/// field to look up) — so it can't sit in `state.replacement_effects`
/// itself the way a real `ReplacementEffect` does. It still has to
/// participate in the *same* CR 616.1 ordering-choice decision a registered
/// effect does, though: `apply_replacements_from` folds this predicate's
/// result into its own `matching` candidate list (identified by the
/// protected object's own id, not a registered effect's source — see
/// `apply_one_replacement_or_protection`'s own doc comment for how that's
/// dispatched) rather than applying it unconditionally ahead of the loop
/// the way this engine used to (CR 616.1e: protection's prevention has no
/// fixed priority against another applicable replacement/prevention effect
/// touching the same event — the affected player's choice is real and can
/// produce a different outcome depending on order, e.g. a competing
/// `RedirectDamage` moving the damage to an unprotected object *before*
/// protection ever gets a chance to zero it).
///
/// Returns the protected object's `ObjectId` if protection would zero
/// `event`'s damage, `None` otherwise (non-`DamageDealt` event, a
/// `Target::Player`, or an unresolvable source/target — none of these have
/// protection to check). Re-evaluated fresh every loop iteration by the
/// caller (not memoized), same "recheck against a possibly-`RedirectDamage`-
/// changed target" reasoning the old unconditional `apply_protection` this
/// replaces already had.
fn protection_prevents(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    event: &GameEventKind,
) -> Option<ObjectId> {
    let GameEventKind::DamageDealt { source, target, .. } = event else { return None };
    let Target::Object(target_id) = target else { return None };
    let (Some(source_obj), Some(target_obj)) = (state.object(*source), state.object(*target_id)) else {
        return None;
    };
    let source_controller = source_obj.controller;
    let target_controller = target_obj.controller;
    let target_characteristics = crate::layers::effective_characteristics(state, script_runtime, *target_id);
    let source_characteristics = crate::layers::effective_characteristics(state, script_runtime, *source);
    let protected = target_characteristics.protection_everything
        || target_characteristics.protection_colors.intersects(source_characteristics.colors)
        || target_characteristics.protection_card_types.intersects(source_characteristics.card_types)
        || target_characteristics
            .protection_subtypes
            .iter()
            .any(|s| source_characteristics.subtypes.contains(s))
        || target_characteristics.protection_names.iter().any(|n| *n == source_characteristics.name)
        || target_characteristics.protection_players.iter().any(|who| match who {
            FilterController::You => source_controller == target_controller,
            FilterController::Opponent => source_controller != target_controller,
            FilterController::Any => false,
        });
    protected.then_some(*target_id)
}

/// CR 111.8: a token that has already left the battlefield can't move to
/// another zone (or back onto the battlefield) — "if such a token would
/// change zones, it remains in its current zone instead." Hardcoded ahead
/// of the registered-effects match (unlike protection above, this one still
/// takes that stance — see its own "unreachable via real gameplay" note
/// below for why the CR 616.1 ordering-choice exposure this pattern risks
/// stays purely theoretical for this one, not worth the same fix yet) —
/// this applies to every token unconditionally, not something a card
/// grants. Checked against `state.object(object)`'s own
/// *live* zone (not the proposal's own `from` field, though the two should
/// always agree) — the token's identity/zone don't change until this event
/// actually commits, so this is always the token's true current location.
///
/// Only rewrites `to`, mirroring `ReplacementPayload::ReplaceZoneDestination`'s
/// own shape — the caller still commits a real (no-op) `ZoneChange`, same as
/// every other replacement-rewritten proposal. Doesn't touch the token's
/// own CR 111.7 cessation (`sba::collect_ceasing_tokens`/
/// `commit_token_cessation`, checked separately by state-based actions) —
/// that still fires on the next SBA check regardless of whether this
/// function blocked a second move in the meantime.
///
/// Unreachable via real gameplay today, same as several other CR-accurate
/// but currently-unexercisable checks in this codebase (see e.g. CR
/// 303.4f/h/i/k/m's own note): nothing in this engine can propose a
/// `ZoneChange` whose `from` is anything but `Battlefield` for a token — no
/// "return from graveyard"/"cast from graveyard" path applies to tokens
/// (they aren't cards, so flashback-style alternative costs can't target
/// one), and CR 111.7 removes a token from any non-battlefield zone before
/// most engine code gets a second chance to move it. Proven correct via
/// direct `apply_replacements` construction instead (see this file's own
/// test module) rather than an end-to-end scenario.
fn apply_token_zone_lock(state: &GameState, event: GameEventKind) -> GameEventKind {
    let GameEventKind::ZoneChange { object, from, to, last_known_info } = event else {
        return event;
    };
    let Some(obj) = state.object(object) else {
        return GameEventKind::ZoneChange { object, from, to, last_known_info };
    };
    if obj.is_token && obj.zone != ZoneKind::Battlefield {
        return GameEventKind::ZoneChange { object, from, to: from, last_known_info };
    }
    GameEventKind::ZoneChange { object, from, to, last_known_info }
}

/// Is `spec` one of the "how a permanent enters" battlefield-entry-modifier
/// payloads (`EntersTapped`, `EntersWithCounters` and its Bloodthirst/
/// Sunburst/Devour/Amplify siblings, `EntersAsCopy`)? These are all
/// deliberately excluded from `apply_replacements_from`'s own CR 616.1
/// ordering-choice logic below — every one of them is either applied
/// directly by `apply_enters_battlefield_modifiers` (which already
/// documents *why* no ordering is needed: `EntersTapped` is idempotent,
/// `EntersWithCounters`-family payloads are additive, so unlike a
/// `ReplaceZoneDestination`-style rewrite, no ordering of multiple matches
/// changes the final result) or by one of the dedicated `pending_*_choice`
/// functions (Devour/Amplify/`EntersAsCopy`, which need their own real
/// player choice, not a CR 616.1 ordering pick).
///
/// Bug this closes: `ReplacementEffect::matches` has no way to know a
/// payload is meant to be handled exclusively by that other machinery — so
/// once the entering object's own raw `ZoneChange` event reaches
/// `ReplacementCheck`'s generic pass (as it always does, alongside whatever
/// `apply_enters_battlefield_modifiers` already produced), it would
/// independently re-match every one of these payloads *again*, and two or
/// more matching the same event on the same source id — e.g. a single
/// creature combining `EntersTapped` and `EntersWithCounters`, exactly CR
/// 707.2's own Skyshroud Behemoth worked example — spuriously triggered a
/// CR 616.1 ordering pause for two effects that were never supposed to be
/// ordered against each other in the first place. Caught by a test giving a
/// Clone-copied creature both at once (`phase5/clone-enters-as-copy`) — no
/// prior test had ever combined the two on one permanent.
fn is_battlefield_entry_modifier(spec: &ReplacementSpec) -> bool {
    matches!(
        spec,
        ReplacementSpec::Primitive {
            payload: ReplacementPayload::EntersTapped
                | ReplacementPayload::EntersWithCounters { .. }
                | ReplacementPayload::EntersWithCountersBloodthirst { .. }
                | ReplacementPayload::EntersWithCountersSunburst
                | ReplacementPayload::EntersWithCountersDevour { .. }
                | ReplacementPayload::EntersWithCountersAmplify { .. }
                | ReplacementPayload::EntersAsCopy(_),
            ..
        }
    )
}

/// Rewrites/prevents `proposed` per CR 614/615/616, applying every matching
/// `ReplacementEffect` in `state.replacement_effects` before the event is
/// allowed to commit, re-checking after each rewrite in case the rewritten
/// form matches a *different* replacement (CR 616.1's rewrite-stabilization
/// requirement). Thin wrapper over `apply_replacements_from` starting with
/// nothing excluded — the entry point every fresh event (one that hasn't
/// been through any replacement checking yet) uses.
pub fn apply_replacements(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    proposed: GameEventKind,
    is_destroy: bool,
) -> ReplacementOutcome {
    apply_replacements_from(state, script_runtime, proposed, is_destroy, Vec::new())
}

/// Real body of `apply_replacements` — separated out so `Engine::
/// submit_replacement_choice` (engine.rs) can resume an in-progress
/// resolution with the same `excluded` set a paused `NeedsOrderingChoice`
/// already accumulated, rather than starting a fresh one.
///
/// CR 616.1: "an individual replacement effect can apply only once to a
/// given event" — a `Repeatable`/multi-`LimitedUses` effect isn't excluded
/// from being asked again about a *different* event later, but within one
/// event's resolution it must never re-match its own (possibly unchanged)
/// rewritten output, or the CR 616 stabilization loop never terminates
/// (e.g. a damage-prevention effect that zeroes an already-zero amount
/// would otherwise keep "matching" the same event forever). `excluded`
/// tracks exactly that — every effect already applied to `event` earlier in
/// this same resolution, appended to as each single match applies.
pub(crate) fn apply_replacements_from(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    proposed: GameEventKind,
    is_destroy: bool,
    mut excluded: Vec<ReplacementCandidate>,
) -> ReplacementOutcome {
    let mut event = proposed;
    loop {
        // CR 111.8: still hardcoded ahead of the match below - see
        // apply_token_zone_lock's own doc comment for why this one keeps
        // that stance while protection (below) no longer does.
        event = apply_token_zone_lock(state, event);

        // CR 702.16a/616.1: protection's damage prevention now folds into
        // `matching` as a real candidate (identified by the protected
        // object's own id, wrapped so it's distinguishable from a registered
        // effect sharing the same id - see `ReplacementCandidate`'s own doc
        // comment) rather than applying unconditionally ahead of the loop -
        // see `protection_prevents`'s own doc comment for why.
        let protection_candidate = protection_prevents(state, script_runtime, &event)
            .map(ReplacementCandidate::Protection)
            .filter(|c| !excluded.contains(c));

        if state.replacement_effects.is_empty() && protection_candidate.is_none() {
            // Genuinely nothing to match against - safe to resolve unchanged.
            return ReplacementOutcome::Resolved(event);
        }
        let mut matching: Vec<ReplacementCandidate> = state
            .replacement_effects
            .iter()
            .filter(|r| {
                !excluded.contains(&ReplacementCandidate::Registered(r.source))
                    && !is_battlefield_entry_modifier(&r.spec)
                    && r.matches(&event, is_destroy, state, script_runtime)
            })
            .map(|r| ReplacementCandidate::Registered(r.source))
            .collect();
        if let Some(candidate) = protection_candidate {
            // `Registered(X)`/`Protection(X)` are distinct `ReplacementCandidate`
            // values even when they share the same underlying object id, so
            // this is always pushed as a genuine, separate candidate now -
            // the fix for the collision `ReplacementCandidate`'s own doc
            // comment names (previously this was skipped whenever `matching`
            // already contained the bare `protected_id`, silently collapsing
            // two different candidates into one and never offering the real
            // CR 616.1 choice between them).
            matching.push(candidate);
        }

        match matching.as_slice() {
            [] => return ReplacementOutcome::Resolved(event),
            [only] => {
                // Exactly one match - apply, exclude it from matching again
                // against this same event, and re-check the rewritten form
                // (CR 616's stabilization requirement) by looping rather
                // than returning immediately.
                event = apply_one_replacement_or_protection(state, *only, event);
                excluded.push(*only);
            }
            _ => {
                let affected_player = affected_player_for(&event, state, script_runtime);
                return ReplacementOutcome::NeedsOrderingChoice {
                    affected_player,
                    event,
                    candidates: matching,
                    excluded,
                    is_destroy,
                };
            }
        }
    }
}

/// CR 603.6d/614.1c/113.6h: applies every registered `EntersTapped`/
/// `EntersWithCounters` effect matching `object`'s own move from `from` onto
/// the battlefield — the "how a permanent enters" replacement-effect family,
/// covering both a card's own self-registered ability (CR 113.6h: functions
/// as the object is entering, before it's a real battlefield permanent in
/// the trigger-collection sense) and an externally granted one (e.g. "lands
/// your opponents control enter tapped"). Deliberately not routed through
/// `apply_replacements`/`apply_replacements_from`: those rewrite fields on
/// the proposed `GameEventKind` itself, but neither payload has anything on
/// `GameEventKind::ZoneChange` to rewrite (see `ReplacementPayload::
/// EntersTapped`'s own doc comment).
///
/// The two payloads apply differently, deliberately: `EntersTapped` mutates
/// the already-constructed `GameObject` at `object` directly, right here
/// (mirrors `SearchDestination::Battlefield { tapped }`'s own precedent —
/// no dedicated event exists for "this became tapped," so there's nothing
/// to push). `EntersWithCounters` does *not* mutate here — it only proposes
/// a `GameEventKind::CounterAdded`, same "Rust decides by applying the
/// event, not the proposer" split `AbilityEffect::Counters` already
/// established (`ResolveStackTop` proposes, `CommitEvents` is what actually
/// calls `counters.add`). The caller MUST push the returned events onto a
/// path that reaches `CommitEvents` - mutating both here *and* relying on
/// `CommitEvents`'s existing generic `CounterAdded` handler would double
/// -apply the same counters.
///
/// Called only after `object` is already inserted into `state.objects` *and*
/// its own `run_card_script` pass has run (so a self-registered effect is
/// already sitting in `state.replacement_effects` by the time this runs —
/// see `docs/lua/card-script-basics.md`'s "Registration timing" note for why that
/// ordering already holds at every real call site, without needing a
/// propose-before-the-object-exists split the way `commit_zone_move`'s own
/// callers need).
///
/// CR 616.1's ordering choice (`ReplacementOutcome::NeedsOrderingChoice`) is
/// deliberately not invoked here even when multiple effects match the same
/// entry: `EntersTapped` is idempotent and `EntersWithCounters` is additive,
/// so unlike a `ReplaceZoneDestination`-style rewrite, no ordering of
/// multiple matches ever changes the final result — every match just applies.
/// TODO: if a future payload pairs with this same `WouldMoveZone(_,
/// Battlefield)` filter and *isn't* order-independent, this function's
/// "apply every match, no pause" stance stops being correct and needs a real
/// ordering-choice pause of its own.
///
/// Returns the `GameEventKind::CounterAdded` events produced (one per
/// matching `EntersWithCounters` effect) for the caller to append to its own
/// event list, same ledger-visibility convention `AbilityEffect::Counters`
/// already established.
///
/// `colors_spent` (CR 702.44, Sunburst): the distinct colors of mana spent
/// to pay for `object`'s own cast, if any — sourced from `StackObject::
/// costs_paid`, which only exists while a spell is still on the stack, so
/// only the resolve-spell-from-stack call site can supply a real value.
/// `ColorSet::empty()` at every other call site (`PlayLand`, `Search`) is
/// exactly correct, not a placeholder — CR 702.44b already denies Sunburst's
/// counters whenever the object isn't entering from the stack as a resolving
/// spell, which an empty set naturally does too (indistinguishable from "no
/// colored mana was spent," and both cases mean the same thing here: no
/// counters).
pub fn apply_enters_battlefield_modifiers(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    object: ObjectId,
    from: ZoneKind,
    colors_spent: ColorSet,
) -> Vec<GameEventKind> {
    // `GameEventKind` isn't `Clone` - rebuilt fresh at each use site below
    // instead of cloned once, same shape every time (`object`/`from` are
    // both `Copy`, `last_known_info` is always `None` - this candidate is
    // never actually committed, only matched against).
    let candidate = || GameEventKind::ZoneChange { object, from, to: ZoneKind::Battlefield, last_known_info: None };
    // Real indices, not just `source` ids - a single re-registered source
    // can carry more than one matching payload at once (e.g. a Clone-copied
    // creature with both `EntersTapped` and `EntersWithCounters`, CR 707.2's
    // own Skyshroud Behemoth example), and `source` alone can't distinguish
    // between them for the re-lookup below.
    let matching: Vec<usize> = state
        .replacement_effects
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            matches!(
                &r.spec,
                ReplacementSpec::Primitive {
                    payload: ReplacementPayload::EntersTapped
                        | ReplacementPayload::EntersWithCounters { .. }
                        | ReplacementPayload::EntersWithCountersBloodthirst { .. }
                        | ReplacementPayload::EntersWithCountersSunburst,
                    ..
                }
            )
        })
        .filter(|(_, r)| r.matches(&candidate(), false, state, script_runtime))
        .map(|(i, _)| i)
        .collect();

    let mut counter_events = Vec::new();
    // Reverse order: a OneShot/LimitedUses(1) consumption's removal below
    // never invalidates a not-yet-processed lower index this way.
    for idx in matching.into_iter().rev() {
        let payload = match &state.replacement_effects[idx].spec {
            ReplacementSpec::Primitive { payload, .. } => payload.clone(),
            ReplacementSpec::Script(_) => continue,
        };
        match payload {
            ReplacementPayload::EntersTapped => {
                if let Some(obj) = state.object_mut(object) {
                    obj.tapped = true;
                }
            }
            // Unlike `EntersTapped` above, this doesn't mutate `object`
            // directly - it only proposes the event, same "Rust decides by
            // applying the event, not the proposer" split `AbilityEffect::
            // Counters` already established (`ResolveStackTop` pushes
            // `CounterAdded`; `CommitEvents` is what actually calls
            // `counters.add`). The caller MUST push this into its own
            // event list on a path that reaches `CommitEvents` - mutating
            // here too would double-apply once that generic handler runs.
            ReplacementPayload::EntersWithCounters { kind, amount } => {
                counter_events.push(GameEventKind::CounterAdded { object, kind, amount });
            }
            // CR 702.54a: same event-proposal shape as `EntersWithCounters`
            // above, gated on `opponent_damaged_this_turn` - `object` is
            // already present in `state.objects` by this point (same
            // "already-constructed GameObject" precondition every other
            // arm here relies on), so its controller is read directly
            // rather than threaded in as a parameter.
            ReplacementPayload::EntersWithCountersBloodthirst { kind, amount } => {
                let controller = state.object(object).map(|o| o.controller);
                if controller.is_some_and(|c| opponent_damaged_this_turn(state, c)) {
                    counter_events.push(GameEventKind::CounterAdded { object, kind, amount });
                }
            }
            // CR 702.44a/b: both `kind` and `amount` are derived here rather
            // than carried by the payload - see `EntersWithCountersSunburst`'s
            // own doc comment. `colors_spent.bits().count_ones()` is exactly
            // "the number of colors" since `ColorSet` is a bitflag over the
            // 5 WUBRG bits, one bit per color, never double-counted.
            ReplacementPayload::EntersWithCountersSunburst => {
                let amount = colors_spent.bits().count_ones() as i32;
                if amount > 0 {
                    let kind = if state
                        .object(object)
                        .is_some_and(|o| o.definition.card_types.contains(CardTypeSet::CREATURE))
                    {
                        CounterKind::PlusOnePlusOne
                    } else {
                        CounterKind::Charge
                    };
                    counter_events.push(GameEventKind::CounterAdded { object, kind, amount });
                }
            }
            _ => continue,
        }
        // CR 614.13/701.19b-style consumption bookkeeping (OneShot/
        // LimitedUses removal, Repeatable untouched) - index-based, not
        // `apply_one_replacement`'s own source-based lookup (see this
        // loop's own `matching: Vec<usize>` comment for why); the rewritten
        // event it returns is discarded, since neither payload here ever
        // rewrites one.
        apply_replacement_at_index(state, idx, candidate());
    }
    counter_events
}

/// CR 702.82a (Devour): is `object` (already on the battlefield, `from`/
/// `controller` describing how it got there — same call-site convention
/// `apply_enters_battlefield_modifiers` uses) carrying a registered
/// `EntersWithCountersDevour` effect that still needs its choice made?
/// Checked *separately* from `apply_enters_battlefield_modifiers`'s own
/// matching loop (which deliberately never touches this payload at all —
/// see `EntersWithCountersDevour`'s own doc comment) since only a caller
/// that can actually raise a Devour choice pause should ever call this.
/// Two real producers do: `ResolveStackTop` (`ProcessState::
/// AwaitingDevourChoice`, resumed by `Engine::submit_devour_choice`) and, as
/// of `phase5/devour-amplify-search-entry`, `Search`'s own battlefield-entry
/// loop (`ProcessState::AwaitingSearchDevourChoice`, resumed by
/// `Engine::submit_search_devour_choice`) — `continue_search_battlefield_
/// entry` calls this with `from: ZoneKind::Library` the same way
/// `ResolveStackTop`'s own `continue_permanent_entry` calls it with
/// `ZoneKind::Stack`, since nothing in this function's own signature is
/// producer-specific. `PlayLand` (only ever moves lands, which can never
/// have Devour) and `CreateToken` (realistically only reachable via
/// `TokenSpec::CopyOf` copying a Devour creature — no real card in this
/// engine's test corpus does that — narrowed out on purpose, same "no real
/// producer reaches this" posture `aura-attach-choice`'s own dormant
/// `AwaitingAttachChoice` already established) still never call this at
/// all, so a Devour effect registered by either path silently grants no
/// counters (falls through `apply_enters_battlefield_modifiers`'s own
/// `_ => continue` arm) rather than erroring.
///
/// Returns `Some((per_creature, candidates))` — `candidates` is every
/// creature `controller` currently controls on the battlefield, narrowed
/// further to `quality` (CR 702.82c) if the registered effect has one (CR 701.17a:
/// sacrificing is always a permanent's own controller's choice, never an
/// arbitrary creature regardless of who controls it), `object` included
/// (real Magic allows devouring the source itself, and this engine doesn't
/// special-case it out — if chosen, the `CounterAdded` this produces later
/// just silently finds no object left to add to, same "skip rather than
/// guess" stance every other post-departure counter/effect lookup in this
/// codebase already takes). `None` if no such effect is registered, or the
/// creature-controlling player controls zero creatures at all (CR 702.82a's
/// "any number," including zero, needs no choice to make — 0 counters is
/// the only possible outcome either way).
pub fn pending_devour_choice(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    object: ObjectId,
    from: ZoneKind,
    controller: PlayerId,
) -> Option<(i32, Vec<ObjectId>)> {
    let candidate = GameEventKind::ZoneChange { object, from, to: ZoneKind::Battlefield, last_known_info: None };
    let (per_creature, quality) = state.replacement_effects.iter().find_map(|r| match &r.spec {
        ReplacementSpec::Primitive { payload: ReplacementPayload::EntersWithCountersDevour { per_creature, quality }, .. }
            if r.matches(&candidate, false, state, script_runtime) =>
        {
            Some((*per_creature, quality.clone()))
        }
        _ => None,
    })?;
    let candidates: Vec<ObjectId> = state
        .objects
        .values()
        .filter(|obj| {
            if obj.zone != ZoneKind::Battlefield || obj.controller != controller {
                return false;
            }
            let effective = crate::layers::effective_characteristics(state, script_runtime, obj.object_id);
            if !effective.card_types.contains(CardTypeSet::CREATURE) {
                return false;
            }
            // CR 702.82c: "Devour [quality] N" narrows the sacrifice to
            // permanents matching `quality` instead of any creature.
            if let Some(quality) = &quality {
                if !effective.subtypes.contains(quality) {
                    return false;
                }
            }
            true
        })
        .map(|obj| obj.object_id)
        .collect();
    if candidates.is_empty() {
        return None;
    }
    Some((per_creature, candidates))
}

/// CR 702.38a (Amplify): is `object` (already on the battlefield, `from`/
/// `controller` describing how it got there — same call-site convention
/// `pending_devour_choice` above uses) carrying a registered
/// `EntersWithCountersAmplify` effect that still needs its choice made?
/// Same "checked separately from `apply_enters_battlefield_modifiers`, only
/// a caller that can actually pause should call this" posture as
/// `pending_devour_choice` — see `EntersWithCountersAmplify`'s own doc
/// comment. Same two real producers as `pending_devour_choice` too, as of
/// `phase5/devour-amplify-search-entry`: `ResolveStackTop` and `Search`'s
/// own battlefield-entry loop (`ProcessState::AwaitingSearchAmplifyChoice`,
/// resumed by `Engine::submit_search_amplify_choice`).
///
/// Returns `Some((per_card, candidates))` — `candidates` is every card in
/// `controller`'s hand whose `effective_characteristics` subtypes overlap
/// `object`'s own (CR 702.38a "share a creature type with it" — approximated
/// as raw subtype overlap, this engine has no separate "creature type"
/// category, same imprecision `pending_devour_choice`'s siblings
/// `landwalk_subtypes`/`bands_with_other_subtypes` already accept).
/// `None` if no such effect is registered, or `object` has no subtypes to
/// share, or nothing in hand shares one — CR 702.38a's "any number,"
/// including zero, needs no choice to make either way.
pub fn pending_amplify_choice(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    object: ObjectId,
    from: ZoneKind,
    controller: PlayerId,
) -> Option<(i32, Vec<ObjectId>)> {
    let candidate = GameEventKind::ZoneChange { object, from, to: ZoneKind::Battlefield, last_known_info: None };
    let per_card = state.replacement_effects.iter().find_map(|r| match &r.spec {
        ReplacementSpec::Primitive { payload: ReplacementPayload::EntersWithCountersAmplify { per_card }, .. }
            if r.matches(&candidate, false, state, script_runtime) =>
        {
            Some(*per_card)
        }
        _ => None,
    })?;
    let object_subtypes = crate::layers::effective_characteristics(state, script_runtime, object).subtypes;
    if object_subtypes.is_empty() {
        return None;
    }
    let candidates: Vec<ObjectId> = state
        .objects
        .values()
        .filter(|obj| {
            obj.zone == ZoneKind::Hand
                && obj.controller == controller
                && crate::layers::effective_characteristics(state, script_runtime, obj.object_id)
                    .subtypes
                    .iter()
                    .any(|s| object_subtypes.contains(s))
        })
        .map(|obj| obj.object_id)
        .collect();
    if candidates.is_empty() {
        return None;
    }
    Some((per_card, candidates))
}

/// CR 707.6/614.12a (Clone itself): is `object` (already on the battlefield,
/// `from` describing how it got there — same call-site convention
/// `pending_devour_choice`/`pending_amplify_choice` above use) carrying a
/// registered `EntersAsCopy` effect that still needs its choice made? Same
/// "checked separately from `apply_enters_battlefield_modifiers`, only the
/// one call site that can actually pause should call this" posture as its
/// siblings — see `EntersAsCopy`'s own doc comment.
///
/// No `controller` parameter, unlike its siblings — Clone's own wording is
/// "any creature on the battlefield," unrestricted by who controls it
/// (unlike Devour's "creatures you control" or Amplify's "cards from your
/// hand"), so there's nothing to filter candidates by.
///
/// Returns `Some((exceptions, candidates))` — `candidates` is every
/// creature currently on the battlefield, `object` itself included (not
/// special-cased away, same "don't guess past an unlikely legal choice"
/// stance `pending_devour_choice`'s own self-sacrifice case already takes,
/// even though copying oneself is a real but pointless legal choice).
/// `None` if no such effect is registered, or there are no creatures on the
/// battlefield at all to choose from — CR 707.6's "may," including
/// declining, needs no choice to make either way.
pub fn pending_copy_choice(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    object: ObjectId,
    from: ZoneKind,
) -> Option<(CopyExceptions, Vec<ObjectId>)> {
    let candidate = GameEventKind::ZoneChange { object, from, to: ZoneKind::Battlefield, last_known_info: None };
    let exceptions = state.replacement_effects.iter().find_map(|r| match &r.spec {
        ReplacementSpec::Primitive { payload: ReplacementPayload::EntersAsCopy(exceptions), .. }
            if r.matches(&candidate, false, state, script_runtime) =>
        {
            Some(exceptions.clone())
        }
        _ => None,
    })?;
    let candidates: Vec<ObjectId> = state
        .objects
        .values()
        .filter(|obj| {
            obj.zone == ZoneKind::Battlefield
                && crate::layers::effective_characteristics(state, script_runtime, obj.object_id)
                    .card_types
                    .contains(CardTypeSet::CREATURE)
        })
        .map(|obj| obj.object_id)
        .collect();
    if candidates.is_empty() {
        return None;
    }
    Some((exceptions, candidates))
}

/// CR 702.54a's condition: was any opponent of `controller` dealt damage
/// this turn, by any source (combat or otherwise - real Bloodthirst
/// rulings aren't restricted to combat damage). "This turn" is bounded by
/// the most recent `GameEventKind::TurnStarted` in `state.ledger` (append-
/// only, so its position is always the current turn's start); no
/// `TurnStarted` yet (a fixture built mid-turn with no pregame history)
/// scans from the beginning of the ledger instead of treating the
/// condition as unknowable. Damage to an opponent's *permanents* doesn't
/// count - only `GameEventKind::DamageDealt` whose `target` is
/// `Target::Player`, matching the keyword's own "an opponent was dealt
/// damage" wording (a player, not their creatures).
pub fn opponent_damaged_this_turn(state: &GameState, controller: PlayerId) -> bool {
    let turn_start = state.ledger.iter().rposition(|e| matches!(e.kind, GameEventKind::TurnStarted { .. }));
    let scan_from = turn_start.map(|i| i + 1).unwrap_or(0);
    state.ledger[scan_from..]
        .iter()
        .any(|e| matches!(&e.kind, GameEventKind::DamageDealt { target: Target::Player(p), .. } if *p != controller))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::card::{CardDefinition, CardTypeSet, KeywordSet, ModeChoice, PTValue, ScriptReference, SupertypeSet};
    use crate::types::counter::CounterSet;
    use crate::types::cost::FilterController;
    use crate::types::effect::{EffectFilter, EventFilter, ReplacementEffect, ReplacementPayload, ReplacementSpec};
    use crate::types::game::RngState;
    use crate::types::ids::SequenceNo;
    use crate::types::mana::ManaPool;
    use crate::types::object::{GameObject, VisibilityPolicy};
    use crate::types::player::PlayerState;
    use crate::types::turn::{Phase, Step};
    use crate::types::zone::ZoneKind;
    use std::collections::HashMap;

    fn object(id: ObjectId, controller: PlayerId, card_types: CardTypeSet) -> GameObject {
        GameObject {
            object_id: id,
            lineage: crate::types::ids::ObjectLineage { previous: None },
            definition: CardDefinition {
                definition_id: 1,
                name: "Test Object".to_string(),
                mana_cost: crate::types::mana::ManaCost { symbols: Vec::new() },
                color_indicator: None,
                supertypes: SupertypeSet::empty(),
                card_types,
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
                mode_choice: ModeChoice::Exactly(1),
            },
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
            attached_to: None, attached_to_player: None,
            timestamp: SequenceNo(0),
            controlled_since_turn: 0,
            is_commander: false,
            is_token: false,
            visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
            copy_generation: 0,
        }
    }

    fn state_with(objects: Vec<GameObject>) -> GameState {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        GameState {
            players: vec![
                PlayerState {
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
                },
                PlayerState {
                    player_id: p1,
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
                },
            ],
            turn_order: vec![p0, p1],
            active_player: p0,
            turn_number: 1,
            phase: Phase::Main,
            step: Step::PreCombatMain,
            priority_player: p0,
            consecutive_passes: 0,
            lands_played_this_turn: HashMap::new(),
            combat: None,
            last_active_turn_number: HashMap::new(),
            objects: objects.into_iter().map(|o| (o.object_id, o)).collect(),
            zones: Vec::new(),
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            cost_modifiers: Vec::new(),
            hand_size_modifiers: Vec::new(),
            hand_reveal_modifiers: Vec::new(),
            draw_limit_modifiers: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState { seed: 1, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 1000,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
            linked_ability_generation: std::collections::HashMap::new(),
        }
    }

    fn no_filter() -> EffectFilter {
        EffectFilter {  card_types: None, controller: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, }
    }

    #[test]
    fn resolves_unchanged_with_nothing_registered() {
        let mut state = state_with(Vec::new());
        let event = GameEventKind::DamageDealt { source: ObjectId(0), target: Target::Player(PlayerId(0)), amount: 5, is_combat_damage: false };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("expected Resolved with no replacement effects registered");
        };
        assert!(matches!(resolved, GameEventKind::DamageDealt { amount: 5, .. }));
    }

    #[test]
    fn resolves_unchanged_when_nothing_matches() {
        let effect_source = ObjectId(0);
        let mut state = state_with(vec![object(effect_source, PlayerId(0), CardTypeSet::ARTIFACT)]);
        state.replacement_effects.push(ReplacementEffect {
            source: effect_source,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt {
                    affected: EffectFilter {  card_types: Some(CardTypeSet::CREATURE), ..no_filter() },
                },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::Repeatable,
        });
        // Target is an Artifact, not a Creature - the filter doesn't match.
        let event = GameEventKind::DamageDealt { source: ObjectId(1), target: Target::Object(effect_source), amount: 5, is_combat_damage: false };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("expected Resolved - filter doesn't match");
        };
        assert!(matches!(resolved, GameEventKind::DamageDealt { amount: 5, .. }));
    }

    #[test]
    fn one_shot_effect_applies_once_then_is_removed() {
        let effect_source = ObjectId(0);
        let mut state = state_with(vec![object(effect_source, PlayerId(0), CardTypeSet::ARTIFACT)]);
        state.replacement_effects.push(ReplacementEffect {
            source: effect_source,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt { affected: no_filter() },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::OneShot,
        });

        let event = GameEventKind::DamageDealt { source: ObjectId(1), target: Target::Player(PlayerId(0)), amount: 5, is_combat_damage: false };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("expected a single match to resolve");
        };
        assert!(matches!(resolved, GameEventKind::DamageDealt { amount: 0, .. }));
        assert!(state.replacement_effects.is_empty(), "OneShot must be consumed after applying");
    }

    #[test]
    fn limited_uses_decrements_then_expires() {
        let effect_source = ObjectId(0);
        let mut state = state_with(vec![object(effect_source, PlayerId(0), CardTypeSet::ARTIFACT)]);
        state.replacement_effects.push(ReplacementEffect {
            source: effect_source,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt { affected: no_filter() },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::LimitedUses(2),
        });

        let event = || GameEventKind::DamageDealt { source: ObjectId(1), target: Target::Player(PlayerId(0)), amount: 5, is_combat_damage: false };
        apply_replacements(&mut state, &ScriptRuntime::new(), event(), false);
        assert_eq!(
            state.replacement_effects.first().map(|r| r.consumption),
            Some(ConsumptionPolicy::LimitedUses(1)),
            "first use decrements 2 -> 1, doesn't remove"
        );

        apply_replacements(&mut state, &ScriptRuntime::new(), event(), false);
        assert!(state.replacement_effects.is_empty(), "second use exhausts LimitedUses(1) and removes it");
    }

    #[test]
    fn repeatable_effect_is_never_consumed() {
        let effect_source = ObjectId(0);
        let mut state = state_with(vec![object(effect_source, PlayerId(0), CardTypeSet::ARTIFACT)]);
        state.replacement_effects.push(ReplacementEffect {
            source: effect_source,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt { affected: no_filter() },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::Repeatable,
        });

        let event = || GameEventKind::DamageDealt { source: ObjectId(1), target: Target::Player(PlayerId(0)), amount: 5, is_combat_damage: false };
        apply_replacements(&mut state, &ScriptRuntime::new(), event(), false);
        apply_replacements(&mut state, &ScriptRuntime::new(), event(), false);
        assert_eq!(state.replacement_effects.len(), 1, "Repeatable is never removed or decremented");
    }

    #[test]
    fn two_or_more_matches_pause_for_an_ordering_choice() {
        let a = ObjectId(0);
        let b = ObjectId(1);
        let mut state = state_with(vec![
            object(a, PlayerId(0), CardTypeSet::ARTIFACT),
            object(b, PlayerId(0), CardTypeSet::ARTIFACT),
        ]);
        for source in [a, b] {
            state.replacement_effects.push(ReplacementEffect {
                source,
                spec: ReplacementSpec::Primitive {
                    filter: EventFilter::DamageDealt { affected: no_filter() },
                    payload: ReplacementPayload::PreventDamage,
                },
                consumption: ConsumptionPolicy::Repeatable,
            });
        }

        let event = GameEventKind::DamageDealt { source: ObjectId(2), target: Target::Player(PlayerId(0)), amount: 5, is_combat_damage: false };
        let ReplacementOutcome::NeedsOrderingChoice { affected_player, candidates, .. } =
            apply_replacements(&mut state, &ScriptRuntime::new(), event, false)
        else {
            panic!("expected a pause - two effects match the same event");
        };
        assert_eq!(affected_player, PlayerId(0), "damage to a player - that player is affected");
        assert_eq!(candidates.len(), 2);
        assert!(
            candidates.contains(&ReplacementCandidate::Registered(a))
                && candidates.contains(&ReplacementCandidate::Registered(b))
        );
        // Neither effect was consumed - the choice hasn't been made yet.
        assert_eq!(state.replacement_effects.len(), 2);
    }

    #[test]
    fn stabilizes_across_two_different_replacements() {
        // Effect A: damage dealt to p1 gets redirected to p0.
        // Effect B: damage dealt to p0 is prevented entirely.
        // The original event only matches A - applying A produces a new
        // event that only then matches B (CR 616's stabilization
        // requirement: the rewritten form must be re-checked).
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let source_a = ObjectId(0);
        let source_b = ObjectId(1);
        let mut state =
            state_with(vec![object(source_a, p1, CardTypeSet::ARTIFACT), object(source_b, p0, CardTypeSet::ARTIFACT)]);
        state.replacement_effects.push(ReplacementEffect {
            source: source_a,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt {
                    affected: EffectFilter {  controller: Some(FilterController::You), ..no_filter() },
                },
                payload: ReplacementPayload::RedirectDamage(Target::Player(p0)),
            },
            consumption: ConsumptionPolicy::Repeatable,
        });
        state.replacement_effects.push(ReplacementEffect {
            source: source_b,
            spec: ReplacementSpec::Primitive {
                filter: EventFilter::DamageDealt {
                    affected: EffectFilter {  controller: Some(FilterController::You), ..no_filter() },
                },
                payload: ReplacementPayload::PreventDamage,
            },
            consumption: ConsumptionPolicy::Repeatable,
        });

        let event = GameEventKind::DamageDealt { source: ObjectId(2), target: Target::Player(p1), amount: 5, is_combat_damage: false };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("only one effect matches at any single point - never a real multi-match pause here");
        };
        assert!(
            matches!(resolved, GameEventKind::DamageDealt { target: Target::Player(pid), amount: 0, .. } if pid == p0),
            "redirected to p0 by A, then prevented by B: {resolved:?}"
        );
    }

    // -----------------------------------------------------------------
    // CR 111.8: a token that's already left the battlefield can't move
    // again - no in-game producer can construct this scenario today (see
    // apply_token_zone_lock's own doc comment), so these call
    // apply_replacements directly rather than driving a full Engine.
    // -----------------------------------------------------------------

    #[test]
    fn token_zone_lock_blocks_a_second_move_for_an_already_departed_token() {
        let id = ObjectId(0);
        let token = GameObject { zone: ZoneKind::Graveyard, is_token: true, ..object(id, PlayerId(0), CardTypeSet::CREATURE) };
        let mut state = state_with(vec![token]);
        let event = GameEventKind::ZoneChange { object: id, from: ZoneKind::Graveyard, to: ZoneKind::Exile, last_known_info: None };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("no registered effects, no ordering choice possible");
        };
        assert!(
            matches!(resolved, GameEventKind::ZoneChange { to: ZoneKind::Graveyard, .. }),
            "CR 111.8: the token must stay in its current zone instead of moving to Exile: {resolved:?}"
        );
    }

    #[test]
    fn token_zone_lock_blocks_a_departed_token_from_returning_to_the_battlefield() {
        let id = ObjectId(0);
        let token = GameObject { zone: ZoneKind::Graveyard, is_token: true, ..object(id, PlayerId(0), CardTypeSet::CREATURE) };
        let mut state = state_with(vec![token]);
        let event = GameEventKind::ZoneChange { object: id, from: ZoneKind::Graveyard, to: ZoneKind::Battlefield, last_known_info: None };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("no registered effects, no ordering choice possible");
        };
        assert!(
            matches!(resolved, GameEventKind::ZoneChange { to: ZoneKind::Graveyard, .. }),
            "CR 111.8 explicitly covers \"come back onto the battlefield\" too: {resolved:?}"
        );
    }

    #[test]
    fn token_zone_lock_allows_the_first_move_off_the_battlefield() {
        let id = ObjectId(0);
        let token = GameObject { zone: ZoneKind::Battlefield, is_token: true, ..object(id, PlayerId(0), CardTypeSet::CREATURE) };
        let mut state = state_with(vec![token]);
        let event = GameEventKind::ZoneChange { object: id, from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, last_known_info: None };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("no registered effects, no ordering choice possible");
        };
        assert!(
            matches!(resolved, GameEventKind::ZoneChange { to: ZoneKind::Graveyard, .. }),
            "a token's first move off the battlefield isn't covered by CR 111.8: {resolved:?}"
        );
    }

    #[test]
    fn token_zone_lock_ignores_non_token_objects() {
        let id = ObjectId(0);
        // Not a token (object()'s own default) - CR 111.8 is token-specific.
        let card = GameObject { zone: ZoneKind::Graveyard, ..object(id, PlayerId(0), CardTypeSet::CREATURE) };
        let mut state = state_with(vec![card]);
        let event = GameEventKind::ZoneChange { object: id, from: ZoneKind::Graveyard, to: ZoneKind::Exile, last_known_info: None };
        let ReplacementOutcome::Resolved(resolved) = apply_replacements(&mut state, &ScriptRuntime::new(), event, false) else {
            panic!("no registered effects, no ordering choice possible");
        };
        assert!(
            matches!(resolved, GameEventKind::ZoneChange { to: ZoneKind::Exile, .. }),
            "a non-token card already off the battlefield isn't locked in place: {resolved:?}"
        );
    }
}
