use crate::{
    AtomicManaSymbol, CardTypeSet, Color, GameObject, GameState, ManaCost, ManaPool, ManaSymbol,
    ManaUnit, ObjectId, PlayerId, Subtype, SupertypeSet, ZoneKind,
};

/// Structured stand-in for "filter described in oracle text" — e.g.
/// "sacrifice a creature", "discard a card at random", "tap an untapped
/// artifact you control", "search your library for a basic land card".
/// Covers the common, structurally-expressible shapes; `extra` is a
/// script-bind-time-refined catch-all for filters that don't reduce to
/// type/supertype/subtype + controller + count (e.g. "with power 4 or
/// greater", named-card restrictions).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FilterSpec {
    pub card_types: Option<CardTypeSet>,
    /// CR 205.4a supertypes — e.g. `Some(SupertypeSet::BASIC)` for "basic
    /// land card". Added alongside backlog item 14b (Search/tutor), since a
    /// fetch land's own filter is the overwhelmingly common real-world case
    /// this was missing; every other `FilterSpec` caller (Sacrifice/Discard/
    /// TapPermanents costs) benefits for free by leaving this `None`.
    pub supertypes: Option<SupertypeSet>,
    /// CR 205.4b/205.4g subtypes — e.g. `Some(vec![Subtype("Forest")])` for
    /// "Forest card", or `Some(vec![Subtype("Angel")])` for "Angel card".
    /// Matches if `object` has *any* subtype in this list (an "or", not an
    /// "and") — no real card asks a search filter for two subtypes at once.
    pub subtypes: Option<Vec<Subtype>>,
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
        if let Some(supertypes) = self.supertypes {
            if !object.definition.supertypes.intersects(supertypes) {
                return false;
            }
        }
        if let Some(subtypes) = &self.subtypes {
            if !subtypes.iter().any(|s| object.definition.subtypes.contains(s)) {
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
    /// e.g. flashback, escape — paying this cost replaces the mana cost
    /// entirely, and is announced as part of casting (CR 601.2b), not
    /// decided at payment time (same phase as an `{X}` announcement).
    Replacement {
        description: String,
        cost: CostSpec,
        /// Which zone the card must actually be in for this alternative to
        /// apply (flashback: `Graveyard`) — CR 601.2b's alternative-cost
        /// permission and the zone permission it grants ("you may cast this
        /// from your graveyard") are the same grant, not two separate
        /// checks. Named generically, not "flashback-specific", so
        /// escape/jump-start/aftermath/mayhem can reuse this same shape
        /// later (all of them are "cast from graveyard/exile, replace the
        /// normal cost").
        from_zone: ZoneKind,
        /// CR 702.34a: a spell cast this way is exiled instead of going to
        /// its normal destination (graveyard on resolving or fizzling) when
        /// it next leaves the stack. Baked onto `StackObject` at push time
        /// once this alternative is chosen (see `StackObject::exile_on_leaving_stack`).
        exile_on_leaving_stack: bool,
    },
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

/// CR 601.2f/118.9: a static "spells [you cast/of some type] cost `{N}`
/// more/less to cast" effect (Goblin Warchief, Thalia, ...) — deliberately
/// **not** a `ContinuousEffect`/CR 613 layer: cost modification isn't a
/// characteristic query, it's a one-time adjustment applied while
/// determining a spell's total cost (CR 601.2f), so it needs its own
/// mechanism rather than extending `layers::effective_characteristics`.
///
/// Scoped to spells only this slice (an activated ability's own cost isn't
/// touched, same "spells first" narrowing `{X}`/alternative-costs both
/// took) and to a flat, fixed `amount` — no dynamic/count-dependent amounts
/// (e.g. Affinity), no colored-pip-specific modification (only the generic
/// component, see `ManaCost::apply_cost_delta`).
///
/// `filter`/`controller` reuse `cost.rs`'s existing `FilterSpec` rather than
/// inventing a parallel "which spells" shape: `FilterSpec::matches` already
/// takes an arbitrary `&GameObject` + a `viewer: PlayerId` and answers
/// card_types/relative-controller questions, and a spell being cast is
/// still a plain `GameObject` (with `.controller` = caster) at the point
/// `commands.rs` needs to ask "does this modifier apply" — no new type
/// needed, `viewer` is just `self.controller` (whose "You"/"Opponent"
/// perspective the modifier's own text is written from).
///
/// TODO(CR 613.7-style ordering): when two modifiers could apply at the same
/// timestamp, the affected player chooses application order; this engine
/// has no timestamp/ordering-choice concept for cost modifiers yet —
/// `apply_cost_modifiers` below applies matches in `state.cost_modifiers`'s
/// registration order instead, un-chosen.
/// TODO: no `Game.CreateCostModifier` script builder yet — same "Rust
/// struct-literal fixtures only" bootstrap stance replacement effects and
/// static abilities each took before their own builder slice landed. Only
/// constructible today via a direct `CostModifier` literal.
#[derive(Clone, Debug)]
pub struct CostModifier {
    pub source: ObjectId,
    /// Whose "you"/"opponent" the modifier's own `filter` is relative to —
    /// e.g. "spells your opponents cast cost {1} more" has `controller` set
    /// to whoever controls this effect's source, and `filter.controller ==
    /// Some(FilterController::Opponent)`.
    pub controller: PlayerId,
    pub filter: FilterSpec,
    /// Negative reduces, positive increases the generic component (CR
    /// 118.9's `{0}` floor is enforced by `ManaCost::apply_cost_delta`, not
    /// here).
    pub amount: i32,
}

/// Applies every `CostModifier` in `modifiers` whose `filter` matches
/// `spell` (evaluated from that modifier's own `controller`'s perspective),
/// one at a time in list order, each via `ManaCost::apply_cost_delta` — see
/// that method's own doc comment for why this isn't "sum every delta, then
/// clamp once."
pub fn apply_cost_modifiers(base: &ManaCost, modifiers: &[CostModifier], spell: &GameObject) -> ManaCost {
    let mut cost = base.clone();
    for modifier in modifiers {
        if modifier.filter.matches(spell, modifier.controller) {
            cost = cost.apply_cost_delta(modifier.amount);
        }
    }
    cost
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

/// One way to settle a single symbol's requirement — the unit of choice
/// that `assign_branches`'s backtracking search picks between. `Color`
/// reserves one dedicated unit of that color out of the shared pool;
/// `Snow` reserves one dedicated snow-tagged unit of *any* color/colorless
/// (CR 107.4h); `Generic`/`Life` don't reserve anything up front, they add
/// to a running total settled by the final exactness check (a
/// leftover-units total for `Generic`, a declared-life total for `Life`).
#[derive(Clone, Copy)]
enum PayBranch {
    Color(Color),
    Snow,
    Generic(u32),
    Life(u32),
}

/// The branches a single atom offers (CR 107.4f: a `Phyrexian` atom is
/// inherently two-way — pay that color, or pay 2 life — even outside a
/// `Hybrid`). `None` for atoms this slice doesn't support paying for yet
/// (`Colorless` — the `{C}` symbol, unrelated to `ManaUnit::Colorless` —
/// isn't modeled as its own cost-side atom yet).
fn atom_branches(atom: AtomicManaSymbol) -> Option<Vec<PayBranch>> {
    match atom {
        AtomicManaSymbol::Colored(c) => Some(vec![PayBranch::Color(c)]),
        AtomicManaSymbol::Generic(n) => Some(vec![PayBranch::Generic(n)]),
        AtomicManaSymbol::Phyrexian(c) => Some(vec![PayBranch::Color(c), PayBranch::Life(2)]),
        AtomicManaSymbol::Snow => Some(vec![PayBranch::Snow]),
        AtomicManaSymbol::Colorless => None,
    }
}

/// The branches a whole cost symbol offers. A plain `Atom` just forwards
/// its atom's branches (a plain colored/generic pip ends up with exactly
/// one branch — no real choice, but the same machinery handles it). A
/// `Hybrid(a, b)` (CR 107.4e/f) offers the union of both atoms' branches —
/// this is what lets a hybrid Phyrexian symbol like `{W/U/P}` end up with
/// three real options (`Colored(U)`'s one branch plus `Phyrexian(W)`'s two)
/// out of only two atom slots, per mana.rs's own doc comment on `Hybrid`.
/// `None` (an unsupported atom shape, or bare `X`) means this whole symbol
/// isn't payable by this slice.
fn symbol_branches(symbol: &ManaSymbol) -> Option<Vec<PayBranch>> {
    match symbol {
        ManaSymbol::Atom(atom) => atom_branches(*atom),
        ManaSymbol::Hybrid(a, b) => {
            let mut branches = atom_branches(*a)?;
            branches.extend(atom_branches(*b)?);
            Some(branches)
        }
        ManaSymbol::X => None,
    }
}

/// Snapshot of the payer's remaining declared units as `assign_branches`'s
/// backtracking search consumes them — mirrors `ManaPool`'s color/colorless
/// split, plus the snow subcounts CR 107.4h's `{S}` symbol needs.
/// `snow_colored[i] <= colored[i]` (and `snow_colorless <= colorless`) is an
/// invariant every mutation below preserves, same as `ManaPool` itself.
#[derive(Clone, Copy)]
struct SpentPool {
    colored: [u32; 5],
    colorless: u32,
    snow_colored: [u32; 5],
    snow_colorless: u32,
}

/// Checks whether `spent` (the payer's chosen units, already tallied by
/// `spent_tally`) plus `phyrexian_life` (the payer's declared life payment,
/// CR 107.4f) can satisfy `required` (the cost's symbols) per CR 118.13a.
/// `None` means `required` contains a symbol shape this slice doesn't
/// support paying for yet (see `symbol_branches`/`atom_branches` for exactly
/// what's unsupported and why). `Some(false)` means every symbol is a
/// supported shape but this particular payment can't cover them.
///
/// Which branch each symbol takes isn't decidable by tallying once 2+
/// multi-branch symbols compete for the same pool of spent units — e.g.
/// two `{W/U}` paid with one white + one blue unit is legal only if each
/// symbol claims a *different* unit. So this is a small backtracking
/// assignment search over every symbol's branch options, not a sum
/// comparison — costs are never more than a handful of symbols, so this
/// doesn't need real search-algorithm sophistication.
fn mana_payment_satisfies(
    required: &[ManaSymbol],
    spent: SpentPool,
    phyrexian_life: u32,
) -> Option<bool> {
    let mut branch_sets = Vec::with_capacity(required.len());
    for symbol in required {
        branch_sets.push(symbol_branches(symbol)?);
    }
    Some(assign_branches(&branch_sets, 0, spent, 0, 0, phyrexian_life))
}

/// Backtracking step for `mana_payment_satisfies` — tries each remaining
/// symbol's branch options against the current leftover pool, recursing
/// before committing. At the base case (`idx` past the last symbol), the
/// pool remaining after every `Color`/`Snow` reservation must equal the
/// accumulated `Generic` total exactly; and the accumulated `Life` total
/// must equal the payer's declared `phyrexian_life` exactly — no more, no
/// less either way (CR 118.13a).
///
/// `Color(c)` tries *both* a non-snow and a snow-tagged unit of `c` when
/// both are available, not just "any unit of color `c`" — which specific
/// unit backs a colored pip changes what's left over for a later `Snow`
/// symbol to claim, so it's part of the same search, not an independent
/// choice. `Snow` tries every color-or-colorless bucket that still has a
/// snow-tagged unit (CR 107.4h: `{S}` doesn't care which type, only that it
/// came from a snow source) — removing one always debits both the color
/// total and its snow subcount together, keeping the invariant intact.
fn assign_branches(
    branch_sets: &[Vec<PayBranch>],
    idx: usize,
    spent: SpentPool,
    generic_needed: u32,
    life_needed: u32,
    phyrexian_life: u32,
) -> bool {
    if idx == branch_sets.len() {
        let leftover: u32 = spent.colored.iter().sum::<u32>() + spent.colorless;
        return leftover == generic_needed && life_needed == phyrexian_life;
    }
    for branch in &branch_sets[idx] {
        match branch {
            PayBranch::Color(c) => {
                let ci = color_index(*c);
                if spent.colored[ci] > spent.snow_colored[ci] {
                    let mut next = spent;
                    next.colored[ci] -= 1;
                    if assign_branches(branch_sets, idx + 1, next, generic_needed, life_needed, phyrexian_life)
                    {
                        return true;
                    }
                }
                if spent.snow_colored[ci] > 0 {
                    let mut next = spent;
                    next.colored[ci] -= 1;
                    next.snow_colored[ci] -= 1;
                    if assign_branches(branch_sets, idx + 1, next, generic_needed, life_needed, phyrexian_life)
                    {
                        return true;
                    }
                }
            }
            PayBranch::Snow => {
                for ci in 0..5 {
                    if spent.snow_colored[ci] > 0 {
                        let mut next = spent;
                        next.colored[ci] -= 1;
                        next.snow_colored[ci] -= 1;
                        if assign_branches(
                            branch_sets,
                            idx + 1,
                            next,
                            generic_needed,
                            life_needed,
                            phyrexian_life,
                        ) {
                            return true;
                        }
                    }
                }
                if spent.snow_colorless > 0 {
                    let mut next = spent;
                    next.colorless -= 1;
                    next.snow_colorless -= 1;
                    if assign_branches(branch_sets, idx + 1, next, generic_needed, life_needed, phyrexian_life)
                    {
                        return true;
                    }
                }
            }
            PayBranch::Generic(n) => {
                if assign_branches(branch_sets, idx + 1, spent, generic_needed + n, life_needed, phyrexian_life)
                {
                    return true;
                }
            }
            PayBranch::Life(n) => {
                if assign_branches(branch_sets, idx + 1, spent, generic_needed, life_needed + n, phyrexian_life)
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Tallies `units` into per-color spent counts, total colorless spent, and
/// their snow subcounts (CR 107.4h) — the payer's-side mirror of
/// `atom_branches`, but over literal `ManaUnit`s rather than cost-shape
/// symbols (there's no "generic unit"; every unit spent is some specific
/// color or colorless, snow-tagged or not).
fn spent_tally(units: &[ManaUnit]) -> ([u32; 5], u32, [u32; 5], u32) {
    let mut colored = [0u32; 5];
    let mut colorless = 0u32;
    let mut snow_colored = [0u32; 5];
    let mut snow_colorless = 0u32;
    for unit in units {
        match unit {
            ManaUnit::Colored(c) => colored[color_index(*c)] += 1,
            ManaUnit::Colorless => colorless += 1,
            ManaUnit::SnowColored(c) => {
                colored[color_index(*c)] += 1;
                snow_colored[color_index(*c)] += 1;
            }
            ManaUnit::SnowColorless => {
                colorless += 1;
                snow_colorless += 1;
            }
        }
    }
    (colored, colorless, snow_colored, snow_colorless)
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
            ManaUnit::SnowColored(Color::White) => {
                pool.white -= 1;
                pool.snow_white -= 1;
            }
            ManaUnit::SnowColored(Color::Blue) => {
                pool.blue -= 1;
                pool.snow_blue -= 1;
            }
            ManaUnit::SnowColored(Color::Black) => {
                pool.black -= 1;
                pool.snow_black -= 1;
            }
            ManaUnit::SnowColored(Color::Red) => {
                pool.red -= 1;
                pool.snow_red -= 1;
            }
            ManaUnit::SnowColored(Color::Green) => {
                pool.green -= 1;
                pool.snow_green -= 1;
            }
            ManaUnit::SnowColorless => {
                pool.colorless -= 1;
                pool.snow_colorless -= 1;
            }
        }
    }
}

pub enum CostPayment {
    /// The exact literal payment for a mana cost (CR 118.13a) — not an echo
    /// of the cost's shape. `units` is the exact literal mana spent from the
    /// pool; `phyrexian_life` is however much life the payer declared toward
    /// Phyrexian-mana pips (CR 107.4f) paid via the life option instead of
    /// colored mana — 0 when the cost has no Phyrexian symbols, or every
    /// Phyrexian pip present was paid with mana instead. See `cost.rs`'s
    /// `PaymentPlan` doc comment for why this is the payer's *choice*, not a
    /// restatement of what was owed.
    Mana { units: Vec<ManaUnit>, phyrexian_life: u32 },
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
    /// `{X}` costs, and the `{C}` colorless mana symbol on the *cost* side
    /// (not modeled as its own atom yet — see `atom_branches`). Plain
    /// colored/generic pips, `Phyrexian` pips (paid with colored mana or 2
    /// life, CR 107.4f), `Snow` pips (paid with any snow-tagged mana, CR
    /// 107.4h), and `Hybrid` symbols built from any of those atoms (`{W/U}`,
    /// `{2/W}`, `{W/U/P}`) *are* handled. `AdditionalCost::Other` always
    /// fails validation too — no `CostPayment` shape can express an
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
                let Some(CostPayment::Mana { units, phyrexian_life }) = payments.next() else {
                    return false;
                };
                let (spent_colored, spent_colorless, spent_snow_colored, spent_snow_colorless) =
                    spent_tally(units);
                let spent = SpentPool {
                    colored: spent_colored,
                    colorless: spent_colorless,
                    snow_colored: spent_snow_colored,
                    snow_colorless: spent_snow_colorless,
                };
                match mana_payment_satisfies(required, spent, *phyrexian_life) {
                    Some(true) => {}
                    Some(false) | None => return false,
                }
                let Some(player) = state.player(payer) else { return false };
                // CR 119.4: a player can't pay more life toward Phyrexian
                // pips than they have.
                if player.life < *phyrexian_life as i32 {
                    return false;
                }
                let pool = &player.mana_pool;
                let pool_colored = [pool.white, pool.blue, pool.black, pool.red, pool.green];
                if spent_colored.iter().zip(pool_colored).any(|(spent, have)| *spent > have) {
                    return false;
                }
                if spent_colorless > pool.colorless {
                    return false;
                }
                // The snow-tagged units declared can't exceed how much of
                // the pool is actually snow-tagged - the color/colorless
                // check above only bounds the total, not which of it is
                // snow-sourced (CR 107.4h).
                let pool_snow_colored =
                    [pool.snow_white, pool.snow_blue, pool.snow_black, pool.snow_red, pool.snow_green];
                if spent_snow_colored.iter().zip(pool_snow_colored).any(|(spent, have)| *spent > have) {
                    return false;
                }
                if spent_snow_colorless > pool.snow_colorless {
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
    use crate::types::card::{CardDefinition, KeywordSet, PTValue, ScriptReference, Subtype, SupertypeSet};
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
                alternative_costs: Vec::new(),
                effect: None,
                modes: Vec::new(),
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
            is_commander: false,
            is_token: false,
            visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
        }
    }

    #[test]
    fn card_types_true_and_false_cases() {
        let p0 = PlayerId(0);
        let filter = FilterSpec {
            card_types: Some(CardTypeSet::CREATURE),
            supertypes: None,
            subtypes: None,
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
            supertypes: None,
            subtypes: None,
            controller: Some(FilterController::You),
            random: false,
            extra: None,
        };
        assert!(you_filter.matches(&creature(p0, CardTypeSet::CREATURE), p0));
        assert!(!you_filter.matches(&creature(p1, CardTypeSet::CREATURE), p0));

        let opponent_filter = FilterSpec {
            card_types: None,
            supertypes: None,
            subtypes: None,
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
            supertypes: None,
            subtypes: None,
            controller: None,
            random: false,
            extra: Some("power 4 or greater".to_string()),
        };
        assert!(!filter.matches(&creature(p0, CardTypeSet::CREATURE), p0));
    }

    #[test]
    fn supertypes_true_and_false_cases() {
        let p0 = PlayerId(0);
        let mut basic_land = creature(p0, CardTypeSet::LAND);
        basic_land.definition.supertypes = SupertypeSet::BASIC;
        let nonbasic_land = creature(p0, CardTypeSet::LAND);
        let filter = FilterSpec {
            card_types: None,
            supertypes: Some(SupertypeSet::BASIC),
            subtypes: None,
            controller: None,
            random: false,
            extra: None,
        };
        assert!(filter.matches(&basic_land, p0), "a basic land card matches Some(SupertypeSet::BASIC)");
        assert!(!filter.matches(&nonbasic_land, p0), "a nonbasic land doesn't match");
    }

    #[test]
    fn subtypes_true_and_false_cases() {
        let p0 = PlayerId(0);
        let mut forest = creature(p0, CardTypeSet::LAND);
        forest.definition.subtypes = vec![Subtype("Forest".to_string())];
        let mut island = creature(p0, CardTypeSet::LAND);
        island.definition.subtypes = vec![Subtype("Island".to_string())];
        let filter = FilterSpec {
            card_types: None,
            supertypes: None,
            subtypes: Some(vec![Subtype("Forest".to_string())]),
            controller: None,
            random: false,
            extra: None,
        };
        assert!(filter.matches(&forest, p0), "a Forest card matches a Forest-subtype filter");
        assert!(!filter.matches(&island, p0), "an Island doesn't match a Forest-only filter");
    }

    fn spent(colored: [u32; 5], colorless: u32) -> SpentPool {
        SpentPool { colored, colorless, snow_colored: [0; 5], snow_colorless: 0 }
    }

    fn spent_snow(colored: [u32; 5], colorless: u32, snow_colored: [u32; 5], snow_colorless: u32) -> SpentPool {
        SpentPool { colored, colorless, snow_colored, snow_colorless }
    }

    #[test]
    fn hybrid_color_color_paid_with_either_color() {
        let required = [ManaSymbol::Hybrid(
            AtomicManaSymbol::Colored(Color::White),
            AtomicManaSymbol::Colored(Color::Blue),
        )];
        // {W/U} paid with white - legal.
        assert_eq!(mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 0), 0), Some(true));
        // {W/U} paid with blue - legal.
        assert_eq!(mana_payment_satisfies(&required, spent([0, 1, 0, 0, 0], 0), 0), Some(true));
        // {W/U} paid with black - rejected (not one of the two options).
        assert_eq!(mana_payment_satisfies(&required, spent([0, 0, 1, 0, 0], 0), 0), Some(false));
    }

    #[test]
    fn hybrid_generic_color_paid_either_branch() {
        let required = [ManaSymbol::Hybrid(
            AtomicManaSymbol::Generic(2),
            AtomicManaSymbol::Colored(Color::White),
        )];
        // {2/W} paid with 2 generic-eligible (colorless) units - legal.
        assert_eq!(mana_payment_satisfies(&required, spent([0, 0, 0, 0, 0], 2), 0), Some(true));
        // {2/W} paid with 1 white - legal via the color branch, 0 generic owed.
        assert_eq!(mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 0), 0), Some(true));
    }

    #[test]
    fn two_hybrid_symbols_require_real_assignment_not_just_a_count_check() {
        let required = [
            ManaSymbol::Hybrid(
                AtomicManaSymbol::Colored(Color::White),
                AtomicManaSymbol::Colored(Color::Blue),
            ),
            ManaSymbol::Hybrid(
                AtomicManaSymbol::Colored(Color::White),
                AtomicManaSymbol::Colored(Color::Blue),
            ),
        ];
        // One white + one blue - legal, but only because each symbol can
        // claim a different unit (a plain tally can't tell this apart from
        // an unpayable case without the assignment).
        assert_eq!(mana_payment_satisfies(&required, spent([1, 1, 0, 0, 0], 0), 0), Some(true));
        // Two white - also legal, both symbols take the white branch.
        assert_eq!(mana_payment_satisfies(&required, spent([2, 0, 0, 0, 0], 0), 0), Some(true));
    }

    #[test]
    fn generic_pip_mixed_with_hybrid_leftover_must_match_exactly() {
        let required = [
            ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
            ManaSymbol::Hybrid(
                AtomicManaSymbol::Colored(Color::White),
                AtomicManaSymbol::Colored(Color::Blue),
            ),
        ];
        // {1}{W/U} paid with white (for the hybrid) + one colorless (for
        // the plain generic) - legal.
        assert_eq!(mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 1), 0), Some(true));
        // Same payment but one colorless short - the generic component
        // isn't fully covered, rejected.
        assert_eq!(mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 0), 0), Some(false));
        // One extra, unneeded colorless unit - overpaying is rejected too
        // (leftover must equal the generic component exactly, no more).
        assert_eq!(mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 2), 0), Some(false));
    }

    #[test]
    fn plain_phyrexian_paid_with_color_or_life() {
        let required = [ManaSymbol::Atom(AtomicManaSymbol::Phyrexian(Color::White))];
        // {W/P} paid with white - legal.
        assert_eq!(mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 0), 0), Some(true));
        // {W/P} paid with 2 life, no mana - legal.
        assert_eq!(mana_payment_satisfies(&required, spent([0; 5], 0), 2), Some(true));
        // {W/P} paid with black - rejected, not the pip's color and no life declared.
        assert_eq!(mana_payment_satisfies(&required, spent([0, 0, 1, 0, 0], 0), 0), Some(false));
        // {W/P} "paid" with only 1 life - short of the flat 2, rejected.
        assert_eq!(mana_payment_satisfies(&required, spent([0; 5], 0), 1), Some(false));
    }

    #[test]
    fn hybrid_phyrexian_offers_three_real_branches() {
        // {G/U/P}-style: Hybrid(Phyrexian(Blue), Colored(Green)) - pay blue,
        // pay green, or pay 2 life (mana.rs's own doc comment on `Hybrid`).
        let required = [ManaSymbol::Hybrid(
            AtomicManaSymbol::Phyrexian(Color::Blue),
            AtomicManaSymbol::Colored(Color::Green),
        )];
        assert_eq!(mana_payment_satisfies(&required, spent([0, 1, 0, 0, 0], 0), 0), Some(true));
        assert_eq!(mana_payment_satisfies(&required, spent([0, 0, 0, 0, 1], 0), 0), Some(true));
        assert_eq!(mana_payment_satisfies(&required, spent([0; 5], 0), 2), Some(true));
        // Red is none of the three options - rejected.
        assert_eq!(mana_payment_satisfies(&required, spent([0, 0, 0, 1, 0], 0), 0), Some(false));
    }

    #[test]
    fn generic_pip_mixed_with_phyrexian_life_must_match_exactly() {
        let required = [
            ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
            ManaSymbol::Atom(AtomicManaSymbol::Phyrexian(Color::White)),
        ];
        // {1}{W/P} paid with 2 life for the Phyrexian pip + 1 colorless for
        // the generic - legal.
        assert_eq!(mana_payment_satisfies(&required, spent([0; 5], 1), 2), Some(true));
        // Declaring only 1 life toward a pip that needs the full 2 - rejected.
        assert_eq!(mana_payment_satisfies(&required, spent([0; 5], 1), 1), Some(false));
        // Declaring life the cost never asked for (paid the pip with white
        // mana instead, but still declared 2 life) - rejected, no more no less.
        assert_eq!(mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 1), 2), Some(false));
    }

    #[test]
    fn snow_pip_paid_with_any_snow_tagged_mana() {
        let required = [ManaSymbol::Atom(AtomicManaSymbol::Snow)];
        // {S} paid with a snow-tagged white unit - legal.
        assert_eq!(
            mana_payment_satisfies(&required, spent_snow([1, 0, 0, 0, 0], 0, [1, 0, 0, 0, 0], 0), 0),
            Some(true)
        );
        // {S} paid with a snow-tagged colorless unit - legal (any type works).
        assert_eq!(mana_payment_satisfies(&required, spent_snow([0; 5], 1, [0; 5], 1), 0), Some(true));
        // {S} "paid" with an ordinary (non-snow) white unit - rejected, right
        // color but wrong source.
        assert_eq!(mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 0), 0), Some(false));
    }

    #[test]
    fn colored_pip_and_snow_pip_compete_for_the_same_snow_unit() {
        // {W}{S} with exactly one snow white unit and nothing else - only
        // one symbol can claim it, so this must be rejected (not a plain
        // tally, which could wrongly see "1 white available" and think
        // both symbols are covered).
        let required =
            [ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White)), ManaSymbol::Atom(AtomicManaSymbol::Snow)];
        assert_eq!(
            mana_payment_satisfies(&required, spent_snow([1, 0, 0, 0, 0], 0, [1, 0, 0, 0, 0], 0), 0),
            Some(false)
        );
        // Same cost, but with a second, non-snow white unit available too -
        // now {W} can take the non-snow one, freeing the snow one for {S}.
        assert_eq!(
            mana_payment_satisfies(&required, spent_snow([2, 0, 0, 0, 0], 0, [1, 0, 0, 0, 0], 0), 0),
            Some(true)
        );
    }

    #[test]
    fn generic_pip_mixed_with_snow_leftover_must_match_exactly() {
        let required =
            [ManaSymbol::Atom(AtomicManaSymbol::Generic(1)), ManaSymbol::Atom(AtomicManaSymbol::Snow)];
        // {1}{S} paid with a snow white unit (for {S}) + a colorless unit
        // (for the generic) - legal.
        assert_eq!(
            mana_payment_satisfies(&required, spent_snow([1, 0, 0, 0, 0], 1, [1, 0, 0, 0, 0], 0), 0),
            Some(true)
        );
        // Same, but the generic component is short - rejected.
        assert_eq!(
            mana_payment_satisfies(&required, spent_snow([1, 0, 0, 0, 0], 0, [1, 0, 0, 0, 0], 0), 0),
            Some(false)
        );
    }

    #[test]
    fn unsupported_symbol_shapes_bail_with_none() {
        // The {C} colorless cost symbol and bare X requirements aren't
        // modeled yet.
        assert_eq!(
            mana_payment_satisfies(&[ManaSymbol::Atom(AtomicManaSymbol::Colorless)], spent([0; 5], 1), 0),
            None
        );
        assert_eq!(mana_payment_satisfies(&[ManaSymbol::X], spent([0; 5], 0), 0), None);
    }
}
