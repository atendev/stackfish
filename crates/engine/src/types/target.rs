use crate::types::card::CardTypeSet;
use crate::types::cost::FilterController;
use crate::types::ids::ScriptHandle;
use crate::types::zone::ZoneKind;

/// What CR 601.2c/602.2b/115 restrict a chosen target to. Mirrors the
/// `EffectFilter` (effect.rs) / `FilterSpec` (cost.rs) "controller +
/// card_types" shape rather than inventing a third one, plus a `zone` (CR
/// 115.2's battlefield-by-default rule) and the two non-object target
/// shapes (CR 115.4's "any target", plain "target player").
///
/// TODO: no supertype/subtype restriction ("target Human"), no "another
/// target" (must differ from one specific other object, not just dedup
/// within this one requirement per CR 115.3), no negative filters ("target
/// creature you don't control that isn't a Wall") — only what a first real
/// spell needs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetFilter {
    /// "target creature" / "target permanent you control" / etc. `zone:
    /// None` defaults to `Battlefield` (CR 115.2: only permanents are legal
    /// targets unless a spell/ability says otherwise) — `Some(zone)` is for
    /// the rarer case of targeting a card in another zone (e.g. a spell on
    /// the stack, CR 115.2b), not exercised by any card built this slice.
    Object {
        card_types: Option<CardTypeSet>,
        controller: Option<FilterController>,
        zone: Option<ZoneKind>,
    },
    /// "target player" / "target opponent". TODO: a controller restriction
    /// ("target opponent" specifically, vs. "target player" meaning any) is
    /// not threaded through here yet — this variant always accepts any
    /// player.
    Player,
    /// CR 115.4's "any target" — creature, player, planeswalker, or battle.
    /// TODO: planeswalkers/battles are DESIGN.md non-goals overall, so the
    /// `CardTypeSet` bits are checked here for classification correctness
    /// only — nothing else in the engine treats them as real permanents
    /// yet.
    AnyTarget,
}

/// A fixed number of targets matching `filter`, chosen once for one
/// instance of the word "target" (CR 601.2c/115.3's dedup applies within
/// this one requirement, not across several). `count` is exact.
///
/// TODO(CR 601.2c/115.6): variable ("up to N") and "N or more" shapes
/// aren't modeled — every real requirement this slice needs is a fixed
/// count.
///
/// TODO: a spell/ability with more than one distinct "target" word (e.g.
/// "target creature and target player") needs more than one
/// `TargetRequirement`, plural — not modeled; this is exactly one
/// requirement per spell/ability.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TargetRequirement {
    pub filter: TargetFilter,
    pub count: u32,
}

/// Same `Primitive`/`Script` split every other filter-shaped type in this
/// codebase already uses (`ConditionSpec`, `AffectedFilter`). `Script` is
/// structurally present but dead code this slice — nothing constructs it,
/// mirroring `ContinuousEffect::applies_to`'s own `Script` stub. A real
/// script-backed target-legality predicate needs its own "Lua asks, Rust
/// decides" convention, not designed yet (same shape of gap as backlog item
/// 3a's continuous-effect `Script` arm).
#[derive(Clone, Copy, Debug)]
pub enum TargetSpec {
    Primitive(TargetRequirement),
    Script(ScriptHandle),
}
