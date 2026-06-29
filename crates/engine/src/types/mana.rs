use bitflags::bitflags;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    White,
    Blue,
    Black,
    Red,
    Green,
}

bitflags! {
    /// WUBRG bitmask. Used for color identity, color indicators, etc.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct ColorSet: u8 {
        const WHITE = 1 << 0;
        const BLUE  = 1 << 1;
        const BLACK = 1 << 2;
        const RED   = 1 << 3;
        const GREEN = 1 << 4;
    }
}

/// A single indivisible mana symbol — the smallest unit a hybrid symbol
/// chooses between. Not itself a full cost symbol (see `ManaSymbol`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtomicManaSymbol {
    Colored(Color),
    Generic(u32),
    Colorless,
    Snow,
    /// Pay the named color, or pay 2 life instead.
    Phyrexian(Color),
}

impl AtomicManaSymbol {
    /// Colors this atom contributes to a card's color (CR 202.2) — empty for
    /// `Generic`/`Colorless`/`Snow`.
    pub fn colors(&self) -> ColorSet {
        // TODO: Colored(c) -> ColorSet with just c set. Phyrexian(c) -> same
        // (CR 107.4f: Phyrexian symbols are colored symbols). Generic/
        // Colorless/Snow -> ColorSet::empty().
        todo!()
    }
}

/// One symbol within a mana cost string. Hybrid is always exactly two
/// atoms, "pay either one" — covers color/color (`{W/U}`), generic/color
/// (`{2/W}`), and hybrid Phyrexian (`{W/P}` paired with a second color,
/// e.g. `Hybrid(Phyrexian(Blue), Colored(Green))`) uniformly. The hybrid
/// Phyrexian case still only needs two slots: `Phyrexian(Color)` already
/// bundles two payment options (that color, or 2 life) into one atom, so
/// pairing it with a second atom in a 2-slot `Hybrid` yields three total
/// ways to pay without needing a third slot. Plain Phyrexian (`{W/P}` on
/// its own, pay color or 2 life, no second color involved) is a single
/// `Atom(Phyrexian(_))`, not a hybrid at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManaSymbol {
    Atom(AtomicManaSymbol),
    Hybrid(AtomicManaSymbol, AtomicManaSymbol),
    X,
}

impl ManaSymbol {
    /// Union of colors contributed by this symbol's atom(s) — for `Hybrid`,
    /// a hybrid symbol is all of its component colors (CR 107.4e/f), not
    /// just one.
    pub fn colors(&self) -> ColorSet {
        // TODO: Atom(a) -> a.colors(). Hybrid(a, b) -> a.colors() | b.colors()
        // (CR 107.4e/f: a hybrid symbol is all of its component colors).
        // X -> ColorSet::empty() (X has no inherent color).
        todo!()
    }
}

#[derive(Clone, Debug)]
pub struct ManaCost {
    pub symbols: Vec<ManaSymbol>,
}

impl ManaCost {
    pub fn from_symbols(symbols: Vec<ManaSymbol>) -> Self {
        // TODO: ManaCost { symbols }.
        todo!()
    }

    pub fn from_pool(pool: &ManaPool) -> Self {
        // TODO: one Atom(Colored(_))/Atom(Colorless) symbol per unit of
        // mana actually present in pool (e.g. pool.white=2 -> two
        // Atom(Colored(White)) symbols) — this represents floating mana as
        // a cost shape, used wherever something needs to compare/describe
        // pool contents using ManaCost's vocabulary.
        todo!()
    }

    pub fn is_empty(&self) -> bool {
        // TODO: self.symbols.is_empty().
        todo!()
    }

    /// Sum of all `Generic`/numeric components. Doesn't include colored or
    /// `X` symbols.
    pub fn generic_total(&self) -> u32 {
        // TODO: sum Generic(n) across self.symbols (Atom and inside Hybrid
        // pairs both count, e.g. {2/W} contributes 2 if paid generically).
        todo!()
    }

    /// Union of colors across all symbols (CR 202.2).
    pub fn colors(&self) -> ColorSet {
        // TODO: union ManaSymbol::colors() across self.symbols.
        todo!()
    }
}

/// A player's floating mana pool. Cleared at the end of each step/phase per CR 106.4.
pub struct ManaPool {
    pub white: u32,
    pub blue: u32,
    pub black: u32,
    pub red: u32,
    pub green: u32,
    pub colorless: u32,
}

/// One literal unit of floating mana, as it actually exists in a `ManaPool`
/// — deliberately a narrower type than `AtomicManaSymbol`: that type also
/// has `Generic`/`Snow`/`Phyrexian` variants, which describe *cost shapes*
/// or *payment choices*, not things that can sit in a pool waiting to be
/// spent (`ManaPool` itself has no snow-mana field, for instance). Used by
/// `CostPayment::Mana` to record exactly which units a `PaymentPlan` spends
/// — see that type's doc comment in `cost.rs` for why this replaced an
/// earlier design that only recorded the cost's *shape*, not the payer's
/// actual choice of units (CR 118.13a).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManaUnit {
    Colored(Color),
    Colorless,
}

impl ManaPool {
    pub fn empty() -> Self {
        // TODO: all fields zero.
        todo!()
    }

    pub fn add(&mut self, symbol: AtomicManaSymbol, amount: u32) {
        // TODO: match symbol: Colored(c) -> add amount to the matching
        // field; Colorless -> add to colorless; Generic/Snow/Phyrexian
        // aren't valid things to *add* to a pool (you add the colors mana
        // resolves to, not a cost shape) — decide whether to panic or be
        // unreachable for those once callers exist.
        todo!()
    }

    /// Total mana of any type/color currently floating.
    pub fn total(&self) -> u32 {
        // TODO: sum all six fields.
        todo!()
    }

    /// Cleared at the end of each step/phase per CR 106.4.
    pub fn clear(&mut self) {
        // TODO: set all six fields to 0.
        todo!()
    }
}
