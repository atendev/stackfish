/// Index into the turn-order player list, stable for the game. `Ord` gives a
/// deterministic map and sort key, not a rules ordering.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PlayerId(pub u32);

/// Unique per `GameObject`. A zone change that makes a new object (CR 400.7)
/// mints a new id. `Ord` gives a stable tiebreak where `HashMap` iteration
/// order is undefined.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ObjectId(pub u64);

/// Links a new `ObjectId` back to the object it succeeded, if any.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObjectLineage {
    pub previous: Option<ObjectId>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct EventId(pub u64);

/// Strictly increasing. Totally orders the event ledger.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SequenceNo(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct CommandId(pub u64);

/// Unique per `RegisteredAbility` and stable for its lifetime, including a
/// CR 400.7 remap of `source`. Drawn from the `SequenceNo` counter by
/// `GameState::allocate_ability_id`, but a distinct type.
///
/// `PendingTrigger` holds this instead of a list position, because the list
/// can change across an `AwaitingTriggerOrderChoice` pause (CR 613.1f).
/// `ActivateAbility::ability_index` stays positional: it is chosen and used
/// within one step.
// TODO: stripping `RegisteredAbility` entries for "loses all abilities" (CR 613.1f)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AbilityId(pub u64);

/// Anything that can be targeted, attacked or dealt damage. Battles and
/// planeswalkers are `Object`; `Player` is the one addressable thing with no
/// `GameObject`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Target {
    Object(ObjectId),
    Player(PlayerId),
    /// CR 113.9, 701.6a: an ability on the stack, by its
    /// `StackObject::stack_id`. Unlike a spell it has no `GameObject`.
    Ability(ObjectId),
}

/// Opaque reference to a Lua callback loaded in a `ScriptRuntime`. A handle
/// keeps data types free of `mlua` and of VM state.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ScriptHandle(pub u64);
