use crate::{
    CardTypeSet, Subtype, SupertypeSet, ObjectId, PlayerId, SequenceNo, ColorSet, ManaCost,
    GameObject, GameState, GameEventKind, ScriptHandle, Target, ZoneKind,
};
use crate::types::ability::{CopyExceptions, InterveningIfSpec};
use crate::types::card::{CardDefinition, KeywordSet, PTValue};
use crate::types::cost::FilterController;
use crate::types::counter::CounterKind;
use crate::script::ScriptRuntime;

/// CR 612.6 "full text" payload: only the fields `EffectiveCharacteristics`
/// derives from a `CardDefinition` (name, mana cost, color, type line,
/// keywords, power/toughness). The layer query is pure and read-only, so it
/// can apply these plain fields but cannot re-run a script or register an
/// ability the way `BecomeCopy` and `ExchangeTextBoxes` do. A Volrath's
/// Shapeshifter effect therefore looks and fights like the copied creature
/// but gains none of its abilities. This follows from modeling CR 612.6 as a
/// live continuous effect that reuses `EffectDuration`'s expiry.
///
/// `PartialEq` is hand-written by name, because `ManaCost` and `PTValue`
/// don't derive it and `EffectPrimitive` only needs some impl.
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
    /// CR 105.2a, 202.2: the `color_indicator` or the mana cost, as in
    /// `CardDefinition::colors()`.
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
    /// Exists so `EffectPrimitive`'s derive has an impl to call. Compares by
    /// name only.
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Eq for FullTextOverride {}

/// CR 613.1 layers, with the layer 7 sublayers. `TypeChanging` (4),
/// `ColorChanging` (5), `AbilityAdding` (6), `PowerToughnessSetting` (7b),
/// `PowerToughnessModifying` (7c) and `PowerToughnessSwitching` (7e) have a
/// query path in `layers::effective_characteristics`. The others are named to
/// keep the layer list complete and ordered, but no effect can target them
/// and the query ignores them.
///
/// Layer 1 (CR 707, 613.1a) has no `ContinuousEffect` case, because a copy's
/// values freeze when applied (CR 707.2b, 707.2c) and a live query would
/// break that. `GameObject.copiable_override` holds the snapshot, written by
/// `AbilityEffect::BecomeCopy` or `TokenSpec::CopyOf` and read ahead of
/// `definition`.
/// TODO: CR 707.9 exceptions, and an enters-as-a-copy replacement (Clone)
///
/// Layer 2 (CR 108.4, 613.1): `layers::effective_controller` queries
/// `EffectPrimitive::GainControl`, where a later timestamp overrides
/// outright. No CR 613.8 dependency applies, since one `GainControl` never
/// depends on another. No gameplay site reads it yet: combat, targeting, cost
/// filters, `has_summoning_sickness` and `FilterSpec::matches` still read the
/// raw `GameObject.controller`.
/// TODO: migrate gameplay sites to `layers::effective_controller`
///
/// Layer 3 (CR 613.1c, 612) covers name setting only (CR 612.8,
/// `EffectPrimitive::SetName`). Word substitution (CR 612.2, 612.6, 612.7,
/// 612.9, 612.10) is not modeled, and `CardDefinition.oracle_text` is never
/// parsed. "Loses all abilities" (CR 613.1f) is layer 6 and clears the
/// `KeywordSet` through `RemoveAllAbilities`. A permanent's own
/// `RegisteredAbility` entries stay.
///
/// Layers 4 and 5 have no CR sublayers (CR 613.3), so every primitive in them
/// folds into one timestamp-ordered pass in
/// `layers::effective_characteristics`. Card types, subtypes and supertypes
/// share layer 4 (CR 613.1d), with supertypes as a separate field (CR 205.4b).
///
/// Dependencies (CR 613.6-613.8): `layers::order_by_dependency` reorders a
/// layer's effects around real dependencies, which today means `DoublePT`
/// against `ModifyPT` or `DoublePT` in layer 7c. Everything else, and a
/// dependency loop (CR 613.8b), stays in timestamp order. `depends_on`
/// resolves both sides through `layers::resolve_primitive`, so a `Script`
/// effect that decodes to those shapes takes part too. Cross-layer
/// dependency is out, because CR 613.8 is same-layer only and
/// `effective_characteristics` fixes the layer order.
/// TODO: a script that decides dynamically whether it depends on another effect; it needs partial layer state in `state_to_lua`
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

/// CR 702.14a: a landwalk grant's "[type]" beyond a bare land type. All
/// specified fields must hold at once against a candidate land, and `None` or
/// empty imposes no constraint on that axis. `subtypes` is a `Vec` so
/// "nonbasic landwalk" can be empty. See the `DeclareBlockers` enforcement in
/// `commands`.
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

/// The recognized shapes of a `ContinuousEffect`'s modification: a
/// Rust-native primitive needing no evaluation, as opposed to the script
/// fallback (like `ConditionSpec` and `AbilityEffect`). See `Layer` for the
/// layers with no primitive.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EffectPrimitive {
    /// Layer 3 (CR 612.8): sets the name outright, replacing earlier results
    /// (override, not additive, like `GainControl` and `SetPT`). Depends on no
    /// other effect (CR 613.8a), since a flat override depends only on
    /// timestamp. The only non-`Copy` `EffectPrimitive`; see
    /// `layers::resolve_primitive`.
    SetName(String),
    /// Layer 3 (CR 612.6, Volrath's Shapeshifter): another object's full text
    /// overrides name, mana cost, color, type line, keywords and
    /// power/toughness at once. A later `SetFullText` replaces an earlier one,
    /// and a later `SetName` still overrides just the name on top. Boxed to
    /// keep the other variants small. See `FullTextOverride` for why abilities
    /// are not covered.
    SetFullText(Box<FullTextOverride>),
    /// Layer 2 (CR 108.4, 613.1): the controller becomes this player,
    /// replacing earlier results (override, not additive). It never depends on
    /// another effect (CR 613.8a), only on the latest timestamp. See
    /// `layers::effective_controller`.
    GainControl(PlayerId),
    /// Layer 7b (CR 613.3): sets power and toughness exactly, replacing
    /// earlier results at a later timestamp.
    SetPT { power: i32, toughness: i32 },
    /// Layer 7c (CR 613.3): adds (or, for a negative value, subtracts) from
    /// power/toughness. Additive across every applicable effect in this
    /// layer, applied in timestamp order.
    ModifyPT { power: i32, toughness: i32 },
    /// Layer 7c (CR 613.3): doubles the current power and/or toughness
    /// (Berserk), per the `true` fields. Its result depends on the current
    /// value, so it always depends on a `ModifyPT` in the sublayer touching
    /// the same field (CR 613.8a). See `layers::order_by_dependency`.
    DoublePT { power: bool, toughness: bool },
    /// Layer 6 (CR 613.1): grants the listed keywords, as a union. See
    /// `RemoveKeyword` and `RemoveAllAbilities` for removal.
    AddKeyword(KeywordSet),
    /// Layer 6 (CR 613.1f): removes only the listed keywords. Unlike
    /// `RemoveAllAbilities`, it leaves protection and other layer-6 fields
    /// alone. It folds in at its own timestamp, so a later `AddKeyword`
    /// re-adds what it removed, and the reverse, with no CR 613.8a dependency.
    ///
    /// Wrong for "can attack as though it didn't have defender" (Assault
    /// Formation): the creature keeps the keyword and only the restriction is
    /// voided. Use `IgnoreKeyword`.
    RemoveKeyword(KeywordSet),
    /// Layer 6 (CR 613.1f, "as though it didn't have [keyword]"): the
    /// keywords stay in `EffectiveCharacteristics::keywords`, but the
    /// restriction each imposes is voided. Assault Formation's "can attack as
    /// though they didn't have defender" is `IgnoreKeyword(DEFENDER)`: the
    /// Wall still has defender for other cards, and CR 302.6b's summoning
    /// sickness still applies, with only CR 702.3b lifted. A union that
    /// accumulates into `EffectiveCharacteristics::ignored_keywords`, which
    /// `RemoveAllAbilities` clears.
    ///
    /// Only `DEFENDER` has an enforcement site (`commands::combat::declare`).
    /// The other bits restrict a blocker's legality against this object and
    /// not its own actions, which this primitive doesn't cover. The payload is
    /// a full `KeywordSet` so a later restriction bit can reuse it.
    IgnoreKeyword(KeywordSet),
    /// Layer 6 (CR 613.1, 702.16e): grants protection from the listed colors,
    /// as a union. A separate field from keywords; see
    /// `EffectiveCharacteristics::protection_colors`.
    AddProtectionColor(ColorSet),
    /// Layer 6 (CR 613.1, 702.16): grants protection from the listed card
    /// types, as a union. A separate field from `AddCardType`, since
    /// protection and "has this type" are unrelated characteristics
    /// (`EffectiveCharacteristics::protection_card_types`).
    AddProtectionCardType(CardTypeSet),
    /// Layer 6 (CR 613.1, 702.16): grants protection from one subtype
    /// ("protection from Dragons"). One quality per primitive, as CR 702.16g's
    /// "A and from B" is two registered abilities. It can't be a bitflag `Set`
    /// because `Subtype` is an open-ended string; see
    /// `EffectiveCharacteristics::protection_subtypes`.
    AddProtectionSubtype(Subtype),
    /// Layer 6 (CR 613.1, 702.16a): grants protection from one card name
    /// ("protection from Progenitus"). This primitive is the specification
    /// CR 702.16a requires for a name quality, so it never clashes with a
    /// subtype string. Matched against the source's effective name
    /// (`EffectiveCharacteristics::name`, after any `SetName`).
    AddProtectionName(String),
    /// Layer 6 (CR 613.1, 702.16j): protection from everything, regardless of
    /// the source's characteristics. No payload, just a bit
    /// (`EffectiveCharacteristics::protection_everything`). Repeats collapse
    /// to one bit, which is right since redundant instances do nothing
    /// (CR 702.16m).
    AddProtectionEverything,
    /// Layer 6 (CR 613.1, 702.16k): protection from a player's objects
    /// ("protection from an opponent"). Carries a relative `FilterController`
    /// (`You` or `Opponent`), the vocabulary `EffectFilter.controller` exposes
    /// to Lua, because the engine has no arbitrary-player targeting and a
    /// script is never handed a `PlayerId` at registration. Resolved against
    /// the protected object's controller at each enforcement site. `Any` is
    /// rejected at construction (`Game.Effects.AddProtectionPlayer`), since
    /// that is `AddProtectionEverything` (CR 702.16j).
    AddProtectionPlayer(FilterController),
    /// Layer 6 (CR 613.1, 702.14): grants unblockability while the defending
    /// player controls a permanent with the named subtype ("Island" for
    /// islandwalk). One subtype per primitive, so a card with two landwalks
    /// registers two static abilities. Always effect-granted, with no printed
    /// `CardDefinition` field, like every `AddProtection*`. Plain
    /// single-land-type form only; the card-type and supertype combinations
    /// are `AddLandwalkQuality`. See `EffectiveCharacteristics::landwalk_subtypes`
    /// and the `DeclareBlockers` check, which reads the defending player's
    /// battlefield.
    AddLandwalkSubtype(Subtype),
    /// Layer 6 (CR 613.1, 702.14a): the combination forms of landwalk, "the
    /// card type land plus any combination of land types, card types and
    /// supertypes": "artifact landwalk" (`card_types: Some(ARTIFACT)`),
    /// "nonbasic landwalk" (`excluded_supertypes: Some(BASIC)`) and "snow
    /// swampwalk" (`supertypes: Some(SNOW), subtypes: vec![Swamp]`). See
    /// `LandwalkQuality` for how the fields combine. Folded into
    /// `EffectiveCharacteristics::landwalk_qualities` and checked beside
    /// `landwalk_subtypes`, since the payloads differ.
    AddLandwalkQuality(LandwalkQuality),
    /// Layer 6 (CR 613.1, 702.22b, 702.22c): the second band-formation path
    /// for a named subtype ("bands with other Elves"). One subtype per
    /// primitive and always effect-granted, like `AddLandwalkSubtype`.
    /// `DeclareBands` reads `EffectiveCharacteristics::bands_with_other_subtypes`
    /// to accept a band whose members share the quality. The supertype form
    /// ("bands with other Legends") is `AddBandsWithOtherSupertype`, since a
    /// `Subtype` and a `SupertypeSet` differ in shape.
    AddBandsWithOther(Subtype),
    /// Layer 6 (CR 613.1, 702.22c, 702.22m): the supertype form of "bands with
    /// other [quality]", such as the Legends-block "bands with other
    /// Legends". `DeclareBands` and the chooser-flip check in
    /// `commands::combat::damage` accept any one match, as with the subtype
    /// form.
    AddBandsWithOtherSupertype(crate::types::card::SupertypeSet),
    /// Layer 6 (CR 613.1f, "loses all abilities"): resets the `keywords`,
    /// `ignored_keywords`, `protection_*`, `landwalk_subtypes` and
    /// `bands_with_other_subtypes` accumulators at this effect's position in
    /// `effective_characteristics`'s layer-6 fold. Protection counts as an
    /// ability. It isn't a flat override: a later `AddKeyword` or
    /// `AddProtection*` applies on top, which the CR 613.7 timestamp fold
    /// gives for free, and a strip-all never depends on an add (CR 613.8a).
    /// No payload. See `RemoveKeyword` for a scoped removal.
    ///
    /// Narrow on purpose: a permanent's custom `RegisteredAbility` entries are
    /// never filtered by continuous effects (`collect_triggers` and activation
    /// checks read `state.abilities` directly), and silencing them would need
    /// a layer-6 lookup through that whole call graph.
    /// TODO: silence `RegisteredAbility` entries for "loses all abilities" (CR 613.1f)
    RemoveAllAbilities,
    /// Layer 4 (CR 613.1): grants the listed card types as a union, never
    /// removing one. "Target land becomes a 3/3 Elemental creature in addition
    /// to its other types" is this plus a `SetPT` (layer 7b).
    AddCardType(CardTypeSet),
    /// Layer 4 (CR 613.1a, "becomes"): sets the card types outright,
    /// replacing earlier results (override, not additive, like `SetPT`). It
    /// does not stack with an earlier `AddCardType`, since both fold into one
    /// running `card_types` in timestamp order (see
    /// `layers::effective_characteristics`).
    /// TODO: an Instant or Sorcery keeps its type through a card-type overwrite (CR 205.1a)
    SetCardTypes(CardTypeSet),
    /// Layer 5 (CR 613.1): grants the listed colors as a union ("enchanted
    /// creature is also red").
    AddColor(ColorSet),
    /// Layer 5 (CR 613.1a, "becomes"): sets the colors outright, as
    /// `SetCardTypes` does for layer 4.
    SetColors(ColorSet),
    /// Layer 4 (CR 613.1d, 205.4b): grants the listed supertypes as a union,
    /// keeping the others ("target permanent becomes legendary"). Additive
    /// only, with no `SetSupertypes`, since no real card needs one.
    AddSupertype(SupertypeSet),
    /// Layer 4 (CR 613.1d, 205.1b): appends subtypes not already present,
    /// never removing one ("becomes an Elemental in addition to its other
    /// types"). The script is trusted, so CR 205.3d (no subtype that doesn't
    /// match a card type) is not validated.
    AddSubtype(Vec<Subtype>),
    /// Layer 7e (CR 613.4d): swaps power and toughness after 7b and 7c. A
    /// toggle with no fields, and two of them cancel (CR 613.4d's third
    /// example) because a swap is its own inverse.
    SwitchPT,
}

/// A `ContinuousEffect`'s modification: a recognized `EffectPrimitive`
/// evaluated with no script calls, or a `ScriptHandle` fallback for anything
/// else. A `Script` effect that cannot be resolved is skipped, not guessed, as
/// for CDA power/toughness.
#[derive(Clone, Debug, PartialEq)]
pub enum EffectPayload {
    Primitive(EffectPrimitive),
    Script(ScriptHandle),
}

/// Structural filter for which objects a `ContinuousEffect` affects, with a
/// Rust-native check for the common shapes and `AffectedFilter::Script` as
/// the catch-all, like `FilterSpec`. It has no `random`, since continuous
/// effects never pick one object, and no `extra` string.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EffectFilter { 
    /// `You`, `Opponent` or `Any`, resolved against the effect source's
    /// controller (CR 613), not a caller-supplied player.
    pub controller: Option<FilterController>,
    pub card_types: Option<CardTypeSet>,
    /// CR 205.4b: matches any listed subtype, as in
    /// `TargetFilter::Object::subtypes`. Checked against the raw
    /// `GameObject.definition.subtypes`, not `effective_characteristics`, like
    /// `card_types` in `matches_effect_filter`. `applies_to` runs inside
    /// `layers::applicable_effects_in_layer`, which computes
    /// `effective_characteristics`, so a layered read would recurse.
    pub subtypes: Option<Vec<Subtype>>,
    /// CR 601.2c, 115, 205.4b negated ("nonbasic lands", "creatures that
    /// aren't Walls"). Raw field, as with `subtypes`.
    pub excluded_subtypes: Option<Vec<Subtype>>,
    /// CR 205.4a: a bitset like `card_types`, matching the other supertype
    /// fields. Raw field, as with `subtypes`.
    pub supertypes: Option<SupertypeSet>,
    /// CR 601.2c, 115, 205.4a negated ("nonbasic lands"). Raw field, as with
    /// `subtypes`.
    pub excluded_supertypes: Option<SupertypeSet>,
    /// Only the effect's own source object.
    pub self_only: bool,
 }

/// A `ContinuousEffect`'s affected-object predicate, with the same
/// `Primitive` and `Script` split as `EffectPayload`.
///
/// `EnchantedOnly` is CR 303.4m's "enchanted [object]": the object
/// `ContinuousEffect::source` is attached to (`GameObject::attached_to`), as
/// in "Enchanted creature gets +1/+1". It is a variant and not an
/// `EffectFilter` field, because `EffectFilter` has 129 literal sites and no
/// `Default`. It does not combine with `self_only`, which pins the effect's
/// own source. It is fieldless and cannot combine with `controller` or
/// `card_types`, because no real card needs the combination.
///
/// `EnchantedPlayerControls` is CR 303.4m's "enchanted player" for a general
/// continuous effect (Curse of Vitality: "Creatures enchanted player controls
/// have lifelink"). It matches an object whose controller is the player
/// `source` is attached to via `attached_to_player`, and is fieldless and
/// exclusive with `Primitive` for the same reason as `EnchantedOnly`.
#[derive(Clone, Debug, PartialEq)]
pub enum AffectedFilter {
    Primitive(EffectFilter),
    Script(ScriptHandle),
    EnchantedOnly,
    EnchantedPlayerControls,
}

/// How long a continuous effect lasts, beyond "until end of turn" and
/// permanent: `UntilYourNextTurn`, `UntilEndOfCombat`, `WhileConditionHolds`
/// ("as long as you control a Forest", re-evaluated continuously) and
/// `Custom` for wording no structured case fits.
#[derive(Clone, Debug, PartialEq)]
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

/// A registered continuous effect. `affected` and `effect` use the
/// `Primitive`/`Script` split of `AffectedFilter` and `EffectPayload`, as
/// `ConditionSpec` and `AbilityEffect` do for abilities.
#[derive(Clone, PartialEq, Debug)]
pub struct ContinuousEffect {
    pub source: ObjectId,
    pub layer: Layer,
    pub affected: AffectedFilter,
    pub effect: EffectPayload,
    pub timestamp: SequenceNo,
    pub duration: EffectDuration,
    /// CR 611.2c: "applies only as long as X is true", re-evaluated on every
    /// `layers::applicable_effects_in_layer` call. `None` is unconditional.
    /// This, not `duration`, is the gate: nothing reads `duration` to decide
    /// whether an effect is active. It reuses `InterveningIfSpec` since both
    /// are a live board-state predicate returning a bool.
    pub condition: Option<InterveningIfSpec>,
}

/// Whether `object` matches `filter` relative to `source_id` and
/// `source_controller`, which resolve `self_only` and `controller`. Shared by
/// `ContinuousEffect::applies_to` and `AbilityEffect::DestroyAll` (CR 701.8),
/// which has no `ContinuousEffect`.
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
    if let Some(types) = filter.card_types
        && !object.definition.card_types.intersects(types) {
            return false;
        }
    if let Some(subtypes) = &filter.subtypes
        && !subtypes.iter().any(|s| object.definition.subtypes.contains(s)) {
            return false;
        }
    if let Some(subtypes) = &filter.excluded_subtypes
        && subtypes.iter().any(|s| object.definition.subtypes.contains(s)) {
            return false;
        }
    if let Some(types) = filter.supertypes
        && !object.definition.supertypes.intersects(types) {
            return false;
        }
    if let Some(types) = filter.excluded_supertypes
        && object.definition.supertypes.intersects(types) {
            return false;
        }
    true
}

impl ContinuousEffect {
    /// Whether `object` is in this effect's affected set. `Primitive` uses
    /// `matches_effect_filter`, with `source` resolving the controller and
    /// `self_only` identity. `EnchantedOnly` (CR 303.4m) reads
    /// `source.attached_to` and `EnchantedPlayerControls` reads
    /// `source.attached_to_player` against `object.controller`, so neither
    /// needs a `GameState` lookup. `Script` counts as not matching, not
    /// guessed, as `layers::effective_characteristics` does for an
    /// unresolvable CDA.
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

/// What a `ReplacementEffect` watches for, using the same `EffectFilter` as
/// `ContinuousEffect::affected` against an event's object or player. Two
/// shapes: damage prevention and redirection (CR 615) and "if this would die,
/// instead" (CR 614.13, 704.5g). Other event shapes are not modeled, and
/// there is no `Script` escape hatch here, since `ReplacementSpec` has one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EventFilter {
    /// Matches a `DamageDealt` whose `target` satisfies `affected`. A player
    /// is checked by `controller` only, and `card_types` or `self_only` never
    /// match one. An object is checked as in `ContinuousEffect::applies_to`.
    DamageDealt { affected: EffectFilter },
    /// Matches a `ZoneChange` of `object` from the battlefield to the
    /// graveyard (CR 700.4, "dies" from any cause) where `object` satisfies
    /// `affected`. `ReplacementPayload::Regenerate` pairs with this too but
    /// also needs the event tagged as destruction (CR 614.8); see
    /// `ReplacementEffect::matches` for why that is a parameter.
    WouldDie { affected: EffectFilter },
    /// Matches any `ZoneChange` of `object` from `from` to `to` (`None` is a
    /// wildcard) where `object` satisfies `affected`. The general form of
    /// `WouldDie`, for shapes it cannot express ("would be exiled", drawing,
    /// discarding, milling). A sibling variant so `WouldDie` sites stay
    /// untouched. Pairs only with `ReplacementPayload::ReplaceZoneDestination`,
    /// since `Regenerate`'s CR 614.8 meaning is tied to `WouldDie`'s shape.
    ///
    /// Every zone-change producer routes through
    /// `replacement::apply_replacements` except token zone-lock, which runs
    /// first. `(Library, Hand)` is reachable via `Game.Ops.DrawCards` and
    /// `Engine::continue_draw_cards`, and `(Hand, Graveyard)` via
    /// `Engine::submit_discard` (CR 514.1) and Sacrifice and Discard costs.
    /// Other pairs are correct but may be unreachable in gameplay.
    WouldMoveZone { from: Option<ZoneKind>, to: Option<ZoneKind>, affected: EffectFilter },
    /// CR 702.89a (Umbra armor): "if enchanted permanent would be destroyed".
    /// The `WouldDie` zone shape, but the affected object is always the
    /// source's `attached_to` target. Fieldless and a sibling variant, because
    /// `WouldDie` has 23 literal sites and no `Default`. Pairs only with
    /// `ReplacementPayload::UmbraArmor`.
    WouldDieEnchanted,
    /// CR 303.4m, 616 (Wheel of Sun and Moon): "if a card would be put into
    /// enchanted player's graveyard from anywhere". The `WouldMoveZone` shape
    /// with a wildcard `from` and `to: Graveyard`, but the affected side is
    /// whether the card's owner (graveyards are owner-keyed, CR 400.7) is the
    /// player the source is attached to. Fieldless and not gated on
    /// `is_destroy`, since the rule says "from anywhere". Pairs with
    /// `ReplacementPayload::ReplaceZoneDestination` as is, since "put on the
    /// bottom of its owner's library" is a destination substitution given
    /// `commit_zone_move`'s `push_bottom` convention.
    /// TODO: "revealed" for this effect; there is no per-viewer hidden-information boundary, since `VisibilityPolicy::is_visible_to` has no callers
    WouldEnterEnchantedPlayerGraveyard,
}

/// A `ReplacementEffect`'s rewrite (CR 614.1: a different event happens
/// instead), narrower than the rules text and matching `EventFilter`'s two
/// shapes. A payload is paired with its filter at construction, so applying
/// one to a mismatched event is unreachable, and `ReplacementEffect::apply`
/// returns the event unchanged.
#[derive(Clone, Debug, PartialEq)]
pub enum ReplacementPayload {
    /// CR 615: the matched damage doesn't happen. The event still fires with
    /// its amount zeroed, so downstream consumers see "damage was dealt,
    /// amount 0".
    PreventDamage,
    /// CR 614.9: the matched damage goes to this target instead.
    /// `ReplacementEffect::matches` rechecks on every application that the
    /// target is still valid (a battlefield creature, planeswalker or battle,
    /// or a player in the game). If not, the effect does nothing and the
    /// damage goes through.
    RedirectDamage(Target),
    /// CR 614.13-style "instead" zone-change substitution (e.g. "exile it
    /// instead of putting it into its owner's graveyard") — the matched
    /// `ZoneChange`'s `to` becomes this zone instead.
    ReplaceZoneDestination(ZoneKind),
    /// CR 614.8, 701.19: rewrites the matched destroy `ZoneChange` into a
    /// `Regenerated` event. This can't be a `ReplaceZoneDestination`, since
    /// the permanent changes no zone.
    Regenerate,
    /// CR 603.6d, 113.6h: the permanent enters tapped. `ZoneChange` has no
    /// `tapped` field to rewrite, so `ReplacementEffect::apply` returns the
    /// event unchanged. `replacement::apply_enters_battlefield_modifiers`
    /// mutates the constructed `GameObject` instead, because ETB modifiers
    /// don't fit the propose-rewrite-commit loop.
    EntersTapped,
    /// CR 614.1c, 122.6: the permanent enters with `amount` counters of
    /// `kind` (Fading, Modular, Graft: fixed printed numbers only). Sunburst
    /// derives both values at apply time (`EntersWithCountersSunburst`).
    /// Devour and Amplify each need a mid-ETB player choice and have their own
    /// pause states. Applied by `replacement::apply_enters_battlefield_modifiers`,
    /// not `apply()`.
    EntersWithCounters { kind: CounterKind, amount: i32 },
    /// CR 702.54a (Bloodthirst N): enters with `amount` counters of `kind`
    /// only if an opponent of its controller was dealt damage this turn, from
    /// any source. Answered from `state.ledger`; see
    /// `replacement::opponent_damaged_this_turn`.
    // TODO: Bloodthirst X, which counts the total damage dealt to opponents this turn (CR 702.54b)
    EntersWithCountersBloodthirst { kind: CounterKind, amount: i32 },
    /// CR 702.138c (Escape N): enters with `amount` counters of `kind` if it
    /// escaped, checked against `GameObject::entered_via_escape`.
    // TODO: the linked "when it enters this way" trigger (CR 702.138c)
    EntersWithCountersEscape { kind: CounterKind, amount: i32 },
    /// CR 702.44 (Sunburst): both `kind` and `amount` are derived at apply
    /// time. `amount` is the number of distinct colors of mana spent on the
    /// spell (CR 702.44b), zero if none or if the object did not enter as a
    /// resolving spell, read from `StackObject.costs_paid`. `kind` is
    /// `PlusOnePlusOne` if the printed `CardDefinition.card_types` include
    /// Creature and `Charge` otherwise, since CR 702.44a ignores
    /// type-changing effects. See `replacement::apply_enters_battlefield_modifiers`
    /// for where `colors_spent` comes from and why PlayLand and Search pass an
    /// empty set.
    EntersWithCountersSunburst,
    /// CR 702.82a (Devour N): enters with N +1/+1 counters per creature
    /// sacrificed as it enters. It needs a mid-ETB player choice, so
    /// `apply_enters_battlefield_modifiers` never applies it.
    /// `ResolveStackTop` checks `replacement::pending_devour_choice` instead,
    /// pauses on `AwaitingDevourChoice` and resumes in
    /// `Engine::submit_devour_choice`.
    ///
    /// `quality` is CR 702.82c's "Devour [quality]", checked against the
    /// effective subtypes (`layers::effective_characteristics`). That is safe
    /// because this runs once from `ResolveStackTop` and not inside the
    /// layer-2 recursion. `None` means any creature.
    // TODO: offer Devour for a Search-fetched creature; `PlayLand` and `Search` never check for it
    EntersWithCountersDevour { per_creature: i32, quality: Option<Subtype> },
    /// CR 702.38a (Amplify N): enters with N +1/+1 counters per hand card
    /// revealed that shares a creature type. Like Devour, it needs a mid-ETB
    /// player choice, so `ResolveStackTop` checks
    /// `replacement::pending_amplify_choice`, pauses on `AwaitingAmplifyChoice`
    /// and resumes in `Engine::submit_amplify_choice`. Revealing changes no
    /// zone, so the resume just folds a `CounterAdded` into `events_so_far`.
    ///
    /// "Shares a creature type" is raw subtype overlap
    /// (`EffectiveCharacteristics::subtypes`), since the engine has no
    /// creature-type category apart from other subtypes.
    // TODO: Amplify cannot reveal cards entering at the same time (CR 702.38a); unreachable today, since nothing enters several permanents from hand at once
    // TODO: multiple Amplify instances work separately (CR 702.38b); `pending_amplify_choice` matches only the first
    EntersWithCountersAmplify { per_card: i32 },
    /// CR 707.6, 614.12a (Clone): "you may have this creature enter as a copy
    /// of any creature on the battlefield". It needs a mid-ETB player choice,
    /// so `ResolveStackTop` checks `replacement::pending_copy_choice`, pauses
    /// on `AwaitingCopyChoice` and resumes in `Engine::submit_copy_choice`.
    ///
    /// A real choice reuses `AbilityEffect::BecomeCopy`'s machinery
    /// (`commands::purge_granted_abilities` and a fresh `run_card_script`
    /// against the chosen creature's snapshot, with the carried
    /// `CopyExceptions` applied first) and logs the same `BecameCopy` event.
    /// The resume always continues into `apply_enters_battlefield_modifiers`,
    /// with the real `colors_spent` carried through the pause. A Clone of a
    /// Fading, enters-tapped creature then enters tapped with fade counters,
    /// and a Clone of a Sunburst creature gets the real count.
    ///
    /// Only `ResolveStackTop`'s resolving-spell path checks this; `Search`
    /// does not. The resume chains into `apply_enters_battlefield_modifiers`
    /// only, not back into Devour or Amplify, which no Clone-family card has.
    /// Self-copy is not special-cased out.
    // TODO: Clone for a Search-fetched creature, and chaining into the Devour and Amplify pauses
    EntersAsCopy(CopyExceptions),
    /// CR 702.89a (Umbra armor): "instead remove all damage marked on
    /// [enchanted permanent] and destroy this Aura". Pairs only with
    /// `EventFilter::WouldDieEnchanted`. Rewrites the enchanted permanent's
    /// destroy into `UmbraArmorConsumed`, since it changes no zone (as with
    /// `Regenerate`). The Aura also has to leave, so `sba::commit_umbra_armor`
    /// makes `sba::commit_death_or_regeneration` return a `Vec`.
    UmbraArmor,
}

/// The `Primitive`/`Script` split used by every filter-shaped type here.
/// `Script` has no Lua builder. `Primitive` is built from Lua by
/// `Game.CreateReplacementEffect` (CR 701.19b), which supports only the
/// `WouldDie` and `Regenerate` pairing. The other pairings have no Lua entry
/// point.
#[derive(Clone, Debug, PartialEq)]
pub enum ReplacementSpec {
    Primitive { filter: EventFilter, payload: ReplacementPayload },
    Script(ScriptHandle),
}

#[derive(Clone, PartialEq, Debug)]
pub struct ReplacementEffect {
    pub source: ObjectId,
    pub spec: ReplacementSpec,
    pub consumption: ConsumptionPolicy,
}

impl ReplacementEffect {
    /// Whether `event` is one this effect would replace. For
    /// `ReplacementSpec::Primitive` it resolves `self.source`'s live
    /// controller (`layers::effective_controller`, CR 108.4), so a
    /// control-changed source's "you" and "opponent" read correctly, and the
    /// event's object or player via `state`. `Script` counts as not matching.
    /// There is no recursion risk: replacement effects are a separate list
    /// that `effective_controller` never consults.
    ///
    /// `is_destroy` says the caller proposes `event` as genuine destruction
    /// (CR 614.8), not another battlefield-to-graveyard move such as zero
    /// toughness or the world rule (CR 704.5f, 704.5k). Only
    /// `sba::resolve_deaths` sets it `true`. It is a parameter and not an
    /// event field, to avoid touching ~60 `ZoneChange` construction sites. It
    /// matters only for `WouldDie` with `Regenerate`, and a non-destroy
    /// `WouldDie` effect still matches (CR 700.4).
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
                    // CR 614.9: a redirect whose destination left the battlefield, stopped being a creature, planeswalker or battle, or is a player who left the game never matches, so the original damage proceeds.
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
                    // CR 701.19c: rechecked here, not only at shield creation. A `Repeatable` shield (CR 701.19b) is registered once and must re-gate on every destroy attempt, since CANT_BE_REGENERATED can change later. A `OneShot` shield (CR 701.19a) is also skipped at creation as a shortcut, but this check is authoritative.
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
                // CR 702.89a: only genuine destruction, the same `is_destroy` gate as the `Regenerate` arm.
                is_destroy && state.object(self.source).and_then(|o| o.attached_to) == Some(*object)
            }
            (
                EventFilter::WouldEnterEnchantedPlayerGraveyard,
                GameEventKind::ZoneChange { object, to: ZoneKind::Graveyard, .. },
            ) => {
                // CR 400.7: graveyards are owner-keyed, so check `object`'s owner, not its controller.
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

    /// Rewrites `event` per this effect's payload, once `matches` confirmed it
    /// applies. A mismatched pairing falls back to returning the event
    /// unchanged.
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

/// CR 614.9: the destination must still be valid, checked fresh on every
/// `RedirectDamage` application, since the world can change after the effect
/// is created. Mirrors the `AnyTarget` arm of `commands::target_is_legal`
/// (CR 115.4) without calling it, because that also checks protection, which
/// doesn't matter when CR 614.9 fizzles only on a missing destination, and it
/// lives a layer above `types`.
fn redirect_target_still_valid(target: Target, state: &GameState, script_runtime: &ScriptRuntime) -> bool {
    match target {
        // `state.player()` keeps eliminated players, so check the flag for CR 614.9's "left the game".
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
        // CR 614.9: an ability is never a legal redirection destination.
        Target::Ability(_) => false,
    }
}

/// Shared by `ReplacementEffect::matches`'s `EventFilter` arms: the
/// controller, card type and `self_only` check of
/// `ContinuousEffect::applies_to`, generalized over `Target` since an event's
/// slot can be a player. `effect_source` resolves the controller for `You`
/// and `Opponent` and, for `self_only`, is the identity `target` must match
/// (CR 614.13: "if this would die").
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
                // A player has no card types, subtypes or supertypes and is never the effect's source, so a positive filter never matches.
                // A negated `excluded_*` filter passes vacuously.
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
            if let Some(types) = filter.card_types
                && !object.definition.card_types.intersects(types) {
                    return false;
                }
            // CR 205.4b: raw field, not `effective_characteristics`, as in `matches_effect_filter`. This is a near-duplicate with a layers-aware controller and an event-shaped `Target`, so the check is repeated.
            if let Some(subtypes) = &filter.subtypes
                && !subtypes.iter().any(|s| object.definition.subtypes.contains(s)) {
                    return false;
                }
            if let Some(subtypes) = &filter.excluded_subtypes
                && subtypes.iter().any(|s| object.definition.subtypes.contains(s)) {
                    return false;
                }
            if let Some(types) = filter.supertypes
                && !object.definition.supertypes.intersects(types) {
                    return false;
                }
            if let Some(types) = filter.excluded_supertypes
                && object.definition.supertypes.intersects(types) {
                    return false;
                }
            true
        }
        // Never reached in practice: call sites pass a `DamageDealt` target (never an ability, CR 614.9) or a `Target::Object`.
        Target::Ability(_) => false,
    }
}

/// The live characteristics of an object, computed on demand by the layer
/// engine from `CardDefinition` base values and every applicable
/// `ContinuousEffect`. Never stored on `GameObject`, so reordering and
/// dependency changes always show.
#[derive(Clone, PartialEq, Debug)]
pub struct EffectiveCharacteristics {
    pub name: String,
    pub mana_cost: ManaCost,
    /// The base color (`color_indicator`) folded against layer 5 `AddColor`
    /// (union) and `SetColors` (overwrite) in timestamp order.
    pub colors: ColorSet,
    /// `CardDefinition.card_types` folded against layer 4 `AddCardType` (union)
    /// and `SetCardTypes` (overwrite) in timestamp order.
    pub card_types: CardTypeSet,
    /// `CardDefinition.supertypes` unioned with layer 4 `AddSupertype`. No
    /// layer removes a supertype (CR 205.4b).
    pub supertypes: SupertypeSet,
    /// `CardDefinition.subtypes` extended with layer 4 `AddSubtype`, skipping
    /// duplicates. No layer removes a subtype.
    pub subtypes: Vec<Subtype>,
    /// `CardDefinition.keywords` unioned with layer 6 `AddKeyword`, and reset
    /// by `RemoveAllAbilities` (CR 613.1f).
    pub keywords: KeywordSet,
    /// CR 613.1f: keywords whose restriction is voided, unioned from layer 6
    /// `IgnoreKeyword` and reset by `RemoveAllAbilities`. Separate from
    /// `keywords`, where the bit stays set (not a `RemoveKeyword`; see Assault
    /// Formation there). A restriction applies only when the keyword is in
    /// `keywords` and absent here.
    pub ignored_keywords: KeywordSet,
    /// CR 702.16e: colors this object has protection from, unioned from layer
    /// 6 `AddProtectionColor`. A separate field from `keywords`, because
    /// protection carries which colors. `CardDefinition` has no
    /// printed-protection field, so every protection ability is
    /// effect-granted. Enforced for damage (`apply_replacements`), targeting
    /// (`target_is_legal`) and blocking (`DeclareBlockers`).
    pub protection_colors: ColorSet,
    /// CR 702.16: card types this object has protection from, from
    /// `AddProtectionCardType`. Its own field for the reason
    /// `protection_colors` gives, and the same three call sites check it.
    pub protection_card_types: CardTypeSet,
    /// CR 702.16: subtypes this object has protection from, from
    /// `AddProtectionSubtype`. A `Vec` because `Subtype` is an open-ended
    /// string. The three call sites check color, card type and subtype as an
    /// OR.
    pub protection_subtypes: Vec<Subtype>,
    /// CR 702.16a: card names this object has protection from, from
    /// `AddProtectionName`. A `Vec` because "protection from A and from B" is
    /// several registered abilities (CR 702.16g). Matched by exact equality
    /// against the source's effective name.
    pub protection_names: Vec<String>,
    /// CR 702.16j: "protection from everything", set by
    /// `AddProtectionEverything`. An unconditional flag that applies
    /// regardless of the source's characteristics, so the three call sites
    /// check it first, before any source lookup.
    pub protection_everything: bool,
    /// CR 702.16k: relative players (`You`, `Opponent`) this object has
    /// protection from, from `AddProtectionPlayer`. A `Vec` for the CR 702.16g
    /// reason. Matching needs this object's own controller as a reference, so
    /// each of the three call sites resolves it against the incoming source's
    /// raw `GameObject.controller`, not here.
    pub protection_players: Vec<FilterController>,
    /// CR 702.14: land subtypes that make this object unblockable while the
    /// defending player controls a matching land, from `AddLandwalkSubtype`. A
    /// `Vec` for the `protection_subtypes` reason, and always effect-granted.
    /// `commands::DeclareBlockers` reads it on the attacker but checks the
    /// defending player's battlefield (CR 702.14c), not the blocker's
    /// characteristics.
    pub landwalk_subtypes: Vec<Subtype>,
    /// CR 702.14a: the combination forms ("artifact landwalk", "nonbasic
    /// landwalk", "snow swampwalk") from `AddLandwalkQuality`. Checked apart
    /// from `landwalk_subtypes`, since the payloads differ.
    pub landwalk_qualities: Vec<LandwalkQuality>,
    /// CR 702.22b, 702.22c: subtypes this object can band with under the
    /// second band-formation path, from `AddBandsWithOther`. Shaped like
    /// `landwalk_subtypes`. `DeclareBands` checks it against the bandmates'
    /// subtypes, since both sides are in the same band.
    pub bands_with_other_subtypes: Vec<Subtype>,
    /// CR 702.22b, 702.22c, 702.22m: the supertype form ("bands with other
    /// Legends") from `AddBandsWithOtherSupertype`, OR-accumulated as a
    /// bitset. Checked beside `bands_with_other_subtypes`, not merged into it.
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
            script_ability_ids: std::collections::HashSet::new(),
            ability_function_zones: std::collections::HashMap::new(),
            off_battlefield_scripts_run: std::collections::HashSet::new(),
            linked_exiles: Vec::new(),
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
        // `You` is relative to the effect source's controller, p0.
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
        // PreventDamage on a ZoneChange: no real builder makes this pairing, but a struct literal can, and apply() must not panic.
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
