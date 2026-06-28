use crate::{CardTypeSet, GameObject, ManaCost, ObjectId};

/// Structured stand-in for "filter described in oracle text" — e.g.
/// "sacrifice a creature", "discard a card at random", "tap an untapped
/// artifact you control". Covers the common, structurally-expressible
/// shapes; `extra` is a script-bind-time-refined catch-all for filters that
/// don't reduce to type + controller + count (e.g. "with power 4 or
/// greater", named-card restrictions).
pub struct FilterSpec {
    pub card_types: Option<CardTypeSet>,
    pub controller: Option<FilterController>,
    pub random: bool,
    pub extra: Option<String>,
}

impl FilterSpec {
    /// Whether `object` satisfies this filter. Resolving `extra` (if set)
    /// requires the script bridge, so this needs more context than just the
    /// object once that's wired up — signature may grow.
    pub fn matches(&self, object: &GameObject) -> bool {
        todo!()
    }
}

pub enum FilterController {
    You,
    Opponent,
    Any,
}

pub enum AdditionalCost {
    Sacrifice { filter: FilterSpec },
    Discard { filter: FilterSpec },
    TapPermanents { filter: FilterSpec, count: u32 },
    LifePayment { amount: u32 },
    Other { description: String },
}

pub enum AlternativeCost {
    /// e.g. flashback, escape — paying this cost replaces the mana cost entirely.
    Replacement { description: String, cost: CostSpec },
}

pub struct CostSpec {
    pub mana: Option<ManaCost>,
    pub additional: Vec<AdditionalCost>,
    pub alternative: Vec<AlternativeCost>,
}

impl CostSpec {
    pub fn mana_only(cost: ManaCost) -> Self {
        todo!()
    }
}

pub enum CostPayment {
    Mana(ManaCost),
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
    /// Validates this plan atomically against `spec` (every cost present,
    /// nothing extra, mana actually affordable). Returning `bool` here as a
    /// placeholder — real version will likely return a `Result` with a
    /// reason once validation failure modes are designed.
    pub fn validate(&self, spec: &CostSpec) -> bool {
        todo!()
    }
}
