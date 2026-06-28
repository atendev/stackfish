use crate::types::counter::CounterKind;
use crate::types::ids::{CommandId, EventId, ObjectId, PlayerId, SequenceNo, Target};
use crate::types::zone::ZoneKind;

/// Ledger granularity (DESIGN.md open question): logs every micro-step, not
/// just card-level moves, so the replay viewer can scrub at fine resolution
/// and the live action log can filter/group from a single underlying stream.
/// This variant list is a first pass — expect additions as Phase 5 dry runs
/// surface gaps.
pub enum GameEventKind {
    ZoneChange { object: ObjectId, from: ZoneKind, to: ZoneKind },
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
        todo!()
    }
}
