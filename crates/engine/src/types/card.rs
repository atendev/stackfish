use bitflags::bitflags;

use crate::types::ability::AbilityEffect;
use crate::types::cost::AdditionalCost;
use crate::types::counter::{CounterKind, CounterSet};
use crate::types::mana::{ColorSet, ManaCost};

/// Printed/base value for power, toughness, loyalty, or defense. `*`-style
/// values (and `* + 1` etc — the modifier is folded into whatever number the
/// characteristic-defining ability script returns, there's no separate
/// "base star plus offset" case to represent) are characteristic-defining
/// abilities: the number isn't known until the script callback runs against
/// current game state, so it can't be stored as a plain `i32`.
///
/// `CharacteristicDefining.script` mirrors `CardDefinition.script_ref`'s own
/// `Inline`/`ByKey` split (CR 604.3, 208.2a) rather than a pre-loaded
/// `ScriptHandle` — this type is meant to stay plain, runtime-independent
/// data, same as `CardDefinition` itself. `None` means no script was
/// supplied (nothing can resolve the number - `layers::effective_characteristics`
/// treats it exactly like an unresolvable script, "skip rather than guess",
/// not a panic). `display` is unchanged from before this field existed —
/// literal oracle text (`"*"`, `"*+1"`) for UI/debugging only, never
/// consulted for resolution.
#[derive(Clone, Debug)]
pub enum PTValue {
    Fixed(i32),
    CharacteristicDefining { display: String, script: Option<ScriptReference> },
}

impl PTValue {
    /// `Some` only for `Fixed` — a CDA's number isn't known without running
    /// its script callback against current game state, which this can't do.
    /// See `layers::effective_characteristics` for where a `CharacteristicDefining`
    /// with a real `script` actually gets resolved.
    pub fn as_fixed(&self) -> Option<i32> {
        match self {
            PTValue::Fixed(n) => Some(*n),
            PTValue::CharacteristicDefining { .. } => None,
        }
    }

    /// `as_fixed`, netted against `+1/+1`/`-1/-1` counters (CR 121.3's
    /// counters-affect-characteristics rule) — `None` for the same reason
    /// `as_fixed` is `None`. Doesn't merge/annihilate opposing counters out
    /// of `counters` itself (CR 704.5q's state-based action does that as a
    /// real mutation) — this only nets them arithmetically for a read,
    /// which gives the same effective number without needing that SBA
    /// implemented yet.
    pub fn net_with_counters(&self, counters: &CounterSet) -> Option<i32> {
        let base = self.as_fixed()?;
        Some(net_counters(base, counters))
    }
}

/// CR 121.3: nets `+1/+1`/`-1/-1` counters against an already-resolved base
/// value. Shared by `PTValue::net_with_counters` (base = a `Fixed` literal)
/// and `layers::effective_characteristics`'s CDA resolution (base = a
/// script-resolved number, CR 613.4a) — same formula, two different sources
/// for `base`.
pub(crate) fn net_counters(base: i32, counters: &CounterSet) -> i32 {
    base + counters.count(&CounterKind::PlusOnePlusOne) - counters.count(&CounterKind::MinusOneMinusOne)
}

bitflags! {
    /// Bitmask of a card's types (CR 300). A card can be multiple types at
    /// once (e.g. artifact creature).
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct CardTypeSet: u16 {
        const ARTIFACT     = 1 << 0;
        const CREATURE     = 1 << 1;
        const ENCHANTMENT  = 1 << 2;
        const INSTANT      = 1 << 3;
        const LAND         = 1 << 4;
        const SORCERY      = 1 << 5;
        const KINDRED      = 1 << 6;
        /// Stub: not implemented v1, slot reserved per DESIGN.md non-goals.
        const PLANESWALKER = 1 << 7;
        /// Stub: not implemented v1, slot reserved per DESIGN.md non-goals.
        const BATTLE       = 1 << 8;
    }
}

bitflags! {
    /// Bitmask of a card's supertypes (CR 205.4a).
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct SupertypeSet: u8 {
        const BASIC     = 1 << 0;
        const LEGENDARY = 1 << 1;
        const SNOW      = 1 << 2;
        const WORLD     = 1 << 3;
    }
}

bitflags! {
    /// Bitmask of `Keyword`, per CR 702. Only the subset that changes how an
    /// existing engine checkpoint behaves (blocking legality, summoning
    /// sickness, timing, SBA lethal-checks, damage application) — this is
    /// deliberately NOT meant to grow to cover all ~140 CR 702 keywords.
    /// Keywords that are really just sugar for an activated/triggered
    /// ability (Cycling, Equip, Persist, Undying, ...) don't belong here at
    /// all; they're registered as an ordinary `RegisteredAbility` instead.
    /// See the TODO in `script_api.rs` for the still-unbuilt keyword-name ->
    /// primitive-or-ability mapping layer that would let a card's script
    /// just say `Game.GrantKeyword(card, "Flying")` instead of every card's
    /// script constructing a `KeywordSet` by hand.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct KeywordSet: u32 {
        /// CR 702.9 - evasion: blockable only by Flying/Reach.
        const FLYING        = 1 << 0;
        /// CR 702.17 - can block Flying (no evasion of its own).
        const REACH         = 1 << 1;
        /// CR 702.111 - can't be blocked except by two or more creatures.
        const MENACE        = 1 << 2;
        /// CR 702.20 - doesn't tap to attack.
        const VIGILANCE     = 1 << 3;
        /// CR 702.10 - ignores summoning sickness (attacking and tap-cost
        /// activated abilities).
        const HASTE         = 1 << 4;
        /// CR 702.3 - can't attack.
        const DEFENDER      = 1 << 5;
        /// CR 702.8 - castable any time its controller could cast an
        /// instant, regardless of the card's own type.
        const FLASH         = 1 << 6;
        /// CR 702.2 - any nonzero damage dealt by this source is lethal,
        /// regardless of amount.
        const DEATHTOUCH    = 1 << 7;
        /// CR 702.12 - ignores the lethal-damage SBA (704.5g); does NOT
        /// ignore the 0-toughness SBA (704.5f).
        const INDESTRUCTIBLE = 1 << 8;
        /// CR 702.15 - damage this source deals also gains its controller
        /// that much life.
        const LIFELINK      = 1 << 9;
        /// CR 702.80 - damage this source deals to a creature becomes
        /// `-1/-1` counters instead of marked damage.
        const WITHER        = 1 << 10;
        /// CR 702.90 - implies Wither's creature-damage behavior, and also
        /// turns damage to a player into poison counters instead of life
        /// loss.
        const INFECT        = 1 << 11;
        /// CR 702.7 - deals combat damage in the first-strike combat damage
        /// step instead of the regular one (CR 510.1a/510.4).
        const FIRST_STRIKE  = 1 << 12;
        /// CR 702.7 - deals combat damage in both the first-strike and the
        /// regular combat damage steps (CR 510.1a/510.4). Independent of
        /// `FIRST_STRIKE` - a creature only needs this bit set to
        /// participate in both steps, it doesn't also need `FIRST_STRIKE`.
        const DOUBLE_STRIKE = 1 << 13;
        /// CR 702.19 - modifies combat damage assignment: once every
        /// blocker is assigned lethal damage, the controller may assign the
        /// rest to the defending player instead. Trample-over-planeswalkers
        /// (CR 702.19c/e/f/g) is a separate keyword, not modeled - this bit
        /// only ever spills excess to a `Target::Player`, never a
        /// planeswalker/battle `Target::Object` (unmodeled per DESIGN.md).
        const TRAMPLE       = 1 << 14;
        /// CR 701.19c - a regeneration shield can't be *created* for this
        /// permanent (checked at the moment `AbilityEffect::Regenerate`
        /// would create one, not a `ReplacementEffect` shape itself - CR
        /// 701.19c is explicit that the shield-creating spell/ability still
        /// resolves normally, only the shield itself fails to come into
        /// being). Deliberately not modeled as "shields can't apply" -
        /// 701.19c's own wording is about *creation*, and this engine has no
        /// shields sitting around unused to make "can't apply" meaningfully
        /// different from "was never created" anyway.
        const CANT_BE_REGENERATED = 1 << 15;
        /// CR 702.36 - evasion: can't be blocked except by artifact and/or
        /// black creatures.
        const FEAR          = 1 << 16;
        /// CR 702.13 - evasion: can't be blocked except by artifact
        /// creatures and/or creatures that share a color with it.
        const INTIMIDATE    = 1 << 17;
        /// CR 702.28 - evasion: can only block/be blocked by other shadow
        /// creatures (symmetric - a non-shadow creature can't block a
        /// shadow one either).
        const SHADOW        = 1 << 18;
        /// CR 702.31 - evasion: can't be blocked by creatures without
        /// horsemanship (a horsemanship creature may itself block anything).
        const HORSEMANSHIP  = 1 << 19;
        /// CR 702.22 - a static ability modifying combat's blocking and
        /// damage-assignment rules once grouped into a "band"
        /// (`CombatState::bands`, declared via `PlayerActionKind::
        /// DeclareBands`) rather than a per-creature blocking restriction
        /// like the other bits in this set. Only the plain form is
        /// representable - CR 702.22b/c's "bands with other [quality]"
        /// variant needs a parameter (which quality) - same reasoning
        /// `EffectiveCharacteristics::landwalk_subtypes`'s doc comment gives
        /// for keeping Landwalk off this bitflag entirely; it would need
        /// its own field/`EffectPrimitive` rather than a plain bit here,
        /// and isn't modeled yet.
        const BANDING       = 1 << 20;
    }
}

/// Named keyword, per CR 702. Use this for matching/comparison; use
/// `KeywordSet` for storage on a card.
pub enum Keyword {
    Flying,
    Reach,
    Menace,
    Vigilance,
    Haste,
    Defender,
    Flash,
    Deathtouch,
    Indestructible,
    Lifelink,
    Wither,
    Infect,
    FirstStrike,
    DoubleStrike,
}

/// Newtype over the subtype string (creature types, land types, spell
/// subtypes, etc.). The full subtype space is large and open-ended (new
/// creature types are added over time), so this stays string-backed rather
/// than a fixed enum — the wrapper exists for type-fill/compile-time
/// distinction from arbitrary `String`s, not to enumerate every value.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Subtype(pub String);

#[derive(Clone, Debug)]
pub enum ScriptReference {
    /// Lookup key into the external card-database project's script registry.
    ByKey(String),
    /// Literal Lua source, as supplied directly by the external DB project.
    Inline(String),
}

/// Immutable, shared. Sourced from the external card database project — this
/// engine consumes it, does not author or update it.
#[derive(Clone, Debug)]
pub struct CardDefinition {
    /// External DB's stable card identifier.
    pub definition_id: u64,
    pub name: String,
    pub mana_cost: ManaCost,
    pub color_indicator: Option<ColorSet>,
    pub supertypes: SupertypeSet,
    pub card_types: CardTypeSet,
    pub subtypes: Vec<Subtype>,
    pub oracle_text: String,
    /// Oracle/printed base power. `None` for non-creatures. This is the base
    /// value layer 7 starts from, not the live in-game value — see
    /// `EffectiveCharacteristics` in `effect.rs` for the latter (which is
    /// always a resolved `i32` — by query time any CDA has already run).
    pub power: Option<PTValue>,
    /// Oracle/printed base toughness. See `power` doc above.
    pub toughness: Option<PTValue>,
    /// Stub: planeswalker loyalty, unused v1.
    pub loyalty: Option<PTValue>,
    /// Stub: battle defense, unused v1.
    pub defense: Option<PTValue>,
    pub script_ref: ScriptReference,
    /// Printed CR 702 keywords this card has natively - see `KeywordSet`'s
    /// own doc comment for what is and isn't represented here.
    pub keywords: KeywordSet,
    /// `None` means this card requires no targets (CR 601.2c: "a spell may
    /// require some targets only if... otherwise the spell is cast as
    /// though it did not require those targets" — this engine doesn't model
    /// modes/kicker yet, so a card either always requires this or never
    /// does). See `TargetRequirement`/`TargetSpec` (target.rs) for what's
    /// and isn't representable here.
    pub target_requirement: Option<crate::types::target::TargetSpec>,
    /// CR 601.2b's alternative costs this card grants (flashback, escape,
    /// etc — see `AlternativeCost`'s own doc comment for which of those are
    /// actually representable yet). Empty for the overwhelming majority of
    /// cards, which only ever pay their printed `mana_cost`. Indexed by
    /// `PlayerActionKind::CastSpell::alternative_cost` — the caster
    /// announces which one (if any) they're invoking, same "announced at
    /// cast time" pattern as `x_value`.
    pub alternative_costs: Vec<crate::types::cost::AlternativeCost>,
    /// A spell's Rust-native fast-path effect (CR 608.2m resolution) —
    /// same `AbilityEffect` shape `RegisteredAbility.effect` already uses
    /// for activated/triggered abilities, e.g. `Some(AbilityEffect::
    /// Damage(2))` for "Shock deals 2 damage to any target". `None` means
    /// this spell's effect can only be carried out via the script bridge —
    /// same gap as `script_ref`'s `operation` lookup, which doesn't exist
    /// yet either (see `commands::CastSpell`'s own `ScriptHandle(0)`
    /// placeholder comment). A permanent spell (creature/artifact/
    /// enchantment) has no use for this field — CR 608.2m says it simply
    /// enters the battlefield; any ETB behavior is a separate `Triggered`-
    /// kind `RegisteredAbility`, not this field.
    ///
    /// Ignored (should be left `None`) for a modal card — see `modes` below,
    /// which carries the per-mode effect instead once `modes` is non-empty.
    pub effect: Option<AbilityEffect>,
    /// CR 700.2 modal spells — empty for the overwhelming majority of
    /// cards (non-modal), which use `target_requirement`/`effect` above
    /// directly. Non-empty means "Choose N —"-style (`mode_choice` below
    /// says how many): the caster picks that many entries
    /// (`PlayerActionKind::CastSpell::modes` supplies that many valid
    /// indices into this `Vec` — non-repeating unless `mode_choice` is one
    /// of the `*Repeatable` variants, CR 700.2d), and each chosen mode's own
    /// `target_requirement`/`effect` are used for the rest of casting and
    /// resolution instead of this struct's own singular fields — see
    /// `commands::CastSpell`'s own doc comment for how 2+ chosen modes
    /// combine (CR 700.2d, `phase5/modal-multi-mode`).
    ///
    /// TODO: CR 700.2i (pawprint mode budgets) is still unimplemented. CR
    /// 700.2h (a mode's own additional cost) is real now too — see
    /// `SpellMode::additional_cost`. CR 700.2e (another player chooses) is
    /// real now too, narrowly — see `ModeChoice::ExactlyChosenByOpponent`'s
    /// own doc comment.
    /// TODO: CR 700.2b (a modal *activated/triggered* ability, as opposed
    /// to a modal spell) is untouched — `RegisteredAbility` has no `modes`
    /// field of its own yet.
    pub modes: Vec<SpellMode>,
    /// How many of `modes` above must be chosen — meaningless (but still
    /// must be set; `CardDefinition` has no `Default` impl) when `modes` is
    /// empty. `ModeChoice::Exactly(1)` is the pre-existing "Choose one —"
    /// behavior every modal card used before `phase5/modal-multi-mode`.
    pub mode_choice: ModeChoice,
}

/// One option of a CR 700.2 modal spell — see `CardDefinition::modes`.
#[derive(Clone, Debug)]
pub struct SpellMode {
    pub target_requirement: Option<crate::types::target::TargetSpec>,
    pub effect: Option<AbilityEffect>,
    /// CR 700.2h: "some modal spells have one or more modes with a cost
    /// listed before the effect of that mode" — paid as part of the overall
    /// cast, alongside the mana cost, only if this mode is chosen.
    /// `commands::execute_command`'s `CastSpell` arm appends every chosen
    /// mode's own `additional_cost` (if any) to the spell's `CostSpec.
    /// additional`, reusing the exact same generic `AdditionalCost` payment
    /// machinery an activated ability's own additional costs already go
    /// through (`PaymentPlan::validate`/`Engine::submit_payment` don't
    /// distinguish a spell's cost from an ability's) — no new payment code
    /// needed, only new population of an existing, already-generic field.
    /// `None` (the common case) for a mode with no cost of its own.
    pub additional_cost: Option<AdditionalCost>,
}

/// CR 700.2's "Choose one —"/"Choose two —"/"Choose one or more —" instruction
/// preceding a modal spell/ability's bulleted mode list — see
/// `CardDefinition::mode_choice`. `Exactly(n)`: the caster must choose
/// exactly `n` distinct modes (`n == 1` is the common "Choose one —" case).
/// `UpTo(n)`: the caster may choose any number from 1 to `n` distinct modes
/// (e.g. "Choose one or both —" is `UpTo(2)`) — CR 700.2a still requires at
/// least one mode be chosen either way ("choose a number of those options,"
/// never zero); a spell with no legal mode at all simply can't be cast
/// (601.2b), same as any other spell with no legal choice.
///
/// `ExactlyRepeatable(n)`/`UpToRepeatable(n)` are CR 700.2d's reminder-text-
/// gated exception ("You may choose the same mode more than once," e.g.
/// Profane Command) — same count semantics as `Exactly`/`UpTo`, but a
/// repeated mode index is allowed rather than rejected. Added as new variants
/// rather than a field on `Exactly`/`UpTo` (which would break every existing
/// `ModeChoice::Exactly(1)` construction site) or on `CardDefinition` itself
/// (~120 test files construct it as a bare struct literal with no `Default`
/// impl — the same ripple class `phase5/modal-multi-mode` already hit once).
///
/// `ExactlyChosenByOpponent(n)`/`UpToChosenByOpponent(n)` are CR 700.2e's
/// "some spells and abilities specify that a player other than their
/// controller chooses a mode for it" exception. Same count semantics again,
/// but the mode(s) aren't supplied by the caster's own `CastSpell` action at
/// all — `commands::execute_command`'s `CastSpell` arm defers the choice to
/// a new resolution-time pause (`ProcessState::AwaitingModeChoice`,
/// `Engine::submit_mode_choice`) answered by the other player instead, since
/// `PlayerActionKind::CastSpell` is a single-actor atomic command (87
/// construction sites) that can't be restructured to accept a second
/// player's input mid-action. Narrowed on purpose, checked at cast time:
/// every mode on the card must have `target_requirement: None` and
/// `additional_cost: None` (supporting either would mean deferring target
/// selection and/or cost computation to the same late pause — real
/// additional plumbing this slice doesn't build), and the game must have
/// exactly two non-eliminated players (CR 700.2e's own "if there's more than
/// one other player, the controller decides which" sub-clause needs a
/// further choice mechanism no 3+-player test scaffolding exists for yet).
/// Combining with `*Repeatable` isn't supported either — no real card needs
/// both at once.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModeChoice {
    Exactly(u32),
    UpTo(u32),
    ExactlyRepeatable(u32),
    UpToRepeatable(u32),
    ExactlyChosenByOpponent(u32),
    UpToChosenByOpponent(u32),
}

impl ModeChoice {
    /// `(min, max, repeatable)` — CR 700.2a's "choose a number of those
    /// options" floor is always at least 1 (`UpTo`/`UpToRepeatable`/
    /// `UpToChosenByOpponent` never mean "or none"), never 0. Purely a
    /// numeric decode of the variant itself — doesn't distinguish *who*
    /// makes the choice (caster/controller vs. CR 700.2e's "chosen by
    /// opponent"), since that's a pipeline-level routing decision each
    /// caller already makes on its own (`commands::cast::execute_cast_spell`
    /// branches on the `*ChosenByOpponent` variants before ever reaching
    /// this method; `Engine::push_ordered_triggers`/
    /// `submit_trigger_mode_choice` deliberately don't, a named narrowing —
    /// see `RegisteredAbility::mode_choice`'s own doc comment).
    pub(crate) fn bounds(&self) -> (u32, u32, bool) {
        match *self {
            ModeChoice::Exactly(n) => (n, n, false),
            ModeChoice::UpTo(n) => (1, n, false),
            ModeChoice::ExactlyRepeatable(n) => (n, n, true),
            ModeChoice::UpToRepeatable(n) => (1, n, true),
            ModeChoice::ExactlyChosenByOpponent(n) => (n, n, false),
            ModeChoice::UpToChosenByOpponent(n) => (1, n, false),
        }
    }
}

impl CardDefinition {
    /// Per CR 105.2a: color comes from mana cost symbols and/or an explicit
    /// `color_indicator`, not stored as its own redundant field.
    pub fn colors(&self) -> ColorSet {
        match self.color_indicator {
            Some(indicator) => indicator,
            None => self.mana_cost.colors(),
        }
    }

}
