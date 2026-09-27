use std::collections::HashMap;

use crate::types::ability::RegisteredAbility;
use crate::types::combat::CombatState;
use crate::types::cost::CostModifier;
use crate::types::effect::{ContinuousEffect, ReplacementEffect};
use crate::types::event::GameEvent;
use crate::types::ids::{AbilityId, EventId, ObjectId, PlayerId, SequenceNo};
use crate::types::object::GameObject;
use crate::types::player::{DrawLimitModifier, HandRevealModifier, HandSizeModifier, LandPlayModifier, PlayerState};
use crate::types::stack::StackObject;
use crate::types::turn::{Phase, Step};
use crate::types::zone::{Zone, ZoneKind};

/// Seeded once at game start. Every draw is itself a logged `GameEvent`
/// (`RngDrawn`), not an ambient side effect, so replay is exact.
pub struct RngState {
    pub seed: u64,
    pub draws_consumed: u64,
}

/// Owns the full mutable state for one game. Player count is a runtime
/// parameter — not hardcoded to 2 or 4 — to support N-player Commander.
pub struct GameState {
    pub players: Vec<PlayerState>,
    /// Turn order as a list of player indices into `players`; APNAP order is
    /// this list rotated to start at the active player.
    pub turn_order: Vec<PlayerId>,
    pub active_player: PlayerId,
    pub turn_number: u32,
    pub phase: Phase,
    pub step: Step,
    /// Folded in from the Phase 5 PassPriority dry run (see
    /// docs/implementation.md) — neither field existed in Phase 1.
    /// `turn_order`/`active_player` alone don't tell you who currently has
    /// priority or how many players in a row have passed it (CR 117.3a,
    /// 117.4).
    pub priority_player: PlayerId,
    pub consecutive_passes: u32,
    /// CR 305.2: a player can normally play one land per turn, raised by
    /// `land_play_modifiers` below (`phase5/land-play-limit`). Keyed by
    /// player rather than a single counter since CR 305.2a's comparison is
    /// per-player ("the number of lands they have already played this
    /// turn"). Reset to all-zero whenever a new turn begins
    /// (Beginning/Untap) — `land_play_modifiers` itself is not reset here,
    /// same "persists for as long as its source does" continuous-effect
    /// posture every other modifier list in this cluster already takes.
    pub lands_played_this_turn: HashMap<PlayerId, u32>,
    /// CR 506-509: `None` outside combat, `Some` from `DeclareAttackers`
    /// onward, cleared at `EndOfCombat` (`engine::Engine::advance_turn_step`)
    /// so it never leaks into the next turn's combat.
    pub combat: Option<CombatState>,
    /// CR 302.6/602.5a's "controller's most recent turn" — the
    /// `turn_number` recorded the moment each player most recently became
    /// the active player. Written by `advance_turn_step` whenever
    /// `active_player` rotates (plus once for the very first turn, from
    /// `PreGame`). Consulted by `commands::has_summoning_sickness` against
    /// `GameObject.controlled_since_turn` — see that field's doc comment
    /// for why this is a derived comparison rather than a stored bool.
    pub last_active_turn_number: HashMap<PlayerId, u32>,
    /// Object store — single source of truth for object data, looked up by
    /// id. `zones` below holds only ids in order; that's the single source
    /// of truth for *order*, so order is never duplicated on the object
    /// itself (see the no-`zone_position` note on `GameObject`).
    pub objects: HashMap<ObjectId, GameObject>,
    /// Every library/hand/graveyard/command zone (one set per player) plus
    /// the shared battlefield and exile — one flat list rather than a
    /// per-player struct, since the zone's own `owner: Option<PlayerId>`
    /// already disambiguates whose zone it is. The Stack is the deliberate
    /// exception: it's tracked in `stack` below instead, since it needs
    /// `StackObject` metadata, not just an id list.
    pub zones: Vec<Zone>,
    pub stack: Vec<StackObject>,
    pub continuous_effects: Vec<ContinuousEffect>,
    pub replacement_effects: Vec<ReplacementEffect>,
    /// CR 601.2f/118.9 cost-modification effects (Goblin Warchief, Thalia,
    /// ...) — flat list, same "scan-friendly, not nested" convention as
    /// `continuous_effects`/`replacement_effects` above. See `CostModifier`'s
    /// own doc comment (cost.rs) for why this is a distinct mechanism from
    /// the CR 613 layer system those two lists feed.
    pub cost_modifiers: Vec<CostModifier>,
    /// CR 402.2 max-hand-size effects ("you have no maximum hand size", ...)
    /// — same "distinct mechanism from the CR 613 layer system" reasoning
    /// `cost_modifiers` above already established; see `HandSizeModifier`'s
    /// own doc comment (types/player.rs).
    pub hand_size_modifiers: Vec<HandSizeModifier>,
    /// CR 701.20a/701.18c "plays with hand revealed"-style effects — same
    /// "distinct mechanism from the CR 613 layer system" reasoning
    /// `hand_size_modifiers` above already established; see
    /// `HandRevealModifier`'s own doc comment (types/player.rs).
    /// `phase5/hand-reveal-modifier`: a mechanism only, no real enforcement
    /// consumer exists yet (see that struct's own doc comment).
    pub hand_reveal_modifiers: Vec<HandRevealModifier>,
    /// CR 121.2b "player can't draw more than one card each turn"-style
    /// effects — same "distinct mechanism from the CR 613 layer system"
    /// reasoning `hand_size_modifiers`/`hand_reveal_modifiers` above already
    /// established; see `DrawLimitModifier`'s own doc comment (types/player.rs).
    pub draw_limit_modifiers: Vec<DrawLimitModifier>,
    /// CR 305.2 "play an additional land each turn"-style effects — same
    /// "distinct mechanism from the CR 613 layer system" reasoning
    /// `draw_limit_modifiers` above already established; see
    /// `LandPlayModifier`'s own doc comment (types/player.rs) for why this
    /// one sums instead of taking the latest timestamp. Consulted by
    /// `commands::execute_play_land` in place of the old hardcoded `1`.
    pub land_play_modifiers: Vec<LandPlayModifier>,
    /// Every card's registered triggered/activated/static abilities, flat —
    /// same reasoning as `continuous_effects`/`replacement_effects` above.
    pub abilities: Vec<RegisteredAbility>,
    pub ledger: Vec<GameEvent>,
    pub rng: RngState,
    /// Dedicated monotonic counter for `EventId` allocation — deliberately
    /// not derived from `ledger.len()` (that broke as soon as more than one
    /// event could be in flight before all of them land in the ledger, or
    /// events get generated somewhere other than the exact moment of the
    /// push). See `allocate_event_id`.
    pub next_event_id: u64,
    /// Dedicated monotonic counter for `SequenceNo` allocation. Same
    /// rationale as `next_event_id` — `ledger.len()` at commit time isn't a
    /// stable source: it depends on how many events from the same batch
    /// already landed, so replaying the same command stream against a
    /// different batching of `CommitEvents` calls could reorder/reuse
    /// sequence numbers. See `allocate_sequence_no`.
    pub next_sequence_no: u64,
    /// Dedicated monotonic counter for `ObjectId` allocation. Same rationale
    /// as `next_event_id`/`next_sequence_no` — needed because CR 400.7
    /// requires a zone change that creates a "new object" to mint a fresh
    /// `ObjectId` rather than reuse the old one (see `ObjectLineage`). See
    /// `allocate_object_id`.
    pub next_object_id: u64,
    /// CR 500.7/500.8: queued step skips — each entry is a (player, step)
    /// pair meaning that player's next occurrence of that step is skipped
    /// entirely (no turn-based actions, no priority window). Consumed by
    /// `consume_step_skip` at the top of `AdvanceTurnStep`. Card effects that
    /// say "skip your next draw step" (etc.) push here; the CR 103.8a
    /// two-player turn-1 draw skip is seeded here at game start.
    pub pending_step_skips: Vec<(PlayerId, Step)>,
    /// CR 603.2h: which triggered abilities (by stable `AbilityId` — see
    /// that type's own doc comment) have already triggered this turn, for a
    /// `ConditionSpec::PrimitiveOncePerTurn` condition ("This ability
    /// triggers only once each turn"). Checked and populated by
    /// `triggers::collect_triggers`/`Engine`'s `CollectTriggers` stage;
    /// reset alongside `lands_played_this_turn` whenever a new turn begins.
    pub once_per_turn_triggered: std::collections::HashSet<AbilityId>,
    /// CR 903.9a: which commander object ids currently sitting in a
    /// graveyard or exile have already been offered the "owner may put it
    /// into the command zone" state-based choice — a *declined* offer must
    /// not be re-asked on every subsequent SBA pass; this is a one-time
    /// window per arrival ("since state-based actions were last checked"),
    /// not a standing question. An *accepted* return mints a new `ObjectId`
    /// on the way to the command zone (CR 400.7), so its old id here becomes
    /// moot. Entries are never removed, only ever grow — same low-cost,
    /// never-reused-id bookkeeping `once_per_turn_triggered` already
    /// accepts, just not reset per-turn since this isn't a per-turn cap.
    pub commander_return_offered: std::collections::HashSet<ObjectId>,
    /// CR 707.9e/g: tags a `RegisteredAbility` (by stable `AbilityId`) with
    /// the `GameObject::copy_generation` its source object was at when a
    /// copy effect granted it — a side-channel map rather than a field on
    /// `RegisteredAbility` itself, which has 100+ struct-literal construction
    /// sites and no `Default` impl (the same "field would ripple everywhere,
    /// most of them meaninglessly `None`" reasoning `ManaPool.restricted`'s
    /// own doc comment already gives for a sibling side-channel). Consulted
    /// by `triggers::collect_triggers`: a tagged ability only matches an
    /// event if its source's *current* `copy_generation` still equals the
    /// tagged value — once another copy effect applies to the same object
    /// (bumping the counter), the tag is stale and the ability simply never
    /// matches again (CR 707.9g, "the ability doesn't trigger"). Entries are
    /// never removed — a stale tag just means permanent non-matching, not
    /// something that needs cleanup, same low-cost stance
    /// `commander_return_offered` already takes.
    ///
    /// Real, tested enforcement — nothing writes to this map yet, since no
    /// `CopyExceptions` shape in this engine grants a brand-new ability as
    /// part of copying (only characteristic-modification exceptions exist
    /// so far); same "real mechanism, no producer wired in yet" posture
    /// `HandRevealModifier`'s own doc comment already takes. CR 707.9e's
    /// "additional effect" exception is the same underlying question against
    /// the same counter, with no consumer of any kind in this engine yet.
    pub linked_ability_generation: std::collections::HashMap<AbilityId, u32>,
    /// Ids of every `RegisteredAbility` that `ScriptRuntime::run_card_script`
    /// itself registered, as opposed to one built directly (a token's
    /// hand-built Clue ability, a copy effect's granted trigger, a test
    /// fixture). `commands::remap_abilities` drops these on a zone change
    /// instead of carrying them to the new `ObjectId`: the script is re-run
    /// for whichever zone the object lands in, so carrying the old
    /// registration forward would register everything twice. A side set
    /// rather than a field on `RegisteredAbility` (~160 construction sites).
    pub script_ability_ids: std::collections::HashSet<AbilityId>,
    /// CR 702.35b: one-shot timing-restriction waiver, set immediately
    /// before `Engine::submit_madness_cast` queues its synthetic `CastSpell`
    /// command and consumed (`.take()`) by `commands::cast::execute_cast_spell`'s
    /// own sorcery-speed/priority gate — the resolving triggered ability's
    /// own "you may cast it" text is itself the CR 601.3 "effect [that]
    /// allows" casting outside the caster's normal timing permission, not a
    /// property of the alternative cost declaration itself (unlike
    /// `AlternativeCost::Replacement.escape`, which is real provenance data
    /// consulted long after the cast). A side-channel `Option<ObjectId>`
    /// rather than a new field threaded through `PlayerActionKind::CastSpell`
    /// — that enum has ~87 construction sites, the same ripple class this
    /// project has repeatedly avoided (see `ModeChoice`'s own history).
    /// `.take()`-consumed unconditionally so it can never leak into a later,
    /// unrelated ordinary cast of the same card.
    pub pending_timing_bypass: Option<ObjectId>,
    /// CR 702.85a Cascade: a one-shot "cast this exact object paying no
    /// mana cost at all" grant, set immediately before `Engine::
    /// submit_cascade_cast` queues its synthetic `CastSpell` command and
    /// consumed (`.take()`) by `commands::cast::execute_cast_spell`, which
    /// synthesizes a transient `{0}`-mana `AlternativeCost::Replacement`
    /// for it rather than reading one off the object's own (nonexistent —
    /// a random library card was never printed with a Cascade-shaped
    /// alternative cost) `CardDefinition.alternative_costs`. Distinct from
    /// `pending_timing_bypass` above: Madness's own cost is real (its own
    /// printed alternative cost), only the *timing* needed waiving; Cascade
    /// waives the *cost* itself, and (like Madness) also needs the timing
    /// waiver, so `submit_cascade_cast` sets both fields together.
    pub pending_free_cast: Option<ObjectId>,
}

impl GameState {
    pub fn allocate_event_id(&mut self) -> EventId {
        let id = EventId(self.next_event_id);
        self.next_event_id += 1;
        id
    }

    pub fn allocate_sequence_no(&mut self) -> SequenceNo {
        let seq = SequenceNo(self.next_sequence_no);
        self.next_sequence_no += 1;
        seq
    }

    pub fn allocate_object_id(&mut self) -> ObjectId {
        let id = ObjectId(self.next_object_id);
        self.next_object_id += 1;
        id
    }

    /// Mints a fresh `AbilityId` — see that type's own doc comment for why
    /// this draws from `allocate_sequence_no`'s counter rather than a
    /// dedicated one.
    pub fn allocate_ability_id(&mut self) -> AbilityId {
        AbilityId(self.allocate_sequence_no().0)
    }

    pub fn object(&self, id: ObjectId) -> Option<&GameObject> {
        self.objects.get(&id)
    }

    pub fn object_mut(&mut self, id: ObjectId) -> Option<&mut GameObject> {
        self.objects.get_mut(&id)
    }

    pub fn player(&self, id: PlayerId) -> Option<&PlayerState> {
        self.players.iter().find(|p| p.player_id == id)
    }

    pub fn player_mut(&mut self, id: PlayerId) -> Option<&mut PlayerState> {
        self.players.iter_mut().find(|p| p.player_id == id)
    }

    /// Looks up the one `Zone` matching `kind`/`owner` in `zones`. Panics-
    /// worthy if more than one matches — that'd mean `zones` was built
    /// wrong (e.g. two battlefields) — but that invariant isn't enforced
    /// yet (Phase 6).
    pub fn zone(&self, kind: ZoneKind, owner: Option<PlayerId>) -> Option<&Zone> {
        self.zones.iter().find(|z| z.kind == kind && z.owner == owner)
    }

    pub fn zone_mut(&mut self, kind: ZoneKind, owner: Option<PlayerId>) -> Option<&mut Zone> {
        self.zones.iter_mut().find(|z| z.kind == kind && z.owner == owner)
    }

    /// CR 500.7/500.8: checks whether `player`'s current occurrence of `step`
    /// should be skipped, and if so consumes the entry. Returns `true` if the
    /// step is skipped (caller should advance turn without running any
    /// turn-based actions or opening a priority window).
    pub fn consume_step_skip(&mut self, player: PlayerId, step: Step) -> bool {
        if let Some(pos) = self.pending_step_skips.iter().position(|(p, s)| *p == player && *s == step) {
            self.pending_step_skips.remove(pos);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ids::ObjectId;
    use std::collections::VecDeque;

    /// Direct unit test against `Zone::remove`/`push_bottom` and
    /// `GameState::zone_mut`/`object_mut` themselves, isolated from the
    /// command layer (`PlayLand` in commands.rs exercises the same helpers
    /// through a real command, but this pins down the helpers' own
    /// behavior independently).
    #[test]
    fn zone_helpers_roundtrip() {
        let hand = PlayerId(0);
        let mut state = GameState {
            players: Vec::new(),
            turn_order: vec![hand],
            active_player: hand,
            turn_number: 1,
            phase: crate::types::turn::Phase::PreGame,
            step: crate::types::turn::Step::Untap,
            priority_player: hand,
            consecutive_passes: 0,
            lands_played_this_turn: HashMap::new(),
            combat: None,
            last_active_turn_number: HashMap::new(),
            objects: HashMap::new(),
            zones: vec![
                Zone { kind: ZoneKind::Hand, owner: Some(hand), contents: VecDeque::new() },
                Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() },
            ],
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            cost_modifiers: Vec::new(),
            hand_size_modifiers: Vec::new(),
            hand_reveal_modifiers: Vec::new(),
            draw_limit_modifiers: Vec::new(),
            land_play_modifiers: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState { seed: 1, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 1,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
            linked_ability_generation: std::collections::HashMap::new(),
            script_ability_ids: std::collections::HashSet::new(),
            pending_timing_bypass: None,
            pending_free_cast: None,
        };

        let card = ObjectId(0);
        state.zone_mut(ZoneKind::Hand, Some(hand)).unwrap().push_bottom(card);
        assert!(state.zone(ZoneKind::Hand, Some(hand)).unwrap().contents.contains(&card));

        let removed = state.zone_mut(ZoneKind::Hand, Some(hand)).unwrap().remove(card);
        assert_eq!(removed, Some(card));
        assert!(!state.zone(ZoneKind::Hand, Some(hand)).unwrap().contents.contains(&card));

        state.zone_mut(ZoneKind::Battlefield, None).unwrap().push_bottom(card);
        assert!(state.zone(ZoneKind::Battlefield, None).unwrap().contents.contains(&card));

        // Nonexistent zone (no command zone registered here) -> None, not a panic.
        assert!(state.zone_mut(ZoneKind::Command, Some(hand)).is_none());

        // object()/object_mut() round-trip against an absent id.
        assert!(state.object(card).is_none());
        assert!(state.object_mut(card).is_none());
    }
}
