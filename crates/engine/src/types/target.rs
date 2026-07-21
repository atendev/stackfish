use crate::types::card::{CardTypeSet, Subtype, SupertypeSet};
use crate::types::cost::FilterController;
use crate::types::ids::ScriptHandle;
use crate::types::zone::ZoneKind;

/// What CR 601.2c/602.2b/115 restrict a chosen target to. Mirrors the
/// `EffectFilter` (effect.rs) / `FilterSpec` (cost.rs) "controller +
/// card_types" shape rather than inventing a third one, plus a `zone` (CR
/// 115.2's battlefield-by-default rule) and the two non-object target
/// shapes (CR 115.4's "any target", plain "target player").
///
/// TODO: no negative filters ("target creature you don't control that
/// isn't a Wall") — only what a first real spell needs. "Another target"
/// (must differ from the spell/ability's own source, not just dedup within
/// this one requirement per CR 115.3) is real now — see `TargetRequirement::
/// exclude_source`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TargetFilter {
    /// "target creature" / "target permanent you control" / etc. `zone:
    /// None` defaults to `Battlefield` (CR 115.2: only permanents are legal
    /// targets unless a spell/ability says otherwise) — `Some(zone)` is for
    /// the rarer case of targeting a card in another zone (e.g. a spell on
    /// the stack, CR 115.2b), not exercised by any card built this slice.
    ///
    /// `subtypes` (CR 205.4b, "target Human," "Equip Dragons") matches if
    /// the target has *any* subtype in the list (an "or," not an "and") —
    /// same convention `cost.rs`'s `FilterSpec::subtypes` already
    /// documents for the identical reason (no real card asks for two
    /// subtypes at once).
    ///
    /// `supertypes` (CR 205.4a, "target legendary creature") is an
    /// intersection test against `SupertypeSet` (a bitflag, unlike
    /// `subtypes`' bare-string `Vec`) — same shape and "or across the set's
    /// bits" semantics `cost.rs`'s `FilterSpec::supertypes` already has, now
    /// mirrored here.
    Object {
        card_types: Option<CardTypeSet>,
        subtypes: Option<Vec<Subtype>>,
        supertypes: Option<SupertypeSet>,
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
    /// A custom Lua legality predicate (CR 601.2c/602.2b), for a filter
    /// shape `Object`'s flat option-bag can't express (e.g. "target
    /// creature with power 4 or greater"). Nested here rather than living
    /// at the old `TargetSpec::Script` top level (removed by this variant)
    /// so `count`/`up_to` still apply normally — a bare `ScriptHandle` with
    /// no count field couldn't express "exactly one" vs "up to three",
    /// which is why that top-level variant was never actually usable in 5+
    /// slices since `targeting-basics` first stubbed it.
    ///
    /// `ScriptRuntime::invoke_target_legal` is the real body, called once
    /// per already-chosen candidate from `commands::target_is_legal` — same
    /// per-candidate-predicate shape `AffectedFilter::Script`/
    /// `invoke_continuous_affects` already established, not a coroutine
    /// suspension (that's `Game.SelectTargets`'s unrelated job, see its own
    /// doc comment). Candidate is restricted to `Target::Object` — a
    /// player-target script predicate stays unmodeled (rare enough on real
    /// cards to skip for now). No zone default either (unlike `Object`'s
    /// `zone: None` defaulting to Battlefield) — the script sees full
    /// public state and decides zone itself if it cares.
    Script(ScriptHandle),
}

/// A number of targets matching `filter`, chosen once for one instance of
/// the word "target" (CR 601.2c/115.3's dedup applies within this one
/// requirement, not across several). `count` is the maximum — exactly
/// `count` if `up_to` is `false` (the common case, e.g. "target creature"),
/// or anywhere from 0 to `count` if `up_to` is `true` (CR 601.2c/115.6,
/// e.g. "choose up to three target creatures"). Choosing zero under `up_to`
/// is a legal, common outcome — CR 601.2c doesn't require a caster to use
/// the full allowance.
///
/// TODO(CR 601.2c/115.6): "N or more" isn't modeled — rare enough on real
/// cards that it wasn't worth bundling into this slice alongside "up to N".
///
/// TODO: a spell/ability with more than one distinct "target" word (e.g.
/// "target creature and target player") needs more than one
/// `TargetRequirement`, plural — not modeled; this is exactly one
/// requirement per spell/ability.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TargetRequirement {
    pub filter: TargetFilter,
    pub count: u32,
    pub up_to: bool,
    /// CR 115.4/"another target" (e.g. Olivia Voldaren's "deals 1 damage to
    /// another target creature"): when `true`, the spell/ability's own
    /// source is never a legal candidate for this requirement, regardless
    /// of whether it otherwise matches `filter`. Distinct from CR
    /// 601.2c/115.3's same-instance dedup (enforced unconditionally in
    /// `commands::validate_targets`, independent of this field) — that
    /// rejects choosing the *same target twice*; this excludes one
    /// specific object (the source) from ever being chosen at all, even
    /// once. `false` (the default/common case) for every requirement that
    /// doesn't say "another." Checked in `commands::target_is_legal`.
    pub exclude_source: bool,
}

/// `None` means "takes no targets" (`CardDefinition`/`RegisteredAbility`'s
/// own doc comments); `Some(Primitive(_))` is every real requirement this
/// engine builds. No top-level `Script` variant here (there used to be one,
/// never constructed in 5+ slices) — a script-backed legality predicate now
/// lives at `TargetFilter::Script`, nested inside `TargetRequirement`, so it
/// still gets a real `count`/`up_to` instead of an ungoverned bare handle.
#[derive(Clone, Debug)]
pub enum TargetSpec {
    Primitive(TargetRequirement),
}
