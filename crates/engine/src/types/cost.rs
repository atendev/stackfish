use crate::script::ScriptRuntime;
use crate::{
    AtomicManaSymbol, CardTypeSet, Color, ColorSet, GameObject, GameState, ManaCost, ManaPool,
    ManaSymbol, ManaUnit, ObjectId, PlayerId, Subtype, SupertypeSet, ZoneKind,
};

/// Structured stand-in for an oracle-text filter such as "sacrifice a
/// creature" or "search your library for a basic land card". `extra` is a
/// catch-all refined at script bind time for filters that do not reduce to
/// type, supertype, subtype, controller and count ("power 4 or greater",
/// named-card restrictions).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FilterSpec {
    pub card_types: Option<CardTypeSet>,
    /// CR 205.4a supertypes, such as `Some(SupertypeSet::BASIC)` for "basic
    /// land card". `None` for every caller that doesn't care.
    pub supertypes: Option<SupertypeSet>,
    /// CR 205.4b, 205.4g subtypes. Matches if `object` has any listed
    /// subtype, since no real card asks a search filter for two at once.
    pub subtypes: Option<Vec<Subtype>>,
    pub controller: Option<FilterController>,
    pub random: bool,
    pub extra: Option<String>,
}

impl FilterSpec {
    /// Whether `object` satisfies this filter from `viewer`'s perspective
    /// (`You` and `Opponent` are relative to the payer). The caller resolves
    /// `object_controller` with `layers::effective_controller` (CR 108.4),
    /// because this type never depends on `ScriptRuntime`. `random` doesn't
    /// affect matching. A filter with `extra` set never matches, since
    /// resolving it needs the script bridge.
    pub(crate) fn matches(
        &self,
        object: &GameObject,
        object_controller: PlayerId,
        viewer: PlayerId,
    ) -> bool {
        if self.extra.is_some() {
            return false;
        }
        if let Some(types) = self.card_types
            && !object.definition.card_types.intersects(types)
        {
            return false;
        }
        if let Some(supertypes) = self.supertypes
            && !object.definition.supertypes.intersects(supertypes)
        {
            return false;
        }
        if let Some(subtypes) = &self.subtypes
            && !subtypes
                .iter()
                .any(|s| object.definition.subtypes.contains(s))
        {
            return false;
        }
        if let Some(fc) = self.controller {
            let ok = match fc {
                FilterController::You => object_controller == viewer,
                FilterController::Opponent => object_controller != viewer,
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

#[derive(Clone, Debug, PartialEq)]
pub enum AdditionalCost {
    /// CR 602.5a: `{T}` on the ability's own source, as opposed to
    /// `TapPermanents`, which taps other permanents. Always the activating
    /// object (`PaymentPlan::validate`'s `source`).
    TapSelf,
    Sacrifice {
        filter: FilterSpec,
    },
    /// Sacrifice the ability's own source. `Sacrifice{filter}` cannot express
    /// this, because its filter never checks identity and another matching
    /// permanent could pay.
    SacrificeSelf,
    Discard {
        filter: FilterSpec,
    },
    /// CR 702.29a Cycling's "Discard this card": always the ability's own
    /// source, unlike `Discard { filter }`, which lets the payer discard any
    /// matching hand card.
    DiscardSelf,
    TapPermanents {
        filter: FilterSpec,
        count: u32,
    },
    /// CR 702.138a Escape's "Exile N other cards from your graveyard", in the
    /// `TapPermanents` shape against the graveyard. The escaping card is
    /// already on the stack (CR 601.2a), so it is never a candidate.
    ExileFromGraveyard {
        filter: FilterSpec,
        count: u32,
    },
    LifePayment {
        amount: u32,
    },
    Other {
        description: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum AlternativeCost {
    /// Flashback, escape and similar: replaces the mana cost entirely and is
    /// announced as part of casting (CR 601.2b), like `{X}`.
    Replacement {
        description: String,
        cost: CostSpec,
        /// Zone the card must be in for this alternative to apply (flashback:
        /// `Graveyard`). The cost permission and the zone permission are one
        /// grant (CR 601.2b). Generic so escape, jump-start and aftermath can
        /// reuse it.
        from_zone: ZoneKind,
        /// CR 702.34a: exile instead of the normal destination when it leaves
        /// the stack. Copied to `StackObject::exile_on_leaving_stack` at push.
        exile_on_leaving_stack: bool,
        /// CR 702.138b: Escape's own alternative cost, `false` for Flashback
        /// and the rest. Both cast from the graveyard, so this is the only
        /// mark of an "escaped" cast. It is its own field because
        /// `exile_on_leaving_stack` means something unrelated. Copied to
        /// `StackObject::escaped`, then to `GameObject::entered_via_escape`.
        escape: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct CostSpec {
    pub mana: Option<ManaCost>,
    pub additional: Vec<AdditionalCost>,
    pub alternative: Vec<AlternativeCost>,
}

impl CostSpec {
    pub fn mana_only(cost: ManaCost) -> Self {
        CostSpec {
            mana: Some(cost),
            additional: Vec::new(),
            alternative: Vec::new(),
        }
    }
}

/// CR 601.2f, 118.9: a static "spells cost {N} more or less" effect (Goblin
/// Warchief, Thalia). Not a CR 613 layer, because it is a one-time adjustment
/// while determining the total cost and not a characteristic query.
///
/// Scoped to spells and to a fixed `amount`; ability costs and
/// count-dependent amounts (Affinity) are not covered. Colored-pip
/// modification (CR 118.7b-d) is covered; see `target`.
///
/// `filter` and `controller` reuse `FilterSpec`, which already answers card
/// type and relative controller questions for a plain `GameObject`. The
/// viewer is `self.controller`, whose "you" the text is written from.
///
/// `apply_cost_modifiers` sums every modifier per component (generic, each
/// color, colorless) and floors once, so registration order does not matter.
/// Applying them one by one differs when an increase and a reduction both
/// match: base `{1}` with `-2` then `+1` gives `{1}`, and the reverse gives
/// `{0}`. The sum stays order-independent for a colored component only while
/// every modifier of that color is a reduction. A mixed increase and
/// reduction on one color would bring the order dependence back, which is why
/// `target` limits `Colored` and `Colorless` to reductions.
///
/// `Game.CreateCostModifier` (`CostModifierBuilder`) registers one from a
/// script, and a Rust struct literal works too.
#[derive(Clone, Debug, PartialEq)]
pub struct CostModifier {
    pub source: ObjectId,
    /// Whose "you" and "opponent" `filter` is relative to. "Spells your
    /// opponents cast cost {1} more" sets this to the source's controller and
    /// `filter.controller` to `Opponent`.
    pub controller: PlayerId,
    pub filter: FilterSpec,
    pub target: CostModifierTarget,
    /// Negative reduces and positive increases the `target` component. The
    /// `{0}` floor (CR 118.9) is enforced by the `apply_*_cost_delta` methods.
    pub amount: i32,
}

/// CR 118.7: which mana component a `CostModifier`'s `amount` affects.
/// `Generic` is unconditional in both directions. `Colored` and `Colorless`
/// implement CR 118.7b-d: with none of that component, the whole `amount`
/// goes to generic (CR 118.7b), and a reduction larger than the component
/// zeroes it and spills the excess to generic (CR 118.7c, 118.7d). They cover
/// reductions only, since no real card increases a specific pip. A positive
/// `amount` just adds that many pips and is untested; mixing it with a
/// same-color reduction would bring back order dependence (see `CostModifier`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CostModifierTarget {
    Generic,
    Colored(Color),
    Colorless,
}

/// Applies every `CostModifier` whose `filter` matches `spell`, from that
/// modifier's own `controller`'s perspective. Amounts are summed within each
/// group (generic, each color, colorless) before applying, not applied one by
/// one. See `CostModifier` for why.
pub(crate) fn apply_cost_modifiers(
    base: &ManaCost,
    modifiers: &[CostModifier],
    spell: &GameObject,
) -> ManaCost {
    let matching = modifiers
        .iter()
        // `spell.controller` is used as is, since no control-change effect can target a spell on the stack (CR 108.4).
        .filter(|modifier| {
            modifier
                .filter
                .matches(spell, spell.controller, modifier.controller)
        });

    let mut generic_delta = 0i32;
    let mut colored_deltas: Vec<(Color, i32)> = Vec::new();
    let mut colorless_delta = 0i32;
    for modifier in matching {
        match modifier.target {
            CostModifierTarget::Generic => generic_delta += modifier.amount,
            CostModifierTarget::Colored(color) => {
                match colored_deltas.iter_mut().find(|(c, _)| *c == color) {
                    Some((_, delta)) => *delta += modifier.amount,
                    None => colored_deltas.push((color, modifier.amount)),
                }
            }
            CostModifierTarget::Colorless => colorless_delta += modifier.amount,
        }
    }

    let mut result = base.clone();
    // Colored and colorless spillover (CR 118.7b-d) folds into the one generic delta computed below.
    let mut spillover = 0i32;
    for (color, delta) in colored_deltas {
        if delta == 0 {
            continue;
        }
        let (adjusted, spill) = result.apply_colored_cost_delta(color, delta);
        result = adjusted;
        spillover += spill;
    }
    if colorless_delta != 0 {
        let (adjusted, spill) = result.apply_colorless_cost_delta(colorless_delta);
        result = adjusted;
        spillover += spill;
    }

    let net_generic = generic_delta + spillover;
    // A net-zero generic delta must skip the call: `apply_cost_delta(0)` drops an explicit `Generic(0)` symbol, such as an `{X}` cost resolved with X=0 by `resolve_x`, and that marker has to survive.
    if net_generic != 0 {
        result = result.apply_cost_delta(net_generic);
    }
    result
}

/// Indexes the `[u32; 5]` colored-mana tally, shared by
/// `PaymentPlan::validate` and `spend` so the two agree on slot order.
fn color_index(c: Color) -> usize {
    match c {
        Color::White => 0,
        Color::Blue => 1,
        Color::Black => 2,
        Color::Red => 3,
        Color::Green => 4,
    }
}

/// One way to settle a single symbol, the choice `assign_branches` searches
/// over. `Color` reserves one unit of that color. `Snow` reserves one
/// snow-tagged unit of any type (CR 107.4h). `Generic` and `Life` reserve
/// nothing up front and add to totals settled by the final exactness check.
#[derive(Clone, Copy)]
enum PayBranch {
    Color(Color),
    /// CR 107.4c: `{C}` is paid only with one colorless mana.
    Colorless,
    Snow,
    Generic(u32),
    Life(u32),
}

/// Branches for one atom. A `Phyrexian` atom always has two (that color or 2
/// life, CR 107.4f), even outside a `Hybrid`. `Colorless` is `{C}`
/// (CR 107.4c), payable only with colorless mana.
fn atom_branches(atom: AtomicManaSymbol) -> Option<Vec<PayBranch>> {
    match atom {
        AtomicManaSymbol::Colored(c) => Some(vec![PayBranch::Color(c)]),
        AtomicManaSymbol::Generic(n) => Some(vec![PayBranch::Generic(n)]),
        AtomicManaSymbol::Phyrexian(c) => Some(vec![PayBranch::Color(c), PayBranch::Life(2)]),
        AtomicManaSymbol::Snow => Some(vec![PayBranch::Snow]),
        AtomicManaSymbol::Colorless => Some(vec![PayBranch::Colorless]),
    }
}

/// Branches for a whole symbol. An `Atom` forwards its atom's branches. A
/// `Hybrid(a, b)` (CR 107.4e, 107.4f) offers the union, so `{W/U/P}` gets
/// three options from two slots (see `Hybrid` in `mana.rs`). `None` means an
/// unsupported atom or a bare `X`, so the symbol is unpayable here.
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

/// Remaining declared units as `assign_branches` consumes them. Mirrors
/// `ManaPool`'s split plus the snow subcounts (CR 107.4h), and keeps
/// `snow_colored[i] <= colored[i]` through every mutation.
#[derive(Clone, Copy)]
struct SpentPool {
    colored: [u32; 5],
    colorless: u32,
    snow_colored: [u32; 5],
    snow_colorless: u32,
}

/// Whether `spent` plus `phyrexian_life` (CR 107.4f) satisfies `required`
/// (CR 118.13a). `None` means a symbol shape this engine cannot pay for (see
/// `symbol_branches`), and `Some(false)` means every shape is supported but
/// this payment doesn't cover them.
///
/// A tally is not enough once 2+ multi-branch symbols share spent units: two
/// `{W/U}` paid with one white and one blue unit is legal only if each symbol
/// claims a different unit. So this is a backtracking assignment search,
/// which is cheap for the handful of symbols a cost has.
fn mana_payment_satisfies(
    required: &[ManaSymbol],
    spent: SpentPool,
    phyrexian_life: u32,
) -> Option<bool> {
    let mut branch_sets = Vec::with_capacity(required.len());
    for symbol in required {
        branch_sets.push(symbol_branches(symbol)?);
    }
    Some(assign_branches(
        &branch_sets,
        0,
        spent,
        0,
        0,
        phyrexian_life,
    ))
}

/// Backtracking step for `mana_payment_satisfies`. At the base case the pool
/// left after every `Color` and `Snow` reservation must equal the accumulated
/// `Generic` total, and the accumulated `Life` total must equal
/// `phyrexian_life`, both exactly (CR 118.13a).
///
/// `Color(c)` tries both a non-snow and a snow-tagged unit when both exist,
/// because the choice changes what a later `Snow` symbol can claim. `Snow`
/// tries every bucket with a snow-tagged unit (CR 107.4h: any type, from a
/// snow source), debiting the color total and snow subcount together.
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
            PayBranch::Colorless => {
                if spent.colorless > spent.snow_colorless {
                    let mut next = spent;
                    next.colorless -= 1;
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
                if spent.snow_colorless > 0 {
                    let mut next = spent;
                    next.colorless -= 1;
                    next.snow_colorless -= 1;
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
            PayBranch::Generic(n) => {
                if assign_branches(
                    branch_sets,
                    idx + 1,
                    spent,
                    generic_needed + n,
                    life_needed,
                    phyrexian_life,
                ) {
                    return true;
                }
            }
            PayBranch::Life(n) => {
                if assign_branches(
                    branch_sets,
                    idx + 1,
                    spent,
                    generic_needed,
                    life_needed + n,
                    phyrexian_life,
                ) {
                    return true;
                }
            }
        }
    }
    false
}

/// Tallies `units` into per-color counts, the colorless count and their snow
/// subcounts (CR 107.4h), the payer-side mirror of `atom_branches`. A
/// restricted unit (CR 106.11) folds into its base bucket, since a
/// restriction narrows what it can pay for and not which pip it satisfies.
/// `validate` calls this again on a restricted-filtered slice for the
/// pool-affordability check.
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
            ManaUnit::RestrictedColored(c, _) => colored[color_index(*c)] += 1,
            ManaUnit::RestrictedColorless(_) => colorless += 1,
        }
    }
    (colored, colorless, snow_colored, snow_colorless)
}

/// CR 118.3a: deducts exactly `units` from `pool`, with no inference, since
/// `PaymentPlan::validate` made the payer declare them (CR 118.13a). Call it
/// only after `validate` confirmed the pool covers `units`. It does not
/// recheck and would underflow.
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
            // CR 106.11: a restricted unit is not in the plain counts, so remove its entry from the side-channel. `validate` already confirmed it exists.
            unit @ (ManaUnit::RestrictedColored(..) | ManaUnit::RestrictedColorless(..)) => {
                let idx = pool
                    .restricted
                    .iter()
                    .position(|candidate| candidate == unit)
                    .expect("validate() already confirmed this restricted unit is available");
                pool.restricted.remove(idx);
            }
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum CostPayment {
    /// CR 118.13a: the exact literal payment, not an echo of the cost's
    /// shape. `units` is the mana spent and `phyrexian_life` is the life
    /// declared toward Phyrexian pips (CR 107.4f), 0 when none. See
    /// `PaymentPlan` for why this is the payer's choice.
    Mana {
        units: Vec<ManaUnit>,
        phyrexian_life: u32,
    },
    Sacrifice(ObjectId),
    Discard(ObjectId),
    /// CR 702.138a Escape's exile cost, one entry per exiled card.
    Exile(ObjectId),
    Tap(ObjectId),
    Life(u32),
    Other {
        description: String,
    },
}

/// CR 702.44b (Sunburst): distinct colors of mana spent across every
/// `CostPayment::Mana` in `payments`. A snow unit counts as its color
/// (CR 702.44a). Other payments contribute nothing.
pub fn colors_spent(payments: &[CostPayment]) -> ColorSet {
    let mut set = ColorSet::empty();
    for payment in payments {
        let CostPayment::Mana { units, .. } = payment else {
            continue;
        };
        for unit in units {
            let bit = match unit {
                ManaUnit::Colored(c)
                | ManaUnit::SnowColored(c)
                | ManaUnit::RestrictedColored(c, _) => match c {
                    Color::White => ColorSet::WHITE,
                    Color::Blue => ColorSet::BLUE,
                    Color::Black => ColorSet::BLACK,
                    Color::Red => ColorSet::RED,
                    Color::Green => ColorSet::GREEN,
                },
                ManaUnit::Colorless
                | ManaUnit::SnowColorless
                | ManaUnit::RestrictedColorless(_) => continue,
            };
            set |= bit;
        }
    }
    set
}

/// A fully chosen set of payments for a `CostSpec`. The spec says what is
/// owed, and the player's choices (which lands to tap, what to sacrifice,
/// whether to use the alternative cost) arrive as `SubmitPayment`. The engine
/// validates the whole plan against the spec before committing any of it, so
/// no partial payment ever lands in state.
#[derive(Clone, PartialEq, Debug)]
pub struct PaymentPlan {
    pub payments: Vec<CostPayment>,
}

impl PaymentPlan {
    /// Validates this plan atomically against `spec`: every component
    /// present, nothing extra, mana affordable from `payer`'s pool, and every
    /// additional-cost payment a real, legal object. `source` is the
    /// activating object, so `TapSelf` can confirm it taps that one.
    ///
    /// `self.payments` mirrors `spec`: a `Mana` entry first, only when
    /// `spec.mana` has symbols to pay (a `{0}` cost needs none, CR 118.5a),
    /// then one entry per `spec.additional` component in order, with
    /// `TapPermanents` taking `count` consecutive `Tap` entries. The order
    /// tells same-shaped payments apart. `commit` doesn't need it.
    ///
    /// CR 118.13a: a colored pip takes that exact color, but the generic part
    /// takes any leftover units, including extra units of a color that paid a
    /// pip (`{1}{G}` with three green). The `units` in a `Mana` entry must
    /// total exactly the cost.
    ///
    /// Returns `false` for `spec.alternative` costs, an unresolved `{X}`
    /// (callers run `ManaCost::resolve_x` first) and `AdditionalCost::Other`,
    /// which no `CostPayment` can express. Colored, generic, `{C}`
    /// (CR 107.4c), Phyrexian (CR 107.4f), snow (CR 107.4h) and hybrid symbols
    /// are handled.
    // TODO: return a `Result` with a reason once the failure modes are designed
    pub(crate) fn validate(
        &self,
        spec: &CostSpec,
        state: &GameState,
        script_runtime: &ScriptRuntime,
        payer: PlayerId,
        source: ObjectId,
    ) -> bool {
        if !spec.alternative.is_empty() {
            return false;
        }
        let mut payments = self.payments.iter();

        if let Some(mana_cost) = &spec.mana {
            let required = mana_cost.symbols.as_slice();
            if !required.is_empty() {
                let Some(CostPayment::Mana {
                    units,
                    phyrexian_life,
                }) = payments.next()
                else {
                    return false;
                };
                // CR 118.13a: a restricted unit folds into its base bucket here (see `spent_tally`).
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
                let Some(player) = state.player(payer) else {
                    return false;
                };
                // CR 119.4: a player can't pay more life toward Phyrexian
                // pips than they have.
                if player.life < *phyrexian_life as i32 {
                    return false;
                }
                let pool = &player.mana_pool;

                // Pool affordability: restricted units are in `pool.restricted`, not the plain fields, so only the non-restricted declared units are checked against those.
                let (plain_units, restricted_units): (Vec<ManaUnit>, Vec<ManaUnit>) =
                    units.iter().copied().partition(|u| {
                        !matches!(
                            u,
                            ManaUnit::RestrictedColored(..) | ManaUnit::RestrictedColorless(..)
                        )
                    });
                let (plain_colored, plain_colorless, plain_snow_colored, plain_snow_colorless) =
                    spent_tally(&plain_units);
                let pool_colored = [pool.white, pool.blue, pool.black, pool.red, pool.green];
                if plain_colored
                    .iter()
                    .zip(pool_colored)
                    .any(|(spent, have)| *spent > have)
                {
                    return false;
                }
                if plain_colorless > pool.colorless {
                    return false;
                }
                // Declared snow units cannot exceed the pool's snow-tagged mana (CR 107.4h). The check above bounds only totals.
                let pool_snow_colored = [
                    pool.snow_white,
                    pool.snow_blue,
                    pool.snow_black,
                    pool.snow_red,
                    pool.snow_green,
                ];
                if plain_snow_colored
                    .iter()
                    .zip(pool_snow_colored)
                    .any(|(spent, have)| *spent > have)
                {
                    return false;
                }
                if plain_snow_colorless > pool.snow_colorless {
                    return false;
                }

                // CR 106.11: each declared restricted unit must be in `pool.restricted`, consumed as a multiset so two declared copies cannot share one entry, and be eligible for this cost. Only the "cast a [card type] spell" shape exists: `source` is on the stack (the signal `PendingPaymentRequest::source` uses for a spell) and its card types intersect the restriction.
                let mut remaining_restricted = pool.restricted.clone();
                for unit in &restricted_units {
                    let Some(idx) = remaining_restricted
                        .iter()
                        .position(|candidate| candidate == unit)
                    else {
                        return false;
                    };
                    remaining_restricted.remove(idx);
                    let types = match unit {
                        ManaUnit::RestrictedColored(_, types)
                        | ManaUnit::RestrictedColorless(types) => *types,
                        _ => unreachable!("partitioned into restricted_units above"),
                    };
                    let eligible = state.object(source).is_some_and(|o| {
                        o.zone == ZoneKind::Stack && o.definition.card_types.intersects(types)
                    });
                    if !eligible {
                        return false;
                    }
                }
            }
            // CR 118.5a: an empty `required` is satisfied by having no Mana entry.
        }

        for additional in &spec.additional {
            match additional {
                AdditionalCost::TapSelf => {
                    let Some(CostPayment::Tap(id)) = payments.next() else {
                        return false;
                    };
                    if *id != source {
                        return false;
                    }
                    let Some(obj) = state.object(*id) else {
                        return false;
                    };
                    if obj.tapped {
                        return false;
                    }
                }
                AdditionalCost::TapPermanents { filter, count } => {
                    let mut used: Vec<ObjectId> = Vec::with_capacity(*count as usize);
                    for _ in 0..*count {
                        let Some(CostPayment::Tap(id)) = payments.next() else {
                            return false;
                        };
                        if used.contains(id) {
                            return false;
                        }
                        let Some(obj) = state.object(*id) else {
                            return false;
                        };
                        if obj.tapped || obj.zone != ZoneKind::Battlefield {
                            return false;
                        }
                        let controller =
                            crate::layers::effective_controller(state, script_runtime, *id);
                        if !filter.matches(obj, controller, payer) {
                            return false;
                        }
                        used.push(*id);
                    }
                }
                AdditionalCost::ExileFromGraveyard { filter, count } => {
                    let mut used: Vec<ObjectId> = Vec::with_capacity(*count as usize);
                    for _ in 0..*count {
                        let Some(CostPayment::Exile(id)) = payments.next() else {
                            return false;
                        };
                        if used.contains(id) {
                            return false;
                        }
                        let Some(obj) = state.object(*id) else {
                            return false;
                        };
                        // A graveyard card's `.controller` equals its owner (CR 108.4).
                        if obj.zone != ZoneKind::Graveyard || obj.controller != payer {
                            return false;
                        }
                        if !filter.matches(obj, obj.controller, payer) {
                            return false;
                        }
                        used.push(*id);
                    }
                }
                AdditionalCost::Sacrifice { filter } => {
                    let Some(CostPayment::Sacrifice(id)) = payments.next() else {
                        return false;
                    };
                    let Some(obj) = state.object(*id) else {
                        return false;
                    };
                    // CR 701.20a: you can only sacrifice a permanent you
                    // *currently* control (layers::effective_controller).
                    let controller =
                        crate::layers::effective_controller(state, script_runtime, *id);
                    if obj.zone != ZoneKind::Battlefield || controller != payer {
                        return false;
                    }
                    if !filter.matches(obj, controller, payer) {
                        return false;
                    }
                }
                AdditionalCost::SacrificeSelf => {
                    let Some(CostPayment::Sacrifice(id)) = payments.next() else {
                        return false;
                    };
                    if *id != source {
                        return false;
                    }
                    let Some(obj) = state.object(*id) else {
                        return false;
                    };
                    // CR 701.20a: the `Sacrifice{filter}` check without the filter, since the id is pinned to source.
                    let controller =
                        crate::layers::effective_controller(state, script_runtime, *id);
                    if obj.zone != ZoneKind::Battlefield || controller != payer {
                        return false;
                    }
                }
                AdditionalCost::Discard { filter } => {
                    let Some(CostPayment::Discard(id)) = payments.next() else {
                        return false;
                    };
                    let Some(obj) = state.object(*id) else {
                        return false;
                    };
                    // A hand card's `.controller` equals its owner (CR 108.4), so the raw field is correct.
                    if obj.zone != ZoneKind::Hand || obj.controller != payer {
                        return false;
                    }
                    if !filter.matches(obj, obj.controller, payer) {
                        return false;
                    }
                }
                AdditionalCost::DiscardSelf => {
                    let Some(CostPayment::Discard(id)) = payments.next() else {
                        return false;
                    };
                    if *id != source {
                        return false;
                    }
                    let Some(obj) = state.object(*id) else {
                        return false;
                    };
                    // CR 702.29a: the source must still be in its controller's hand.
                    if obj.zone != ZoneKind::Hand || obj.controller != payer {
                        return false;
                    }
                }
                AdditionalCost::LifePayment { amount } => {
                    let Some(CostPayment::Life(paid)) = payments.next() else {
                        return false;
                    };
                    if paid != amount {
                        return false;
                    }
                    // CR 119.4: a player can't pay a life-payment cost that
                    // would take them below 0 life.
                    let Some(player) = state.player(payer) else {
                        return false;
                    };
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
    use crate::types::card::{
        CardDefinition, KeywordSet, ModeChoice, PTValue, ScriptReference, Subtype, SupertypeSet,
    };
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
                mana_cost: ManaCost {
                    symbols: Vec::new(),
                },
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
                buyback_cost: None,
                effect: None,
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
            },
            copiable_override: None,
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
            attached_to_player: None,
            timestamp: SequenceNo(0),
            controlled_since_turn: 0,
            is_commander: false,
            is_token: false,
            visibility: VisibilityPolicy {
                public: true,
                visible_to: Vec::new(),
            },
            copy_generation: 0,
            entered_via_escape: false,
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
        assert!(filter.matches(&creature(p0, CardTypeSet::CREATURE), p0, p0));
        assert!(!filter.matches(&creature(p0, CardTypeSet::LAND), p0, p0));
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
        assert!(you_filter.matches(&creature(p0, CardTypeSet::CREATURE), p0, p0));
        assert!(!you_filter.matches(&creature(p1, CardTypeSet::CREATURE), p1, p0));

        let opponent_filter = FilterSpec {
            card_types: None,
            supertypes: None,
            subtypes: None,
            controller: Some(FilterController::Opponent),
            random: false,
            extra: None,
        };
        assert!(opponent_filter.matches(&creature(p1, CardTypeSet::CREATURE), p1, p0));
        assert!(!opponent_filter.matches(&creature(p0, CardTypeSet::CREATURE), p0, p0));
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
        assert!(!filter.matches(&creature(p0, CardTypeSet::CREATURE), p0, p0));
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
        assert!(
            filter.matches(&basic_land, p0, p0),
            "a basic land card matches Some(SupertypeSet::BASIC)"
        );
        assert!(
            !filter.matches(&nonbasic_land, p0, p0),
            "a nonbasic land doesn't match"
        );
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
        assert!(
            filter.matches(&forest, p0, p0),
            "a Forest card matches a Forest-subtype filter"
        );
        assert!(
            !filter.matches(&island, p0, p0),
            "an Island doesn't match a Forest-only filter"
        );
    }

    fn spent(colored: [u32; 5], colorless: u32) -> SpentPool {
        SpentPool {
            colored,
            colorless,
            snow_colored: [0; 5],
            snow_colorless: 0,
        }
    }

    fn spent_snow(
        colored: [u32; 5],
        colorless: u32,
        snow_colored: [u32; 5],
        snow_colorless: u32,
    ) -> SpentPool {
        SpentPool {
            colored,
            colorless,
            snow_colored,
            snow_colorless,
        }
    }

    #[test]
    fn hybrid_color_color_paid_with_either_color() {
        let required = [ManaSymbol::Hybrid(
            AtomicManaSymbol::Colored(Color::White),
            AtomicManaSymbol::Colored(Color::Blue),
        )];
        assert_eq!(
            mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 0), 0),
            Some(true)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([0, 1, 0, 0, 0], 0), 0),
            Some(true)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([0, 0, 1, 0, 0], 0), 0),
            Some(false)
        );
    }

    #[test]
    fn hybrid_generic_color_paid_either_branch() {
        let required = [ManaSymbol::Hybrid(
            AtomicManaSymbol::Generic(2),
            AtomicManaSymbol::Colored(Color::White),
        )];
        assert_eq!(
            mana_payment_satisfies(&required, spent([0, 0, 0, 0, 0], 2), 0),
            Some(true)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 0), 0),
            Some(true)
        );
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
        // Each symbol must claim a different unit; a plain tally cannot tell this from an unpayable case.
        assert_eq!(
            mana_payment_satisfies(&required, spent([1, 1, 0, 0, 0], 0), 0),
            Some(true)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([2, 0, 0, 0, 0], 0), 0),
            Some(true)
        );
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
        assert_eq!(
            mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 1), 0),
            Some(true)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 0), 0),
            Some(false)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 2), 0),
            Some(false)
        );
    }

    #[test]
    fn plain_phyrexian_paid_with_color_or_life() {
        let required = [ManaSymbol::Atom(AtomicManaSymbol::Phyrexian(Color::White))];
        assert_eq!(
            mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 0), 0),
            Some(true)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([0; 5], 0), 2),
            Some(true)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([0, 0, 1, 0, 0], 0), 0),
            Some(false)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([0; 5], 0), 1),
            Some(false)
        );
    }

    #[test]
    fn hybrid_phyrexian_offers_three_real_branches() {
        // Hybrid(Phyrexian(Blue), Colored(Green)): pay blue, green or 2 life (CR 107.4f).
        let required = [ManaSymbol::Hybrid(
            AtomicManaSymbol::Phyrexian(Color::Blue),
            AtomicManaSymbol::Colored(Color::Green),
        )];
        assert_eq!(
            mana_payment_satisfies(&required, spent([0, 1, 0, 0, 0], 0), 0),
            Some(true)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([0, 0, 0, 0, 1], 0), 0),
            Some(true)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([0; 5], 0), 2),
            Some(true)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([0, 0, 0, 1, 0], 0), 0),
            Some(false)
        );
    }

    #[test]
    fn generic_pip_mixed_with_phyrexian_life_must_match_exactly() {
        let required = [
            ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
            ManaSymbol::Atom(AtomicManaSymbol::Phyrexian(Color::White)),
        ];
        assert_eq!(
            mana_payment_satisfies(&required, spent([0; 5], 1), 2),
            Some(true)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([0; 5], 1), 1),
            Some(false)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 1), 2),
            Some(false)
        );
    }

    #[test]
    fn snow_pip_paid_with_any_snow_tagged_mana() {
        let required = [ManaSymbol::Atom(AtomicManaSymbol::Snow)];
        assert_eq!(
            mana_payment_satisfies(
                &required,
                spent_snow([1, 0, 0, 0, 0], 0, [1, 0, 0, 0, 0], 0),
                0
            ),
            Some(true)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent_snow([0; 5], 1, [0; 5], 1), 0),
            Some(true)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 0), 0),
            Some(false)
        );
    }

    #[test]
    fn colored_pip_and_snow_pip_compete_for_the_same_snow_unit() {
        // One snow white cannot cover both {W} and {S}; a plain tally would miss it.
        let required = [
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White)),
            ManaSymbol::Atom(AtomicManaSymbol::Snow),
        ];
        assert_eq!(
            mana_payment_satisfies(
                &required,
                spent_snow([1, 0, 0, 0, 0], 0, [1, 0, 0, 0, 0], 0),
                0
            ),
            Some(false)
        );
        assert_eq!(
            mana_payment_satisfies(
                &required,
                spent_snow([2, 0, 0, 0, 0], 0, [1, 0, 0, 0, 0], 0),
                0
            ),
            Some(true)
        );
    }

    #[test]
    fn generic_pip_mixed_with_snow_leftover_must_match_exactly() {
        let required = [
            ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
            ManaSymbol::Atom(AtomicManaSymbol::Snow),
        ];
        assert_eq!(
            mana_payment_satisfies(
                &required,
                spent_snow([1, 0, 0, 0, 0], 1, [1, 0, 0, 0, 0], 0),
                0
            ),
            Some(true)
        );
        assert_eq!(
            mana_payment_satisfies(
                &required,
                spent_snow([1, 0, 0, 0, 0], 0, [1, 0, 0, 0, 0], 0),
                0
            ),
            Some(false)
        );
    }

    #[test]
    fn a_bare_x_bails_with_none() {
        // `resolve_x` runs before payment, so an unresolved {X} is never payable.
        assert_eq!(
            mana_payment_satisfies(&[ManaSymbol::X], spent([0; 5], 0), 0),
            None
        );
    }

    #[test]
    fn colorless_symbol_takes_only_colorless_mana() {
        let required = [ManaSymbol::Atom(AtomicManaSymbol::Colorless)];
        assert_eq!(
            mana_payment_satisfies(&required, spent([0; 5], 1), 0),
            Some(true)
        );
        // CR 107.4c
        assert_eq!(
            mana_payment_satisfies(&required, spent([1, 0, 0, 0, 0], 0), 0),
            Some(false)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([0; 5], 0), 0),
            Some(false)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent_snow([0; 5], 1, [0; 5], 1), 0),
            Some(true)
        );
    }

    #[test]
    fn colorless_symbol_beside_generic_must_match_exactly() {
        // {2}{C}: the {C} must take the colorless unit, and the {2} can take white.
        let required = [
            ManaSymbol::Atom(AtomicManaSymbol::Generic(2)),
            ManaSymbol::Atom(AtomicManaSymbol::Colorless),
        ];
        assert_eq!(
            mana_payment_satisfies(&required, spent([0; 5], 3), 0),
            Some(true)
        );
        assert_eq!(
            mana_payment_satisfies(&required, spent([2, 0, 0, 0, 0], 1), 0),
            Some(true)
        );
        // Three white has no colorless unit for the {C}.
        assert_eq!(
            mana_payment_satisfies(&required, spent([3, 0, 0, 0, 0], 0), 0),
            Some(false)
        );
    }

    #[test]
    fn filter_spec_matches_combined_conditions() {
        let p0 = PlayerId(0);
        let mut forest = creature(p0, CardTypeSet::LAND);
        forest.definition.supertypes = SupertypeSet::BASIC;
        forest.definition.subtypes = vec![Subtype("Forest".to_string())];

        let basic_forest_filter = FilterSpec {
            card_types: Some(CardTypeSet::LAND),
            supertypes: Some(SupertypeSet::BASIC),
            subtypes: Some(vec![Subtype("Forest".to_string())]),
            controller: Some(FilterController::You),
            random: false,
            extra: None,
        };
        assert!(basic_forest_filter.matches(&forest, p0, p0));

        let nonbasic_forest = {
            let mut c = creature(p0, CardTypeSet::LAND);
            c.definition.subtypes = vec![Subtype("Forest".to_string())];
            c
        };
        assert!(!basic_forest_filter.matches(&nonbasic_forest, p0, p0));
    }

    #[test]
    fn apply_cost_modifiers_generic_reduction_floors_at_zero() {
        let base = ManaCost::parse("{2}{R}").unwrap();
        let red_creature = creature(PlayerId(0), CardTypeSet::CREATURE);
        let modifier = CostModifier {
            source: ObjectId(0),
            controller: PlayerId(0),
            filter: FilterSpec {
                card_types: Some(CardTypeSet::CREATURE),
                supertypes: None,
                subtypes: None,
                controller: None,
                random: false,
                extra: None,
            },
            target: CostModifierTarget::Generic,
            amount: -3,
        };
        let adjusted = apply_cost_modifiers(&base, &[modifier], &red_creature);
        assert_eq!(
            adjusted.symbols,
            vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))]
        );
    }

    #[test]
    fn apply_cost_modifiers_colored_reduction_spills_to_generic() {
        // CR 118.7c: reducing a color beyond the cost's pips zeroes that color and spills to generic.
        // {W} reduced by 3 on {1}{W}{W} removes both white pips and spills -1 generic, which then
        // floors the existing {1}, leaving no symbols at all.
        let base = ManaCost::parse("{1}{W}{W}").unwrap();
        let white_creature = creature(PlayerId(0), CardTypeSet::CREATURE);
        let modifier = CostModifier {
            source: ObjectId(0),
            controller: PlayerId(0),
            filter: FilterSpec {
                card_types: Some(CardTypeSet::CREATURE),
                supertypes: None,
                subtypes: None,
                controller: None,
                random: false,
                extra: None,
            },
            target: CostModifierTarget::Colored(Color::White),
            amount: -3,
        };
        let adjusted = apply_cost_modifiers(&base, &[modifier], &white_creature);
        assert!(adjusted.symbols.is_empty());
    }

    #[test]
    fn apply_cost_modifiers_unmatched_filter_is_ignored() {
        let base = ManaCost::parse("{2}").unwrap();
        let creature_obj = creature(PlayerId(0), CardTypeSet::CREATURE);
        let modifier = CostModifier {
            source: ObjectId(0),
            controller: PlayerId(0),
            filter: FilterSpec {
                card_types: Some(CardTypeSet::LAND),
                supertypes: None,
                subtypes: None,
                controller: None,
                random: false,
                extra: None,
            },
            target: CostModifierTarget::Generic,
            amount: -1,
        };
        let adjusted = apply_cost_modifiers(&base, &[modifier], &creature_obj);
        assert_eq!(adjusted.symbols, base.symbols);
    }

    #[test]
    fn apply_cost_modifiers_mixed_reductions_sum_before_clamping() {
        // -2 generic and -1 red on {1}{R}{R} leaves {R}, because colored spillover (-1 red over two pips leaves {0} generic)
        // and generic -2 zero the {1} generic; the remaining red pip survives.
        let base = ManaCost::parse("{1}{R}{R}").unwrap();
        let red_creature = creature(PlayerId(0), CardTypeSet::CREATURE);
        let modifiers = vec![
            CostModifier {
                source: ObjectId(0),
                controller: PlayerId(0),
                filter: FilterSpec {
                    card_types: Some(CardTypeSet::CREATURE),
                    supertypes: None,
                    subtypes: None,
                    controller: None,
                    random: false,
                    extra: None,
                },
                target: CostModifierTarget::Generic,
                amount: -2,
            },
            CostModifier {
                source: ObjectId(0),
                controller: PlayerId(0),
                filter: FilterSpec {
                    card_types: Some(CardTypeSet::CREATURE),
                    supertypes: None,
                    subtypes: None,
                    controller: None,
                    random: false,
                    extra: None,
                },
                target: CostModifierTarget::Colored(Color::Red),
                amount: -1,
            },
        ];
        let adjusted = apply_cost_modifiers(&base, &modifiers, &red_creature);
        assert_eq!(
            adjusted.symbols,
            vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))]
        );
    }

    #[test]
    fn colors_spent_collects_distinct_colors_from_mana_payments() {
        let payments = vec![
            CostPayment::Mana {
                units: vec![
                    ManaUnit::Colored(Color::White),
                    ManaUnit::Colored(Color::Blue),
                ],
                phyrexian_life: 0,
            },
            CostPayment::Mana {
                units: vec![ManaUnit::SnowColored(Color::Red), ManaUnit::Colorless],
                phyrexian_life: 0,
            },
        ];
        assert_eq!(
            colors_spent(&payments),
            ColorSet::WHITE | ColorSet::BLUE | ColorSet::RED
        );
    }

    #[test]
    fn colors_spent_ignores_non_mana_payments_and_colorless() {
        let payments = vec![
            CostPayment::Mana {
                units: vec![ManaUnit::Colorless, ManaUnit::SnowColorless],
                phyrexian_life: 0,
            },
            CostPayment::Life(2),
        ];
        assert_eq!(colors_spent(&payments), ColorSet::empty());
    }

    #[test]
    fn cost_spec_mana_only_builds_expected_shape() {
        let cost = ManaCost::parse("{1}{G}").unwrap();
        let spec = CostSpec::mana_only(cost.clone());
        assert_eq!(spec.mana, Some(cost));
        assert!(spec.additional.is_empty());
        assert!(spec.alternative.is_empty());
    }
}
