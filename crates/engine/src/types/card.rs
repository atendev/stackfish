use bitflags::bitflags;

use crate::types::ability::AbilityEffect;
use crate::types::cost::AdditionalCost;
use crate::types::counter::{CounterKind, CounterSet};
use crate::types::mana::{ColorSet, ManaCost};

/// Printed or base power, toughness, loyalty or defense. A `*` value is a
/// characteristic-defining ability (CR 604.3, 208.2a) whose number comes from
/// a script run against game state, so it cannot be a plain `i32`. A `+ 1`
/// offset is folded into the script's result.
///
/// `script` mirrors `CardDefinition.script_ref` and keeps this type plain
/// data. `None` counts as an unresolvable script, which
/// `layers::effective_characteristics` skips. `display` is the oracle text
/// (`"*+1"`), for UI and debugging only.
#[derive(Clone, Debug, PartialEq)]
pub enum PTValue {
    Fixed(i32),
    CharacteristicDefining { display: String, script: Option<ScriptReference> },
}

impl PTValue {
    /// `Some` only for `Fixed`. A CDA needs its script run; see
    /// `layers::effective_characteristics`.
    pub fn as_fixed(&self) -> Option<i32> {
        match self {
            PTValue::Fixed(n) => Some(*n),
            PTValue::CharacteristicDefining { .. } => None,
        }
    }

    /// `as_fixed` netted against `+1/+1` and `-1/-1` counters (CR 121.3). It
    /// nets arithmetically for the read and does not annihilate the counters
    /// (CR 704.5q).
    pub fn net_with_counters(&self, counters: &CounterSet) -> Option<i32> {
        let base = self.as_fixed()?;
        Some(net_counters(base, counters))
    }
}

/// CR 121.3: nets `+1/+1` and `-1/-1` counters against a resolved base, from
/// a `Fixed` literal or a script-resolved CDA (CR 613.4a).
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
        /// Reserved slot, a non-goal in `DESIGN.md`.
        const PLANESWALKER = 1 << 7;
        /// Reserved slot, a non-goal in `DESIGN.md`.
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
    /// Bitmask of `Keyword` (CR 702), limited to keywords that change an
    /// existing engine checkpoint: blocking legality, summoning sickness,
    /// timing, SBA lethal checks and damage. Keywords that are sugar for an
    /// activated or triggered ability (Cycling, Equip, Persist) are registered
    /// as a `RegisteredAbility` instead.
    /// TODO: a keyword-name to primitive-or-ability mapping, so a script can say `Game.GrantKeyword(card, "Flying")`
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
        /// CR 702.19: damage beyond lethal to every blocker may go to the
        /// defending player. Trample over planeswalkers (CR 702.19c-g) is not
        /// modeled, so the excess spills only to a `Target::Player`.
        const TRAMPLE       = 1 << 14;
        /// CR 701.19c: no regeneration shield can be created for this
        /// permanent. Checked when `AbilityEffect::Regenerate` would create
        /// one, and the spell or ability still resolves. Modeled as creation
        /// and not application, since this engine keeps no unused shields.
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
        /// CR 702.22: modifies blocking and damage assignment once creatures
        /// are grouped into a band (`CombatState::bands`, from
        /// `DeclareBands`). Only the plain form is representable. "Bands with
        /// other [quality]" (CR 702.22b, 702.22c) needs a parameter, so it
        /// would need its own field, as Landwalk does.
        const BANDING       = 1 << 20;
        /// CR 702.11b: can't be the target of spells or abilities an opponent
        /// controls. Enforced in `commands::target_is_legal` (cast,
        /// activation, trigger scan and the CR 608.2b recheck) through
        /// `layers::effective_characteristics`, so granted and removed
        /// keywords count. It doesn't stop untargeted effects (CR 601.2c) or
        /// detach an Aura or Equipment, unlike protection (CR 702.16d).
        ///
        /// TODO: "hexproof from [quality]" needs a parameter, so it is not a plain bit (CR 702.11d-g)
        /// TODO: no hexproof on players, which have no keyword set here (CR 702.11c)
        const HEXPROOF      = 1 << 21;
        /// CR 702.18a: can't be the target of any spell or ability, including
        /// its controller's. Same enforcement and exclusions as `HEXPROOF`.
        ///
        /// TODO: no shroud on players (CR 702.18a)
        const SHROUD        = 1 << 22;
        /// CR 508.1d: "attacks each combat if able". Not a CR 702 keyword but
        /// a static requirement on declaration, so it shares the layer-6 fold
        /// and a granted or removed requirement counts. Enforced by
        /// `commands::combat::predicate`, which also marks the creature
        /// `required` in the declare-attackers entries.
        ///
        /// TODO: "attacks a specific player if able" and "blocks if able" need a parameter or a second bit (CR 509.1c)
        const ATTACKS_EACH_COMBAT = 1 << 23;
    }
}

/// Named keyword, per CR 702. Use this for matching/comparison; use
/// `KeywordSet` for storage on a card.
#[derive(Clone, PartialEq, Debug)]
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

/// Subtype string (creature type, land type, spell subtype). The space is
/// open-ended, so it stays string-backed. The wrapper only distinguishes it
/// from arbitrary `String`s.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Subtype(pub String);

#[derive(Clone, Debug, PartialEq)]
pub enum ScriptReference {
    /// Lookup key into the external card-database project's script registry.
    ByKey(String),
    /// Literal Lua source, as supplied directly by the external DB project.
    Inline(String),
}

/// Immutable, shared. Sourced from the external card database project — this
/// engine consumes it, does not author or update it.
#[derive(Clone, Debug, PartialEq)]
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
    /// Printed base power, `None` for non-creatures. Layer 7 starts here. The
    /// live value is in `EffectiveCharacteristics` (`effect.rs`).
    pub power: Option<PTValue>,
    /// Oracle/printed base toughness. See `power` doc above.
    pub toughness: Option<PTValue>,
    /// Reserved for planeswalker loyalty, a non-goal in `DESIGN.md`.
    pub loyalty: Option<PTValue>,
    /// Reserved for battle defense, a non-goal in `DESIGN.md`.
    pub defense: Option<PTValue>,
    pub script_ref: ScriptReference,
    /// Printed CR 702 keywords. See `KeywordSet` for what is represented.
    pub keywords: KeywordSet,
    /// `None` means no targets (CR 601.2c). A card either always requires
    /// this or never does. See `TargetSpec` for what is representable.
    pub target_requirement: Option<crate::types::target::TargetSpec>,
    /// CR 601.2b alternative costs this card grants (flashback, escape).
    /// Empty for most cards. Indexed by `CastSpell::alternative_cost`; see
    /// `AlternativeCost`.
    pub alternative_costs: Vec<crate::types::cost::AlternativeCost>,
    /// CR 702.27a: an additional cost paid as the spell is cast, announced
    /// via `CastSpell::pay_buyback`. A separate field from `alternative_costs`
    /// because it is paid in addition to the cost, not instead of it. A flat
    /// `ManaCost` because every real Buyback card is one, and a `CostSpec`
    /// would need a merge primitive with no other caller.
    pub buyback_cost: Option<ManaCost>,
    /// A spell's Rust-native fast-path effect (CR 608.2m), in the
    /// `AbilityEffect` shape `RegisteredAbility.effect` uses, such as
    /// `Damage(2)` for Shock. `None` leaves the effect to the script bridge.
    /// A permanent spell has no use for it, since ETB behavior is a triggered
    /// `RegisteredAbility`. Leave `None` for a modal card, whose `modes` carry
    /// the effects.
    pub effect: Option<AbilityEffect>,
    /// CR 700.2 modal spells. Empty for non-modal cards, which use
    /// `target_requirement` and `effect` above. Otherwise the caster picks as
    /// many entries as `mode_choice` allows (`CastSpell::modes` holds the
    /// indices, non-repeating unless `*Repeatable`, CR 700.2d), and each
    /// chosen mode's own fields replace this struct's singular ones. See
    /// `commands::CastSpell` for how 2+ modes combine.
    ///
    /// Also covers pawprint budgets (CR 700.2i, `ModeChoice::PawprintBudget`),
    /// per-mode additional costs (CR 700.2h, `SpellMode::additional_cost`) and
    /// another player choosing (CR 700.2e, narrowly, see
    /// `ModeChoice::ExactlyChosenByOpponent`).
    /// TODO: CR 700.2b, a modal activated or triggered ability as opposed to a modal spell; `RegisteredAbility` has no `modes` field of its own
    pub modes: Vec<SpellMode>,
    /// How many of `modes` must be chosen. Set even when `modes` is empty,
    /// because `CardDefinition` has no `Default`.
    pub mode_choice: ModeChoice,
}

/// One option of a CR 700.2 modal spell — see `CardDefinition::modes`.
#[derive(Clone, Debug, PartialEq)]
pub struct SpellMode {
    pub target_requirement: Option<crate::types::target::TargetSpec>,
    pub effect: Option<AbilityEffect>,
    /// CR 700.2h: a cost paid with the mana cost only if this mode is chosen.
    /// The `CastSpell` arm appends each chosen mode's cost to the spell's
    /// `CostSpec.additional`, reusing the generic `AdditionalCost` payment
    /// path. `None` for most modes.
    pub additional_cost: Option<AdditionalCost>,
    /// CR 700.2i: this mode's pawprint ({P}) weight. Read only under
    /// `ModeChoice::PawprintBudget`, but always set because the struct has no
    /// `Default`.
    pub pawprint_weight: u32,
}

/// CR 700.2's "Choose one" instruction before a modal spell's modes; see
/// `CardDefinition::mode_choice`. `Exactly(n)` chooses exactly `n` distinct
/// modes. `UpTo(n)` chooses 1 to `n`, never zero (CR 700.2a), and a spell with
/// no legal mode cannot be cast (CR 601.2b).
///
/// `ExactlyRepeatable` and `UpToRepeatable` allow a repeated mode index
/// (CR 700.2d, Profane Command). They are variants because a field would
/// break every `Exactly(1)` site, and `CardDefinition` has ~120 literal sites.
///
/// `ExactlyChosenByOpponent` and `UpToChosenByOpponent` are CR 700.2e, where
/// another player chooses. The `CastSpell` arm defers the choice to a
/// resolution-time pause (`ProcessState::AwaitingModeChoice`,
/// `Engine::submit_mode_choice`), because `CastSpell` is a single-actor
/// command. Checked at cast: every mode must have no target requirement and no
/// additional cost, and exactly two players must remain, since the
/// multiplayer sub-clause of CR 700.2e has no choice mechanism. They do not
/// combine with `*Repeatable`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModeChoice {
    Exactly(u32),
    UpTo(u32),
    ExactlyRepeatable(u32),
    UpToRepeatable(u32),
    ExactlyChosenByOpponent(u32),
    UpToChosenByOpponent(u32),
    /// CR 700.2i: modes whose combined `SpellMode::pawprint_weight` fits this
    /// budget. It limits weight, not count, so it stays out of `bounds()`,
    /// which cannot derive a count from a budget. Each validation site sums
    /// weights before calling `bounds()`, as it special-cases the
    /// opponent-chosen variants. There is no floor, so zero modes is legal.
    /// Repeatable and opponent-chosen forms are not supported.
    PawprintBudget(u32),
}

impl ModeChoice {
    /// `(min, max, repeatable)`. The floor is at least 1 for `UpTo*`
    /// (CR 700.2a). It decodes only the numbers, not who chooses: callers
    /// route that themselves (`commands::cast::execute_cast_spell` branches on
    /// `*ChosenByOpponent` first, and see `RegisteredAbility::mode_choice` for
    /// triggers).
    ///
    /// `PawprintBudget` returns `(0, budget, false)` as a display hint only,
    /// with `max` meaning the budget. Real enforcement is each site's weight
    /// sum.
    pub(crate) fn bounds(&self) -> (u32, u32, bool) {
        match *self {
            ModeChoice::Exactly(n) => (n, n, false),
            ModeChoice::UpTo(n) => (1, n, false),
            ModeChoice::ExactlyRepeatable(n) => (n, n, true),
            ModeChoice::UpToRepeatable(n) => (1, n, true),
            ModeChoice::ExactlyChosenByOpponent(n) => (n, n, false),
            ModeChoice::UpToChosenByOpponent(n) => (1, n, false),
            ModeChoice::PawprintBudget(n) => (0, n, false),
        }
    }
}

impl CardDefinition {
    /// CR 105.2a: color comes from the mana cost or `color_indicator`, not a
    /// stored field.
    pub fn colors(&self) -> ColorSet {
        match self.color_indicator {
            Some(indicator) => indicator,
            None => self.mana_cost.colors(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ids::SequenceNo;
    use crate::types::mana::ManaCost;

    fn minimal_definition() -> CardDefinition {
        CardDefinition {
            definition_id: 1,
            name: "Test".to_string(),
            mana_cost: ManaCost { symbols: Vec::new() },
            color_indicator: None,
            supertypes: SupertypeSet::empty(),
            card_types: CardTypeSet::CREATURE,
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
        }
    }

    #[test]
    fn pt_value_as_fixed_returns_some_for_fixed() {
        assert_eq!(PTValue::Fixed(3).as_fixed(), Some(3));
        assert_eq!(
            PTValue::CharacteristicDefining { display: "*".to_string(), script: None }.as_fixed(),
            None
        );
    }

    #[test]
    fn pt_value_net_with_counters_applies_plus_and_minus() {
        // CR 121.3
        let mut counters = CounterSet::new();
        counters.add(CounterKind::PlusOnePlusOne, 2, SequenceNo(0));
        counters.add(CounterKind::MinusOneMinusOne, 1, SequenceNo(1));
        assert_eq!(PTValue::Fixed(2).net_with_counters(&counters), Some(3));
    }

    #[test]
    fn pt_value_net_with_counters_returns_none_for_cda() {
        let cda = PTValue::CharacteristicDefining { display: "*".to_string(), script: None };
        assert_eq!(cda.net_with_counters(&CounterSet::new()), None);
    }

    #[test]
    fn card_definition_colors_from_mana_cost() {
        let mut def = minimal_definition();
        def.mana_cost = ManaCost::parse("{1}{R}{G}").unwrap();
        assert_eq!(def.colors(), ColorSet::RED | ColorSet::GREEN);
    }

    #[test]
    fn card_definition_colors_use_indicator_when_present() {
        let mut def = minimal_definition();
        def.mana_cost = ManaCost::parse("{1}{R}").unwrap();
        def.color_indicator = Some(ColorSet::BLUE);
        assert_eq!(def.colors(), ColorSet::BLUE);
    }

    #[test]
    fn card_definition_colors_empty_for_colorless() {
        let def = minimal_definition();
        assert_eq!(def.colors(), ColorSet::empty());
    }

    #[test]
    fn mode_choice_bounds() {
        assert_eq!(ModeChoice::Exactly(2).bounds(), (2, 2, false));
        assert_eq!(ModeChoice::UpTo(3).bounds(), (1, 3, false));
        assert_eq!(ModeChoice::ExactlyRepeatable(2).bounds(), (2, 2, true));
        assert_eq!(ModeChoice::UpToRepeatable(3).bounds(), (1, 3, true));
        assert_eq!(ModeChoice::ExactlyChosenByOpponent(1).bounds(), (1, 1, false));
        assert_eq!(ModeChoice::UpToChosenByOpponent(2).bounds(), (1, 2, false));
        // PawprintBudget uses max as a budget display hint.
        assert_eq!(ModeChoice::PawprintBudget(5).bounds(), (0, 5, false));
    }

    #[test]
    fn card_type_set_operations() {
        let artifact_creature = CardTypeSet::ARTIFACT | CardTypeSet::CREATURE;
        assert!(artifact_creature.contains(CardTypeSet::ARTIFACT));
        assert!(artifact_creature.contains(CardTypeSet::CREATURE));
        assert!(!artifact_creature.contains(CardTypeSet::LAND));
        assert!(artifact_creature.intersects(CardTypeSet::CREATURE));
        assert!(!artifact_creature.intersects(CardTypeSet::LAND | CardTypeSet::SORCERY));
    }

    #[test]
    fn supertype_set_operations() {
        let legendary_basic = SupertypeSet::LEGENDARY | SupertypeSet::BASIC;
        assert!(legendary_basic.contains(SupertypeSet::LEGENDARY));
        assert!(legendary_basic.intersects(SupertypeSet::BASIC));
    }

    #[test]
    fn keyword_set_operations() {
        let flying_reach = KeywordSet::FLYING | KeywordSet::REACH;
        assert!(flying_reach.contains(KeywordSet::FLYING));
        assert!(!flying_reach.contains(KeywordSet::TRAMPLE));
    }
}
