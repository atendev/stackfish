use crate::{
    CardTypeSet, Subtype, SupertypeSet, ObjectId, PlayerId, SequenceNo, ColorSet, ManaCost,
    GameObject, GameState, GameEventKind, ScriptHandle, Target, ZoneKind,
};
use crate::types::ability::InterveningIfSpec;
use crate::types::card::KeywordSet;
use crate::types::cost::FilterController;
use crate::script::ScriptRuntime;

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
/// modeled here) — `AddCardType`/`AddColor` below are each a single
/// primitive per layer, which is faithful to the rule, not a simplification
/// of it. Narrowed to additive-only this slice (CR 305.7/205's "gains/loses
/// types"-style text, in the common "in addition to its other types" shape)
/// — a full overwrite ("loses all other types/colors") isn't modeled; see
/// `EffectPrimitive`'s own TODO for that.
///
/// Dependency system (CR 613.6-613.8): `layers::order_by_dependency`
/// reorders a layer's effects around real CR 613.8a dependency (today, only
/// `EffectPrimitive::DoublePT` vs. `ModifyPT`/`DoublePT` in layer 7c actually
/// produces one — see `DoublePT`'s own doc comment for why no other
/// primitive can), falling back to plain timestamp order for everything
/// else and for dependency loops (CR 613.8b's last sentence). Two real
/// gaps still open, not silently narrowed:
/// - `EffectPayload::Script`/`AffectedFilter::Script` effects are always
///   treated as independent (never depend on anything, nothing depends on
///   them) — a script-declared dependency would need the script bridge to
///   expose partial/in-progress layer state mid-computation, which
///   `state_to_lua`'s snapshot deliberately doesn't do yet.
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
    /// its own — see `RemoveAllAbilities` below for the strip shape.
    AddKeyword(KeywordSet),
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
    /// `initial_effect(c)` is never handed a `PlayerId` at registration time
    /// to name one with anyway. Resolved relative to the *protected
    /// object's own* controller at each enforcement site (not the
    /// continuous effect's source — the two coincide for every `self_only`
    /// grant this engine can build, same as `EffectFilter`'s own "You"
    /// resolution). `FilterController::Any` is rejected at construction
    /// time (`script.rs`'s `Game.Effects.AddProtectionPlayer`) — "protection
    /// from any player" isn't a CR 702.16k quality, that's what
    /// `AddProtectionEverything` (702.16j) already covers.
    AddProtectionPlayer(FilterController),
    /// Layer 6 (CR 613.1f, ability-removing effects — "~ loses all
    /// abilities"): resets the running `keywords`, `protection_colors`,
    /// `protection_card_types`, `protection_subtypes`, `protection_names`,
    /// `protection_everything`, *and* `protection_players` accumulators to
    /// empty/false at this effect's position in `effective_characteristics`'s
    /// layer-6 fold — protection is an ability too, regardless of which
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
    /// clears everything accumulated so far, there's no narrower "loses
    /// these specific keywords" shape here.
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
    /// `SetPT` (layer 7b) for the 3/3. Union, never removes a type — see
    /// this enum's own overwrite TODO above.
    AddCardType(CardTypeSet),
    /// Layer 5 (CR 613.1, general "gains colors" text): grants the listed
    /// color(s) in addition to whatever the object already has — e.g.
    /// "enchanted creature is also red". Union, never removes a color.
    AddColor(ColorSet),
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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
#[derive(Clone, Copy, Debug)]
pub enum ReplacementPayload {
    /// CR 615: the matched damage doesn't happen at all — the event still
    /// fires (so anything downstream that cares "damage was dealt, amount
    /// 0" sees a real event), just with its amount zeroed.
    PreventDamage,
    /// CR 614.13-style redirection — the matched damage is dealt to this
    /// target instead of its original one.
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
            (_, other) => other,
        }
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
            if filter.card_types.is_some() || filter.self_only {
                // A player has no card types and can't be "this effect's own
                // source" - both are real, if unusual, non-matches rather
                // than something to guess past.
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
            true
        }
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
    /// 6 `EffectPrimitive::AddKeyword`, reset to empty by an intervening
    /// `RemoveAllAbilities` (CR 613.1f "loses all abilities") — see that
    /// primitive's own doc comment.
    pub keywords: KeywordSet,
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
    /// `None` for non-creatures (mirrors `CardDefinition`).
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    pub loyalty: Option<i32>,
    pub defense: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::card::{CardDefinition, PTValue, ScriptReference, SupertypeSet as SupertypeSetT};
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
                effect: None,
                modes: Vec::new(),
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
            attached_to: None,
            timestamp: SequenceNo(0),
            controlled_since_turn: 0,
            is_commander: false,
            is_token: false,
            visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
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
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState { seed: 1, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 1000,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
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
                affected: EffectFilter { card_types: Some(CardTypeSet::CREATURE), controller: None, self_only: false },
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
                affected: EffectFilter { card_types: None, controller: Some(FilterController::You), self_only: false },
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
                affected: EffectFilter { card_types: None, controller: None, self_only: true },
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
    fn apply_prevent_damage_zeroes_amount_and_keeps_source_and_target() {
        let effect = replacement(
            ObjectId(0),
            EventFilter::DamageDealt { affected: EffectFilter { card_types: None, controller: None, self_only: false } },
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
            EventFilter::DamageDealt { affected: EffectFilter { card_types: None, controller: None, self_only: false } },
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
    fn apply_replace_zone_destination_changes_to_only() {
        let effect = replacement(
            ObjectId(0),
            EventFilter::WouldDie { affected: EffectFilter { card_types: None, controller: None, self_only: false } },
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
            EventFilter::WouldDie { affected: EffectFilter { card_types: None, controller: None, self_only: false } },
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
}
