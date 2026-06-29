/// Index into the turn-order player list. Stable for the lifetime of the game.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PlayerId(pub u32);

/// Unique per `GameObject` instance. A zone change that creates a "new object"
/// (per MTG rules) mints a new `ObjectId` rather than reusing the old one.
/// `Hash`/`Eq` so it can key the object store map in `GameState`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ObjectId(pub u64);

/// Links a new `ObjectId` back to the object it succeeded, if any.
#[derive(Clone, Copy, Debug)]
pub struct ObjectLineage {
    pub previous: Option<ObjectId>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct EventId(pub u64);

/// Strictly increasing. Defines total order of the event ledger.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SequenceNo(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct CommandId(pub u64);

/// Anything that can legally be targeted, attacked, or dealt damage:
/// permanents/spells/abilities (`Object`) or a player directly (`Player`).
/// Battles and planeswalkers are still `Object` — they're `GameObject`s like
/// any other permanent; `Player` only covers the one addressable thing in
/// the game that *isn't* a `GameObject`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Target {
    Object(ObjectId),
    Player(PlayerId),
}

/// Opaque reference to one loaded Lua callback (a condition/cost/target/
/// operation function, a continuous-effect predicate, a replacement
/// pattern-matcher, etc) inside a `ScriptRuntime`. A plain handle rather
/// than holding an `mlua::Function` directly so data types (`ContinuousEffect`,
/// `ReplacementEffect`, `RegisteredAbility`) can reference "which script"
/// without themselves depending on `mlua` or holding VM-tied state.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ScriptHandle(pub u64);
