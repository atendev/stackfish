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
/// "Another target" (must differ from the spell/ability's own source, not
/// just dedup within this one requirement per CR 115.3) is real now — see
/// `TargetRequirement::exclude_source`.
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
    ///
    /// `excluded_card_types`/`excluded_subtypes`/`excluded_supertypes` (CR
    /// 601.2c/115/205.4a-b's negated form — "target nonland permanent,"
    /// "target creature that isn't a Wall," "target nonlegendary creature")
    /// reject a target whose card types intersect `excluded_card_types`,
    /// whose subtypes include any of `excluded_subtypes`, or whose
    /// supertypes intersect `excluded_supertypes` — the negation of the
    /// positive `card_types`/`subtypes`/`supertypes` checks above, checked
    /// independently of them (a filter can combine a positive `card_types`
    /// with an `excluded_subtypes`, e.g. "target creature that isn't a
    /// Wall").
    Object {
        card_types: Option<CardTypeSet>,
        excluded_card_types: Option<CardTypeSet>,
        subtypes: Option<Vec<Subtype>>,
        excluded_subtypes: Option<Vec<Subtype>>,
        supertypes: Option<SupertypeSet>,
        excluded_supertypes: Option<SupertypeSet>,
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
    /// doc comment). Candidate is restricted to `Target::Object` — see
    /// `PlayerScript` for the player-candidate twin. No zone default either
    /// (unlike `Object`'s `zone: None` defaulting to Battlefield) — the
    /// script sees full public state and decides zone itself if it cares.
    Script(ScriptHandle),
    /// `Script`'s player-candidate twin (CR 601.2c/602.2b), for a player
    /// legality shape plain `Player` can't express (e.g. "target player
    /// with 10 or less life"). Kept as a separate variant rather than
    /// widening `Script` to accept either target kind — `Script`'s Lua
    /// contract already passes a bare object id number as the first call
    /// argument, so folding players in would mean either breaking that
    /// contract for every existing object-script card or tagging every call
    /// with a kind, neither of which any real card built so far needs.
    ///
    /// `ScriptRuntime::invoke_player_target_legal` is the real body, same
    /// per-candidate shape as `Script`/`invoke_target_legal` with `player`
    /// in place of `object`. No protection check here, same as plain
    /// `Player` above — this engine doesn't model player-side protection.
    PlayerScript(ScriptHandle),
}

/// A number of targets matching `filter`, chosen once for one instance of
/// the word "target" (CR 601.2c/115.3's dedup applies within this one
/// requirement, not across several). `count`'s meaning depends on `up_to`/
/// `at_least` (mutually exclusive - at most one is ever `true`):
/// - both `false` (the common case, e.g. "target creature"): exactly `count`.
/// - `up_to` (CR 601.2c/115.6, e.g. "choose up to three target creatures"):
///   anywhere from 0 to `count`. Choosing zero is a legal, common outcome —
///   CR 601.2c doesn't require a caster to use the full allowance.
/// - `at_least` (CR 601.2c/115.6, e.g. "target two or more creatures" or
///   "any number of target creatures", the latter being `count: 0`): `count`
///   or more, with no upper bound of its own — capped only by how many
///   legal candidates the caster actually submits, same as every other
///   shape (`commands::validate_targets` never enumerates "all possible
///   legal targets" itself).
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
    /// CR 601.2c/115.6's "N or more" shape - see this struct's own doc
    /// comment for the full three-way `count`/`up_to`/`at_least` semantics.
    /// Checked in `commands::validate_targets`.
    pub at_least: bool,
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
/// own doc comments); `Some(Primitive(_))` is the overwhelmingly common real
/// requirement this engine builds (a spell/ability with exactly one distinct
/// "target" word). No top-level `Script` variant here (there used to be one,
/// never constructed in 5+ slices) — a script-backed legality predicate now
/// lives at `TargetFilter::Script`, nested inside `TargetRequirement`, so it
/// still gets a real `count`/`up_to` instead of an ungoverned bare handle.
#[derive(Clone, Debug)]
pub enum TargetSpec {
    Primitive(TargetRequirement),
    /// CR 601.2c: a spell/ability that uses the word "target" in more than
    /// one place (e.g. "Destroy target nonblack creature and target land" —
    /// CR 608.2b's own worked example). Each entry is one distinct target
    /// word, in order — CR 601.2c/115.3's same-target dedup applies *within*
    /// each entry independently, never across entries (the rule's own
    /// example: "the same object or player can be chosen once for each
    /// instance of the word 'target'"). `commands::validate_targets`/the CR
    /// 608.2b resolution recheck split a flat submitted-targets list into
    /// one chunk per entry (`commands::split_targets_by_requirement`) —
    /// every entry with an exact count (`up_to`/`at_least` both `false`)
    /// claims exactly `count`; with at most one variable entry
    /// (`up_to`/`at_least`), that one entry's own count is derived
    /// positionally, "whatever's left after every exact entry claims its
    /// share" (since nothing in this engine's flat `targets: Vec<Target>`
    /// announces a variable word's count separately). Two or more variable
    /// entries (`phase5/multi-word-variable-count-filter-assignment`) fall
    /// back to filter-based assignment instead (`assign_targets_by_filter`)
    /// — each submitted target is assigned to a word whose own `TargetFilter`
    /// it satisfies, accepting any assignment meeting every word's own count
    /// bound rather than requiring a *unique* one (see that function's own
    /// doc comment for why "some valid partition exists" is the right bar
    /// given every multi-word `AbilityEffect` here applies uniformly across
    /// the flattened result regardless of word origin). Real resolution of
    /// a *uniform-verb* multi-word spell (Plague Spores'
    /// "destroy" applying across both words) needs no further engine
    /// support: downstream effect arms (`AbilityEffect::Destroy` etc.)
    /// already just iterate the concatenated per-word legal-target list
    /// unchanged. A *role-differentiated* multi-word spell (different words
    /// feeding different verbs) has no matching `AbilityEffect` primitive
    /// either way and would need a `Script` effect regardless — not a new
    /// gap this variant introduces.
    Multiple(Vec<TargetRequirement>),
}
