use crate::{
    AtomicManaSymbol, CardTypeSet, Color, GameObject, GameState, ManaCost, ManaPool, ManaSymbol,
    ManaUnit, ObjectId, PlayerId, ZoneKind,
};

/// Structured stand-in for "filter described in oracle text" — e.g.
/// "sacrifice a creature", "discard a card at random", "tap an untapped
/// artifact you control". Covers the common, structurally-expressible
/// shapes; `extra` is a script-bind-time-refined catch-all for filters that
/// don't reduce to type + controller + count (e.g. "with power 4 or
/// greater", named-card restrictions).
#[derive(Clone, Debug)]
pub struct FilterSpec {
    pub card_types: Option<CardTypeSet>,
    pub controller: Option<FilterController>,
    pub random: bool,
    pub extra: Option<String>,
}

impl FilterSpec {
    /// Whether `object` satisfies this filter, from `viewer`'s perspective
    /// (`FilterController::You`/`Opponent` are relative to whoever is paying
    /// the cost). `random` doesn't affect matching itself — only which
    /// matching object gets picked, irrelevant here. `extra` requires the
    /// script bridge, not available from this signature — a filter with
    /// `extra` set is treated as "doesn't match" rather than guessed at (see
    /// this struct's own doc comment for why `extra` exists at all).
    pub fn matches(&self, object: &GameObject, viewer: PlayerId) -> bool {
        if self.extra.is_some() {
            return false;
        }
        if let Some(types) = self.card_types {
            if !object.definition.card_types.intersects(types) {
                return false;
            }
        }
        if let Some(fc) = self.controller {
            let ok = match fc {
                FilterController::You => object.controller == viewer,
                FilterController::Opponent => object.controller != viewer,
                FilterController::Any => true,
            };
            if !ok {
                return false;
            }
        }
        true
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilterController {
    You,
    Opponent,
    Any,
}

#[derive(Clone, Debug)]
pub enum AdditionalCost {
    /// CR 602.5a's `{T}` on an activated ability's own source — distinct
    /// from `TapPermanents`, which taps *other* permanents chosen by the
    /// payer. Always targets whichever object is activating the ability
    /// (`PaymentPlan::validate`'s `source` param), never a filter choice.
    TapSelf,
    Sacrifice { filter: FilterSpec },
    Discard { filter: FilterSpec },
    TapPermanents { filter: FilterSpec, count: u32 },
    LifePayment { amount: u32 },
    Other { description: String },
}

#[derive(Clone, Debug)]
pub enum AlternativeCost {
    /// e.g. flashback, escape — paying this cost replaces the mana cost entirely.
    Replacement { description: String, cost: CostSpec },
}

#[derive(Clone, Debug)]
pub struct CostSpec {
    pub mana: Option<ManaCost>,
    pub additional: Vec<AdditionalCost>,
    pub alternative: Vec<AlternativeCost>,
}

impl CostSpec {
    pub fn mana_only(cost: ManaCost) -> Self {
        CostSpec { mana: Some(cost), additional: Vec::new(), alternative: Vec::new() }
    }
}

/// `Color` enum order used to index a fixed-size `[u32; 5]` colored-mana
/// tally — shared between `PaymentPlan::validate` and `spend` so the two
/// can't drift apart on which slot means which color.
fn color_index(c: Color) -> usize {
    match c {
        Color::White => 0,
        Color::Blue => 1,
        Color::Black => 2,
        Color::Red => 3,
        Color::Green => 4,
    }
}

/// Tallies `symbols` into (per-color counts, total generic) — `None` if any
/// symbol isn't one of the shapes this slice supports paying for (see
/// docs/implementation.md's "next slice" TODO list: `Colorless`/`Snow`/
/// `Phyrexian`/`Hybrid`/`X` payment are all explicitly out of scope here).
fn colored_and_generic(symbols: &[ManaSymbol]) -> Option<([u32; 5], u32)> {
    let mut colored = [0u32; 5];
    let mut generic = 0u32;
    for symbol in symbols {
        match symbol {
            ManaSymbol::Atom(AtomicManaSymbol::Colored(c)) => colored[color_index(*c)] += 1,
            ManaSymbol::Atom(AtomicManaSymbol::Generic(n)) => generic += n,
            _ => return None,
        }
    }
    Some((colored, generic))
}

/// Tallies `units` into per-color spent counts and total colorless spent —
/// the payer's-side mirror of `colored_and_generic`, but over literal
/// `ManaUnit`s rather than cost-shape `ManaSymbol`s (there's no "generic
/// unit"; every unit spent is some specific color or colorless).
fn spent_tally(units: &[ManaUnit]) -> ([u32; 5], u32) {
    let mut colored = [0u32; 5];
    let mut colorless = 0u32;
    for unit in units {
        match unit {
            ManaUnit::Colored(c) => colored[color_index(*c)] += 1,
            ManaUnit::Colorless => colorless += 1,
        }
    }
    (colored, colorless)
}

/// Deducts exactly `units` from `pool` (CR 118.3a) — no priority order, no
/// inference: `units` already says precisely which mana came out, because
/// that's what `PaymentPlan::validate` required it to declare (CR 118.13a's
/// payment choice). Only call this after `validate` has confirmed the pool
/// can actually cover it — this function trusts that and does not re-check,
/// so calling it against a pool that doesn't hold `units` would underflow.
pub(crate) fn spend(units: &[ManaUnit], pool: &mut ManaPool) {
    for unit in units {
        match unit {
            ManaUnit::Colored(Color::White) => pool.white -= 1,
            ManaUnit::Colored(Color::Blue) => pool.blue -= 1,
            ManaUnit::Colored(Color::Black) => pool.black -= 1,
            ManaUnit::Colored(Color::Red) => pool.red -= 1,
            ManaUnit::Colored(Color::Green) => pool.green -= 1,
            ManaUnit::Colorless => pool.colorless -= 1,
        }
    }
}

pub enum CostPayment {
    /// The exact literal units spent (CR 118.13a) — not an echo of the
    /// cost's shape. See `cost.rs`'s `PaymentPlan` doc comment for why this
    /// is the payer's *choice*, not a restatement of what was owed.
    Mana(Vec<ManaUnit>),
    Sacrifice(ObjectId),
    Discard(ObjectId),
    Tap(ObjectId),
    Life(u32),
    Other { description: String },
}

/// A fully chosen, validated-before-commit set of payments for a `CostSpec`.
///
/// Flow: a `CostSpec` describes what's owed (mana, plus any additional/
/// alternative costs) but not yet *how* it'll be paid — that requires player
/// choice (which lands to tap, which creature to sacrifice, whether to take
/// the alternative cost instead). The host collects those choices from the
/// player as a `PlayerActionKind::SubmitPayment` and assembles them into a
/// `PaymentPlan`. The engine then validates the whole plan atomically against
/// the originating `CostSpec` — every payment present, nothing extra, mana
/// payments actually affordable from the pool — and only commits it (taps the
/// lands, removes the mana, sacrifices the creature, etc.) if the entire plan
/// checks out. No partial payment ever lands in game state; either the whole
/// `PaymentPlan` is paid or none of it is.
pub struct PaymentPlan {
    pub payments: Vec<CostPayment>,
}

impl PaymentPlan {
    /// Validates this plan atomically against `spec` — every cost component
    /// present, nothing extra, mana actually affordable from `payer`'s pool,
    /// every additional-cost payment resolving to a real, legal object.
    /// `state`/`payer` replace the original `pool: &ManaPool` param (kept for
    /// mana affordability, same reasoning as before) since validating
    /// `spec.additional` needs to look objects up by id and check their
    /// zone/controller/tapped state — `source` is the object activating the
    /// ability/spell, needed so `AdditionalCost::TapSelf` can confirm the
    /// payment taps *that* object specifically, not some other one.
    ///
    /// `self.payments` is ordered to mirror `spec`: a `Mana` entry first, iff
    /// `spec.mana` is `Some` *and* actually has symbols to pay (a `{0}` cost
    /// — CR 118.5a's "must explicitly acknowledge" requirement — is
    /// satisfied by there being no `Mana` entry at all, same convention this
    /// function used before generalizing to additional costs), then exactly
    /// one entry per `spec.additional` component in order (`TapPermanents`'s
    /// `count` consumes that many consecutive `Tap` entries) — this
    /// ordering is what lets several same-shaped payments (e.g. two `Tap`
    /// entries, one for `TapSelf` and one for a `TapPermanents` component)
    /// disambiguate which requirement each one is satisfying. `commit`
    /// doesn't need this ordering — once validated, every payment's *action*
    /// (tap this id, sacrifice this id, ...) is unambiguous on its own.
    ///
    /// CR 118.13a's payment choice: a colored pip must be paid with that
    /// *exact* color — no substitution — but the generic component can be
    /// paid with any leftover units, including extra units of a color that
    /// also satisfied a colored pip (e.g. paying `{1}{G}` with three green
    /// units: one for the `{G}` pip, two covering the `{1}`). The `units`
    /// list inside a `CostPayment::Mana` entry must total *exactly* the
    /// cost — every pip accounted for by the right color, every leftover
    /// unit summing to exactly the generic amount, no more, no less.
    ///
    /// Out of scope this slice (returns `false`): `spec.alternative` costs,
    /// and any mana symbol shape other than plain colored/generic on the
    /// *cost* side (`Colorless`/`Snow`/`Phyrexian`/`Hybrid`/`X` requirements
    /// aren't modeled — see `colored_and_generic`). `AdditionalCost::Other`
    /// always fails validation too — no `CostPayment` shape can express an
    /// unstructured-text cost. Returning `bool` here as a placeholder — real
    /// version will likely return a `Result` with a reason once validation
    /// failure modes are designed.
    pub fn validate(&self, spec: &CostSpec, state: &GameState, payer: PlayerId, source: ObjectId) -> bool {
        if !spec.alternative.is_empty() {
            return false;
        }
        let mut payments = self.payments.iter();

        if let Some(mana_cost) = &spec.mana {
            let required = mana_cost.symbols.as_slice();
            if !required.is_empty() {
                let Some(CostPayment::Mana(units)) = payments.next() else { return false };
                let Some((required_colored, generic_needed)) = colored_and_generic(required) else {
                    return false;
                };
                let (spent_colored, spent_colorless) = spent_tally(units);

                // Every colored pip must be paid with that exact color - no
                // substitution, checked before anything about totals.
                if required_colored.iter().zip(spent_colored).any(|(need, have)| have < *need) {
                    return false;
                }
                // Whatever's spent beyond each pip's exact requirement, plus
                // any colorless spent, must cover the generic component
                // exactly.
                let leftover: u32 = spent_colored
                    .iter()
                    .zip(required_colored)
                    .map(|(have, need)| have - need)
                    .sum::<u32>()
                    + spent_colorless;
                if leftover != generic_needed {
                    return false;
                }
                let Some(player) = state.player(payer) else { return false };
                let pool = &player.mana_pool;
                let pool_colored = [pool.white, pool.blue, pool.black, pool.red, pool.green];
                if spent_colored.iter().zip(pool_colored).any(|(spent, have)| *spent > have) {
                    return false;
                }
                if spent_colorless > pool.colorless {
                    return false;
                }
            }
            // required.is_empty(): CR 118.5a's explicit-acknowledgment
            // requirement is satisfied by the absence of a Mana entry here -
            // nothing to consume.
        }

        for additional in &spec.additional {
            match additional {
                AdditionalCost::TapSelf => {
                    let Some(CostPayment::Tap(id)) = payments.next() else { return false };
                    if *id != source {
                        return false;
                    }
                    let Some(obj) = state.object(*id) else { return false };
                    if obj.tapped {
                        return false;
                    }
                }
                AdditionalCost::TapPermanents { filter, count } => {
                    let mut used: Vec<ObjectId> = Vec::with_capacity(*count as usize);
                    for _ in 0..*count {
                        let Some(CostPayment::Tap(id)) = payments.next() else { return false };
                        if used.contains(id) {
                            return false;
                        }
                        let Some(obj) = state.object(*id) else { return false };
                        if obj.tapped || obj.zone != ZoneKind::Battlefield {
                            return false;
                        }
                        if !filter.matches(obj, payer) {
                            return false;
                        }
                        used.push(*id);
                    }
                }
                AdditionalCost::Sacrifice { filter } => {
                    let Some(CostPayment::Sacrifice(id)) = payments.next() else { return false };
                    let Some(obj) = state.object(*id) else { return false };
                    // CR 701.20a: you can only sacrifice a permanent you control.
                    if obj.zone != ZoneKind::Battlefield || obj.controller != payer {
                        return false;
                    }
                    if !filter.matches(obj, payer) {
                        return false;
                    }
                }
                AdditionalCost::Discard { filter } => {
                    let Some(CostPayment::Discard(id)) = payments.next() else { return false };
                    let Some(obj) = state.object(*id) else { return false };
                    if obj.zone != ZoneKind::Hand || obj.controller != payer {
                        return false;
                    }
                    if !filter.matches(obj, payer) {
                        return false;
                    }
                }
                AdditionalCost::LifePayment { amount } => {
                    let Some(CostPayment::Life(paid)) = payments.next() else { return false };
                    if paid != amount {
                        return false;
                    }
                    // CR 119.4: a player can't pay a life-payment cost that
                    // would take them below 0 life.
                    let Some(player) = state.player(payer) else { return false };
                    if player.life < *amount as i32 {
                        return false;
                    }
                }
                AdditionalCost::Other { .. } => return false,
            }
        }

        payments.next().is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::card::{CardDefinition, KeywordSet, PTValue, ScriptReference, SupertypeSet};
    use crate::types::counter::CounterSet;
    use crate::types::ids::SequenceNo;
    use crate::types::object::VisibilityPolicy;

    fn creature(controller: PlayerId, card_types: CardTypeSet) -> GameObject {
        GameObject {
            object_id: ObjectId(0),
            lineage: crate::types::ids::ObjectLineage { previous: None },
            definition: CardDefinition {
                definition_id: 1,
                name: "Test Object".to_string(),
                mana_cost: ManaCost { symbols: Vec::new() },
                color_indicator: None,
                supertypes: SupertypeSet::empty(),
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
            },
            owner: controller,
            controller,
            zone: ZoneKind::Battlefield,
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
            visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
        }
    }

    #[test]
    fn card_types_true_and_false_cases() {
        let p0 = PlayerId(0);
        let filter = FilterSpec {
            card_types: Some(CardTypeSet::CREATURE),
            controller: None,
            random: false,
            extra: None,
        };
        assert!(filter.matches(&creature(p0, CardTypeSet::CREATURE), p0));
        assert!(!filter.matches(&creature(p0, CardTypeSet::LAND), p0));
    }

    #[test]
    fn controller_you_and_opponent_true_and_false_cases() {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let you_filter = FilterSpec {
            card_types: None,
            controller: Some(FilterController::You),
            random: false,
            extra: None,
        };
        assert!(you_filter.matches(&creature(p0, CardTypeSet::CREATURE), p0));
        assert!(!you_filter.matches(&creature(p1, CardTypeSet::CREATURE), p0));

        let opponent_filter = FilterSpec {
            card_types: None,
            controller: Some(FilterController::Opponent),
            random: false,
            extra: None,
        };
        assert!(opponent_filter.matches(&creature(p1, CardTypeSet::CREATURE), p0));
        assert!(!opponent_filter.matches(&creature(p0, CardTypeSet::CREATURE), p0));
    }

    #[test]
    fn extra_is_never_guessed_at() {
        let p0 = PlayerId(0);
        let filter = FilterSpec {
            card_types: None,
            controller: None,
            random: false,
            extra: Some("power 4 or greater".to_string()),
        };
        assert!(!filter.matches(&creature(p0, CardTypeSet::CREATURE), p0));
    }
}
