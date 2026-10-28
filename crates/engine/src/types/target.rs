use crate::types::card::{CardTypeSet, Subtype, SupertypeSet};
use crate::types::cost::FilterController;
use crate::types::ids::ScriptHandle;
use crate::types::zone::ZoneKind;

/// Comparison in `TargetFilter::Object::mana_value`. Real cards use only
/// equality or an inclusive bound, so there is no `LessThan` or `GreaterThan`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManaValueComparison {
    Equal,
    AtMost,
    AtLeast,
}

/// A fixed number, or the casting's own announced X. `X` resolves to 0 when
/// no X was announced (CR 107.3a), which is a script authoring mistake and
/// not an engine gap.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManaValueBound {
    Fixed(u32),
    X,
}

/// CR 202.3, 601.2c, 115: "target spell with mana value X (or less, or
/// greater)". See `TargetFilter::Object::mana_value`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ManaValueRequirement {
    pub comparison: ManaValueComparison,
    pub bound: ManaValueBound,
}

/// What CR 601.2c, 602.2b and 115 restrict a target to. It mirrors the
/// controller and card type shape of `EffectFilter` and `FilterSpec`, adds a
/// `zone` (battlefield by default, CR 115.2) and has the two non-object
/// shapes (CR 115.4, target player). "Another target" is
/// `TargetRequirement::exclude_source`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TargetFilter {
    /// `zone: None` means the battlefield (CR 115.2). `Some(zone)` targets a
    /// card elsewhere, such as a spell on the stack (CR 115.2b).
    ///
    /// `subtypes` matches any listed subtype (CR 205.4b), like
    /// `FilterSpec::subtypes`. `supertypes` is a `SupertypeSet` intersection
    /// (CR 205.4a). The `excluded_*` fields reject a target matching any of
    /// them, independent of the positive fields (CR 205.4a, 205.4b).
    Object {
        card_types: Option<CardTypeSet>,
        excluded_card_types: Option<CardTypeSet>,
        subtypes: Option<Vec<Subtype>>,
        excluded_subtypes: Option<Vec<Subtype>>,
        supertypes: Option<SupertypeSet>,
        excluded_supertypes: Option<SupertypeSet>,
        controller: Option<FilterController>,
        zone: Option<ZoneKind>,
        /// CR 202.3, 601.2c, 115: compared with the candidate's mana value in
        /// `commands::target_is_legal`. A candidate on the stack uses its
        /// announced X and any other zone uses X = 0 (CR 202.3b). `None` means
        /// no restriction.
        mana_value: Option<ManaValueRequirement>,
    },
    /// "Target player" or "target opponent" (CR 115.1). `None` is any player,
    /// and `controller` is relative to the caster or activator.
    Player { controller: Option<FilterController> },
    /// CR 115.4: creature, player, planeswalker or battle.
    /// TODO: planeswalkers and battles are non-goals in DESIGN.md, so their type bits are classified here and nowhere else treats them as permanents
    AnyTarget,
    /// CR 601.2c, 602.2b: custom Lua legality predicate for shapes `Object`
    /// cannot express, such as "power 4 or greater". Nested here so `count`
    /// and `up_to` still apply. `ScriptRuntime::invoke_target_legal` runs once
    /// per chosen candidate from `commands::target_is_legal`. Candidates are
    /// `Target::Object`; see `PlayerScript`. The script sees full public state
    /// and decides the zone itself.
    Script(ScriptHandle),
    /// CR 601.2c, 602.2b: `Script` for player candidates, such as "target
    /// player with 10 or less life". It is separate because `Script` passes an
    /// object id number to Lua, which a player would break.
    /// `ScriptRuntime::invoke_player_target_legal` runs once per candidate.
    /// There is no protection check, as with `Player`.
    PlayerScript(ScriptHandle),
    /// CR 113.9, 701.6a: "counter target activated or triggered ability",
    /// matched against `Target::Ability(stack_id)`. Separate from
    /// `Object { zone: Some(Stack) }` because an ability has no `GameObject`
    /// for `state.object(id)` to find. `commands::target_is_legal` checks
    /// `state.stack`, and a copy of an ability matches too (CR 707.10).
    Ability,
    /// CR 702.21a: the spell or ability whose event triggered this ability.
    /// `push_ordered_triggers` fills it from `PendingTrigger::triggering_stack`
    /// instead of raising a CR 603.3d choice, and drops the trigger if that
    /// object is gone. It stays legal while on the stack, so the CR 608.2b
    /// recheck fizzles it once the object leaves. Only a triggered ability can
    /// have one.
    TriggeringStackObject,
}

/// `count` targets matching `filter` for one instance of the word "target"
/// (CR 601.2c, 115.3). `up_to` and `at_least` are mutually exclusive:
/// - neither: exactly `count`.
/// - `up_to` (CR 115.6): 0 to `count`. Choosing zero is legal.
/// - `at_least` (CR 115.6): `count` or more with no upper bound. "Any number"
///   is `count: 0`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TargetRequirement {
    pub filter: TargetFilter,
    pub count: u32,
    pub up_to: bool,
    /// CR 115.6, "N or more". Checked in `commands::validate_targets`. See the
    /// struct doc.
    pub at_least: bool,
    /// CR 115.4, "another target": the source is never a legal candidate.
    /// This is separate from the same-target dedup of CR 115.3, which
    /// `commands::validate_targets` always applies. Checked in
    /// `commands::target_is_legal`.
    pub exclude_source: bool,
}

/// The `None` of an `Option<TargetSpec>` means no targets. `Primitive` is a
/// spell or ability with one "target" word. A script predicate lives in
/// `TargetFilter::Script` so it keeps `count` and `up_to`.
#[derive(Clone, Debug, PartialEq)]
pub enum TargetSpec {
    Primitive(TargetRequirement),
    /// CR 601.2c: a spell or ability with several "target" words, one entry
    /// per word ("Destroy target nonblack creature and target land"). Dedup
    /// applies within an entry and never across entries (CR 115.3).
    /// `commands::split_targets_by_requirement` cuts the flat `targets` list
    /// into per-entry chunks. Exact entries claim `count`, and one variable
    /// entry takes the remainder. With two or more variable entries,
    /// `assign_targets_by_filter` gives each target to a word whose filter it
    /// satisfies and accepts any partition that meets every bound. A spell
    /// whose words feed different verbs needs a `Script` effect.
    Multiple(Vec<TargetRequirement>),
}
