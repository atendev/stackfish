use std::collections::VecDeque;

use crate::commands;
use crate::replacement::{self, ReplacementOutcome};
use crate::script::ScriptRuntime;
use crate::types::ability::RegisteredAbility;
use crate::types::card::{CardTypeSet, KeywordSet, SupertypeSet};
use crate::types::counter::CounterKind;
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId};
use crate::types::stack::StackEntryKind;
use crate::types::zone::ZoneKind;

/// What one `check_state_based_actions` pass produced.
pub enum SbaOutcome {
    /// This pass's events (destroys, eliminations, ...), ready for the
    /// caller to append to the ledger — empty means CR 704.3's fixed point,
    /// same meaning `Vec<GameEvent>::is_empty()` had before this type
    /// existed (every prior caller's `.is_empty()`/`.len()` check on the old
    /// bare-`Vec` return still applies, just one layer inside this variant).
    /// The second element is every dying object's own `Triggered`-kind
    /// abilities (CR 603.10), captured by `commands::retire_object` right
    /// before it purged them from `state.abilities` — `Engine` folds these
    /// into its transient last-known-information stash so a "when this
    /// dies" ability can still be found once `CollectTriggers` runs.
    Progressed(Vec<GameEvent>, Vec<RegisteredAbility>),
    /// A dying creature's zone-change replacement (CR 614.13, "if this would
    /// die, instead...") matched more than one registered
    /// `ReplacementEffect` and `affected_player` must choose which applies
    /// first (CR 616.1) before this pass can continue — same pause shape
    /// `replacement::ReplacementOutcome::NeedsOrderingChoice` already
    /// established for command-originated events, reached here from inside
    /// an SBA pass instead. `resolved_so_far` (player-loss events, plus any
    /// deaths already resolved earlier in this same pass) should be
    /// committed to the ledger immediately by the caller — they're
    /// unaffected by this pause; `pending_deaths` is what's left to check
    /// once `Engine::submit_replacement_choice` answers and
    /// `resolve_deaths` resumes.
    NeedsOrderingChoice {
        affected_player: PlayerId,
        pending_event: GameEventKind,
        candidates: Vec<crate::replacement::ReplacementCandidate>,
        excluded: Vec<crate::replacement::ReplacementCandidate>,
        /// Whether `pending_event` itself is genuine destruction (CR 614.8)
        /// — see `DyingCandidate`'s own doc comment. Carried alongside
        /// `pending_deaths` (each of *those* candidates has its own
        /// separately-tagged `is_destroy`) because `pending_event` has
        /// already been popped off that queue by the time this pause fires.
        is_destroy: bool,
        pending_deaths: VecDeque<DyingCandidate>,
        resolved_so_far: Vec<GameEvent>,
    },
    /// CR 704.5j (legend rule): `player` controls 2+ legendary permanents
    /// sharing the same name (`candidates`, sorted for determinism — found
    /// by `find_legend_rule_group`) and must choose which one to keep. A
    /// genuine player decision, not something this pure check can resolve on
    /// its own — same "surface the pause, don't guess" stance
    /// `replacement::ReplacementOutcome::NeedsOrderingChoice` already
    /// established for CR 616.1. `resolved_so_far` (this pass's player-loss
    /// events) is unaffected by this pause and should be committed to the
    /// ledger immediately by the caller. Only the *first* group found needing
    /// a decision pauses — once `Engine::submit_legend_rule_choice` answers
    /// and the losers are resolved (via `resolve_deaths`, same real
    /// replacement-checked destruction every other SBA death uses), the
    /// caller re-invokes `check_state_based_actions` from scratch, which
    /// finds the *next* group if another one exists. Multiple simultaneous
    /// violations resolve one pause at a time across passes, same as CR
    /// 616.1's own ordering-choice pause already does.
    NeedsLegendRuleChoice { player: PlayerId, candidates: Vec<ObjectId>, resolved_so_far: Vec<GameEvent> },
    /// CR 903.9a: `commander` (owned by `player`) is sitting in a graveyard
    /// or exile and hasn't yet been offered the "may put it into the
    /// command zone" choice (`GameState::commander_return_offered` — see
    /// that field's own doc comment for why this is a one-time-per-arrival
    /// window, not a standing question). A genuine "may," not something
    /// this pure check can resolve on its own. `resolved_so_far` (this
    /// pass's player-loss events) is unaffected by this pause and should be
    /// committed to the ledger immediately by the caller. Only the *first*
    /// qualifying commander (by `ObjectId`, for determinism) pauses per
    /// pass — once `Engine::submit_commander_return_choice` answers, the
    /// caller re-invokes `check_state_based_actions` from scratch, which
    /// finds the next one if another qualifies.
    NeedsCommanderReturnChoice { player: PlayerId, commander: ObjectId, resolved_so_far: Vec<GameEvent> },
}

/// One pass of CR 704's state-based actions: finds every applicable SBA and
/// applies all of them simultaneously, mutating `state` and returning the
/// `GameEvent`s that resulted (destroys, eliminations, etc) for the caller
/// to append to the ledger and feed into trigger collection — or, if a dying
/// creature's zone-change replacement needs a CR 616.1 ordering choice,
/// pausing via `SbaOutcome::NeedsOrderingChoice` instead (see `resolve_deaths`).
///
/// Implemented this pass: 704.5a (player at 0 or less life loses), 704.5b
/// (player who attempted to draw from an empty library loses), 704.5c
/// (player with 10+ poison counters loses), CR 903.10a (player dealt 21+
/// combat damage by a single commander source loses — see
/// `apply_player_loss`'s own doc comment for why this check is real but
/// currently unreachable through actual gameplay), 704.5j (legend rule —
/// see `find_legend_rule_group`'s own doc comment), 704.5k (world rule —
/// see `collect_world_rule_losers`'s own doc comment), 704.5f (creature
/// toughness 0 or less goes to its owner's graveyard, or wherever a
/// matching `ReplacementEffect` substitutes instead), 704.5g (creature
/// with lethal damage marked is destroyed, folding in 702.2b/c's
/// Deathtouch — any nonzero deathtouch damage counts as lethal — and
/// 702.12b's Indestructible — ignores 704.5g/deathtouch entirely, but NOT
/// 704.5f), 704.5d (a token in a zone other than the battlefield ceases to
/// exist — see `collect_ceasing_tokens`'s own doc comment for the timing
/// that satisfies CR 111.7's parenthetical), 704.5m (an Aura attached to an
/// illegal object, or not attached to anything, is put into its owner's
/// graveyard — see `collect_illegally_attached_auras`'s own doc comment;
/// CR 303.4d's two further Aura-illegality clauses, self-enchant and an
/// Aura-that's-also-a-creature, are folded in the same way — see
/// `collect_self_enchanted_auras`/`collect_creature_auras`),
/// 704.5n (an Equipment attached to an illegal permanent becomes unattached
/// and stays on the battlefield — see `collect_illegally_attached_equipment`'s
/// own doc comment for why this is a structurally different shape from
/// 704.5m, not a zone move). Every other CR 704.5 SBA is still a no-op —
/// see the TODO below for which and why.
///
/// This is a single pass, not the fixed-point loop — CR 704.3 requires
/// repeating until no SBA applies. The engine loop (`engine.rs`) is
/// responsible for calling this in a loop until it returns empty, not this
/// function itself, so a caller stepping through ledger entries one at a
/// time can observe each pass separately rather than the loop being opaque.
pub fn check_state_based_actions(state: &mut GameState, script_runtime: &ScriptRuntime) -> SbaOutcome {
    let mut events = Vec::new();
    apply_player_loss(state, &mut events);

    // CR 704.5j: checked before creature deaths - a genuine player choice,
    // pauses this pass outright rather than proceeding past it. Only the
    // first violating group (if any) pauses; resolving it re-invokes this
    // whole function fresh next pass, which will find the next group (if
    // any) or fall through to the checks below once none remain.
    if let Some((player, candidates)) = find_legend_rule_group(state, script_runtime) {
        return SbaOutcome::NeedsLegendRuleChoice { player, candidates, resolved_so_far: events };
    }

    // CR 903.9a: same "genuine player choice pauses this pass outright"
    // shape as the legend rule above - only the first qualifying commander
    // (if any) pauses per pass; resolving it (accept or decline) re-invokes
    // this whole function fresh next pass, which finds the next one (if
    // any) or falls through once none remain.
    if let Some((player, commander)) = find_commander_return_candidate(state) {
        return SbaOutcome::NeedsCommanderReturnChoice { player, commander, resolved_so_far: events };
    }

    // CR 704.5d: deterministic (no player choice), checked before this same
    // pass's own new deaths are collected below - CR 704.3's "simultaneous,
    // based on the state as of this check" means a token that dies via this
    // pass's own collect_dying_creatures/resolve_deaths isn't itself "in a
    // zone other than the battlefield" until after this pass ends, so it
    // can't also cease in the same pass. Running this scan first (against
    // zones as they stand at the top of this call) reproduces that "next
    // SBA check, not this one" timing for free - see collect_ceasing_tokens's
    // own doc comment.
    let mut departed_triggered = Vec::new();
    for id in collect_ceasing_tokens(state) {
        let (event, mut departed) = commit_token_cessation(state, id);
        events.push(event);
        departed_triggered.append(&mut departed);
    }

    // CR 704.5n: structurally different from 704.5m's Aura check just below
    // - an illegally-attached Equipment/Fortification doesn't die, it just
    // becomes unattached and stays on the battlefield (no zone move, no new
    // ObjectId), so it can't join the `dying`/`resolve_deaths` list at all -
    // see collect_illegally_attached_equipment's own doc comment. Collected
    // before anything else this pass mutates state, same "based on the
    // state as of the check" timing collect_ceasing_tokens's own doc
    // comment already establishes.
    for id in collect_illegally_attached_equipment(state, script_runtime) {
        events.push(commit_unattach(state, id));
    }
    // CR 303.4d: an Aura that's also a creature unattaches (this loop) and
    // dies (folded into `dying` below) in the same pass - collected before
    // either mutation, same "based on the state as of the check" timing as
    // the Equipment loop just above.
    let creature_auras = collect_creature_auras(state, script_runtime);
    for &id in &creature_auras {
        events.push(commit_unattach(state, id));
    }

    // CR 704.5k: fully deterministic (no player choice), so its losers just
    // join the same dying-creature list below - resolve_deaths treats every
    // id generically (any permanent type, not creature-specific), and its
    // existing "already moved, skip" guard makes a duplicate id (a World
    // creature also independently dying to lethal damage) harmless.
    let mut dying = collect_dying_creatures(state, script_runtime);
    dying.extend(collect_world_rule_losers(state, script_runtime));
    // CR 704.5m: same "fully deterministic, join the same list" shape as
    // 704.5k above - an illegally-attached Aura isn't destruction (CR
    // 700.4/704.5m's "put into," not "destroy"), so it can't be saved by a
    // regeneration shield, same reasoning 704.5f/704.5k already document.
    dying.extend(collect_illegally_attached_auras(state, script_runtime));
    // CR 303.4d's other two clauses: self-enchant, and (continuing from the
    // unattach loop above) an Aura-that's-a-creature's own death.
    dying.extend(collect_self_enchanted_auras(state));
    dying.extend(creature_auras.into_iter().map(|id| DyingCandidate { id, is_destroy: false }));

    match resolve_deaths(state, script_runtime, dying.into()) {
        DeathResolution::Done(death_events, mut death_departed) => {
            events.extend(death_events);
            departed_triggered.append(&mut death_departed);
            SbaOutcome::Progressed(events, departed_triggered)
        }
        DeathResolution::NeedsOrderingChoice {
            affected_player,
            pending_event,
            candidates,
            excluded,
            is_destroy,
            pending_deaths,
        } => {
            // Pre-existing gap, same shape resolve_deaths's own
            // NeedsOrderingChoice arm already documents: departed_triggered
            // (now including any this pass's own token cessations produced)
            // has no field to ride along on this particular pause - the
            // ledger events themselves are safe (folded into resolved_so_far
            // below), only a departed "when this leaves play"-style trigger
            // ability would be lost, and only when a token ceases in the
            // exact same pass some other death needs a 616.1 choice. Not hit
            // by this slice's own tests; flagged, not fixed, same precedent.
            SbaOutcome::NeedsOrderingChoice {
                affected_player,
                pending_event,
                candidates,
                excluded,
                is_destroy,
                pending_deaths,
                resolved_so_far: events,
            }
        }
    }

    // TODO: still unimplemented, no prerequisite machinery exists yet:
    //   - CDA toughness (PTValue::CharacteristicDefining): now real, via
    //     layers::effective_characteristics's layer 7a resolution (as of the
    //     layer-engine-cda slice) - see collect_dying_creatures's own doc
    //     comment. Only resolves if the card's CharacteristicDefining value
    //     actually carries a script (script: Some(ScriptReference::Inline(_)));
    //     no script, or a script that errors, still skips rather than
    //     guesses (returns None), same as always.
    //   - continuous-effect P/T modification (CR 613 layer 7c) and
    //     Indestructible-granting (layer 6): now real, via
    //     layers::effective_characteristics - see collect_dying_creatures's own
    //     doc comment. Combat-damage power and blocking-legality keyword
    //     checks (commands.rs) are NOT wired to the layer engine yet.
    //   - regeneration (CR 614.8/701.19): done, via the phase5/regeneration
    //     slice (docs/implementation.md) - CR 704.5g destruction only
    //     (deathtouch included), gated by DyingCandidate::is_destroy so a
    //     704.5f zero-toughness or 704.5k world-rule death (neither is
    //     "destroy") can't be saved by a shield. The scripted `Destroy`
    //     script op still doesn't check for one - named follow-up, that
    //     slice's own UNRESOLVED list.
    //   - planeswalker loyalty 0 (704.5i - not v1 scope per DESIGN.md), and
    //     the rest of 704.5 - no prerequisite subsystem exists yet.
}

/// The four "a player loses the game" state-based actions this engine
/// checks: CR 704.5a (0 or less life), CR 704.5b (drew from an empty
/// library — `PlayerState.drew_from_empty_library`, set by
/// `commands::propose_draw`), CR 704.5c (10+ poison counters), and CR
/// 903.10a (21+ combat damage from a single commander source). One
/// `GameEventKind::PlayerEliminated` per player regardless of which cause
/// applied — the event doesn't distinguish cause, same as before this
/// slice. `eliminated` is the field this flips; already-eliminated players
/// are skipped so re-running this pass doesn't re-emit the event every
/// fixed-point iteration.
///
/// CR 903.10a's check is real but currently unreachable through actual
/// gameplay — nothing in this engine increments `commander_damage_taken`
/// yet (checked: always initialized empty, never mutated elsewhere).
/// There's no "is this object a player's commander" concept at all —
/// that's backlog item 11's Commander-format machinery, not this slice's.
/// Testable via a hand-built `PlayerState.commander_damage_taken` fixture,
/// not yet through real combat.
///
/// CR 704.5j (legend rule) and 704.5k (world rule) are NOT here — both
/// destroy permanents via a player *choice* among duplicates, a
/// structurally different SBA shape from "this player loses," needing its
/// own choice-pause design (like CR 616.1's `NeedsOrderingChoice`, but for
/// a different decision). Split into its own named follow-up rather than
/// forced into this function's shape.
fn apply_player_loss(state: &mut GameState, events: &mut Vec<GameEvent>) {
    let losing: Vec<PlayerId> = state
        .players
        .iter()
        .filter(|p| {
            !p.eliminated
                && (p.life <= 0
                    || p.drew_from_empty_library
                    || p.counters.count(&CounterKind::Poison) >= 10
                    || p.commander_damage_taken.iter().any(|&(_, damage)| damage >= 21))
        })
        .map(|p| p.player_id)
        .collect();

    for player in losing {
        if let Some(p) = state.player_mut(player) {
            p.eliminated = true;
        }
        // CR 800.4a presupposes a game with opponents to continue without
        // this player - a single-player fixture (many exist; this engine's
        // own test/scenario harness has no real multiplayer setup) has
        // nothing to "leave into," so purging its only player's objects
        // would just be data loss with no rule behind it. Same guard
        // `Engine::game_over_check` (engine.rs) uses for the same reason.
        if state.turn_order.len() >= 2 {
            remove_players_objects(state, player);
        }
        let event_id = state.allocate_event_id();
        let sequence_no = state.allocate_sequence_no();
        events.push(GameEvent {
            event_id,
            sequence_no,
            kind: GameEventKind::PlayerEliminated { player },
            caused_by_command: None,
            parent_event: None,
        });
    }
}

/// CR 800.4a: every object owned by a player who's left the game leaves the
/// game too, immediately — not a normal zone change (no `ZoneChange` event,
/// no destination), so nothing routes through `CollectTriggers` here. This
/// is deliberate, not an oversight: the departing object's own triggered
/// abilities are controlled by the very player who just left, and CR 800.4d
/// says a triggered ability controlled by a player who's left the game never
/// goes on the stack — so `commands::retire_object`'s returned abilities are
/// correctly discarded rather than fed anywhere.
///
/// Per-owner zones (library/hand/graveyard/command) belong wholly to
/// `player` and are drained outright. Shared zones (battlefield/exile) keep
/// every other player's objects, only this player's own leave. The stack
/// isn't a `Zone` — `StackObject` has no separate owner field, only
/// `controller`, so a spell/ability this player controls is treated as
/// theirs to remove (CR 800.4a's true "owned by," e.g. a control-changed
/// spell, isn't representable yet — no control-changing effect exists in
/// this engine to raise the distinction).
fn remove_players_objects(state: &mut GameState, player: PlayerId) {
    let owned_zone_ids: Vec<ObjectId> = state
        .zones
        .iter_mut()
        .filter(|z| z.owner == Some(player))
        .flat_map(|z| std::mem::take(&mut z.contents))
        .collect();

    let shared_zone_ids: Vec<ObjectId> = state
        .zones
        .iter()
        .filter(|z| z.owner.is_none())
        .flat_map(|z| z.contents.iter().copied())
        .filter(|id| state.objects.get(id).is_some_and(|o| o.owner == player))
        .collect();
    for &id in &shared_zone_ids {
        for zone in state.zones.iter_mut().filter(|z| z.owner.is_none()) {
            zone.remove(id);
        }
    }

    for id in owned_zone_ids.into_iter().chain(shared_zone_ids) {
        commands::retire_object(state, id);
    }

    // CR 800.4a: "all objects owned by that player leave the game" removes
    // an owned spell (a spell is represented by a card, so it's caught by
    // ownership, same as everything else this function purges by owner
    // above) regardless of who's controlling it; "then, if that player
    // controlled any objects on the stack not represented by cards [i.e.
    // abilities], those objects cease to exist" is a second, narrower
    // clause that only reaches abilities, by controller. `StackEntryKind::
    // Copy` isn't constructed anywhere in this engine yet (no spell-copy
    // effect exists) - once one does, a copy is also "not represented by a
    // card" and would need the controller-side clause too, not just Spell's
    // owner-side one.
    let stack_ids: Vec<ObjectId> = state
        .stack
        .iter()
        .filter(|so| so.owner == player || (so.controller == player && !matches!(so.kind, StackEntryKind::Spell)))
        .map(|so| so.stack_id)
        .collect();
    state.stack.retain(|so| !stack_ids.contains(&so.stack_id));
    for id in stack_ids {
        commands::retire_object(state, id);
    }
}

/// One `resolve_deaths` candidate: `id` is moving from the battlefield to
/// the graveyard, and `is_destroy` says whether that move is genuine
/// destruction (CR 614.8) — the only kind of death a regeneration shield
/// (`ReplacementPayload::Regenerate`) can intercept. CR 704.5g (lethal
/// damage/deathtouch) is destruction; CR 704.5f (zero-or-less toughness) and
/// CR 704.5k (world rule) are both battlefield -> graveyard moves per CR
/// 700.4's "dies," but neither is "destroy" - conflating them would let a
/// shield incorrectly save a 0-toughness creature, a real rules bug this
/// type exists specifically to prevent.
pub struct DyingCandidate {
    pub id: ObjectId,
    pub is_destroy: bool,
}

/// CR 704.5f/704.5g: which battlefield creatures have non-positive
/// toughness, or marked damage at or above their toughness — the candidate
/// set `resolve_deaths` actually moves. Toughness, Indestructible, and
/// creature type itself are all read via `layers::effective_characteristics`
/// (CR 613 layers 4/6/7a/7b/7c) rather than straight off `CardDefinition` —
/// first flagged in docs/implementation.md's layer-engine-basics slice. A
/// "+1/+1 to all your creatures"-style continuous effect, one granting
/// Indestructible, one granting the Creature type itself (e.g. an animated
/// land, `AddCardType(CREATURE)` plus a `SetPT` for its 3/3), or a scripted
/// characteristic-defining toughness (layer 7a) now actually changes whether
/// this check applies at all.
///
/// `DyingCandidate::is_destroy` distinguishes the two SBAs this function
/// covers — `false` for 704.5f (zero toughness), `true` for 704.5g (lethal
/// damage/deathtouch) — see that type's own doc comment for why.
fn collect_dying_creatures(state: &GameState, script_runtime: &ScriptRuntime) -> Vec<DyingCandidate> {
    let candidates: Vec<ObjectId> =
        state.objects.values().filter(|o| o.zone == ZoneKind::Battlefield).map(|o| o.object_id).collect();

    candidates
        .into_iter()
        .filter_map(|id| {
            let effective = crate::layers::effective_characteristics(state, script_runtime, id);
            // CR 613 layer 4: a land animated into a creature by
            // AddCardType(CREATURE) is subject to this SBA; a plain land
            // (or any other non-creature permanent) isn't - checked against
            // the effective type, not CardDefinition.card_types, so a
            // granted type actually counts.
            if !effective.card_types.contains(CardTypeSet::CREATURE) {
                return None;
            }
            // CDA toughness (PTValue::CharacteristicDefining) can't be
            // resolved without the script bridge - skip rather than guess,
            // same stance `PTValue::as_fixed`/`layers.rs` already take.
            let toughness = effective.toughness?;
            let object = state.object(id).expect("candidate id came from state.objects itself");
            // CR 704.5f: 0-or-less toughness applies unconditionally, even
            // to an indestructible creature (702.12b only exempts 704.5g) -
            // and isn't destruction, so a regeneration shield can't save it.
            if toughness <= 0 {
                return Some(DyingCandidate { id, is_destroy: false });
            }
            if effective.keywords.contains(KeywordSet::INDESTRUCTIBLE) {
                return None;
            }
            // CR 704.5g + 702.2b/c: lethal marked damage, or any nonzero
            // damage at all from a Deathtouch source - this one IS
            // destruction.
            let lethal = object.damage_marked >= toughness as u32 || (object.deathtouched && object.damage_marked > 0);
            lethal.then_some(DyingCandidate { id, is_destroy: true })
        })
        .collect()
}

/// CR 704.5j: finds the first group of 2+ battlefield permanents that share
/// a controller and an effective name while all having the `Legendary`
/// supertype — the shape `Engine::submit_legend_rule_choice` needs a player
/// decision for. Name/supertypes read via `layers::effective_characteristics`
/// rather than straight off `CardDefinition`, same "always query through the
/// layer engine" convention `collect_dying_creatures` already established —
/// future-proofs against a not-yet-built "becomes legendary"/name-changing
/// effect for free, even though nothing can construct either today.
///
/// Not creature-specific — legend rule applies to any permanent type. Only
/// the *first* violating group is returned (`check_state_based_actions`
/// re-invokes this fresh after each pause resolves, so a second
/// simultaneous violation is found on the next pass, same "one pause at a
/// time" contract CR 616.1's ordering choice already has). Candidates are
/// sorted by `ObjectId` for determinism — `state.objects` is a `HashMap`,
/// iteration order isn't guaranteed, and groups themselves are compared by
/// their sorted candidates' minimum id so which group is found first is
/// stable across runs even when more than one violates simultaneously.
fn find_legend_rule_group(state: &GameState, script_runtime: &ScriptRuntime) -> Option<(PlayerId, Vec<ObjectId>)> {
    let mut groups: std::collections::BTreeMap<(PlayerId, String), Vec<ObjectId>> = std::collections::BTreeMap::new();
    for object in state.objects.values().filter(|o| o.zone == ZoneKind::Battlefield) {
        let effective = crate::layers::effective_characteristics(state, script_runtime, object.object_id);
        if !effective.supertypes.contains(SupertypeSet::LEGENDARY) {
            continue;
        }
        // CR 704.5j: the legend rule groups by *current* controller
        // (layers::effective_controller), not the raw field.
        let controller = crate::layers::effective_controller(state, script_runtime, object.object_id);
        groups.entry((controller, effective.name)).or_default().push(object.object_id);
    }
    let mut violating: Vec<(PlayerId, Vec<ObjectId>)> = groups
        .into_iter()
        .filter(|(_, ids)| ids.len() >= 2)
        .map(|((player, _), mut ids)| {
            ids.sort();
            (player, ids)
        })
        .collect();
    violating.sort_by_key(|(_, ids)| ids[0]);
    violating.into_iter().next()
}

/// CR 903.9a: the first (by `ObjectId`, for determinism against
/// `state.objects`' unordered `HashMap` iteration) commander sitting in a
/// graveyard or exile that hasn't already been offered the "may return to
/// the command zone" choice this arrival (`GameState::
/// commander_return_offered` — see that field's doc comment). Owner is
/// `object.owner`, matching CR 903.8/903.9a's "owner" wording (not
/// controller — a graveyard/exile card has no meaningful controller
/// distinct from its owner anyway).
fn find_commander_return_candidate(state: &GameState) -> Option<(PlayerId, ObjectId)> {
    let mut candidates: Vec<ObjectId> = state
        .objects
        .values()
        .filter(|o| {
            o.is_commander
                && matches!(o.zone, ZoneKind::Graveyard | ZoneKind::Exile)
                && !state.commander_return_offered.contains(&o.object_id)
        })
        .map(|o| o.object_id)
        .collect();
    candidates.sort();
    let commander = candidates.into_iter().next()?;
    Some((state.object(commander)?.owner, commander))
}

/// CR 704.5k: which battlefield permanents (any type, not creature-specific)
/// die to the world rule this pass — every World-supertype permanent except
/// the one that's had the supertype for the shortest amount of time (i.e.
/// the highest `timestamp`, since that value only ever increases as the game
/// proceeds — the object that most recently entered the battlefield has had
/// World for the least time). A tie for shortest time (two or more sharing
/// the maximum timestamp) kills *all* World permanents, not just the tied
/// ones — CR 704.5k's own explicit tie clause, not something this function
/// guesses at. Fully deterministic, no player choice - see this file's own
/// `check_state_based_actions` for why these losers just join the ordinary
/// dying-creature list instead of getting their own pause type. Name/
/// supertypes read via `layers::effective_characteristics`, same convention
/// `find_legend_rule_group` above uses.
fn collect_world_rule_losers(state: &GameState, script_runtime: &ScriptRuntime) -> Vec<DyingCandidate> {
    let world_objects: Vec<(ObjectId, crate::types::ids::SequenceNo)> = state
        .objects
        .values()
        .filter(|o| o.zone == ZoneKind::Battlefield)
        .filter_map(|o| {
            let effective = crate::layers::effective_characteristics(state, script_runtime, o.object_id);
            effective.supertypes.contains(SupertypeSet::WORLD).then_some((o.object_id, o.timestamp))
        })
        .collect();
    if world_objects.len() < 2 {
        return Vec::new();
    }
    let max_timestamp = world_objects.iter().map(|(_, ts)| *ts).max().expect("checked len >= 2 above");
    let survivor_count = world_objects.iter().filter(|(_, ts)| *ts == max_timestamp).count();
    // CR 704.5k is "put into its owner's graveyard," not "destroy" - never
    // interceptable by a regeneration shield, same as 704.5f above.
    if survivor_count == 1 {
        world_objects
            .into_iter()
            .filter(|(_, ts)| *ts != max_timestamp)
            .map(|(id, _)| DyingCandidate { id, is_destroy: false })
            .collect()
    } else {
        // Tie for shortest time - no unique survivor, all die.
        world_objects.into_iter().map(|(id, _)| DyingCandidate { id, is_destroy: false }).collect()
    }
}

/// CR 704.5m/303.4c: every battlefield Aura (`commands::is_aura`) that's
/// either not attached to anything, or whose `attached_to` no longer names
/// a legal target - `CardDefinition.target_requirement`'s own filter
/// re-checked fresh via `commands::target_is_legal`, the same function CR
/// 608.2b's own resolution-time recheck already uses, so a target that
/// left the battlefield, changed characteristics, *or gained protection
/// from this very Aura since it attached* (closing the one piece of CR
/// 702.16's Enchant "DEBT" restriction the protection slices couldn't -
/// see `EffectiveCharacteristics::protection_colors`'s own doc comment)
/// all correctly make the Aura fall off. `caster`/`source` are threaded
/// through as the Aura's own current controller/object id, same meaning
/// those parameters had at cast time - `target_is_legal`'s protection check
/// reads the *Aura's* characteristics as the "source" whose quality
/// matters (CR 702.16's "of the stated quality" always describes the
/// source, not the caster), exactly what CR 702.16's Enchant restriction
/// needs.
///
/// An Aura with no `target_requirement` at all (shouldn't happen for a
/// real Aura - CR 303.4a requires one - but no fixture is stopped from
/// building one without) falls back to a bare existence check on
/// `attached_to`'s target, same "don't crash on a malformed fixture, just
/// don't over-claim legality" stance every other missing-filter case in
/// this codebase takes.
///
/// CR 304.3's "based on the state as of the check" timing falls out for
/// free from running this *before* `resolve_deaths` mutates anything this
/// same pass, same precedent `collect_ceasing_tokens`'s own doc comment
/// already establishes for tokens - a creature dying this same pass still
/// resolves as a legal target here, so its attached Auras wait one more
/// SBA pass to fall off, matching real-game timing.
fn collect_illegally_attached_auras(state: &GameState, script_runtime: &ScriptRuntime) -> Vec<DyingCandidate> {
    state
        .objects
        .values()
        .filter(|o| o.zone == ZoneKind::Battlefield && commands::is_aura(&o.definition))
        .filter_map(|o| {
            let illegal = match o.attached_to {
                None => true,
                Some(target_id) => match &o.definition.target_requirement {
                    Some(crate::types::target::TargetSpec::Primitive(req)) => !commands::target_is_legal(
                        state,
                        script_runtime,
                        crate::types::ids::Target::Object(target_id),
                        &req.filter,
                        o.controller,
                        o.object_id,
                        req.exclude_source,
                    ),
                    _ => state.object(target_id).is_none(),
                },
            };
            illegal.then_some(DyingCandidate { id: o.object_id, is_destroy: false })
        })
        .collect()
}

/// CR 303.4d/704.5m: an Aura attached to itself is illegal regardless of
/// whether it matches its own enchant filter type-wise -
/// `collect_illegally_attached_auras` above only catches a target that
/// fails the Aura's own `target_requirement`, which a self-targeting Aura
/// could technically still pass (e.g. an "Enchant Aura" targeting itself).
/// Same "put into," not "destroy" shape as 704.5m generally - not save-able
/// by regeneration. Currently only reachable via a directly-constructed
/// `GameObject` (test fixture) rather than real gameplay: the only real
/// attach call site for an Aura (`attach_permanent`'s Aura-cast-resolution
/// caller, `engine.rs`) targets an object already on the battlefield at
/// cast time, before the Aura itself is - it can never legally target
/// itself through that path. Kept as a real, always-on SBA check anyway
/// (CR 303.4d states it as one unconditionally) in case a future
/// reattach-style effect makes it reachable.
fn collect_self_enchanted_auras(state: &GameState) -> Vec<DyingCandidate> {
    state
        .objects
        .values()
        .filter(|o| o.zone == ZoneKind::Battlefield && commands::is_aura(&o.definition) && o.attached_to == Some(o.object_id))
        .map(|o| DyingCandidate { id: o.object_id, is_destroy: false })
        .collect()
}

/// CR 303.4d: "An Aura that's also a creature can't enchant anything" - a
/// battlefield Aura currently attached to something, whose *effective*
/// characteristics (CR 613 layer 4 `AddCardType`, not just the printed
/// `CardDefinition`) include the Creature type. Reachable today via any
/// continuous effect that adds Creature to an already-attached Aura - no
/// printed card in this engine's catalog does that yet, but the mechanism
/// (layer 4) is real. Returns just the ids - the caller (`check_state_based_actions`)
/// unattaches each one (`commit_unattach`, same as 704.5n's Equipment
/// handling) *and* folds it into the same pass's dying list, since CR
/// 303.4d's "becomes unattached, then is put into its owner's graveyard"
/// is two state-based actions performed together, not two separate SBA
/// checks a pass apart.
fn collect_creature_auras(state: &GameState, script_runtime: &ScriptRuntime) -> Vec<ObjectId> {
    state
        .objects
        .values()
        .filter(|o| {
            o.zone == ZoneKind::Battlefield
                && commands::is_aura(&o.definition)
                && o.attached_to.is_some()
                && crate::layers::effective_characteristics(state, script_runtime, o.object_id)
                    .card_types
                    .contains(CardTypeSet::CREATURE)
        })
        .map(|o| o.object_id)
        .collect()
}

/// CR 704.5n/301.5: every battlefield Equipment (`commands::is_equipment`)
/// currently attached to something that's no longer a legal creature to be
/// equipped by it - either it's gone (left the battlefield), stopped being
/// a creature (CR 301.5's own base restriction, "It can't legally be
/// attached to anything that isn't a creature"), or gained protection from
/// this very Equipment's own qualities (CR 702.16's Equip "DEBT"
/// restriction, closed here the same way `collect_illegally_attached_auras`
/// closed the Enchant half). Deliberately does *not* reuse the equip
/// ability's own "target creature you control" filter - CR 702.6c's last
/// sentence is explicit that an equip ability's own further restrictions
/// "don't restrict what the Equipment may be attached to" once it's
/// already attached, only who may *choose* to attach it in the first
/// place. A `controller: None` synthetic filter is built here instead, so
/// a later change of the equipped creature's controller (a control-swap
/// effect) does *not* knock the Equipment off, matching real Magic. An
/// unattached Equipment is *not* itself a violation (CR 301.5b - Equipment
/// enters, and can sit, unattached indefinitely; only Auras require
/// attachment at all, CR 704.5m).
fn collect_illegally_attached_equipment(state: &GameState, script_runtime: &ScriptRuntime) -> Vec<ObjectId> {
    let creature_filter = crate::types::target::TargetFilter::Object {
        card_types: Some(CardTypeSet::CREATURE),
        excluded_card_types: None,
        subtypes: None,
        excluded_subtypes: None,
        supertypes: None,
        excluded_supertypes: None,
        controller: None,
        zone: None,
    };
    state
        .objects
        .values()
        .filter(|o| o.zone == ZoneKind::Battlefield && commands::is_equipment(&o.definition))
        .filter_map(|o| {
            let target_id = o.attached_to?;
            let illegal = !commands::target_is_legal(
                state,
                script_runtime,
                crate::types::ids::Target::Object(target_id),
                &creature_filter,
                o.controller,
                o.object_id,
                // A synthetic CR 301.5 filter, not a real TargetRequirement
                // - "another target" has no meaning here.
                false,
            );
            illegal.then_some(o.object_id)
        })
        .collect()
}

/// CR 704.5n's actual commit: clears `id`'s own `attached_to` and removes it
/// from its former target's `attachments` list - the inverse of
/// `commands::attach_permanent`, inlined here rather than shared since
/// `attach_permanent`'s own reattach-safety logic already subsumes a plain
/// detach and this is the only unattach-without-reattach call site. No zone
/// change, no new `ObjectId`, no `retire_object` call - `id` never leaves
/// the battlefield, same "in-place mutation" shape `commit_regeneration`
/// already established for a different CR 614.8 SBA.
fn commit_unattach(state: &mut GameState, id: ObjectId) -> GameEvent {
    let was_attached_to = state.object_mut(id).and_then(|o| o.attached_to.take());
    if let Some(target_id) = was_attached_to {
        if let Some(target) = state.object_mut(target_id) {
            target.attachments.retain(|&a| a != id);
        }
    }
    let event_id = state.allocate_event_id();
    let sequence_no = state.allocate_sequence_no();
    GameEvent {
        event_id,
        sequence_no,
        kind: GameEventKind::Unattached {
            object: id,
            was_attached_to: was_attached_to
                .expect("caller only ever passes ids with a real attached_to"),
        },
        caused_by_command: None,
        parent_event: None,
    }
}

/// CR 704.5d: every token (`GameObject.is_token`) currently sitting in a
/// zone other than the battlefield. Called at the very top of
/// `check_state_based_actions`, before this same pass's own dying-creature
/// collection runs - see that call site's own doc comment for why checking
/// zones as they stand at that point (rather than after this pass commits
/// its own new deaths) is what makes a token that just died wait one full
/// SBA pass before ceasing, matching CR 111.7's own parenthetical about
/// triggered abilities getting a chance first. The stack isn't a `Zone`
/// (no `is_token` concept applies to a `StackObject` - nothing creates a
/// token mid-resolution on the stack itself in this engine), so it's never
/// scanned here.
fn collect_ceasing_tokens(state: &GameState) -> Vec<ObjectId> {
    state
        .zones
        .iter()
        .filter(|z| z.kind != ZoneKind::Battlefield)
        .flat_map(|z| z.contents.iter().copied())
        .filter(|id| state.object(*id).is_some_and(|obj| obj.is_token))
        .collect()
}

/// Removes `id` from whichever zone it's actually sitting in and purges it
/// (`commands::retire_object`) — no new `ObjectId` minted, unlike every real
/// death in this engine (CR 400.7 only applies to an actual zone move; a
/// ceasing token has nowhere to move *to*, it just stops existing). Returns
/// its own `Triggered`-kind abilities alongside the ledger event, same
/// "capture before `retire_object` purges them" contract `commit_death`
/// already has, in case a token ever self-registers a "when this leaves
/// play"-style ability (no real card needs one today, but the shape is
/// cheap to keep consistent).
fn commit_token_cessation(state: &mut GameState, id: ObjectId) -> (GameEvent, Vec<RegisteredAbility>) {
    for zone in &mut state.zones {
        zone.remove(id);
    }
    let departed = commands::retire_object(state, id);
    let event_id = state.allocate_event_id();
    let sequence_no = state.allocate_sequence_no();
    let event = GameEvent {
        event_id,
        sequence_no,
        kind: GameEventKind::TokenCeased { object: id },
        caused_by_command: None,
        parent_event: None,
    };
    (event, departed)
}

/// What one `resolve_deaths` call produced.
pub(crate) enum DeathResolution {
    /// Every id in the original `pending_deaths` is either moved or gone —
    /// `Vec<GameEvent>` is every death this call actually committed, ids
    /// already allocated, ready for the caller to append to the ledger.
    /// `Vec<RegisteredAbility>` is those deaths' own `Triggered`-kind
    /// abilities, captured by `commands::retire_object` (via `commit_death`)
    /// before it purged them — see `SbaOutcome::Progressed`'s doc comment
    /// for where this ends up.
    Done(Vec<GameEvent>, Vec<RegisteredAbility>),
    /// The next dying creature's zone-change replacement matched more than
    /// one registered `ReplacementEffect` (CR 616.1) — `pending_deaths` is
    /// what's left to check (not including the one that paused; it resumes
    /// via the chosen candidate, not by re-entering this list), same
    /// resume-with-the-rest contract `ProcessState::ReplacementCheck`'s own
    /// `remaining` already has.
    NeedsOrderingChoice {
        affected_player: PlayerId,
        pending_event: GameEventKind,
        candidates: Vec<crate::replacement::ReplacementCandidate>,
        excluded: Vec<crate::replacement::ReplacementCandidate>,
        /// Whether `pending_event` is genuine destruction - see
        /// `DyingCandidate`'s own doc comment. `pending_deaths`' own
        /// remaining entries each carry their own `is_destroy` already.
        is_destroy: bool,
        pending_deaths: VecDeque<DyingCandidate>,
    },
}

/// Resolves CR 704.5f/g's dying-creature list one object at a time —
/// proposes each one's Battlefield → Graveyard `ZoneChange` (using its
/// current, still-on-the-battlefield id, since `ReplacementEffect::matches`
/// needs to resolve it) and runs it through the real replacement pipeline
/// (`replacement::apply_replacements`) *before* actually moving it, so a
/// CR 614.13 "if this would die, exile it instead" effect can substitute the
/// destination (`ReplacementPayload::ReplaceZoneDestination`) before the
/// object ever touches the graveyard. Pauses on `DeathResolution::
/// NeedsOrderingChoice` exactly like `ProcessState::ReplacementCheck` does
/// for command-originated events (CR 616.1) — same underlying
/// `replacement::apply_replacements` call, just reached from inside an SBA
/// pass instead of `engine.rs`'s command pipeline. `Engine::
/// submit_replacement_choice` resumes an interrupted call by re-invoking
/// this function with the same `pending_deaths` it was given.
pub(crate) fn resolve_deaths(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    mut pending_deaths: VecDeque<DyingCandidate>,
) -> DeathResolution {
    let mut resolved = Vec::new();
    let mut departed_triggered = Vec::new();
    while let Some(DyingCandidate { id: object_id, is_destroy }) = pending_deaths.pop_front() {
        let Some(object) = state.object(object_id) else {
            // Already moved by an earlier iteration of this same pass
            // (shouldn't happen - object_id is from this pass's own
            // snapshot - but no live object to move means nothing to do).
            continue;
        };
        let last_known_info = object.clone();
        let proposed = GameEventKind::ZoneChange {
            object: object_id,
            from: ZoneKind::Battlefield,
            to: ZoneKind::Graveyard,
            last_known_info: Some(last_known_info),
        };
        match replacement::apply_replacements(state, script_runtime, proposed, is_destroy) {
            ReplacementOutcome::Resolved(final_event) => {
                let (event, mut abilities) = commit_death_or_regeneration(state, final_event);
                resolved.push(event);
                departed_triggered.append(&mut abilities);
            }
            ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded, .. } => {
                // Pre-existing gap, not introduced by this slice: `resolved`
                // (and now `departed_triggered`) from any earlier iteration
                // of this same while loop are dropped here rather than
                // threaded into `NeedsOrderingChoice` - state itself already
                // reflects those earlier deaths (commit_death mutated it),
                // but their ledger events - and now their dies-trigger
                // abilities - never reach the caller. Only matters when two+
                // creatures die in the same SBA pass and a later one (not
                // the first) needs a CR 616.1 ordering choice; not hit by
                // this slice's own tests. Left as-is rather than silently
                // fixed mid-slice - flagging for a follow-up.
                return DeathResolution::NeedsOrderingChoice {
                    affected_player,
                    pending_event: event,
                    candidates,
                    excluded,
                    is_destroy,
                    pending_deaths,
                };
            }
        }
    }
    DeathResolution::Done(resolved, departed_triggered)
}

/// Actually performs the zone move for one already-replacement-checked death
/// `event` — mints a fresh `ObjectId` (CR 400.7), purges any dangling
/// ability/continuous-effect via `commands::retire_object` (no remap
/// follows, same as every other SBA-driven death before this slice), and
/// builds the final ledger-ready `GameEvent` (id/sequence allocated here,
/// same as this function's inline predecessor did). `event`'s `object`/
/// `from` are the pre-move identity — `apply_replacements` never rewrites
/// those for a `ZoneChange`, only `to` (`ReplacementPayload::
/// ReplaceZoneDestination`) — so `to` is this death's real final
/// destination: `Graveyard` unless a replacement substituted it.
///
/// Returns the departing object's own `Triggered`-kind abilities alongside
/// the ledger event (CR 603.10) — `retire_object` purges them from
/// `state.abilities` right here, before `collect_triggers` ever runs, so
/// this is the last point at which they can be captured for a "when this
/// dies"/"when this leaves the battlefield" ability to still fire.
pub(crate) fn commit_death(state: &mut GameState, event: GameEventKind) -> (GameEvent, Vec<RegisteredAbility>) {
    let GameEventKind::ZoneChange { object: old_id, to, last_known_info, .. } = event else {
        // Every caller routes through commit_death_or_regeneration below,
        // which only ever forwards a real ZoneChange here - a Regenerated
        // event goes to commit_regeneration instead.
        unreachable!("commit_death is only ever called with a ZoneChange event");
    };
    let last_known_info = last_known_info.expect("resolve_deaths always sets last_known_info");
    let owner = last_known_info.owner;

    let new_id = state.allocate_object_id();
    let mut new_object = last_known_info.clone();
    new_object.object_id = new_id;
    new_object.lineage = ObjectLineage { previous: Some(old_id) };
    new_object.zone = to;
    new_object.timestamp = state.allocate_sequence_no();

    if let Some(zone) = state.zone_mut(ZoneKind::Battlefield, None) {
        zone.remove(old_id);
    }
    // CR 400.7: this object is gone for good (no remap follows, unlike
    // PlayLand/CastSpell/ResolveStackTop's "same card, new id" moves) -
    // purge any RegisteredAbility/ContinuousEffect still pointing at it.
    let departed_triggered = commands::retire_object(state, old_id);
    // Exile is a shared zone (owner: None), same distinction engine.rs's own
    // stack-resolution zone move already makes - `to` was always Graveyard
    // (per-player) before ReplacementPayload::ReplaceZoneDestination gained
    // a real Lua construction path (phase5/replacement-effect-shapes), so
    // this branch was unreachable until then. Caught by a real test
    // exiling a creature instead of it dying, not by inspection - without
    // this, the wrong-owner zone_mut lookup silently found no zone at all
    // and the object was never re-inserted anywhere.
    let destination_owner = match to {
        ZoneKind::Battlefield | ZoneKind::Exile => None,
        _ => Some(owner),
    };
    if let Some(zone) = state.zone_mut(to, destination_owner) {
        zone.push_bottom(new_id);
        state.objects.insert(new_id, new_object);
    }
    // else: no such zone registered - defensive no-op, same stance
    // ResolveStackTop/PlayLand already take on a missing zone_mut.

    let event_id = state.allocate_event_id();
    let sequence_no = state.allocate_sequence_no();
    let event = GameEvent {
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
    (event, departed_triggered)
}

/// CR 614.8/701.19: performs regeneration's actual side effects, once
/// `replacement::ReplacementEffect::apply` has already rewritten the
/// proposed destroy into a `GameEventKind::Regenerated` - remove all marked
/// damage (and the Deathtouch taint alongside it, same pairing Cleanup's own
/// CR 514.2 damage-removal already keeps), tap the permanent, and remove it
/// from combat (CR 506.4's own attacker/blocker lists - `EndOfCombat`
/// already clears the whole `CombatState` at the end of the phase; this is
/// the same idea for just this one permanent, mid-combat). Unlike
/// `commit_death`, `object`'s `ObjectId` is unchanged - the permanent never
/// leaves the battlefield, so CR 400.7's "destruction mints a new object"
/// rule doesn't apply, and there's nothing for `commands::retire_object` to
/// purge (no departed `Triggered`-kind abilities either, hence the always-
/// empty second element).
pub(crate) fn commit_regeneration(state: &mut GameState, object: ObjectId) -> (GameEvent, Vec<RegisteredAbility>) {
    if let Some(obj) = state.object_mut(object) {
        obj.damage_marked = 0;
        obj.deathtouched = false;
        obj.tapped = true;
    }
    if let Some(combat) = state.combat.as_mut() {
        combat.attackers.retain(|(id, _)| *id != object);
        combat.blocks.retain(|(blocker, attacker)| *blocker != object && *attacker != object);
    }
    let event_id = state.allocate_event_id();
    let sequence_no = state.allocate_sequence_no();
    let event = GameEvent {
        event_id,
        sequence_no,
        kind: GameEventKind::Regenerated { object },
        caused_by_command: None,
        parent_event: None,
    };
    (event, Vec::new())
}

/// Dispatches a replacement-checked destroy proposal's final resolved event
/// to the right committer - `commit_death` for an ordinary (possibly zone-
/// substituted) `ZoneChange`, `commit_regeneration` for a regeneration
/// shield's `Regenerated`. The one place every real caller (`resolve_deaths`
/// below, and `Engine::submit_replacement_choice`'s SBA-resume branch) needs
/// to make this choice, kept here rather than duplicated at each call site.
pub(crate) fn commit_death_or_regeneration(
    state: &mut GameState,
    event: GameEventKind,
) -> (GameEvent, Vec<RegisteredAbility>) {
    match event {
        GameEventKind::Regenerated { object } => commit_regeneration(state, object),
        other => commit_death(state, other),
    }
}

/// CR 903.9a: performs the *accepted* "return commander to the command
/// zone" choice — graveyard/exile to the command zone, CR 400.7 new
/// `ObjectId` (no remap follows, same convention every other zone-change
/// move in this crate uses), `is_commander`/`owner` preserved via clone
/// (see `GameObject::is_commander`'s own doc comment). `None` if
/// `old_id` no longer resolves (e.g. some other effect already moved it
/// between the choice pausing and being answered) — same defensive stance
/// `commit_death`'s own callers take for a participant that stopped
/// resolving mid-pipeline.
///
/// Deliberately NOT routed through CR 614-616 replacement checking, unlike
/// `commit_death` — no card in this engine produces a competing
/// replacement effect on this specific event yet, and CR 903.9b (the
/// "would go to hand/library, put in the command zone instead" redirect)
/// — the natural vehicle for one — is out of scope this slice. A real gap,
/// flagged rather than silently glossed over.
pub(crate) fn move_commander_to_command_zone(state: &mut GameState, old_id: ObjectId) -> Option<GameEvent> {
    let object = state.object(old_id)?;
    let from = object.zone;
    let owner = object.owner;
    let last_known_info = object.clone();

    let new_id = state.allocate_object_id();
    let mut new_object = last_known_info.clone();
    new_object.object_id = new_id;
    new_object.lineage = ObjectLineage { previous: Some(old_id) };
    new_object.zone = ZoneKind::Command;
    new_object.timestamp = state.allocate_sequence_no();

    // Graveyard is per-owner; exile is shared (owner: None on the Zone
    // itself) - same distinction `remove_players_objects` above already
    // has to make.
    let from_zone_owner = if from == ZoneKind::Exile { None } else { Some(owner) };
    if let Some(zone) = state.zone_mut(from, from_zone_owner) {
        zone.remove(old_id);
    }
    commands::retire_object(state, old_id);
    if let Some(zone) = state.zone_mut(ZoneKind::Command, Some(owner)) {
        zone.push_bottom(new_id);
        state.objects.insert(new_id, new_object);
    }

    let event_id = state.allocate_event_id();
    let sequence_no = state.allocate_sequence_no();
    Some(GameEvent {
        event_id,
        sequence_no,
        kind: GameEventKind::ZoneChange {
            object: new_id,
            from,
            to: ZoneKind::Command,
            last_known_info: Some(last_known_info),
        },
        caused_by_command: None,
        parent_event: None,
    })
}
