use crate::{
    AtomicManaSymbol, CardTypeSet, Color, GameObject, ManaCost, ManaPool, ManaSymbol, ManaUnit,
    ObjectId,
};

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
        // TODO: card_types: if Some(set), object.definition.card_types must
        // intersect set. controller: if Some(c), compare against whatever
        // "you"/"opponent" resolves to for the player running this filter
        // (needs that player passed in - signature will likely need a
        // `viewer: PlayerId` param). random: doesn't affect matching itself,
        // only which matching object gets picked - irrelevant here. extra:
        // requires the script bridge (ScriptRuntime), not available from
        // this signature yet - matches() will need to grow a context param
        // once that's wired up, same caveat as ContinuousEffect::applies_to.
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
    /// Validates this plan atomically against `spec` (every cost present,
    /// nothing extra, mana actually affordable from `pool`). `pool` is the
    /// payer's floating mana — added as a param beyond the original Phase 2
    /// signature since affordability can't be checked without it (flagged
    /// already in this doc's prior TODO; see docs/implementation.md's
    /// cast-spell-payment slice notes for why `&GameState` wasn't needed
    /// instead — there's no real tapping path yet, so the pool is the only
    /// state this needs to read).
    ///
    /// CR 118.13a's payment choice: a colored pip must be paid with that
    /// *exact* color — no substitution — but the generic component can be
    /// paid with any leftover units, including extra units of a color that
    /// also satisfied a colored pip (e.g. paying `{1}{G}` with three green
    /// units: one for the `{G}` pip, two covering the `{1}`). The `units`
    /// list inside the single `CostPayment::Mana` entry must total *exactly*
    /// the cost — every pip accounted for by the right color, every leftover
    /// unit summing to exactly the generic amount, no more, no less.
    ///
    /// Out of scope this slice (returns `false`, doesn't attempt to honor
    /// them): `spec.additional`/`spec.alternative` costs, and any mana
    /// symbol shape other than plain colored/generic on the *cost* side
    /// (`Colorless`/`Snow`/`Phyrexian`/`Hybrid`/`X` requirements aren't
    /// modeled — see `colored_and_generic`). Returning `bool` here as a
    /// placeholder — real version will likely return a `Result` with a
    /// reason once validation failure modes are designed.
    pub fn validate(&self, spec: &CostSpec, pool: &ManaPool) -> bool {
        if !spec.additional.is_empty() || !spec.alternative.is_empty() {
            return false;
        }
        let required = spec.mana.as_ref().map(|m| m.symbols.as_slice()).unwrap_or(&[]);
        if required.is_empty() {
            // CR 118.5/118.5a: a {0}-cost spell still needs an explicit,
            // empty payment - not auto-cast - but there's nothing to pay,
            // so the only valid plan is one with no payments in it either.
            return self.payments.is_empty();
        }
        let [CostPayment::Mana(units)] = self.payments.as_slice() else { return false };
        let Some((required_colored, generic_needed)) = colored_and_generic(required) else {
            return false;
        };
        let (spent_colored, spent_colorless) = spent_tally(units);

        // Every colored pip must be paid with that exact color - no
        // substitution, checked before anything about totals.
        if required_colored.iter().zip(spent_colored).any(|(need, have)| have < *need) {
            return false;
        }
        // Whatever's spent beyond each pip's exact requirement, plus any
        // colorless spent, must cover the generic component exactly.
        let leftover: u32 = spent_colored
            .iter()
            .zip(required_colored)
            .map(|(have, need)| have - need)
            .sum::<u32>()
            + spent_colorless;
        if leftover != generic_needed {
            return false;
        }
        // Finally, the pool must actually hold what's being spent.
        let pool_colored = [pool.white, pool.blue, pool.black, pool.red, pool.green];
        if spent_colored.iter().zip(pool_colored).any(|(spent, have)| *spent > have) {
            return false;
        }
        spent_colorless <= pool.colorless
    }
}
