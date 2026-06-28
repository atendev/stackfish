/// Index into the turn-order player list. Stable for the lifetime of the game.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerId(pub u32);

/// Unique per `GameObject` instance. A zone change that creates a "new object"
/// (per MTG rules) mints a new `ObjectId` rather than reusing the old one.
/// `Hash`/`Eq` so it can key the object store map in `GameState`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectId(pub u64);

/// Links a new `ObjectId` back to the object it succeeded, if any.
pub struct ObjectLineage {
    pub previous: Option<ObjectId>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventId(pub u64);

/// Strictly increasing. Defines total order of the event ledger.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SequenceNo(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommandId(pub u64);
