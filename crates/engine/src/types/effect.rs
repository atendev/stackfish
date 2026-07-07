use crate::{CardTypeSet, Subtype, SupertypeSet, ObjectId, SequenceNo, ColorSet, ManaCost, GameObject, GameEventKind, ScriptHandle};
use crate::types::card::KeywordSet;
use crate::types::cost::FilterController;

/// CR 613.1 layers, with sublayers for layer 7 (power/toughness).
/// `TypeChanging` (4), `ColorChanging` (5), `AbilityAdding` (6),
/// `PowerToughnessSetting` (7b), and `PowerToughnessModifying` (7c) have a
/// real query/apply path — see `layers::effective_characteristics`. The rest
/// are named here (so the CR 613.1 layer list is complete and in the right
/// relative order) but nothing in `state.continuous_effects` can be built
/// against them yet and the query doesn't look for them:
///
/// TODO(layer 1, CopyEffects): needs real copy-object semantics (CR 706 —
/// becoming a copy of another object/printed values), which nothing in this
/// engine models at all yet (no "copy" command, no copiable-values snapshot
/// distinct from `CardDefinition`). Blocks any "create a token that's a copy
/// of X" / clone effect.
///
/// TODO(layer 2, ControlChangingEffects): needs a real control-transfer
/// mechanism (CR 108.4/613.1 layer 2) — `GameObject.controller` exists and
/// is mutated by nothing today except object creation. Blocks any "gain
/// control of target permanent" effect.
///
/// TODO(layer 3, TextChanging): needs some representation of "this object's
/// rules text is treated as different text" (CR 613.1 layer 3, e.g.
/// "removes all abilities" or text-box swaps) — no such representation
/// exists; `CardDefinition.oracle_text` is a plain immutable `String`, not a
/// live/rewritable characteristic.
///
/// Layers 4/5 (CR 613.1) don't have CR-defined sublayers the way layer 7
/// does (CR 613.3 only lettered-sublayers layers 3, 4, 6, and 7 — and layer
/// 4's own lettered sublayers are about control-changing-vs-not, not
/// modeled here) — `AddCardType`/`AddColor` below are each a single
/// primitive per layer, which is faithful to the rule, not a simplification
/// of it. Narrowed to additive-only this slice (CR 305.7/205's "gains/loses
/// types"-style text, in the common "in addition to its other types" shape)
/// — a full overwrite ("loses all other types/colors") isn't modeled; see
/// `EffectPrimitive`'s own TODO for that.
///
/// TODO(dependency system, CR 613.6-613.8): even once more layers exist,
/// `layers::effective_characteristics` only sorts effects *within* a layer
/// by `timestamp` — it does not detect or honor *dependency* between
/// effects (an effect whose own text or amount depends on another effect's
/// current characteristics, e.g. two P/T-doubling effects where the order
/// they're applied in changes the final number, or an effect that only
/// applies "if [some other effect] is applying to this object"). CR 613.8's
/// actual rule requires inspecting what each effect's calculation depends on
/// and reordering around cycles arbitrarily (picked by the affected
/// player/APNAP when genuinely undecidable) — this is a real, separate rules
/// problem, not a sort-stability tweak, and needs its own design pass before
/// attempting it. Timestamp-only order is correct for the large majority of
/// simple static effects (this slice's whole test surface); it will give
/// the *wrong* answer the moment two effects on the board actually depend on
/// each other. Flagging here rather than silently shipping a subtly-wrong
/// dependency model.
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
/// TODO(overwrite shapes): `SetCardTypes(CardTypeSet)`/`SetColors(ColorSet)`
/// — the full "loses all other types/colors" overwrite (mirrors `SetPT`'s
/// override semantics for layer 7b) — aren't modeled yet. `AddCardType`/
/// `AddColor` below only cover the additive "in addition to its other
/// types/colors" shape, deferred per the pre-slice scoping discussion (see
/// docs/implementation.md).
///
/// TODO(supertypes/subtypes): gaining a supertype (e.g. "becomes
/// legendary") or a creature subtype (e.g. "becomes an Elemental in
/// addition to its other types") isn't modeled — `EffectiveCharacteristics.
/// supertypes`/`.subtypes` stay a straight `CardDefinition` copy. Natural
/// next extension of the same `AddCardType` pattern once needed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EffectPrimitive {
    /// Layer 7b (CR 613.3): sets power/toughness to an exact value,
    /// overriding whatever came before this layer (not additive — a later
    /// `SetPT` at a later timestamp replaces an earlier one's result
    /// entirely, it doesn't stack on top of it).
    SetPT { power: i32, toughness: i32 },
    /// Layer 7c (CR 613.3): adds (or, for a negative value, subtracts) from
    /// power/toughness. Additive across every applicable effect in this
    /// layer, applied in timestamp order.
    ModifyPT { power: i32, toughness: i32 },
    /// Layer 6 (CR 613.1): grants the listed keyword(s) in addition to
    /// whatever the object already has. Union, never removes a keyword —
    /// "loses all abilities" (also layer 6, but a text-removal shape) isn't
    /// modeled; see `Layer::TextChanging`'s TODO.
    AddKeyword(KeywordSet),
    /// Layer 4 (CR 613.1, general "gains types" text): grants the listed
    /// card type(s) in addition to whatever the object already has — e.g.
    /// "target land becomes a 3/3 Elemental creature in addition to its
    /// other types" is this primitive (for the type-changing half) plus a
    /// `SetPT` (layer 7b) for the 3/3. Union, never removes a type — see
    /// this enum's own overwrite TODO above.
    AddCardType(CardTypeSet),
    /// Layer 5 (CR 613.1, general "gains colors" text): grants the listed
    /// color(s) in addition to whatever the object already has — e.g.
    /// "enchanted creature is also red". Union, never removes a color.
    AddColor(ColorSet),
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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EffectFilter {
    /// `You`/`Opponent`/`Any`, resolved against the effect's *source's*
    /// controller (CR 613's "creatures you control" always means the
    /// controller of the permanent generating the effect, not some other
    /// player) — not a parameter the caller supplies separately.
    pub controller: Option<FilterController>,
    pub card_types: Option<CardTypeSet>,
    /// "Just this permanent" (e.g. an Aura's own "enchanted creature gets
    /// +2/+2", where the affected object *is* the effect's own attachment
    /// target — modeled here as "the effect's source" for a first cut, since
    /// aura-attachment as its own concept doesn't exist in this engine yet).
    pub self_only: bool,
}

/// A `ContinuousEffect`'s affected-object predicate — same `Primitive`/
/// `Script` split as `EffectPayload` above, same "no card can construct
/// `Script` yet, exists for shape stability" caveat.
#[derive(Clone, Debug)]
pub enum AffectedFilter {
    Primitive(EffectFilter),
    Script(ScriptHandle),
}

/// Not exhaustive in the original draft — MTG has more duration shapes than
/// "until end of turn / permanent". Adding: `UntilYourNextTurn` (e.g. many
/// fight/pump effects), `UntilEndOfCombat`, `WhileConditionHolds` (e.g. "as
/// long as you control a Forest" — re-evaluated continuously, not a fixed
/// end point), and `Custom` as a catch-all for unusual oracle wording that
/// doesn't fit a structured case yet.
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
}

impl ContinuousEffect {
    /// Whether `object` is in this effect's affected set. Real body for
    /// `AffectedFilter::Primitive` — zero further evaluation needed, `source`
    /// only used to resolve `self.source`'s current controller (for
    /// `FilterController::You`/`Opponent`) and identity (for `self_only`).
    /// `Script` isn't evaluable yet (no card can construct one — see
    /// `AffectedFilter`'s doc comment) — treated as "doesn't match" rather
    /// than guessed, same stance `layers::effective_characteristics` takes
    /// for an unresolvable CDA.
    pub fn applies_to(&self, object: &GameObject, source: &GameObject) -> bool {
        let filter = match &self.affected {
            AffectedFilter::Primitive(filter) => filter,
            AffectedFilter::Script(_) => return false,
        };
        if filter.self_only && object.object_id != self.source {
            return false;
        }
        if let Some(controller_filter) = filter.controller {
            let matches_controller = match controller_filter {
                FilterController::You => object.controller == source.controller,
                FilterController::Opponent => object.controller != source.controller,
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
        true
    }
}

/// Consumption policy for a replacement/prevention effect (CR 614, 615).
pub enum ConsumptionPolicy {
    OneShot,
    Repeatable,
    /// e.g. damage prevention shields with a counter budget.
    LimitedUses(u32),
}

pub struct ReplacementEffect {
    pub source: ObjectId,
    /// `ScriptHandle` into the card's loaded Lua script — same shape
    /// `ContinuousEffect::affected` had before this slice's `Primitive`/
    /// `Script` split; replacement effects haven't been given that same
    /// treatment yet (out of this slice's scope, see docs/implementation.md's
    /// slice backlog item 5).
    pub event_pattern: ScriptHandle,
    pub consumption: ConsumptionPolicy,
}

impl ReplacementEffect {
    pub fn matches(&self, event: &GameEventKind) -> bool {
        // TODO: same shape of gap ContinuousEffect::applies_to had before
        // this slice - needs a &ScriptRuntime to invoke self.event_pattern
        // against event. Called from replacement::apply_replacements
        // (replacement.rs) once that wiring exists.
        todo!()
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
    /// Base (`color_indicator`, see `layers.rs`'s own stopgap note on that)
    /// unioned with every applicable layer 5 `EffectPrimitive::AddColor`.
    /// No layer removes a color yet (see `EffectPrimitive`'s overwrite
    /// TODO).
    pub colors: ColorSet,
    /// Base (`CardDefinition.card_types`) unioned with every applicable
    /// layer 4 `EffectPrimitive::AddCardType`. No layer removes a type yet
    /// (see `EffectPrimitive`'s overwrite TODO).
    pub card_types: CardTypeSet,
    /// Straight `CardDefinition` copy — no layer touches supertypes yet
    /// (see `EffectPrimitive`'s supertypes/subtypes TODO).
    pub supertypes: SupertypeSet,
    /// Straight `CardDefinition` copy — no layer touches subtypes yet (see
    /// `EffectPrimitive`'s supertypes/subtypes TODO).
    pub subtypes: Vec<Subtype>,
    /// Base (`CardDefinition.keywords`) unioned with every applicable layer
    /// 6 `EffectPrimitive::AddKeyword`. No layer removes a keyword yet
    /// (see `Layer::TextChanging`'s TODO).
    pub keywords: KeywordSet,
    /// `None` for non-creatures (mirrors `CardDefinition`).
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    pub loyalty: Option<i32>,
    pub defense: Option<i32>,
}
