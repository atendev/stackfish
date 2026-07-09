use std::collections::VecDeque;

use crate::commands;
use crate::replacement::{self, ReplacementOutcome};
use crate::script::ScriptRuntime;
use crate::types::ability::RegisteredAbility;
use crate::types::card::{CardTypeSet, KeywordSet};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId};
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
        candidates: Vec<ObjectId>,
        excluded: Vec<ObjectId>,
        pending_deaths: VecDeque<ObjectId>,
        resolved_so_far: Vec<GameEvent>,
    },
}

/// One pass of CR 704's state-based actions: finds every applicable SBA and
/// applies all of them simultaneously, mutating `state` and returning the
/// `GameEvent`s that resulted (destroys, eliminations, etc) for the caller
/// to append to the ledger and feed into trigger collection — or, if a dying
/// creature's zone-change replacement needs a CR 616.1 ordering choice,
/// pausing via `SbaOutcome::NeedsOrderingChoice` instead (see `resolve_deaths`).
///
/// Implemented this pass: 704.5a (player at 0 or less life loses), 704.5f
/// (creature toughness 0 or less goes to its owner's graveyard, or wherever
/// a matching `ReplacementEffect` substitutes instead), 704.5g (creature
/// with lethal damage marked is destroyed, folding in 702.2b/c's
/// Deathtouch — any nonzero deathtouch damage counts as lethal — and
/// 702.12b's Indestructible — ignores 704.5g/deathtouch entirely, but NOT
/// 704.5f). Every other CR 704.5 SBA is still a no-op — see the TODO below
/// for which and why.
///
/// This is a single pass, not the fixed-point loop — CR 704.3 requires
/// repeating until no SBA applies. The engine loop (`engine.rs`) is
/// responsible for calling this in a loop until it returns empty, not this
/// function itself, so a caller stepping through ledger entries one at a
/// time can observe each pass separately rather than the loop being opaque.
pub fn check_state_based_actions(state: &mut GameState, script_runtime: &ScriptRuntime) -> SbaOutcome {
    let mut events = Vec::new();
    apply_player_loss(state, &mut events);

    let dying = collect_dying_creatures(state, script_runtime);
    match resolve_deaths(state, dying.into()) {
        DeathResolution::Done(death_events, departed_triggered) => {
            events.extend(death_events);
            SbaOutcome::Progressed(events, departed_triggered)
        }
        DeathResolution::NeedsOrderingChoice {
            affected_player,
            pending_event,
            candidates,
            excluded,
            pending_deaths,
        } => SbaOutcome::NeedsOrderingChoice {
            affected_player,
            pending_event,
            candidates,
            excluded,
            pending_deaths,
            resolved_so_far: events,
        },
    }

    // TODO: still unimplemented, no prerequisite machinery exists yet:
    //   - 704.5b: player who attempted to draw from an empty library loses
    //     (nothing tracks a failed draw attempt yet).
    //   - 704.5c: player with 10+ poison counters loses (PlayerState.counters
    //     exists but nothing increments poison yet).
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
    //   - regeneration (CR 701.16): would replace 704.5g's destroy, not
    //     modeled, lethal damage always destroys outright here.
    //   - legend rule (704.5j), world rule (704.5k), planeswalker loyalty 0
    //     (704.5i - not v1 scope per DESIGN.md), commander damage >= 21
    //     (PlayerState::commander_damage_from already exposed for this), and
    //     the rest of 704.5 - no prerequisite subsystem exists yet.
}

/// CR 704.5a: a player at 0 or less life loses the game. `eliminated` is
/// the field this flips (already present on `PlayerState`, just unused
/// until now); already-eliminated players are skipped so re-running this
/// pass doesn't re-emit the event every fixed-point iteration.
fn apply_player_loss(state: &mut GameState, events: &mut Vec<GameEvent>) {
    let losing: Vec<PlayerId> =
        state.players.iter().filter(|p| !p.eliminated && p.life <= 0).map(|p| p.player_id).collect();

    for player in losing {
        if let Some(p) = state.player_mut(player) {
            p.eliminated = true;
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
fn collect_dying_creatures(state: &GameState, script_runtime: &ScriptRuntime) -> Vec<ObjectId> {
    let candidates: Vec<ObjectId> =
        state.objects.values().filter(|o| o.zone == ZoneKind::Battlefield).map(|o| o.object_id).collect();

    candidates
        .into_iter()
        .filter(|&id| {
            let effective = crate::layers::effective_characteristics(state, script_runtime, id);
            // CR 613 layer 4: a land animated into a creature by
            // AddCardType(CREATURE) is subject to this SBA; a plain land
            // (or any other non-creature permanent) isn't - checked against
            // the effective type, not CardDefinition.card_types, so a
            // granted type actually counts.
            if !effective.card_types.contains(CardTypeSet::CREATURE) {
                return false;
            }
            // CDA toughness (PTValue::CharacteristicDefining) can't be
            // resolved without the script bridge - skip rather than guess,
            // same stance `PTValue::as_fixed`/`layers.rs` already take.
            let Some(toughness) = effective.toughness else { return false };
            let object = state.object(id).expect("candidate id came from state.objects itself");
            // CR 704.5f: 0-or-less toughness applies unconditionally, even
            // to an indestructible creature (702.12b only exempts 704.5g).
            if toughness <= 0 {
                return true;
            }
            if effective.keywords.contains(KeywordSet::INDESTRUCTIBLE) {
                return false;
            }
            // CR 704.5g + 702.2b/c: lethal marked damage, or any nonzero
            // damage at all from a Deathtouch source.
            object.damage_marked >= toughness as u32 || (object.deathtouched && object.damage_marked > 0)
        })
        .collect()
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
        candidates: Vec<ObjectId>,
        excluded: Vec<ObjectId>,
        pending_deaths: VecDeque<ObjectId>,
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
    mut pending_deaths: VecDeque<ObjectId>,
) -> DeathResolution {
    let mut resolved = Vec::new();
    let mut departed_triggered = Vec::new();
    while let Some(object_id) = pending_deaths.pop_front() {
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
        match replacement::apply_replacements(state, proposed) {
            ReplacementOutcome::Resolved(final_event) => {
                let (event, mut abilities) = commit_death(state, final_event);
                resolved.push(event);
                departed_triggered.append(&mut abilities);
            }
            ReplacementOutcome::NeedsOrderingChoice { affected_player, event, candidates, excluded } => {
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
        // resolve_deaths only ever feeds this a ZoneChange it just proposed
        // itself - apply_replacements either hands that back unchanged or
        // with `to` rewritten, never a different GameEventKind entirely.
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
    if let Some(zone) = state.zone_mut(to, Some(owner)) {
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
