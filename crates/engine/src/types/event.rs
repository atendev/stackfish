use crate::types::counter::CounterKind;
use crate::types::ids::{CommandId, EventId, ObjectId, PlayerId, SequenceNo, Target};
use crate::types::object::GameObject;
use crate::types::zone::ZoneKind;

/// Ledger granularity (DESIGN.md open question): logs every micro-step, not
/// just card-level moves, so the replay viewer can scrub at fine resolution
/// and the live action log can filter/group from a single underlying stream.
/// This variant list is a first pass — expect additions as Phase 5 dry runs
/// surface gaps.
pub enum GameEventKind {
    /// `last_known_info` (CR 603.10): a snapshot of `object` exactly as it
    /// was in `from`, captured by whoever produces this event *before*
    /// mutating anything — `None` only when the producer is certain nothing
    /// downstream needs it (rare; default to capturing it). Without this,
    /// a leaves-the-battlefield trigger evaluated after `CommitEvents`/
    /// `SbaLoop` settle would have to look `object` up live in
    /// `GameState.objects` — but a battlefield -> graveyard move mints a
    /// *new* `ObjectId` (see `ObjectLineage`), so the old id may already be
    /// gone, or whatever still answers to it may have already lost
    /// characteristics (e.g. an aura-granted ability) that justified the
    /// trigger in the first place. Same shape of gap `StackObject.
    /// source_lki` (CR 608.2b) already solves on the stack side - this is
    /// the zone-change-event equivalent.
    ZoneChange { object: ObjectId, from: ZoneKind, to: ZoneKind, last_known_info: Option<GameObject> },
    LifeChanged { player: PlayerId, delta: i32 },
    DamageDealt { source: ObjectId, target: Target, amount: u32 },
    CounterAdded { object: ObjectId, kind: CounterKind, amount: i32 },
    ManaAdded { player: PlayerId, amount: crate::mana::ManaCost },
    PrioritySet { player: PlayerId },
    PriorityPassed { player: PlayerId },
    StackObjectPushed { stack_id: ObjectId },
    StackObjectResolved { stack_id: ObjectId },
    TriggerQueued { source: ObjectId, controller: PlayerId },
    ReplacementApplied { source: ObjectId, original_event: EventId },
    StateBasedActionApplied { description: String },
    TurnStarted { player: PlayerId, turn_number: u32 },
    PhaseStarted { phase: crate::turn::Phase },
    StepStarted { step: crate::turn::Step },
    PlayerEliminated { player: PlayerId },
    RngDrawn { purpose: String, value: u64 },
}

/// Immutable fact. Once committed, never mutated or removed.
pub struct GameEvent {
    pub event_id: EventId,
    pub sequence_no: SequenceNo,
    pub kind: GameEventKind,
    pub caused_by_command: Option<CommandId>,
    pub parent_event: Option<EventId>,
}

impl GameEvent {
    pub fn new(event_id: EventId, sequence_no: SequenceNo, kind: GameEventKind) -> Self {
        // TODO: GameEvent { event_id, sequence_no, kind, caused_by_command: None,
        // parent_event: None } - caller sets the causal-ref fields after
        // construction if applicable, or this may need optional params added.
        todo!()
    }
}
