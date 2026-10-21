/// Index into the turn-order player list. Stable for the lifetime of the game.
/// `PartialOrd`/`Ord` exist purely as a deterministic sort/map key (e.g.
/// `sba::find_legend_rule_group`'s `BTreeMap` grouping) — not a meaningful
/// "which player is greater" rule concept.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PlayerId(pub u32);

/// Unique per `GameObject` instance. A zone change that creates a "new object"
/// (per MTG rules) mints a new `ObjectId` rather than reusing the old one.
/// `Hash`/`Eq` so it can key the object store map in `GameState`.
/// `PartialOrd`/`Ord` exist purely as a deterministic sort key (e.g.
/// `sba::find_legend_rule_group`'s candidate sort) — a `HashMap`'s iteration
/// order isn't guaranteed, this just gives callers a stable tiebreak.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ObjectId(pub u64);

/// Links a new `ObjectId` back to the object it succeeded, if any.
#[derive(Clone, Copy, Debug, PartialEq)]
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

/// Unique per `RegisteredAbility`, stable for its lifetime (survives a CR
/// 400.7 zone-change remap of its own `source` — `commands::remap_abilities`
/// only rewrites `source` in place, never touches `id`). Minted via
/// `GameState::allocate_ability_id`, which draws from the same counter
/// `allocate_sequence_no` already does (no dedicated `next_ability_id`
/// counter field — `GameState` is constructed as a full literal in 100+
/// test fixtures, and this engine already reuses `SequenceNo`'s counter for
/// more than one purpose, e.g. `GameObject.timestamp`, so a second reuse
/// here avoids that mechanical fallout rather than inventing a new field).
/// A distinct type from `SequenceNo` regardless — same "shared counter,
/// different meaning" stance as `GameObject.timestamp`'s own doc comment.
///
/// Replaces positional "the nth ability registered to this source"
/// indexing for `triggers::PendingTrigger` (`phase5/ability-stable-id`) —
/// that index was computed once at trigger-collection time but consulted
/// later, after an `AwaitingTriggerOrderChoice` pause and potentially
/// arbitrary intervening game actions, by which point a *different*
/// pending trigger's own source could (once CR 613.1f's "loses all
/// abilities" someday strips real `RegisteredAbility` entries, still
/// unimplemented — see `99-backlog.md`) have had its ability list shift
/// out from under a stale position. `PlayerActionKind::ActivateAbility`'s
/// own `ability_index: u32` deliberately keeps its old positional meaning
/// unchanged — it's chosen and consumed in the same synchronous step, never
/// spanning a pause the way a `PendingTrigger` does, so it was never
/// actually at risk (confirmed by finding its only consumer, `find_ability`,
/// is never called across a real time gap).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AbilityId(pub u64);

/// Anything that can legally be targeted, attacked, or dealt damage:
/// permanents/spells (`Object`) or a player directly (`Player`), plus an
/// activated/triggered ability on the stack (`Ability`, CR 113.9 — a
/// distinct target category from `Object`, since it has no `GameObject`).
/// Battles and planeswalkers are still `Object` — they're `GameObject`s like
/// any other permanent; `Player` only covers the one addressable thing in
/// the game that *isn't* a `GameObject`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Target {
    Object(ObjectId),
    Player(PlayerId),
    /// CR 113.9/701.6a: an activated or triggered ability sitting on the
    /// stack — its own `StackObject::stack_id`. Distinct from `Object`
    /// because an ability has no matching `GameObject` in `state.objects`
    /// (only a *spell*'s `StackObject` gets one, at `zone: Stack` — see
    /// `TargetFilter::Ability`'s own doc comment).
    Ability(ObjectId),
}

/// Opaque reference to one loaded Lua callback (a condition/cost/target/
/// operation function, a continuous-effect predicate, a replacement
/// pattern-matcher, etc) inside a `ScriptRuntime`. A plain handle rather
/// than holding an `mlua::Function` directly so data types (`ContinuousEffect`,
/// `ReplacementEffect`, `RegisteredAbility`) can reference "which script"
/// without themselves depending on `mlua` or holding VM-tied state.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ScriptHandle(pub u64);
