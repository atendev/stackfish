use crate::{
    CardTypeSet, Subtype, SupertypeSet, ObjectId, PlayerId, SequenceNo, ColorSet, ManaCost,
    GameObject, GameState, GameEventKind, ScriptHandle, Target, ZoneKind,
};
use crate::types::ability::{CopyExceptions, InterveningIfSpec};
use crate::types::card::{CardDefinition, KeywordSet, PTValue};
use crate::types::cost::FilterController;
use crate::types::counter::CounterKind;
use crate::script::ScriptRuntime;

/// CR 612.6's "full text" payload — just the fields `EffectiveCharacteristics`
/// actually derives from a `CardDefinition` (name, mana cost, color, type
/// line, keywords, power/toughness), not the whole struct: `CardDefinition`
/// also carries ability/cost/target-requirement fields (`alternative_costs`,
/// `target_requirement`, `effect`, `modes`, `mode_choice`, `script_ref`) that
/// have no "text" meaning here and — critically — can't safely transfer via
/// this mechanism at all. `layers::effective_characteristics` is a pure,
/// read-only query with no `&mut GameState`; it can apply this payload's
/// fields (plain data, no side effects) but can never re-run a script or
/// register a new ability the way `AbilityEffect::BecomeCopy`/
/// `ExchangeTextBoxes` do for their own one-time, non-reverting mutations.
/// So CR 612.6's own "full text" is deliberately narrowed here to the
/// *characteristic* half — a Volrath's Shapeshifter-style effect looks and
/// fights like the copied creature (right name/cost/color/type line/P&T),
/// but doesn't gain its activated/triggered abilities. Named, not hidden:
/// this is a structural consequence of modeling CR 612.6 as a live,
/// reversible continuous effect (reusing `EffectDuration`'s existing
/// expiry machinery for free) rather than a permanent one-time mutation.
///
/// Doesn't derive `PartialEq`/`Eq` on its own component fields (`ManaCost`/
/// `PTValue` don't derive them either) — `EffectPrimitive`'s own derive just
/// needs *a* `PartialEq` impl to exist, so a minimal by-name one is
/// hand-written below rather than pulled in through every field.
#[derive(Clone, Debug)]
pub struct FullTextOverride {
    pub name: String,
    pub mana_cost: ManaCost,
    pub color_indicator: Option<ColorSet>,
    pub card_types: CardTypeSet,
    pub supertypes: SupertypeSet,
    pub subtypes: Vec<Subtype>,
    pub keywords: KeywordSet,
    pub power: Option<PTValue>,
    pub toughness: Option<PTValue>,
}

impl FullTextOverride {
    /// CR 105.2a/202.2: same `color_indicator`-or-mana-cost split
    /// `CardDefinition::colors()` already implements.
    pub fn colors(&self) -> ColorSet {
        match self.color_indicator {
            Some(indicator) => indicator,
            None => self.mana_cost.colors(),
        }
    }
}

impl From<&CardDefinition> for FullTextOverride {
    fn from(def: &CardDefinition) -> Self {
        FullTextOverride {
            name: def.name.clone(),
            mana_cost: def.mana_cost.clone(),
            color_indicator: def.color_indicator,
            card_types: def.card_types,
            supertypes: def.supertypes,
            subtypes: def.subtypes.clone(),
            keywords: def.keywords,
            power: def.power.clone(),
            toughness: def.toughness.clone(),
        }
    }
}

impl PartialEq for FullTextOverride {
    /// No real caller compares two `FullTextOverride` payloads for
    /// structural equality — this exists only so `EffectPrimitive`'s own
    /// derive has *something* to call. Name is the one field that reads as
    /// "this override's own identity" for any future debugging/test use.
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Eq for FullTextOverride {}

/// CR 613.1 layers, with sublayers for layer 7 (power/toughness).
/// `TypeChanging` (4), `ColorChanging` (5), `AbilityAdding` (6),
/// `PowerToughnessSetting` (7b), `PowerToughnessModifying` (7c), and
/// `PowerToughnessSwitching` (7e) have a real query/apply path — see
/// `layers::effective_characteristics`. The rest are named here (so the CR
/// 613.1 layer list is complete and in the right relative order) but nothing
/// in `state.continuous_effects` can be built against them yet and the query
/// doesn't look for them:
///
/// Layer 1 (CopyEffects, CR 707/613.1a): unlike every layer below, this has
/// no `ContinuousEffect`/`EffectPrimitive` case and `layers::
/// applicable_effects_in_layer` never queries it - CR 707.2b/707.2c require
/// a copy's granted values to freeze at the moment it's applied, which a
/// live re-derived-every-call query (this engine's shape for layers 2/4-7)
/// would violate outright. Real support is `GameObject.copiable_override`
/// instead: a frozen snapshot written once by whatever produced the copy
/// (`AbilityEffect::BecomeCopy` for an existing permanent re-copying
/// something, `TokenSpec::CopyOf` for a freshly created copy-token), read by
/// `layers::effective_characteristics` ahead of `definition`. Two real
/// gaps, not silently narrowed: no CR 707.9 exception/modification support
/// (always a bare copy of the target's current values) and no ETB-
/// replacement "enters the battlefield as a copy" path (Clone itself) -
/// only the two entry points above exist. See docs/implementation.md's
/// `phase5/layer-engine-copy` slice notes.
///
/// Layer 2 (ControlChangingEffects, CR 108.4/613.1): `layers::
/// effective_controller` has a real query path now (`EffectPrimitive::
/// GainControl`, same "later timestamp overrides outright" semantics as
/// `SetPT`/layer 7b — no CR 613.8 dependency case needed, since one
/// `GainControl`'s result never depends on whether another has already
/// applied). Deliberately not wired into any real consumer yet — every
/// gameplay site that reads `GameObject.controller` directly (combat,
/// targeting, cost filters, `has_summoning_sickness`, `EffectFilter::
/// applies_to`/`FilterSpec::matches`) still reads the raw, never-mutated
/// field, so nothing in live gameplay can observe a `GainControl` effect's
/// result yet — see docs/implementation.md's `phase5/layer-engine-control`
/// slice notes for why that migration is its own follow-up, not bundled in.
///
/// Layer 3 (TextChanging, CR 613.1c/612): narrowed to name-setting only (CR
/// 612.8, "some cards create a continuous effect that sets the name of an
/// object") — see `EffectPrimitive::SetName`/`layers::effective_characteristics`.
/// The general case (CR 612.2's "changes only correctly-used words" word/
/// symbol substitution, 612.6/612.7/612.10's single-card rules, 612.9's name
/// stickers) stays unmodeled; `CardDefinition.oracle_text` is still a plain
/// immutable `String`, never parsed. "Loses all abilities" (CR 613.1f's
/// ability-*removing* half, mechanically layer 6 despite being flagged here
/// alongside layer 3's other text-removal shapes) is closed for the
/// `KeywordSet` portion — see `EffectPrimitive::RemoveAllAbilities`,
/// `phase5/lose-all-abilities` (docs/implementation.md). A permanent's own
/// `RegisteredAbility` entries (custom triggered/activated abilities) are a
/// separate, still-open gap — see that primitive's own doc comment.
///
/// Layers 4/5 (CR 613.1) don't have CR-defined sublayers the way layer 7
/// does (CR 613.3 only lettered-sublayers layers 3, 4, 6, and 7 — and layer
/// 4's own lettered sublayers are about control-changing-vs-not, not
/// modeled here) — every layer-4/5 primitive below (`AddCardType`/
/// `SetCardTypes`/`AddColor`/`SetColors`/`AddSupertype`/`AddSubtype`) folds
/// into one sequential per-layer pass in `layers::effective_characteristics`,
/// applied in timestamp order — CR 613.1d groups card-type/subtype/
/// supertype changes into layer 4 together (not separate layers), and CR
/// 205.4b's "supertypes are independent of card type/subtype" is modeled as
/// three separate fields folded by that same one pass, not three layers.
///
/// Dependency system (CR 613.6-613.8): `layers::order_by_dependency`
/// reorders a layer's effects around real CR 613.8a dependency (today, only
/// `EffectPrimitive::DoublePT` vs. `ModifyPT`/`DoublePT` in layer 7c actually
/// produces one — see `DoublePT`'s own doc comment for why no other
/// primitive can), falling back to plain timestamp order for everything
/// else and for dependency loops (CR 613.8b's last sentence). `depends_on`
/// resolves both sides through `layers::resolve_primitive` before comparing
/// (`phase5/script-effect-dependency`), so an `EffectPayload::Script` effect
/// that decodes to a `DoublePT`/`ModifyPT` shape participates in that same
/// rule exactly like a `Primitive` effect already did, in either direction.
/// One real gap still open, not silently narrowed:
/// - This only closes the "resolves to a known dependency-participating
///   shape" case. A script *dynamically deciding* whether it depends on
///   another specific effect (as opposed to always resolving to a fixed
///   shape like `DoublePT`) would need the script bridge to expose
///   partial/in-progress layer state mid-computation, which `state_to_lua`'s
///   snapshot deliberately doesn't do yet — no real card needs this, since
///   CR 613.8a dependency is a structural property of what an effect *does*
///   (reads a running value vs. sets a fixed one), not something a card's
///   own text ever asks a script to decide case-by-case.
/// - Cross-layer/cross-sublayer dependency isn't modeled — CR 613.8 is
///   explicitly same-layer/sublayer only, and cross-layer sequencing is
///   already handled by the fixed layer order in `effective_characteristics`,
///   not by this algorithm.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    CopyEffects,            // 1
    ControlChangingEffects, // 2
    TextChanging,           // 3
    TypeChanging,           // 4
    ColorChanging,          // 5
    AbilityAdding,          // 6
    PowerToughnessCDA,      // 7a — characteristic-defining abilities
    PowerToughnessSetting,  // 7b
    PowerToughnessModifying,// 7c
    PowerToughnessCounters, // 7d
    PowerToughnessSwitching,// 7e
}

/// The recognized shapes a `ContinuousEffect`'s actual modification can take
/// — mirrors `ConditionSpec`/`AbilityEffect`'s "Rust-native primitive, zero
/// further evaluation" vs. "falls back to the script bridge" split. See
/// `Layer`'s own doc comment for the layers with no primitive yet.
///
/// `SetCardTypes`/`SetColors` (the full "loses all other types/colors"
/// overwrite, mirroring `SetPT`'s override semantics for layer 7b) and
/// `AddSupertype`/`AddSubtype` are real now too (`phase5/layer-overwrite-
/// types-colors`) — see each variant's own doc comment.
/// CR 702.14a: a landwalk grant's "[type]" when it's more than a bare land
/// type — "the card type land plus any combination of land types, card
/// types, and/or supertypes." All specified fields must hold simultaneously
/// against a candidate land (a card names one specific combination, not a
/// menu of alternatives) — `None`/empty fields impose no constraint on that
/// axis. `subtypes` is a `Vec` (not `Option`) for the plain "no subtype
/// required" case (e.g. "nonbasic landwalk") to just be `Vec::new()`, same
/// "empty means unconstrained" reading `AddLandwalkSubtype`'s all-subtypes
/// list would use if it had one. See `commands::DeclareBlockers`'s
/// enforcement for the actual per-field match.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct LandwalkQuality {
    /// Land subtype(s) that must ALL be present (e.g. `vec![Swamp]` for
    /// "snow swampwalk"). Empty = no subtype required.
    pub subtypes: Vec<Subtype>,
    /// If `Some`, the land's own card types must intersect this (e.g.
    /// `ARTIFACT` for "artifact landwalk").
    pub card_types: Option<CardTypeSet>,
    /// If `Some`, the land's supertypes must contain all of these (e.g.
    /// `SNOW` for "snow swampwalk").
    pub supertypes: Option<SupertypeSet>,
    /// If `Some`, the land's supertypes must contain none of these (e.g.
    /// `BASIC` for "nonbasic landwalk").
    pub excluded_supertypes: Option<SupertypeSet>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EffectPrimitive {
    /// Layer 3 (CR 612.8): sets the object's name outright — it "loses any
    /// names it had and has only the specified name," so this overrides
    /// whatever came before this layer entirely, same override-not-additive
    /// shape as `GainControl`/`SetPT` (a later-timestamp `SetName` replaces
    /// an earlier one's result, it doesn't combine with it). Never depends
    /// on another effect in the same layer for the same reason `GainControl`
    /// doesn't (CR 613.8a) — a flat override's result only depends on
    /// timestamp, which plain CR 613.7 ordering already handles. The only
    /// non-`Copy` `EffectPrimitive` variant — see `layers::resolve_primitive`
    /// for why the type dropped `Copy` to accommodate it.
    SetName(String),
    /// Layer 3 (CR 612.6, Volrath's Shapeshifter): "full text of another
    /// object" — overrides name, mana cost, color, type line (card types/
    /// supertypes/subtypes), keywords, and power/toughness all at once,
    /// same override-not-additive shape `SetName`/`SetPT` already establish
    /// (a later-timestamp `SetFullText` replaces an earlier one's result
    /// entirely; a later-timestamp plain `SetName` can still independently
    /// override just the name on top of it, since they touch different
    /// fields). Boxed to avoid bloating every other `EffectPrimitive`
    /// variant with this one's much larger payload. See
    /// `FullTextOverride`'s own doc comment for why this deliberately
    /// doesn't cover the object's actual registered abilities.
    SetFullText(Box<FullTextOverride>),
    /// Layer 2 (CR 108.4/613.1): the object's controller becomes this
    /// player — overrides whatever came before this layer entirely, exactly
    /// like `SetPT`'s override semantics for layer 7b (a later-timestamp
    /// `GainControl` replaces an earlier one's result, it doesn't combine
    /// with it — only one player can control an object at a time). Unlike
    /// `DoublePT`, this never depends on another effect in the same layer
    /// (CR 613.8a) — a flat override's result doesn't change based on
    /// whether some other `GainControl` already applied, only on which one
    /// has the latest timestamp, which plain CR 613.7 ordering already
    /// handles. See `layers::effective_controller`.
    GainControl(PlayerId),
    /// Layer 7b (CR 613.3): sets power/toughness to an exact value,
    /// overriding whatever came before this layer (not additive — a later
    /// `SetPT` at a later timestamp replaces an earlier one's result
    /// entirely, it doesn't stack on top of it).
    SetPT { power: i32, toughness: i32 },
    /// Layer 7c (CR 613.3): adds (or, for a negative value, subtracts) from
    /// power/toughness. Additive across every applicable effect in this
    /// layer, applied in timestamp order.
    ModifyPT { power: i32, toughness: i32 },
    /// Layer 7c (CR 613.3): doubles the object's current power and/or
    /// toughness (e.g. Berserk's "target creature's power is doubled") —
    /// whichever field is `true` gets doubled, the other is untouched. Not a
    /// fixed delta like `ModifyPT` — its result is defined by whatever value
    /// is already there, which is exactly CR 613.8a's dependency condition:
    /// this effect always depends on any `ModifyPT` in the same sublayer
    /// that touches the same field (applying the `ModifyPT` first changes
    /// what number this effect doubles). See `layers::order_by_dependency`.
    DoublePT { power: bool, toughness: bool },
    /// Layer 6 (CR 613.1): grants the listed keyword(s) in addition to
    /// whatever the object already has. Union, never removes a keyword on
    /// its own — see `RemoveAllAbilities` below for the strip-everything
    /// shape, or `RemoveKeyword` below for a scoped removal of just these.
    AddKeyword(KeywordSet),
    /// Layer 6 (CR 613.1f): removes the listed keyword(s), and only these.
    /// Unlike `RemoveAllAbilities`, this never touches protection or any
    /// other layer-6 field, and never clears keywords outside the listed
    /// set — the narrower "loses these specific keywords" shape
    /// `RemoveAllAbilities`'s own doc comment names as deliberately unbuilt
    /// there. Folds into `layers.rs`'s layer-6 pass at this effect's own CR
    /// 613.7 timestamp position, same as `AddKeyword` — a later-timestamp
    /// `AddKeyword` re-adds whatever an earlier `RemoveKeyword` removed (and
    /// vice versa), the identical "no special CR 613.8a dependency needed"
    /// reasoning `RemoveAllAbilities` already documents for the same
    /// interaction.
    ///
    /// NOT the right primitive for "can attack as though it didn't have
    /// defender" (Assault Formation) — that keeps the keyword itself (a
    /// card checking "has defender" still sees it) and only voids the
    /// restriction it imposes. An earlier slice modeled Assault Formation
    /// as `RemoveKeyword(DEFENDER)` anyway; that's a real ruling mismatch,
    /// not a deliberate approximation — see `IgnoreKeyword` below for the
    /// correct primitive, added once a real "ignore, don't strip" mechanic
    /// was actually requested.
    RemoveKeyword(KeywordSet),
    /// Layer 6 (CR 613.1f, "as though it didn't have [keyword]" text): the
    /// listed keyword(s) stay present in `EffectiveCharacteristics::keywords`
    /// — anything checking "does this have [keyword]" still sees it — but
    /// whatever restriction that keyword imposes is voided for the listed
    /// bit(s). E.g. Assault Formation's "Walls you control can attack as
    /// though they didn't have defender" is `IgnoreKeyword(DEFENDER)`:
    /// unlike `RemoveKeyword(DEFENDER)`, the Wall still *has* defender (CR
    /// 302.6b's own separate "summoning-sick creature can't attack" rule
    /// still applies to it, and any other card caring "target creature with
    /// defender" still targets it) — only CR 702.3b's own can't-attack
    /// restriction is lifted. Additive union, same fold shape as `AddKeyword`
    /// (accumulates into its own `EffectiveCharacteristics::
    /// ignored_keywords` field, separate from `keywords` itself),
    /// cleared by `RemoveAllAbilities` alongside every other layer-6 field
    /// (this exemption is itself an ability, same reasoning `RemoveAllAbilities`'s
    /// own doc comment gives for clearing protection/landwalk too).
    ///
    /// Only `KeywordSet::DEFENDER` has a real enforcement call site today
    /// (`commands::combat::declare`'s attack-declaration check) — `KeywordSet`
    /// only has the one true self-restriction bit ("can't do X") right now;
    /// every other bit (Flying/Menace/Fear/Shadow/Horsemanship/...) restricts
    /// a *blocker's* legality against this object, not this object's own
    /// actions, a different shape not covered by this primitive. Payload is
    /// a full `KeywordSet` (not a single `Keyword`) so a future restriction
    /// bit can reuse this primitive without a new one, same "shape it for
    /// the general case even though only one bit is wired up yet" posture
    /// `AddKeyword`/`RemoveKeyword` themselves already take.
    IgnoreKeyword(KeywordSet),
    /// Layer 6 (CR 613.1/702.16e): grants protection from the listed
    /// color(s) in addition to whatever the object already has protection
    /// from. Union, same shape as `AddKeyword` above — see `EffectiveCharacteristics::
    /// protection_colors`'s own doc comment for why this is a separate
    /// primitive/field rather than folded into `AddKeyword`/`KeywordSet`.
    AddProtectionColor(ColorSet),
    /// Layer 6 (CR 613.1/702.16, card-type quality): grants protection from
    /// the listed card type(s) in addition to whatever the object already
    /// has protection from — e.g. "protection from artifacts." Union, same
    /// shape as `AddProtectionColor` above and the same reasoning for why
    /// it's a separate field (`EffectiveCharacteristics::
    /// protection_card_types`) rather than reusing `AddCardType`/
    /// `CardTypeSet` (protection and "has this type" are unrelated
    /// characteristics that happen to share a `CardTypeSet` vocabulary).
    AddProtectionCardType(CardTypeSet),
    /// Layer 6 (CR 613.1/702.16, subtype quality): grants protection from
    /// the named creature/land/etc. subtype in addition to whatever the
    /// object already has protection from — e.g. "protection from Dragons."
    /// One subtype per primitive (CR 702.16g's "protection from A and from
    /// B" is already modeled as two separate registered static abilities,
    /// same as how `AddProtectionColor`/`AddProtectionCardType` each carry
    /// one Lua-supplied value per call) — see `EffectiveCharacteristics::
    /// protection_subtypes`'s own doc comment for why this can't be a
    /// bitflag `Set` the way the color/card-type qualities are (`Subtype` is
    /// an open-ended string, not a fixed enum).
    AddProtectionSubtype(Subtype),
    /// Layer 6 (CR 613.1/702.16a, name quality): grants protection from the
    /// named card in addition to whatever the object already has protection
    /// from — e.g. "protection from Progenitus." CR 702.16a: "If the
    /// quality happens to be a card name, it is treated as such only if the
    /// protection ability specifies that the quality is a name" — this
    /// primitive *is* that specification (unlike `AddProtectionSubtype`,
    /// there's no ambiguity between "this string is a name" vs. "this
    /// string is a subtype," since each has its own primitive/field). One
    /// name per primitive, same convention as the other quality primitives.
    /// Matched against the source's *effective* name (`EffectiveCharacteristics::
    /// name`, post any CR 612.8 `SetName`), not the printed `CardDefinition.
    /// name` — protection has to see a renamed source the same way every
    /// other characteristic-sensitive check in this engine does.
    AddProtectionName(String),
    /// Layer 6 (CR 613.1/702.16j, "protection from everything"): grants
    /// protection unconditionally, regardless of the source's own
    /// characteristics — "A permanent or player with protection from
    /// everything has protection from each object regardless of that
    /// object's characteristic values." No payload, same bare-value shape
    /// as `RemoveAllAbilities`/`SwitchPT` below — there's no quality to
    /// carry, just an on/off flag (`EffectiveCharacteristics::
    /// protection_everything: bool`), unlike the other four
    /// `AddProtection*` primitives which each carry *which* color/type/
    /// subtype/name. A `true` union (once granted by any applicable effect,
    /// stays granted for the rest of this layer-6 fold, same as every other
    /// additive shape here) rather than a `Vec`/`Set` — redundant
    /// `AddProtectionEverything`s from multiple sources collapse to the
    /// same single bit, which is correct (CR 702.16m: "multiple instances
    /// of protection from the same quality... are redundant").
    AddProtectionEverything,
    /// Layer 6 (CR 613.1/702.16k, player quality): grants protection from a
    /// specific player's own controlled/owned-and-uncontested objects — e.g.
    /// "protection from an opponent." Carries a `FilterController`
    /// (`You`/`Opponent`), the same relative-player vocabulary `:SetAffected
    /// ({controller=...})`/`EffectFilter.controller` already expose to Lua,
    /// rather than a raw `PlayerId` — this engine has no multiplayer
    /// arbitrary-player-targeting yet (backlog item 11), and a card script's
    /// `register_abilities(c)` is never handed a `PlayerId` at registration time
    /// to name one with anyway. Resolved relative to the *protected
    /// object's own* controller at each enforcement site (not the
    /// continuous effect's source — the two coincide for every `self_only`
    /// grant this engine can build, same as `EffectFilter`'s own "You"
    /// resolution). `FilterController::Any` is rejected at construction
    /// time (`script.rs`'s `Game.Effects.AddProtectionPlayer`) — "protection
    /// from any player" isn't a CR 702.16k quality, that's what
    /// `AddProtectionEverything` (702.16j) already covers.
    AddProtectionPlayer(FilterController),
    /// Layer 6 (CR 613.1/702.14, landwalk): grants unblockability while the
    /// defending player controls a permanent with the named subtype (e.g.
    /// "Island" for islandwalk), in addition to whatever the object already
    /// has. One subtype per primitive, same "CR 702.16g composability via
    /// multiple registered abilities" convention `AddProtectionSubtype`
    /// documents — a card with two landwalk keywords registers two static
    /// abilities. Same "no printed field on `CardDefinition`, always
    /// effect-granted" shape as every `AddProtection*` primitive: a card
    /// with printed Islandwalk self-registers an always-on static ability
    /// via its script, same as printed protection does. Only the plain
    /// single-land-type form — CR 702.14a's card-type/supertype-combination
    /// forms ("artifact landwalk," "nonbasic landwalk," "snow swampwalk")
    /// are `AddLandwalkQuality` below instead, a separate primitive rather
    /// than widening this one's payload (keeps every existing plain-subtype
    /// call site, in code and Lua, untouched). See
    /// `EffectiveCharacteristics::landwalk_subtypes` and `commands::
    /// DeclareBlockers`'s enforcement (checks the *defending player's*
    /// battlefield, not the blocker's own characteristics — the one respect
    /// this isn't shaped like protection).
    AddLandwalkSubtype(Subtype),
    /// Layer 6 (CR 613.1/702.14a, landwalk's card-type/supertype-combination
    /// forms): same grant as `AddLandwalkSubtype`, but the specified `[type]`
    /// is "the card type land plus any combination of land types, card
    /// types, and/or supertypes" instead of a bare land type — "artifact
    /// landwalk" (`card_types: Some(ARTIFACT)`), "nonbasic landwalk"
    /// (`excluded_supertypes: Some(BASIC)`), "snow swampwalk"
    /// (`supertypes: Some(SNOW), subtypes: vec![Swamp]`). See
    /// `LandwalkQuality`'s own doc comment for how its fields combine, and
    /// `EffectiveCharacteristics::landwalk_qualities`/`commands::
    /// DeclareBlockers` for enforcement (a separate `Vec` folded and checked
    /// alongside `landwalk_subtypes`, not merged into it — the two
    /// primitives' payloads aren't the same shape).
    AddLandwalkQuality(LandwalkQuality),
    /// Layer 6 (CR 613.1/702.22b/c, "bands with other [quality]"): grants
    /// CR 702.22c's second band-formation path for the named quality (e.g.
    /// "Elf" for "bands with other Elves"), in addition to whatever the
    /// object already has. Same shape as `AddLandwalkSubtype` in every
    /// respect (one subtype per primitive, no printed `CardDefinition`
    /// field — always effect-granted via a self-registered static ability)
    /// except what it's checked against: `commands::PlayerActionKind::
    /// DeclareBands` reads `EffectiveCharacteristics::
    /// bands_with_other_subtypes` to accept a band whose members share this
    /// quality, instead of `DeclareBlockers` reading the defending player's
    /// battlefield the way landwalk does. Only single-subtype ("quality")
    /// matching is modeled here — the supertype form ("bands with other
    /// Legends," i.e. Legendary) is the separate `AddBandsWithOtherSupertype`
    /// primitive below, same subtype/quality-combination split
    /// `AddLandwalkSubtype`/`AddLandwalkQuality` already take (a bare
    /// `Subtype` vs a whole `SupertypeSet` aren't the same payload shape).
    AddBandsWithOther(Subtype),
    /// Layer 6 (CR 613.1/702.22c/m, "bands with other [quality]," supertype
    /// form): the historic Legends-block wording ("Bands with other
    /// Legends," i.e. any Legendary creature) — same grant as
    /// `AddBandsWithOther`, but the quality is a supertype
    /// (`crate::types::card::SupertypeSet`) rather than a `Subtype`.
    /// `commands::PlayerActionKind::DeclareBands` and `commands::combat::
    /// damage`'s chooser-flip check both extend their existing subtype-quality
    /// check with this one, same "any one match is enough" posture.
    AddBandsWithOtherSupertype(crate::types::card::SupertypeSet),
    /// Layer 6 (CR 613.1f, ability-removing effects — "~ loses all
    /// abilities"): resets the running `keywords`, `ignored_keywords`,
    /// `protection_colors`, `protection_card_types`, `protection_subtypes`,
    /// `protection_names`, `protection_everything`, `protection_players`,
    /// `landwalk_subtypes`, *and* `bands_with_other_subtypes` accumulators to
    /// empty/false at this effect's position in `effective_characteristics`'s
    /// layer-6 fold —
    /// protection is an ability too, regardless of which
    /// quality it names (CR 613.1f names "ability-adding... and
    /// ability-removing effects" together, no carve-out for protection
    /// specifically). Not a flat override the way `SetName`/`SetPT`/
    /// `GainControl` are — a later-timestamp `AddKeyword`/`AddProtectionColor`/
    /// `AddProtectionCardType`/`AddProtectionSubtype`/`AddProtectionName`/
    /// `AddProtectionEverything`/`AddProtectionPlayer` still
    /// applies on top afterward (CR 613.7's plain timestamp fold already
    /// produces this for free, no special dependency case needed: a
    /// strip-all and an add-keyword/add-protection never depend on each
    /// other under CR 613.8a, same reasoning `SetPT`/`GainControl` already
    /// document for why *they* don't need one). No payload — it always
    /// clears everything accumulated so far; see `RemoveKeyword` above for
    /// the narrower "loses these specific keywords (and nothing else)"
    /// shape.
    ///
    /// Deliberately narrow: only strips these layer-engine fields,
    /// not the full picture of "abilities." A permanent's own custom
    /// triggered/activated `RegisteredAbility` entries are never filtered
    /// by continuous effects at all in this engine (`collect_triggers`/
    /// activation-legality checks read `state.abilities` directly, no
    /// layer-6 consultation) — making this primitive also silence those
    /// would mean threading a layer-6 lookup through that entire call
    /// graph, the same shape as the already-named layer-2-control-consumers
    /// gap. Named here rather than silently narrowed — see
    /// `phase5/lose-all-abilities` (docs/implementation.md) for the scoping
    /// finding.
    RemoveAllAbilities,
    /// Layer 4 (CR 613.1, general "gains types" text): grants the listed
    /// card type(s) in addition to whatever the object already has — e.g.
    /// "target land becomes a 3/3 Elemental creature in addition to its
    /// other types" is this primitive (for the type-changing half) plus a
    /// `SetPT` (layer 7b) for the 3/3. Union, never removes a type.
    AddCardType(CardTypeSet),
    /// Layer 4 (CR 613.1a, "becomes" text): sets the object's card types
    /// outright, overriding whatever came before this layer — same
    /// override-not-additive shape `SetPT`/`SetName`/`GainControl` already
    /// establish (a later-timestamp `SetCardTypes` replaces an earlier
    /// one's result entirely; it doesn't stack with an earlier `AddCardType`
    /// in the same layer, since both fold into the same running `card_types`
    /// value in timestamp order — see `layers::effective_characteristics`'s
    /// own layer-4 fold). CR 205.1a's own carve-out (an object with the
    /// Instant or Sorcery type retains that type through a card-type
    /// overwrite) isn't modeled — no real card in this corpus needs it,
    /// named rather than silently narrowed.
    SetCardTypes(CardTypeSet),
    /// Layer 5 (CR 613.1, general "gains colors" text): grants the listed
    /// color(s) in addition to whatever the object already has — e.g.
    /// "enchanted creature is also red". Union, never removes a color.
    AddColor(ColorSet),
    /// Layer 5 (CR 613.1a, "becomes" text): sets the object's colors
    /// outright, same override-not-additive shape `SetCardTypes` above
    /// establishes for layer 4.
    SetColors(ColorSet),
    /// Layer 4 (CR 613.1d/205.4b: supertypes are an independent category
    /// from card types/subtypes, but still evaluated in this same layer):
    /// grants the listed supertype(s) in addition to whatever the object
    /// already has — e.g. "target permanent becomes legendary." Union,
    /// never removes a supertype (CR 205.4b: "when an object gains ... a
    /// supertype, it retains any other supertypes it had"). Additive-only,
    /// the natural extension of `AddCardType`'s own shape — no
    /// `SetSupertypes` full-overwrite variant, since no real card needs one.
    AddSupertype(SupertypeSet),
    /// Layer 4 (CR 613.1d, same layer as card types/supertypes): grants the
    /// listed subtype(s) in addition to whatever the object already has —
    /// e.g. "target creature becomes an Elemental in addition to its other
    /// types" (CR 205.1b). Appends only subtypes not already present, never
    /// removes one. CR 205.3d's own "can't gain a subtype that doesn't
    /// correspond to one of the object's [card] types" isn't validated here
    /// — same trust-the-script posture every other script-authored
    /// primitive in this engine already takes.
    AddSubtype(Vec<Subtype>),
    /// Layer 7e (CR 613.4d): swaps the object's current power and toughness
    /// — takes whatever value 7b/7c already produced for power and applies
    /// it to toughness, and vice versa. A toggle, not a value, so no fields.
    /// Two `SwitchPT` effects on the same object cancel out exactly (CR
    /// 613.4d's own third example) — a plain swap is its own inverse, so
    /// applying it twice in `layers::effective_characteristics` naturally
    /// reproduces that without any special-casing.
    SwitchPT,
}

/// A `ContinuousEffect`'s actual effect — either a recognized
/// `EffectPrimitive` (evaluated with zero script-bridge calls) or a
/// `ScriptHandle` fallback for anything else. No card can construct the
/// `Script` case yet (`Game.AddContinuousEffect`/static-ability self-
/// registration don't exist — see docs/implementation.md's slice backlog,
/// item 3) — the variant exists now so the type shape doesn't need to
/// change again once that slice adds it, but `layers::effective_characteristics`
/// treats a `Script` effect as unresolvable and skips it (same "skip rather
/// than guess" stance already established for CDA power/toughness), not a
/// panic. Once a real consumer exists, evaluating this will need a
/// `&ScriptRuntime` threaded into the query — not needed for the
/// primitive-only path this slice exercises.
#[derive(Clone, Debug)]
pub enum EffectPayload {
    Primitive(EffectPrimitive),
    Script(ScriptHandle),
}

/// Structural filter for which objects a `ContinuousEffect` affects — the
/// same "common shape as a Rust-native check, `extra`/script as the
/// catch-all" split `cost.rs`'s `FilterSpec` already uses for cost filters,
/// narrowed to what an "affects" predicate actually needs (no `random` —
/// picking *one* matching object isn't a thing continuous effects do; no
/// `extra` catch-all string here either, since the fallback for anything
/// this can't express is the `AffectedFilter::Script` case below, not a
/// second string escape hatch).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EffectFilter { 
    /// `You`/`Opponent`/`Any`, resolved against the effect's *source's*
    /// controller (CR 613's "creatures you control" always means the
    /// controller of the permanent generating the effect, not some other
    /// player) — not a parameter the caller supplies separately.
    pub controller: Option<FilterController>,
    pub card_types: Option<CardTypeSet>,
    /// CR 205.4b: "destroy all Zombies" / "Walls get -1/-0" - matches if the
    /// object has *any* subtype in the list, same "or, not and" convention
    /// `TargetFilter::Object::subtypes` already documents. Checked against
    /// the raw `GameObject.definition.subtypes`, not `effective_characteristics`
    /// - same deliberate "raw field, not layer-resolved" stance `card_types`
    /// above already takes in `matches_effect_filter`, to avoid a real
    /// recursion hazard: `ContinuousEffect::applies_to` (one of this filter's
    /// two callers) is itself invoked from inside
    /// `layers::applicable_effects_in_layer`, which is what computes
    /// `effective_characteristics` in the first place.
    pub subtypes: Option<Vec<Subtype>>,
    /// CR 601.2c/115/205.4b's negated form - "destroy all nonbasic lands,"
    /// "creatures that aren't Walls get -1/-0." Same raw-field caveat as
    /// `subtypes` above.
    pub excluded_subtypes: Option<Vec<Subtype>>,
    /// CR 205.4a: "destroy all legendary permanents" - a bitset like
    /// `card_types` (not a `Vec` like `subtypes` above), since
    /// `CardDefinition.supertypes`/`TargetFilter::Object::supertypes` are
    /// both already bitsets. Same raw-field caveat as `subtypes`.
    pub supertypes: Option<SupertypeSet>,
    /// CR 601.2c/115/205.4a's negated form - "destroy all nonbasic lands."
    /// Same raw-field caveat as `subtypes`.
    pub excluded_supertypes: Option<SupertypeSet>,
    /// "Just this permanent" (e.g. an Aura's own "enchanted creature gets
    /// +2/+2", where the affected object *is* the effect's own attachment
    /// target — modeled here as "the effect's source" for a first cut, since
    /// aura-attachment as its own concept doesn't exist in this engine yet).
    pub self_only: bool,
 }

/// A `ContinuousEffect`'s affected-object predicate — same `Primitive`/
/// `Script` split as `EffectPayload` above, same "no card can construct
/// `Script` yet, exists for shape stability" caveat.
///
/// `EnchantedOnly` is CR 303.4m's "an ability of a permanent that refers to
/// the 'enchanted [object]'" — matches whatever object `ContinuousEffect::
/// source` is currently attached to (`GameObject::attached_to`), i.e. the
/// common real-card shape ("Enchanted creature gets +1/+1"). A new variant
/// rather than a field on `EffectFilter` (129 struct-literal construction
/// sites across the test suite with no `Default` impl — the same ripple
/// class the modal-spell slices already hit and avoided each time) or a
/// combination with `self_only` (a different, pre-existing concept: pinning
/// to the effect's own source id, used for e.g. a target-pinned Pump effect
/// — not the same thing as "whatever this is attached to"). Fieldless,
/// deliberately: can't combine with `controller`/`card_types` the way
/// `Primitive` can, since no real "enchanted creature that's also a
/// [type]"-filtered *affects* clause exists to justify the combinatorics
/// (same narrowing precedent `self_only` already sets).
///
/// `EnchantedPlayerControls` closes the "player-side enchant player" gap
/// this comment used to name as unmodeled (`phase5/enchanted-player-
/// controls`) — CR 303.4m's "enchanted player" reference for a general
/// *continuous* effect (as opposed to the player-level `HandSizeModifier`/
/// `HandRevealModifier`::`EnchantedPlayer` shape, or the replacement-effect
/// `WouldEnterEnchantedPlayerGraveyard` shape) — the real card is Curse of
/// Vitality, "Creatures enchanted player controls have lifelink." Matches an
/// *object* (not the enchanted player themselves) whose controller equals
/// whatever player `ContinuousEffect::source` (the Aura/permanent bearing
/// this static ability) is attached to via `GameObject::attached_to_player`
/// — mirrors `EnchantedOnly`'s exact resolution shape (no `GameState` lookup
/// needed, `source: &GameObject` is already borrowed in `applies_to`), just
/// keyed off the player-attach field instead of the object-attach one.
/// Fieldless and exclusive with `Primitive`, same narrowing `EnchantedOnly`
/// already took and for the same reason: no real "creatures enchanted
/// player controls that are also a [type]" card exists to justify combining
/// this with `controller`/`card_types`.
#[derive(Clone, Debug)]
pub enum AffectedFilter {
    Primitive(EffectFilter),
    Script(ScriptHandle),
    EnchantedOnly,
    EnchantedPlayerControls,
}

/// Not exhaustive in the original draft — MTG has more duration shapes than
/// "until end of turn / permanent". Adding: `UntilYourNextTurn` (e.g. many
/// fight/pump effects), `UntilEndOfCombat`, `WhileConditionHolds` (e.g. "as
/// long as you control a Forest" — re-evaluated continuously, not a fixed
/// end point), and `Custom` as a catch-all for unusual oracle wording that
/// doesn't fit a structured case yet.
#[derive(Clone, Debug)]
pub enum EffectDuration {
    UntilEndOfTurn,
    UntilYourNextTurn,
    UntilEndOfCombat,
    WhileSourceOnBattlefield,
    UntilSourceLeaves,
    WhileConditionHolds,
    Permanent,
    Custom(String),
}

/// A registered continuous effect. `affected`/`effect` replace Phase 2's
/// bare `ScriptHandle` placeholders with the real `Primitive`/`Script` split
/// (see `AffectedFilter`/`EffectPayload` above) — the same shape
/// `ConditionSpec`/`AbilityEffect` already established for triggered/
/// activated abilities, applied here to static/continuous effects.
pub struct ContinuousEffect {
    pub source: ObjectId,
    pub layer: Layer,
    pub affected: AffectedFilter,
    pub effect: EffectPayload,
    pub timestamp: SequenceNo,
    pub duration: EffectDuration,
    /// CR 611.2c: "applies only as long as X is true," re-evaluated fresh on
    /// every `layers::applicable_effects_in_layer` call rather than fixed at
    /// generation time — `None` (every effect before this slice) means
    /// unconditional, today's behavior unchanged. Orthogonal to `duration`
    /// above: a `WhileConditionHolds` duration is the CR-613.7-adjacent label
    /// for this shape, but nothing reads `duration` to decide whether an
    /// effect is currently active — `condition` is the actual gate, checked
    /// the same "recompute everything, no cache" way every other layer-engine
    /// query already is. Reuses `InterveningIfSpec` (CR 603.4's trigger
    /// intervening-if) rather than inventing a second predicate type — both
    /// are "evaluate a live board-state predicate, get a bool."
    pub condition: Option<InterveningIfSpec>,
}

/// Whether `object` matches `filter`, relative to `source_id`/
/// `source_controller` (an effect's own source and that source's
/// controller — `filter.self_only`/`filter.controller` are both resolved
/// against these, never a separately-supplied player). Extracted from
/// `ContinuousEffect::applies_to`'s own body so `AbilityEffect::DestroyAll`
/// (CR 701.8's "destroy all lands," etc. — no `ContinuousEffect` involved at
/// all, just a resolving spell/ability's own `source`/`controller`) can
/// reuse the identical controller/card_types matching logic instead of
/// duplicating it.
pub(crate) fn matches_effect_filter(
    filter: &EffectFilter,
    object: &GameObject,
    source_id: ObjectId,
    source_controller: PlayerId,
) -> bool {
    if filter.self_only && object.object_id != source_id {
        return false;
    }
    if let Some(controller_filter) = filter.controller {
        let matches_controller = match controller_filter {
            FilterController::You => object.controller == source_controller,
            FilterController::Opponent => object.controller != source_controller,
            FilterController::Any => true,
        };
        if !matches_controller {
            return false;
        }
    }
    if let Some(types) = filter.card_types {
        if !object.definition.card_types.intersects(types) {
            return false;
        }
    }
    if let Some(subtypes) = &filter.subtypes {
        if !subtypes.iter().any(|s| object.definition.subtypes.contains(s)) {
            return false;
        }
    }
    if let Some(subtypes) = &filter.excluded_subtypes {
        if subtypes.iter().any(|s| object.definition.subtypes.contains(s)) {
            return false;
        }
    }
    if let Some(types) = filter.supertypes {
        if !object.definition.supertypes.intersects(types) {
            return false;
        }
    }
    if let Some(types) = filter.excluded_supertypes {
        if object.definition.supertypes.intersects(types) {
            return false;
        }
    }
    true
}

impl ContinuousEffect {
    /// Whether `object` is in this effect's affected set. Real body for
    /// `AffectedFilter::Primitive` is `matches_effect_filter` above — `source`
    /// only used to resolve `self.source`'s current controller (for
    /// `FilterController::You`/`Opponent`) and identity (for `self_only`).
    /// Also the real body for `EnchantedOnly` (CR 303.4m) — `source.
    /// attached_to` is already a full field on the `&GameObject` passed in,
    /// so no `GameState` lookup is needed here, unlike `layers::
    /// applicable_effects_in_layer`'s own `Script` arm. Same story for
    /// `EnchantedPlayerControls` (also CR 303.4m, the player-side sibling) —
    /// `source.attached_to_player` is checked against `object.controller`
    /// directly, no lookup either. `Script` isn't evaluable yet (no card can
    /// construct one — see `AffectedFilter`'s doc comment) — treated as
    /// "doesn't match" rather than guessed, same stance `layers::
    /// effective_characteristics` takes for an unresolvable CDA.
    pub fn applies_to(&self, object: &GameObject, source: &GameObject) -> bool {
        match &self.affected {
            AffectedFilter::Primitive(filter) => {
                matches_effect_filter(filter, object, self.source, source.controller)
            }
            AffectedFilter::EnchantedOnly => source.attached_to == Some(object.object_id),
            AffectedFilter::EnchantedPlayerControls => {
                source.attached_to_player.is_some_and(|p| object.controller == p)
            }
            AffectedFilter::Script(_) => false,
        }
    }
}

/// Consumption policy for a replacement/prevention effect (CR 614, 615).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsumptionPolicy {
    OneShot,
    Repeatable,
    /// e.g. damage prevention shields with a counter budget.
    LimitedUses(u32),
}

/// What a `ReplacementEffect` watches for — same `EffectFilter` shape
/// `ContinuousEffect::affected` already uses (controller/card_types/
/// self_only, resolved relative to the replacement's own source), applied
/// against an event's affected object/player instead of a live
/// characteristics query. Two shapes this slice — the two that gate the
/// most real card text: damage prevention/redirection (CR 615) and "if this
/// would die, instead ..." zone-change substitution (CR 614.13/704.5g's
/// "dies" definition). Everything else (ETB-tapped modifiers, counter-
/// addition replacements, non-damage/non-death event shapes, ...) stays
/// unmodeled — `EventFilter` has no catch-all/`Script` escape hatch of its
/// own; `ReplacementSpec`'s own `Primitive`/`Script` split (below) is where
/// that lives.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EventFilter {
    /// Matches a `GameEventKind::DamageDealt` whose `target` satisfies
    /// `affected` — a `Target::Player` is checked by `controller` only
    /// (`card_types`/`self_only` can't apply to a player, never match if
    /// set); a `Target::Object` is checked the same way
    /// `ContinuousEffect::applies_to` already checks a `GameObject`.
    DamageDealt { affected: EffectFilter },
    /// Matches a `GameEventKind::ZoneChange` moving `object` from
    /// `Battlefield` to `Graveyard` (CR 700.4's "dies" - any cause, not just
    /// destruction), where `object` satisfies `affected`. A
    /// `ReplacementPayload::Regenerate` effect is also paired with this same
    /// filter shape (no dedicated `EventFilter` variant of its own) but
    /// additionally requires the proposed event to be tagged as genuine
    /// destruction (CR 614.8) — see `ReplacementEffect::matches`'s own doc
    /// comment for why that's a separate `is_destroy` parameter rather than
    /// data carried on the event itself.
    WouldDie { affected: EffectFilter },
    /// Matches any `GameEventKind::ZoneChange` moving `object` from `from`
    /// to `to` (either `None` = wildcard, matching every zone on that side)
    /// where `object` satisfies `affected` — the general form `WouldDie`
    /// hardcodes to exactly `Some(Battlefield)`/`Some(Graveyard)`. Added so
    /// a replacement effect can match a zone-change shape `WouldDie` can't
    /// express at all (e.g. "would be exiled" instead of "would die", or a
    /// non-battlefield-sourced move like drawing/discarding/milling) — a
    /// sibling variant rather than a `WouldDie` generalization, so every
    /// existing `WouldDie` construction site is untouched. Only
    /// `ReplacementPayload::ReplaceZoneDestination` pairs with this -
    /// `Regenerate`'s own CR 614.8 "genuine destruction" meaning is
    /// specifically tied to `WouldDie`'s hardcoded Battlefield -> Graveyard
    /// shape (see `ReplacementEffect::matches`'s own `is_destroy` doc
    /// comment), not something a wildcard-from/to filter should reinterpret.
    ///
    /// Reachability note: as of this writing every real zone-change producer
    /// routes through `replacement::apply_replacements` except token zone-lock
    /// (which is hardcoded ahead of it). `WouldMoveZone(Library, Hand)` is now
    /// reachable via `Game.Ops.DrawCards(n)`/`Engine::continue_draw_cards`, and
    /// `WouldMoveZone(Hand, Graveyard)` is reachable via `Engine::submit_discard`
    /// (CR 514.1 cleanup discard) and Sacrifice/Discard cost payments. A
    /// `WouldMoveZone` matching other non-Battlefield `from`/`to` pairs is
    /// real and correct but may still be unreachable via actual gameplay,
    /// same "proven via direct construction, ready the moment a real producer
    /// exists" stance `phase5/token-zone-lock`'s own CR 111.8 check already took.
    WouldMoveZone { from: Option<ZoneKind>, to: Option<ZoneKind>, affected: EffectFilter },
    /// CR 702.89a (Umbra armor): "if enchanted permanent would be destroyed."
    /// Same `Battlefield` -> `Graveyard` shape `WouldDie` matches, but the
    /// affected object is always the replacement effect's own source's
    /// `attached_to` target rather than a `controller`/`card_types`/
    /// `self_only` filter — fieldless, sibling to `WouldDie` rather than a
    /// new field on it (23 existing `WouldDie` construction sites, no
    /// `Default` impl — same ripple class `AffectedFilter::EnchantedOnly`
    /// itself already avoided for the continuous-effect sibling of this same
    /// concept, `phase5/enchanted-reference`). Only pairs with
    /// `ReplacementPayload::UmbraArmor`.
    WouldDieEnchanted,
    /// CR 303.4m/616 (Wheel of Sun and Moon's own worked example): "if a
    /// card would be put into enchanted player's graveyard from anywhere."
    /// Same wildcard-`from`/fixed-`to: Graveyard` shape `WouldMoveZone`
    /// already has, but — like `WouldDieEnchanted` relative to `WouldDie` —
    /// the affected side isn't a `controller`/`card_types`/`self_only`
    /// filter at all: it's "does this card's own *owner* (CR 400.7 —
    /// graveyards are owner-keyed, not controller-keyed) equal whichever
    /// player this effect's own source is attached to." Fieldless, sibling
    /// to `WouldMoveZone` rather than a new field on it, same ripple-avoidance
    /// reasoning `WouldDieEnchanted` already gives. Not gated on `is_destroy`
    /// — CR 616's own wording is "from anywhere," matching a discard/mill/
    /// death alike, the same ungated stance the plain `WouldMoveZone` variant
    /// already takes. Only pairs with `ReplacementPayload::
    /// ReplaceZoneDestination` — reused as-is, not a new payload, since
    /// Wheel of Sun and Moon's own effect ("revealed and put on the bottom of
    /// its owner's library") is mechanically just a zone-destination
    /// substitution to `Library` once the always-`push_bottom` convention
    /// `commit_zone_move` already uses is taken into account. "Revealed" is
    /// left unmodeled — this engine has no per-viewer hidden-information
    /// boundary at all yet (`VisibilityPolicy::is_visible_to` has no callers
    /// — same standing gap `HandRevealModifier`'s own doc comment already
    /// names), so there is nothing for "revealed" to mean
    /// beyond the zone move that already happens either way.
    WouldEnterEnchantedPlayerGraveyard,
}

/// A `ReplacementEffect`'s actual rewrite (CR 614.1: the event doesn't
/// happen, a different one does instead) — narrower than the full rules
/// text, matching `EventFilter`'s own two shapes. Applying a payload against
/// a mismatched event shape (e.g. `PreventDamage` against a `ZoneChange`)
/// isn't reachable through the normal match-then-apply path (`EventFilter`
/// and `ReplacementPayload` are paired at construction, same "reject a bad
/// pairing at :Register() time" stance `StaticAbilityBuilder`'s layer/effect
/// check already takes — though no Lua builder exists for replacement
/// effects yet, see this file's own backlog note) — `ReplacementEffect::apply`
/// falls back to returning the event unchanged rather than guessing.
#[derive(Clone, Debug)]
pub enum ReplacementPayload {
    /// CR 615: the matched damage doesn't happen at all — the event still
    /// fires (so anything downstream that cares "damage was dealt, amount
    /// 0" sees a real event), just with its amount zeroed.
    PreventDamage,
    /// CR 614.9 redirection — the matched damage is dealt to this target
    /// instead of its original one. `ReplacementEffect::matches`'s
    /// `DamageDealt` arm re-checks this target is still a valid
    /// destination (still on the battlefield, still a
    /// creature/planeswalker/battle, or still a player in the game) on
    /// every application — CR 614.9's own text: if it isn't, "the effect
    /// does nothing" (fizzles silently, original damage goes through
    /// unprevented), same "recheck fresh, don't trust construction-time
    /// state" stance `Regenerate`'s CANT_BE_REGENERATED check already
    /// takes.
    RedirectDamage(Target),
    /// CR 614.13-style "instead" zone-change substitution (e.g. "exile it
    /// instead of putting it into its owner's graveyard") — the matched
    /// `ZoneChange`'s `to` becomes this zone instead.
    ReplaceZoneDestination(ZoneKind),
    /// CR 614.8/701.19: regeneration. Rewrites the matched destroy
    /// `ZoneChange` into a `GameEventKind::Regenerated` instead of a
    /// different zone move — see that variant's own doc comment for why
    /// this can't be expressed as a `ReplaceZoneDestination` (the permanent
    /// doesn't change zones at all, so there's no destination to substitute).
    Regenerate,
    /// CR 603.6d/113.6h: the matched `WouldMoveZone(_, Battlefield)` event's
    /// resulting permanent enters tapped. Unlike every payload above, this
    /// has nothing on `GameEventKind::ZoneChange` to rewrite (no `tapped`
    /// field exists there) — `ReplacementEffect::apply`'s fallback arm
    /// returns the event unchanged for this payload, same as it would for
    /// any other mismatched pairing. The real effect happens in
    /// `replacement::apply_enters_battlefield_modifiers`, a narrow sibling
    /// driver (not `apply_replacements_from`) that mutates the already-
    /// constructed `GameObject` directly rather than routing through the
    /// event-rewrite loop — see that function's own doc comment for why
    /// entering-the-battlefield modifiers don't fit the "propose, rewrite,
    /// commit" shape every other payload above uses.
    EntersTapped,
    /// CR 614.1c/122.6: the matched `WouldMoveZone(_, Battlefield)` event's
    /// resulting permanent enters with `amount` counters of `kind` on it
    /// (e.g. Fading N, Modular N, Graft N — fixed printed numbers only).
    /// Sunburst's own game-state-dependent amount is `EntersWithCountersSunburst`
    /// below instead, a separate variant since its kind/amount are both
    /// derived at apply time, not fixed at registration. Devour/Amplify
    /// (also below) each needed a genuine mid-ETB player choice on top of a
    /// fixed per-unit rate, so neither is a bare `EntersWithCounters` either
    /// — each has its own pause/resume `ProcessState`, same category of work
    /// as `AwaitingAttachChoice`. Same "nothing to rewrite on the event
    /// itself" stance as `EntersTapped` above — applied by `replacement::
    /// apply_enters_battlefield_modifiers`, not `apply()`.
    EntersWithCounters { kind: CounterKind, amount: i32 },
    /// CR 702.54a (Bloodthirst N): like `EntersWithCounters` above, but
    /// gated on a state query instead of unconditional — the permanent
    /// enters with `amount` counters of `kind` only if an opponent of its
    /// *own* controller was dealt damage this turn (by any source, combat
    /// or otherwise — matching real Bloodthirst rulings, not restricted to
    /// combat damage). Unlike `EntersWithCounters`, this needs no dynamic
    /// script callback: the condition is answerable purely from
    /// `state.ledger` (scan back to the most recent `GameEventKind::
    /// TurnStarted`, then check for a `DamageDealt` targeting a player who
    /// isn't the entering permanent's controller) — see
    /// `replacement::opponent_damaged_this_turn`. CR 702.54b's "Bloodthirst
    /// X" variant (amount = *total* damage dealt to opponents this turn,
    /// not a fixed gate) reuses the identical scan and is a cheap follow-up,
    /// deliberately not bundled in here to keep this primitive matching
    /// exactly what was scoped.
    EntersWithCountersBloodthirst { kind: CounterKind, amount: i32 },
    /// CR 702.138c (Escape N): "[This permanent] escapes with N [kind]
    /// counters" means "if this permanent escaped, it enters with N [kind]
    /// counters" — same no-player-choice gated shape `EntersWithCountersBloodthirst`
    /// already established, checked against `GameObject::entered_via_escape`
    /// (`phase5/escape-provenance`) instead of `opponent_damaged_this_turn`.
    /// CR 702.138c's own second sentence ("may have a triggered ability
    /// linked to it that triggers 'when it enters this way'") isn't modeled
    /// — no real card in this corpus combines Escape with a linked trigger,
    /// same posture the Vesuvan-only `grant_recurring_copy_trigger` shape
    /// already takes for its own narrower CR 707.9a case.
    EntersWithCountersEscape { kind: CounterKind, amount: i32 },
    /// CR 702.44 (Sunburst): like `EntersWithCounters`, but both `kind` and
    /// `amount` are derived at apply time instead of fixed at registration —
    /// no fields on this variant at all. `amount` is the number of *distinct*
    /// colors of mana spent to cast this object (CR 702.44b: zero if no
    /// colored mana was spent, or if this object isn't entering from the
    /// stack as a resolving spell at all — a `ColorSet` this engine has
    /// nowhere else to source except `StackObject.costs_paid`, which only
    /// exists while a spell is still on the stack). `kind` is `PlusOnePlusOne`
    /// if this object's own *printed* `CardDefinition.card_types` include
    /// Creature, `Charge` otherwise — CR 702.44a's "ignoring any
    /// type-changing effects that would affect it" is explicit that this
    /// reads the printed type, not `EffectiveCharacteristics::card_types`
    /// (layer 4), the one respect this isn't shaped like every other
    /// characteristic-sensitive check in this engine. See
    /// `replacement::apply_enters_battlefield_modifiers`'s own doc comment
    /// for where `colors_spent` comes from and why the other two call sites
    /// (PlayLand, Search) always pass an empty `ColorSet`.
    EntersWithCountersSunburst,
    /// CR 702.82a (Devour N): "as this object enters, you may sacrifice any
    /// number of creatures. This permanent enters with N +1/+1 counters on
    /// it for each creature sacrificed this way." Unlike every other
    /// `EntersWithCounters*` variant, this one needs a genuine mid-ETB
    /// player choice — *which* creatures (if any) to sacrifice — so it's
    /// never applied by `replacement::apply_enters_battlefield_modifiers`
    /// at all (that function only ever mutates state/proposes events
    /// synchronously, it can't pause the engine). Instead,
    /// `replacement::pending_devour_choice` is checked directly by
    /// `ResolveStackTop` — the one call site that can raise a real pause
    /// (`ProcessState::AwaitingDevourChoice`) and resume afterward
    /// (`Engine::submit_devour_choice`) — before `apply_enters_battlefield_
    /// modifiers` even runs for this object. `PlayLand`/`Search`'s
    /// battlefield-entry call sites never check for this payload at all —
    /// a land can never have Devour, and a Search-fetched Devour creature
    /// choosing not to offer this choice is a named, narrow gap (see
    /// `pending_devour_choice`'s own doc comment). CR 702.82c's
    /// "Devour [quality] N" variant is real now too (`phase5/devour-quality`)
    /// — `quality`, checked in `pending_devour_choice` against
    /// `layers::effective_characteristics(...).subtypes` (the *effective*
    /// view, not a raw field — safe here since this function is a one-shot
    /// call from `ResolveStackTop`, not inside the layer-2 recursion path
    /// `matches_effect_filter`'s own raw-field policy exists to dodge; this
    /// matches the effective-view `card_types` check this same function
    /// already makes). `None` (every card before this slice) means "any
    /// creature," unchanged.
    EntersWithCountersDevour { per_creature: i32, quality: Option<Subtype> },
    /// CR 702.38a (Amplify N): "as this object enters, reveal any number of
    /// cards from your hand that share a creature type with it. This
    /// permanent enters with N +1/+1 counters on it for each card revealed
    /// this way." Same category of gap as `EntersWithCountersDevour` above —
    /// a genuine mid-ETB player choice, so never applied by
    /// `replacement::apply_enters_battlefield_modifiers` at all. Checked
    /// directly by `ResolveStackTop` via `replacement::pending_amplify_choice`,
    /// pausing on `ProcessState::AwaitingAmplifyChoice` and resuming via
    /// `Engine::submit_amplify_choice`. Unlike Devour, revealing a hand card
    /// changes nothing about its zone — no sacrifice, no dies triggers, no
    /// `continue_scripted_zone_move` needed; the resume just folds a
    /// `CounterAdded` straight into `events_so_far` and finishes the
    /// resolution the same way any other synchronous-only path does.
    /// "Shares a creature type" is approximated as raw subtype overlap
    /// (`EffectiveCharacteristics::subtypes`) — this engine has no separate
    /// "creature type" category distinct from land/artifact/etc. subtypes,
    /// same imprecision `landwalk_subtypes`/`bands_with_other_subtypes`
    /// already accept for their own quality checks. CR 702.38a's "can't
    /// reveal cards entering the battlefield at the same time as this card"
    /// is unmodeled — no producer in this engine ever enters multiple
    /// permanents from hand simultaneously, so the exclusion is unreachable.
    /// CR 702.38b (multiple separate Amplify instances "work separately")
    /// is unmodeled too — `pending_amplify_choice` matches only the first
    /// registered instance, same single-match shape `pending_devour_choice`
    /// already has.
    EntersWithCountersAmplify { per_card: i32 },
    /// CR 707.6/614.12a (Clone itself): "you may have this creature enter
    /// as a copy of any creature on the battlefield." Same category of gap
    /// as `EntersWithCountersDevour`/`EntersWithCountersAmplify` above — a
    /// genuine mid-ETB player choice (*which* creature, if any, to become),
    /// so never applied by `replacement::apply_enters_battlefield_modifiers`
    /// at all. Checked directly by `ResolveStackTop` via
    /// `replacement::pending_copy_choice`, pausing on `ProcessState::
    /// AwaitingCopyChoice` and resuming via `Engine::submit_copy_choice`.
    ///
    /// Unlike `EntersWithCountersDevour`/`Amplify`, a real choice here
    /// reuses `AbilityEffect::BecomeCopy`'s own machinery wholesale
    /// (`commands::purge_granted_abilities` + a fresh `run_card_script`
    /// against the chosen creature's snapshot, with `CopyExceptions`
    /// applied first — same field this variant carries) and logs the same
    /// `GameEventKind::BecameCopy` `BecomeCopy` does, since "this object
    /// became a copy of that one" is exactly the same fact either way, not
    /// a new one. What's unique to this path: the resume always continues
    /// into `apply_enters_battlefield_modifiers` afterward (copied or not),
    /// with the entering object's own real `costs_paid`-derived
    /// `colors_spent` carried through the pause — this is what makes the
    /// comp rules' own worked examples real: a Clone copying a Fading +
    /// "enters tapped" creature enters tapped with fade counters; a Clone
    /// copying a Sunburst creature gets Sunburst's real colors-spent count.
    ///
    /// Narrowed on purpose: only `ResolveStackTop`'s resolving-spell path
    /// checks this (`Search` doesn't either, same "no real producer reaches
    /// this yet" posture `pending_devour_choice`'s own doc comment already
    /// takes). The post-choice resume chains into
    /// `apply_enters_battlefield_modifiers` only, not back into Devour's/
    /// Amplify's own pause checks — no real Clone-family card has either of
    /// those keywords, so this is a named, theoretical gap, not a real one.
    /// Self-copy (choosing the entering object itself as the candidate)
    /// isn't special-cased away, same stance `pending_devour_choice`'s own
    /// self-sacrifice case already takes.
    EntersAsCopy(CopyExceptions),
    /// CR 702.89a (Umbra armor): "instead remove all damage marked on
    /// [enchanted permanent] and destroy this Aura." Only pairs with
    /// `EventFilter::WouldDieEnchanted`. Rewrites the enchanted permanent's
    /// own proposed destroy into `GameEventKind::UmbraArmorConsumed` — same
    /// "nothing to rewrite the *moving* object into, the permanent doesn't
    /// change zones at all" shape `Regenerate`/`Regenerated` already
    /// established (CR 702.89a prevents the destruction entirely, it doesn't
    /// redirect it). Unlike `Regenerate`, a *second* object (the Aura
    /// itself) also has to leave the battlefield as a real consequence —
    /// see `sba::commit_umbra_armor`'s own doc comment for why that widened
    /// `sba::commit_death_or_regeneration`'s return type to a `Vec`.
    UmbraArmor,
}

/// Same `Primitive`/`Script` split every other filter-shaped type in this
/// codebase uses. `Script` stays dead code — no Lua builder exists yet for a
/// card to construct one. `Primitive` now has a real Lua construction path,
/// `Game.CreateReplacementEffect` (`phase5/static-regeneration`, CR 701.19b)
/// — scoped narrowly to exactly the `WouldDie`/`Regenerate` pairing this
/// slice needed proof against; the other `EventFilter`/`ReplacementPayload`
/// pairings (damage prevention/redirection, non-regenerate zone-destination
/// substitution) still have no Lua entry point, same "prove one real shape,
/// name the rest" precedent `Game.CreateStaticAbility` itself started with
/// (layers 6/7b/7c only, before later slices widened it).
#[derive(Clone, Debug)]
pub enum ReplacementSpec {
    Primitive { filter: EventFilter, payload: ReplacementPayload },
    Script(ScriptHandle),
}

pub struct ReplacementEffect {
    pub source: ObjectId,
    pub spec: ReplacementSpec,
    pub consumption: ConsumptionPolicy,
}

impl ReplacementEffect {
    /// Whether `event` is one this effect would replace. Real body for
    /// `ReplacementSpec::Primitive` — resolves `self.source`'s *live*
    /// controller (`layers::effective_controller`, CR 108.4 - not the raw
    /// field, so a control-changed source's "you"/"opponent" reads correctly)
    /// and the event's own object/player via `state`. `Script` isn't
    /// evaluable yet (nothing can construct it) — treated as "doesn't
    /// match," same "skip rather than guess" stance every other
    /// unresolvable-script case in this codebase already takes. No recursion
    /// risk resolving control here (unlike `ContinuousEffect::applies_to`,
    /// evaluated from inside the layer engine's own query) - replacement
    /// effects are a wholly separate `state.replacement_effects` list, never
    /// consulted while computing `effective_controller` itself.
    ///
    /// `is_destroy`: whether the caller is proposing `event` as genuine
    /// destruction (CR 614.8) rather than merely some other battlefield ->
    /// graveyard move (CR 704.5f zero toughness, CR 704.5k world rule -
    /// neither is "destroy"). A plain data field on `GameEventKind::
    /// ZoneChange` was considered and rejected: nothing but the CR 704.5g
    /// SBA path and a "Destroy" effect ever needs to say this, so threading
    /// it as a call parameter (only `sba::resolve_deaths` sets it `true`
    /// this slice - see that function's own doc comment) avoids rippling a
    /// new field through every one of this codebase's ~60 unrelated
    /// `ZoneChange` construction sites. Irrelevant to every filter except
    /// `WouldDie` paired with `ReplacementPayload::Regenerate` - a
    /// non-destroy `WouldDie` effect ("if this would die, exile it
    /// instead") still matches regardless, per CR 700.4's broader "dies."
    pub fn matches(
        &self,
        event: &GameEventKind,
        is_destroy: bool,
        state: &GameState,
        script_runtime: &ScriptRuntime,
    ) -> bool {
        let ReplacementSpec::Primitive { filter, payload } = &self.spec else { return false };
        match (filter, event) {
            (EventFilter::DamageDealt { affected }, GameEventKind::DamageDealt { target, .. }) => {
                if let ReplacementPayload::RedirectDamage(new_target) = payload {
                    // CR 614.9: a redirection effect whose destination has
                    // already left the battlefield (or stopped being a
                    // creature/planeswalker/battle) or is a player who's
                    // left the game does nothing — not "prevent," just
                    // never matches, so the original damage proceeds.
                    if !redirect_target_still_valid(*new_target, state, script_runtime) {
                        return false;
                    }
                }
                event_target_matches(affected, *target, state, script_runtime, self.source)
            }
            (
                EventFilter::WouldDie { affected },
                GameEventKind::ZoneChange { object, from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, .. },
            ) => {
                if matches!(payload, ReplacementPayload::Regenerate) {
                    if !is_destroy {
                        return false;
                    }
                    // CR 701.19c, checked here rather than only at shield
                    // creation time: a `Repeatable` (CR 701.19b, static-
                    // ability-sourced) shield is registered once but must
                    // keep re-gating on every destroy attempt, since the
                    // target's CANT_BE_REGENERATED status can change after
                    // the shield already exists - unlike a `OneShot` (CR
                    // 701.19a) shield, which `engine.rs`'s creation site
                    // already skips creating for a can't-be-regenerated
                    // target (that skip remains, as a cheap optimization -
                    // this check is what's now authoritative for both).
                    if crate::layers::effective_characteristics(state, script_runtime, *object)
                        .keywords
                        .contains(KeywordSet::CANT_BE_REGENERATED)
                    {
                        return false;
                    }
                }
                event_target_matches(affected, Target::Object(*object), state, script_runtime, self.source)
            }
            (
                EventFilter::WouldDieEnchanted,
                GameEventKind::ZoneChange { object, from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, .. },
            ) => {
                // CR 702.89a only fires on genuine destruction, same
                // is_destroy gate Regenerate's own WouldDie arm checks.
                is_destroy && state.object(self.source).and_then(|o| o.attached_to) == Some(*object)
            }
            (
                EventFilter::WouldEnterEnchantedPlayerGraveyard,
                GameEventKind::ZoneChange { object, to: ZoneKind::Graveyard, .. },
            ) => {
                // CR 400.7: graveyards are owner-keyed, not controller-keyed
                // - the check is against `object`'s own owner, not whoever
                // currently controls it.
                let Some(enchanted_player) = state.object(self.source).and_then(|o| o.attached_to_player) else {
                    return false;
                };
                state.object(*object).is_some_and(|o| o.owner == enchanted_player)
            }
            (
                EventFilter::WouldMoveZone { from, to, affected },
                GameEventKind::ZoneChange { object, from: event_from, to: event_to, .. },
            ) => {
                if from.is_some_and(|f| f != *event_from) || to.is_some_and(|t| t != *event_to) {
                    return false;
                }
                event_target_matches(affected, Target::Object(*object), state, script_runtime, self.source)
            }
            _ => false,
        }
    }

    /// Rewrites `event` per this effect's payload — only meaningful once
    /// `matches` has confirmed this effect applies. See this enum's own doc
    /// comment for the mismatched-pairing fallback.
    pub fn apply(&self, event: GameEventKind) -> GameEventKind {
        let ReplacementSpec::Primitive { payload, .. } = &self.spec else { return event };
        match (payload, event) {
            (
                ReplacementPayload::PreventDamage,
                GameEventKind::DamageDealt { source, target, is_combat_damage, .. },
            ) => GameEventKind::DamageDealt { source, target, amount: 0, is_combat_damage },
            (
                ReplacementPayload::RedirectDamage(new_target),
                GameEventKind::DamageDealt { source, amount, is_combat_damage, .. },
            ) => GameEventKind::DamageDealt { source, target: *new_target, amount, is_combat_damage },
            (
                ReplacementPayload::ReplaceZoneDestination(new_zone),
                GameEventKind::ZoneChange { object, from, last_known_info, .. },
            ) => GameEventKind::ZoneChange { object, from, to: *new_zone, last_known_info },
            (ReplacementPayload::Regenerate, GameEventKind::ZoneChange { object, .. }) => {
                GameEventKind::Regenerated { object }
            }
            (ReplacementPayload::UmbraArmor, GameEventKind::ZoneChange { object, .. }) => {
                GameEventKind::UmbraArmorConsumed { enchanted: object, aura: self.source }
            }
            (_, other) => other,
        }
    }
}

/// CR 614.9's own destination-validity clause, checked fresh by
/// `ReplacementEffect::matches` on every `RedirectDamage` application (not
/// just once at construction) — same "the world can change between when a
/// replacement effect is created and when it's asked to apply" stance
/// `Regenerate`'s CANT_BE_REGENERATED recheck already takes. Mirrors
/// `commands::target_is_legal`'s `TargetFilter::AnyTarget` arm (CR 115.4's
/// own creature/planeswalker/battle/player list) — deliberately not calling
/// that function directly, since it also checks protection (irrelevant here:
/// CR 614.9 fizzles on a *missing* destination, not a protected one) and
/// lives in `commands.rs`, a layer above `types/effect.rs`.
fn redirect_target_still_valid(target: Target, state: &GameState, script_runtime: &ScriptRuntime) -> bool {
    match target {
        // `state.player()` doesn't filter by `eliminated` - a player who's
        // left the game stays in `state.players` forever, just flagged (see
        // `sba::check_state_based_actions`'s own CR 104.3 elimination path)
        // - so CR 614.9's "left the game" clause needs the flag checked
        // directly, unlike `commands::target_is_legal`'s `TargetFilter::
        // AnyTarget` arm (a pre-existing, separate gap in CR 115.4 targeting
        // legality, not touched here).
        Target::Player(pid) => state.player(pid).is_some_and(|p| !p.eliminated),
        Target::Object(id) => {
            let Some(object) = state.object(id) else { return false };
            if object.zone != ZoneKind::Battlefield {
                return false;
            }
            crate::layers::effective_characteristics(state, script_runtime, id)
                .card_types
                .intersects(CardTypeSet::CREATURE | CardTypeSet::PLANESWALKER | CardTypeSet::BATTLE)
        }
        // CR 614.9: nothing redirects damage to an ability - it's never a
        // legal redirection destination.
        Target::Ability(_) => false,
    }
}

/// Shared by `ReplacementEffect::matches`'s two `EventFilter` arms — same
/// controller/card_types/self_only check `ContinuousEffect::applies_to`
/// already does for a live characteristics query, generalized over `Target`
/// since an event's affected slot can be a player, not just an object.
/// `effect_source` is the replacement effect's own source `ObjectId` — used
/// both to resolve its controller (for `FilterController::You`/`Opponent`,
/// same role `applies_to`'s own `source: &GameObject` parameter plays) and,
/// for `self_only`, as the identity `target` must match (CR 614.13's "if
/// *this* would die" — "this" is the replacement effect's own source, e.g.
/// an aura or the dying permanent's own ability).
fn event_target_matches(
    filter: &EffectFilter,
    target: Target,
    state: &GameState,
    script_runtime: &ScriptRuntime,
    effect_source: ObjectId,
) -> bool {
    if state.object(effect_source).is_none() {
        return false;
    }
    let source_controller = crate::layers::effective_controller(state, script_runtime, effect_source);
    match target {
        Target::Player(pid) => {
            if filter.card_types.is_some()
                || filter.subtypes.is_some()
                || filter.supertypes.is_some()
                || filter.self_only
            {
                // A player has no card types/subtypes/supertypes and can't be
                // "this effect's own source" - all real, if unusual, non-
                // matches rather than something to guess past.
                // `excluded_subtypes`/`excluded_supertypes` are not included
                // here: a player trivially has none of the listed subtypes/
                // supertypes, so a negated filter is vacuously satisfied
                // rather than a non-match, same "positive filter rejects,
                // negated filter passes" asymmetry `card_types` (positive-
                // only on `EffectFilter`, no `excluded_card_types` field here)
                // already implies.
                return false;
            }
            match filter.controller {
                Some(FilterController::You) => pid == source_controller,
                Some(FilterController::Opponent) => pid != source_controller,
                Some(FilterController::Any) | None => true,
            }
        }
        Target::Object(id) => {
            let Some(object) = state.object(id) else { return false };
            if filter.self_only && id != effect_source {
                return false;
            }
            if let Some(fc) = filter.controller {
                let object_controller = crate::layers::effective_controller(state, script_runtime, id);
                let matches_controller = match fc {
                    FilterController::You => object_controller == source_controller,
                    FilterController::Opponent => object_controller != source_controller,
                    FilterController::Any => true,
                };
                if !matches_controller {
                    return false;
                }
            }
            if let Some(types) = filter.card_types {
                if !object.definition.card_types.intersects(types) {
                    return false;
                }
            }
            // CR 205.4b, same raw-field (not `effective_characteristics`)
            // caveat `matches_effect_filter`'s own `subtypes`/
            // `excluded_subtypes` checks document - this function is a
            // separate near-duplicate of that one (layers-aware
            // `effective_controller`, event-shaped `Target` argument), so the
            // check has to be repeated here rather than shared.
            if let Some(subtypes) = &filter.subtypes {
                if !subtypes.iter().any(|s| object.definition.subtypes.contains(s)) {
                    return false;
                }
            }
            if let Some(subtypes) = &filter.excluded_subtypes {
                if subtypes.iter().any(|s| object.definition.subtypes.contains(s)) {
                    return false;
                }
            }
            if let Some(types) = filter.supertypes {
                if !object.definition.supertypes.intersects(types) {
                    return false;
                }
            }
            if let Some(types) = filter.excluded_supertypes {
                if object.definition.supertypes.intersects(types) {
                    return false;
                }
            }
            true
        }
        // Every real call site passes a DamageDealt target (never an
        // ability, CR 614.9) or a plain Target::Object - never reached in
        // practice, same defensive-total-function stance this file already
        // takes elsewhere.
        Target::Ability(_) => false,
    }
}

/// The live, in-game characteristics of an object — output shape of the
/// layer engine's query, computed on demand from `CardDefinition` base
/// values plus all applicable `ContinuousEffect`s. Never stored as
/// a mutable field on `GameObject`; recomputed whenever queried so layer
/// reordering/dependency changes are always reflected. Query function lands
/// in Phase 2.
pub struct EffectiveCharacteristics {
    pub name: String,
    pub mana_cost: ManaCost,
    /// Base (`color_indicator`, see `layers.rs`'s own stopgap note on that),
    /// folded against every applicable layer 5 `EffectPrimitive::AddColor`/
    /// `SetColors` in timestamp order (`AddColor` unions in, `SetColors`
    /// overwrites outright — see each variant's own doc comment).
    pub colors: ColorSet,
    /// Base (`CardDefinition.card_types`), folded against every applicable
    /// layer 4 `EffectPrimitive::AddCardType`/`SetCardTypes` in timestamp
    /// order (`AddCardType` unions in, `SetCardTypes` overwrites outright).
    pub card_types: CardTypeSet,
    /// Base (`CardDefinition.supertypes`) unioned with every applicable
    /// layer 4 `EffectPrimitive::AddSupertype`. No layer removes a
    /// supertype (CR 205.4b).
    pub supertypes: SupertypeSet,
    /// Base (`CardDefinition.subtypes`) extended with every applicable
    /// layer 4 `EffectPrimitive::AddSubtype` (duplicates skipped). No layer
    /// removes a subtype.
    pub subtypes: Vec<Subtype>,
    /// Base (`CardDefinition.keywords`) unioned with every applicable layer
    /// 6 `EffectPrimitive::AddKeyword`, reset to empty by an intervening
    /// `RemoveAllAbilities` (CR 613.1f "loses all abilities") — see that
    /// primitive's own doc comment.
    pub keywords: KeywordSet,
    /// CR 613.1f "as though it didn't have [keyword]" text: keyword(s) whose
    /// *restriction* is voided, unioned with every applicable layer 6
    /// `EffectPrimitive::IgnoreKeyword`, reset to empty by an
    /// intervening `RemoveAllAbilities` alongside `keywords` itself. Deliberately
    /// separate from `keywords` — the keyword bit(s) named here stay set in
    /// `keywords` too (an "ignore the restriction" grant is not a
    /// `RemoveKeyword`; see that primitive's own doc comment for the real
    /// Assault Formation ruling this distinction exists for). A caller
    /// enforcing a keyword-driven restriction must check both fields: the
    /// keyword is present in `keywords` AND absent from here for the
    /// restriction to actually apply.
    pub ignored_keywords: KeywordSet,
    /// CR 702.16e: colors this object has protection from, unioned with
    /// every applicable layer 6 `EffectPrimitive::AddProtectionColor`. A
    /// separate field from `keywords` rather than another `KeywordSet` bit
    /// — protection has to carry *which* color(s), which a flat bitflag
    /// can't. Empty base (`CardDefinition` has no printed-protection field
    /// of its own yet — every real protection ability is granted via this
    /// primitive, same "no printed-keyword-string parsing" stance
    /// `KeywordSet`'s own construction already takes for every other
    /// ability). Enforced at three real call sites: damage (`apply_
    /// replacements`), targeting (`target_is_legal`), and blocking
    /// (`DeclareBlockers`) — see `phase5/protection-from-color`
    /// (docs/implementation.md) for why those three and not the full CR
    /// 702.16 "DEBT" list (Enchant/Equip needs attach machinery this engine
    /// doesn't have).
    pub protection_colors: ColorSet,
    /// CR 702.16's card-type quality: card type(s) this object has
    /// protection from, unioned with every applicable layer 6
    /// `EffectPrimitive::AddProtectionCardType`. Same reasoning
    /// `protection_colors` documents for why this is its own field rather
    /// than reusing `card_types`/`CardTypeSet` on `AddCardType` — same
    /// three enforcement call sites too, each now checking both quality
    /// fields (protected if *either* intersects the source's own colors or
    /// card types).
    pub protection_card_types: CardTypeSet,
    /// CR 702.16's subtype quality: subtype(s) this object has protection
    /// from, from every applicable layer 6 `EffectPrimitive::
    /// AddProtectionSubtype`. `Vec`, not a bitflag `Set` like `protection_colors`/
    /// `protection_card_types` — `Subtype` is an open-ended string (new
    /// creature types are added over time), same reason `CardDefinition.
    /// subtypes` itself is a `Vec<Subtype>` rather than a fixed enum. Same
    /// three enforcement call sites as the other two quality fields, each
    /// checking all three as an OR — protected if color, card type, *or*
    /// subtype matches the source.
    pub protection_subtypes: Vec<Subtype>,
    /// CR 702.16a's name quality: card name(s) this object has protection
    /// from, from every applicable layer 6 `EffectPrimitive::
    /// AddProtectionName`. `Vec<String>` rather than a single `Option` —
    /// CR 702.16g's "protection from A and from B" already models multiple
    /// simultaneous qualities as multiple registered abilities, same as the
    /// other three quality fields. Same three enforcement call sites,
    /// matched by exact equality against the source's own *effective* name,
    /// not a bitflag/subtype-list membership test.
    pub protection_names: Vec<String>,
    /// CR 702.16j: "protection from everything" — `true` if any applicable
    /// layer 6 `EffectPrimitive::AddProtectionEverything` granted it, `false`
    /// otherwise (the base/default). Unlike the other four `protection_*`
    /// fields, this is an unconditional flag, not a set/list to intersect
    /// against the source's own characteristics — CR 702.16j is explicit
    /// that it applies "regardless of that object's characteristic values,"
    /// so the three enforcement call sites check this field *first*, before
    /// even looking up the source's own characteristics (no source lookup
    /// is needed to know the answer once this is `true`).
    pub protection_everything: bool,
    /// CR 702.16k's player quality: relative player(s) (`FilterController::
    /// You`/`Opponent`) this object has protection from, from every
    /// applicable layer 6 `EffectPrimitive::AddProtectionPlayer`. `Vec`
    /// rather than a single `Option`, same "CR 702.16g composability"
    /// reasoning the other quality fields already document. Unlike the
    /// other four `protection_*` fields, matching this one needs a
    /// reference point — this object's own controller, not a
    /// characteristic — resolved at each of the three enforcement call
    /// sites (not here) against the incoming source's raw `GameObject.
    /// controller` field, same "You"/"Opponent" resolution `EffectFilter`
    /// already does.
    pub protection_players: Vec<FilterController>,
    /// CR 702.14's landwalk: land subtype(s) that make this object
    /// unblockable while the defending player controls a matching land,
    /// from every applicable layer 6 `EffectPrimitive::AddLandwalkSubtype`.
    /// `Vec`, not a bitflag `Set`, same reasoning `protection_subtypes`
    /// documents (`Subtype` is open-ended). Empty base — no printed field on
    /// `CardDefinition`, same "always effect-granted" shape as every
    /// `protection_*` field above. Unlike those, enforcement
    /// (`commands::DeclareBlockers`) reads this field on the *attacker* but
    /// checks it against the *defending player's battlefield*, not the
    /// blocker's own characteristics — CR 702.14c's condition lives outside
    /// either combatant.
    pub landwalk_subtypes: Vec<Subtype>,
    /// CR 702.14a's combination forms ("artifact landwalk," "nonbasic
    /// landwalk," "snow swampwalk") — every applicable layer 6
    /// `EffectPrimitive::AddLandwalkQuality`, checked separately from
    /// `landwalk_subtypes` since the two primitives' payloads aren't the
    /// same shape (a bare `Subtype` vs a whole `LandwalkQuality`). Same
    /// empty-base/always-effect-granted posture.
    pub landwalk_qualities: Vec<LandwalkQuality>,
    /// CR 702.22b/c's "bands with other [quality]": subtype(s) this object
    /// can band with under CR 702.22c's second band-formation path, from
    /// every applicable layer 6 `EffectPrimitive::AddBandsWithOther`. Same
    /// `Vec`/empty-base/always-effect-granted shape as `landwalk_subtypes`.
    /// Enforcement (`commands::PlayerActionKind::DeclareBands`) checks it
    /// against this object's *bandmates'* own subtypes, not an opponent's
    /// battlefield — unlike landwalk, both sides of the check are combatants
    /// in the same declared band.
    pub bands_with_other_subtypes: Vec<Subtype>,
    /// CR 702.22b/c/m's supertype form ("bands with other Legends," i.e.
    /// Legendary): every applicable layer 6
    /// `EffectPrimitive::AddBandsWithOtherSupertype`, OR-accumulated (a
    /// bitset like `SupertypeSet` itself, unlike `bands_with_other_subtypes`'
    /// `Vec`). Same empty-base/always-effect-granted posture, checked
    /// alongside `bands_with_other_subtypes` rather than merged into it.
    pub bands_with_other_supertypes: crate::types::card::SupertypeSet,
    /// `None` for non-creatures (mirrors `CardDefinition`).
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    pub loyalty: Option<i32>,
    pub defense: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::card::{CardDefinition, ModeChoice, PTValue, ScriptReference, SupertypeSet as SupertypeSetT};
    use crate::types::counter::CounterSet;
    use crate::types::game::{GameState, RngState};
    use crate::types::ids::PlayerId;
    use crate::types::mana::ManaPool;
    use crate::types::object::VisibilityPolicy;
    use crate::types::player::PlayerState;
    use std::collections::HashMap;

    fn object(id: ObjectId, controller: PlayerId, card_types: CardTypeSet, zone: ZoneKind) -> GameObject {
        GameObject {
            object_id: id,
            lineage: crate::types::ids::ObjectLineage { previous: None },
            definition: CardDefinition {
                definition_id: 1,
                name: "Test Object".to_string(),
                mana_cost: ManaCost { symbols: Vec::new() },
                color_indicator: None,
                supertypes: SupertypeSetT::empty(),
                card_types,
                subtypes: Vec::new(),
                oracle_text: String::new(),
                power: Some(PTValue::Fixed(1)),
                toughness: Some(PTValue::Fixed(1)),
                loyalty: None,
                defense: None,
                script_ref: ScriptReference::ByKey("test".to_string()),
                keywords: KeywordSet::empty(),
                target_requirement: None,
                alternative_costs: Vec::new(),
                buyback_cost: None,
                effect: None,
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
            },
            copiable_override: None,
            owner: controller,
            controller,
            zone,
            tapped: false,
            flipped: false,
            face_down: false,
            phased_out: false,
            damage_marked: 0,
            deathtouched: false,
            counters: CounterSet::new(),
            attachments: Vec::new(),
            attached_to: None, attached_to_player: None,
            timestamp: SequenceNo(0),
            controlled_since_turn: 0,
            is_commander: false,
            is_token: false,
            visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
            copy_generation: 0,
            entered_via_escape: false,
        }
    }

    fn state_with(objects: Vec<GameObject>) -> GameState {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        GameState {
            players: vec![
                PlayerState {
                    player_id: p0,
                    life: 20,
                    mana_pool: ManaPool::default(),
                    counters: CounterSet::new(),
                    emblems: Vec::new(),
                    commander_damage_taken: Vec::new(),
                    eliminated: false,
                    drew_from_empty_library: false,
                    mulligans_taken: 0,
                    hand_kept: false,
                    commander_casts: std::collections::HashMap::new(),
                },
                PlayerState {
                    player_id: p1,
                    life: 20,
                    mana_pool: ManaPool::default(),
                    counters: CounterSet::new(),
                    emblems: Vec::new(),
                    commander_damage_taken: Vec::new(),
                    eliminated: false,
                    drew_from_empty_library: false,
                    mulligans_taken: 0,
                    hand_kept: false,
                    commander_casts: std::collections::HashMap::new(),
                },
            ],
            turn_order: vec![p0, p1],
            active_player: p0,
            turn_number: 1,
            phase: crate::types::turn::Phase::Main,
            step: crate::types::turn::Step::PreCombatMain,
            priority_player: p0,
            consecutive_passes: 0,
            lands_played_this_turn: HashMap::new(),
            combat: None,
            last_active_turn_number: HashMap::new(),
            objects: objects.into_iter().map(|o| (o.object_id, o)).collect(),
            zones: Vec::new(),
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            cost_modifiers: Vec::new(),
            hand_size_modifiers: Vec::new(),
            hand_reveal_modifiers: Vec::new(),
            draw_limit_modifiers: Vec::new(),
            land_play_modifiers: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState { seed: 1, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 1000,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
            linked_ability_generation: std::collections::HashMap::new(),
            pending_timing_bypass: None,
            pending_free_cast: None,
        }
    }

    fn replacement(source: ObjectId, filter: EventFilter, payload: ReplacementPayload) -> ReplacementEffect {
        ReplacementEffect {
            source,
            spec: ReplacementSpec::Primitive { filter, payload },
            consumption: ConsumptionPolicy::Repeatable,
        }
    }

    #[test]
    fn damage_dealt_matches_object_by_card_types_and_controller() {
        let p0 = PlayerId(0);
        let effect_source = ObjectId(0);
        let creature = ObjectId(1);
        let state = state_with(vec![
            object(effect_source, p0, CardTypeSet::ARTIFACT, ZoneKind::Battlefield),
            object(creature, p0, CardTypeSet::CREATURE, ZoneKind::Battlefield),
        ]);
        let effect = replacement(
            effect_source,
            EventFilter::DamageDealt {
                affected: EffectFilter {  card_types: Some(CardTypeSet::CREATURE), controller: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
            },
            ReplacementPayload::PreventDamage,
        );

        let matching =
            GameEventKind::DamageDealt { source: creature, target: Target::Object(creature), amount: 3, is_combat_damage: false };
        assert!(effect.matches(&matching, false, &state, &ScriptRuntime::new()));

        let land = ObjectId(2);
        let mut state2 = state;
        state2.objects.insert(land, object(land, p0, CardTypeSet::LAND, ZoneKind::Battlefield));
        let non_matching =
            GameEventKind::DamageDealt { source: land, target: Target::Object(land), amount: 3, is_combat_damage: false };
        assert!(!effect.matches(&non_matching, false, &state2, &ScriptRuntime::new()));
    }

    #[test]
    fn damage_dealt_to_player_matches_by_controller_only() {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let effect_source = ObjectId(0);
        // effect_source controlled by p0 - FilterController::You means
        // "matches damage dealt to p0" (the effect's own controller).
        let state = state_with(vec![object(effect_source, p0, CardTypeSet::ARTIFACT, ZoneKind::Battlefield)]);
        let effect = replacement(
            effect_source,
            EventFilter::DamageDealt {
                affected: EffectFilter {  card_types: None, controller: Some(FilterController::You), self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
            },
            ReplacementPayload::PreventDamage,
        );

        let to_p0 =
            GameEventKind::DamageDealt { source: effect_source, target: Target::Player(p0), amount: 3, is_combat_damage: false };
        assert!(effect.matches(&to_p0, false, &state, &ScriptRuntime::new()));
        let to_p1 =
            GameEventKind::DamageDealt { source: effect_source, target: Target::Player(p1), amount: 3, is_combat_damage: false };
        assert!(!effect.matches(&to_p1, false, &state, &ScriptRuntime::new()));
    }

    #[test]
    fn would_die_matches_self_only_and_ignores_other_zone_changes() {
        let p0 = PlayerId(0);
        let this = ObjectId(0);
        let other = ObjectId(1);
        let state = state_with(vec![
            object(this, p0, CardTypeSet::CREATURE, ZoneKind::Battlefield),
            object(other, p0, CardTypeSet::CREATURE, ZoneKind::Battlefield),
        ]);
        let effect = replacement(
            this,
            EventFilter::WouldDie {
                affected: EffectFilter {  card_types: None, controller: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
            },
            ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        );

        let own_death = GameEventKind::ZoneChange {
            object: this,
            from: ZoneKind::Battlefield,
            to: ZoneKind::Graveyard,
            last_known_info: None,
        };
        assert!(effect.matches(&own_death, true, &state, &ScriptRuntime::new()));

        let other_death = GameEventKind::ZoneChange {
            object: other,
            from: ZoneKind::Battlefield,
            to: ZoneKind::Graveyard,
            last_known_info: None,
        };
        assert!(!effect.matches(&other_death, true, &state, &ScriptRuntime::new()), "self_only must not match a different object's death");

        let non_death = GameEventKind::ZoneChange {
            object: this,
            from: ZoneKind::Hand,
            to: ZoneKind::Battlefield,
            last_known_info: None,
        };
        assert!(!effect.matches(&non_death, true, &state, &ScriptRuntime::new()), "only Battlefield -> Graveyard counts as \"would die\"");
    }

    #[test]
    fn would_move_zone_with_both_sides_wildcard_matches_any_zone_change() {
        let p0 = PlayerId(0);
        let this = ObjectId(0);
        let state = state_with(vec![object(this, p0, CardTypeSet::CREATURE, ZoneKind::Battlefield)]);
        let effect = replacement(
            this,
            EventFilter::WouldMoveZone {
                from: None,
                to: None,
                affected: EffectFilter {  card_types: None, controller: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
            },
            ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        );

        let library_to_hand = GameEventKind::ZoneChange { object: this, from: ZoneKind::Library, to: ZoneKind::Hand, last_known_info: None };
        assert!(effect.matches(&library_to_hand, false, &state, &ScriptRuntime::new()), "both-wildcard must match a draw-shaped move");

        let hand_to_graveyard = GameEventKind::ZoneChange { object: this, from: ZoneKind::Hand, to: ZoneKind::Graveyard, last_known_info: None };
        assert!(effect.matches(&hand_to_graveyard, false, &state, &ScriptRuntime::new()), "both-wildcard must match a discard-shaped move too");
    }

    #[test]
    fn would_move_zone_with_from_pinned_ignores_to() {
        let p0 = PlayerId(0);
        let this = ObjectId(0);
        let state = state_with(vec![object(this, p0, CardTypeSet::CREATURE, ZoneKind::Battlefield)]);
        let effect = replacement(
            this,
            EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Battlefield),
                to: None,
                affected: EffectFilter {  card_types: None, controller: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
            },
            ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        );

        let to_graveyard = GameEventKind::ZoneChange { object: this, from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, last_known_info: None };
        assert!(effect.matches(&to_graveyard, false, &state, &ScriptRuntime::new()));
        let to_hand = GameEventKind::ZoneChange { object: this, from: ZoneKind::Battlefield, to: ZoneKind::Hand, last_known_info: None };
        assert!(effect.matches(&to_hand, false, &state, &ScriptRuntime::new()), "to left wildcard - any destination off the battlefield matches");

        let from_hand = GameEventKind::ZoneChange { object: this, from: ZoneKind::Hand, to: ZoneKind::Graveyard, last_known_info: None };
        assert!(!effect.matches(&from_hand, false, &state, &ScriptRuntime::new()), "from is pinned to Battlefield - a Hand-sourced move must not match");
    }

    #[test]
    fn would_move_zone_with_both_sides_pinned_requires_an_exact_match() {
        let p0 = PlayerId(0);
        let this = ObjectId(0);
        let state = state_with(vec![object(this, p0, CardTypeSet::CREATURE, ZoneKind::Battlefield)]);
        let effect = replacement(
            this,
            EventFilter::WouldMoveZone {
                from: Some(ZoneKind::Battlefield),
                to: Some(ZoneKind::Exile),
                affected: EffectFilter {  card_types: None, controller: None, self_only: true, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, },
            },
            ReplacementPayload::ReplaceZoneDestination(ZoneKind::Graveyard),
        );

        let exact = GameEventKind::ZoneChange { object: this, from: ZoneKind::Battlefield, to: ZoneKind::Exile, last_known_info: None };
        assert!(effect.matches(&exact, false, &state, &ScriptRuntime::new()));

        let wrong_to = GameEventKind::ZoneChange { object: this, from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, last_known_info: None };
        assert!(!effect.matches(&wrong_to, false, &state, &ScriptRuntime::new()));
    }

    #[test]
    fn apply_prevent_damage_zeroes_amount_and_keeps_source_and_target() {
        let effect = replacement(
            ObjectId(0),
            EventFilter::DamageDealt { affected: EffectFilter {  card_types: None, controller: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, } },
            ReplacementPayload::PreventDamage,
        );
        let source = ObjectId(1);
        let target = Target::Object(ObjectId(2));
        let rewritten =
            effect.apply(GameEventKind::DamageDealt { source, target, amount: 5, is_combat_damage: false });
        assert!(matches!(
            rewritten,
            GameEventKind::DamageDealt { source: s, target: t, amount: 0, .. } if s == source && t == target
        ));
    }

    #[test]
    fn apply_redirect_damage_changes_target_only() {
        let new_target = Target::Player(PlayerId(1));
        let effect = replacement(
            ObjectId(0),
            EventFilter::DamageDealt { affected: EffectFilter {  card_types: None, controller: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, } },
            ReplacementPayload::RedirectDamage(new_target),
        );
        let source = ObjectId(1);
        let rewritten = effect.apply(GameEventKind::DamageDealt {
            source,
            target: Target::Player(PlayerId(0)),
            amount: 5,
            is_combat_damage: false,
        });
        assert!(matches!(
            rewritten,
            GameEventKind::DamageDealt { source: s, target, amount: 5, .. } if s == source && target == new_target
        ));
    }

    #[test]
    fn redirect_damage_matches_when_destination_still_a_live_creature() {
        let p0 = PlayerId(0);
        let effect_source = ObjectId(0);
        let redirect_dest = ObjectId(1);
        let state = state_with(vec![
            object(effect_source, p0, CardTypeSet::ARTIFACT, ZoneKind::Battlefield),
            object(redirect_dest, p0, CardTypeSet::CREATURE, ZoneKind::Battlefield),
        ]);
        let effect = replacement(
            effect_source,
            EventFilter::DamageDealt { affected: EffectFilter {  card_types: None, controller: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, } },
            ReplacementPayload::RedirectDamage(Target::Object(redirect_dest)),
        );
        let event =
            GameEventKind::DamageDealt { source: effect_source, target: Target::Player(p0), amount: 3, is_combat_damage: false };
        assert!(effect.matches(&event, false, &state, &ScriptRuntime::new()));
    }

    #[test]
    fn redirect_damage_fizzles_cr_614_9_when_destination_object_left_the_battlefield() {
        let p0 = PlayerId(0);
        let effect_source = ObjectId(0);
        let gone = ObjectId(99); // never inserted into state.objects - CR 400.7 retired id
        let state = state_with(vec![object(effect_source, p0, CardTypeSet::ARTIFACT, ZoneKind::Battlefield)]);
        let effect = replacement(
            effect_source,
            EventFilter::DamageDealt { affected: EffectFilter {  card_types: None, controller: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, } },
            ReplacementPayload::RedirectDamage(Target::Object(gone)),
        );
        let event =
            GameEventKind::DamageDealt { source: effect_source, target: Target::Player(p0), amount: 3, is_combat_damage: false };
        assert!(
            !effect.matches(&event, false, &state, &ScriptRuntime::new()),
            "CR 614.9: a redirection effect whose destination is no longer on the battlefield does nothing"
        );
    }

    #[test]
    fn redirect_damage_fizzles_cr_614_9_when_destination_stopped_being_a_valid_type() {
        let p0 = PlayerId(0);
        let effect_source = ObjectId(0);
        let land = ObjectId(1);
        let state = state_with(vec![
            object(effect_source, p0, CardTypeSet::ARTIFACT, ZoneKind::Battlefield),
            object(land, p0, CardTypeSet::LAND, ZoneKind::Battlefield),
        ]);
        let effect = replacement(
            effect_source,
            EventFilter::DamageDealt { affected: EffectFilter {  card_types: None, controller: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, } },
            ReplacementPayload::RedirectDamage(Target::Object(land)),
        );
        let event =
            GameEventKind::DamageDealt { source: effect_source, target: Target::Player(p0), amount: 3, is_combat_damage: false };
        assert!(
            !effect.matches(&event, false, &state, &ScriptRuntime::new()),
            "CR 614.9: a land is never a valid redirection destination (not a creature/planeswalker/battle)"
        );
    }

    #[test]
    fn redirect_damage_fizzles_cr_614_9_when_destination_player_has_left_the_game() {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let effect_source = ObjectId(0);
        let mut state = state_with(vec![object(effect_source, p0, CardTypeSet::ARTIFACT, ZoneKind::Battlefield)]);
        state.player_mut(p1).unwrap().eliminated = true;
        let effect = replacement(
            effect_source,
            EventFilter::DamageDealt { affected: EffectFilter {  card_types: None, controller: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, } },
            ReplacementPayload::RedirectDamage(Target::Player(p1)),
        );
        let event =
            GameEventKind::DamageDealt { source: effect_source, target: Target::Player(p0), amount: 3, is_combat_damage: false };
        assert!(
            !effect.matches(&event, false, &state, &ScriptRuntime::new()),
            "CR 614.9: redirecting to a player who has left the game does nothing"
        );
    }

    #[test]
    fn apply_replace_zone_destination_changes_to_only() {
        let effect = replacement(
            ObjectId(0),
            EventFilter::WouldDie { affected: EffectFilter {  card_types: None, controller: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, } },
            ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile),
        );
        let object_id = ObjectId(1);
        let rewritten = effect.apply(GameEventKind::ZoneChange {
            object: object_id,
            from: ZoneKind::Battlefield,
            to: ZoneKind::Graveyard,
            last_known_info: None,
        });
        assert!(matches!(
            rewritten,
            GameEventKind::ZoneChange { object: o, from: ZoneKind::Battlefield, to: ZoneKind::Exile, .. }
                if o == object_id
        ));
    }

    #[test]
    fn apply_with_mismatched_payload_and_event_falls_back_unchanged() {
        // PreventDamage payload paired with a ZoneChange event - not a
        // constructible pairing through any real builder, but Rust struct
        // literals could still build one; apply() must not panic.
        let effect = replacement(
            ObjectId(0),
            EventFilter::WouldDie { affected: EffectFilter {  card_types: None, controller: None, self_only: false, subtypes: None, excluded_subtypes: None, supertypes: None, excluded_supertypes: None, } },
            ReplacementPayload::PreventDamage,
        );
        let object_id = ObjectId(1);
        let original = GameEventKind::ZoneChange {
            object: object_id,
            from: ZoneKind::Battlefield,
            to: ZoneKind::Graveyard,
            last_known_info: None,
        };
        let rewritten = effect.apply(original);
        assert!(matches!(
            rewritten,
            GameEventKind::ZoneChange { object: o, to: ZoneKind::Graveyard, .. } if o == object_id
        ));
    }

    #[test]
    fn matches_effect_filter_checks_subtypes_and_excluded_subtypes() {
        let p0 = PlayerId(0);
        let source = ObjectId(0);
        let mut zombie = object(ObjectId(1), p0, CardTypeSet::CREATURE, ZoneKind::Battlefield);
        zombie.definition.subtypes = vec![Subtype("Zombie".to_string())];
        let mut wall = object(ObjectId(2), p0, CardTypeSet::CREATURE, ZoneKind::Battlefield);
        wall.definition.subtypes = vec![Subtype("Wall".to_string())];

        let subtypes_filter = EffectFilter {
            controller: None,
            card_types: None,
            subtypes: Some(vec![Subtype("Zombie".to_string())]),
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: None,
            self_only: false,
        };
        assert!(matches_effect_filter(&subtypes_filter, &zombie, source, p0));
        assert!(!matches_effect_filter(&subtypes_filter, &wall, source, p0));

        let excluded_filter = EffectFilter {
            controller: None,
            card_types: None,
            subtypes: None,
            excluded_subtypes: Some(vec![Subtype("Wall".to_string())]),
            supertypes: None,
            excluded_supertypes: None,
            self_only: false,
        };
        assert!(matches_effect_filter(&excluded_filter, &zombie, source, p0));
        assert!(!matches_effect_filter(&excluded_filter, &wall, source, p0));
    }

    #[test]
    fn matches_effect_filter_checks_supertypes_and_excluded_supertypes() {
        let p0 = PlayerId(0);
        let source = ObjectId(0);
        let mut legendary = object(ObjectId(1), p0, CardTypeSet::CREATURE, ZoneKind::Battlefield);
        legendary.definition.supertypes = SupertypeSetT::LEGENDARY;
        let plain = object(ObjectId(2), p0, CardTypeSet::CREATURE, ZoneKind::Battlefield);

        let supertypes_filter = EffectFilter {
            controller: None,
            card_types: None,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: Some(SupertypeSetT::LEGENDARY),
            excluded_supertypes: None,
            self_only: false,
        };
        assert!(matches_effect_filter(&supertypes_filter, &legendary, source, p0));
        assert!(!matches_effect_filter(&supertypes_filter, &plain, source, p0));

        let excluded_filter = EffectFilter {
            controller: None,
            card_types: None,
            subtypes: None,
            excluded_subtypes: None,
            supertypes: None,
            excluded_supertypes: Some(SupertypeSetT::LEGENDARY),
            self_only: false,
        };
        assert!(matches_effect_filter(&excluded_filter, &plain, source, p0));
        assert!(!matches_effect_filter(&excluded_filter, &legendary, source, p0));
    }
}
