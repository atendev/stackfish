use bitflags::bitflags;

pub enum Color {
    White,
    Blue,
    Black,
    Red,
    Green,
}

bitflags! {
    /// WUBRG bitmask. Used for color identity, color indicators, etc.
    #[derive(Clone, Copy, PartialEq, Eq)]
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
        todo!()
    }
}

pub struct ManaCost {
    pub symbols: Vec<ManaSymbol>,
}

impl ManaCost {
    pub fn from_symbols(symbols: Vec<ManaSymbol>) -> Self {
        todo!()
    }

    pub fn from_pool(pool: &ManaPool) -> Self {
        todo!()
    }

    pub fn is_empty(&self) -> bool {
        todo!()
    }

    /// Sum of all `Generic`/numeric components. Doesn't include colored or
    /// `X` symbols.
    pub fn generic_total(&self) -> u32 {
        todo!()
    }

    /// Union of colors across all symbols (CR 202.2).
    pub fn colors(&self) -> ColorSet {
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

impl ManaPool {
    pub fn empty() -> Self {
        todo!()
    }

    pub fn add(&mut self, symbol: AtomicManaSymbol, amount: u32) {
        todo!()
    }

    /// Total mana of any type/color currently floating.
    pub fn total(&self) -> u32 {
        todo!()
    }

    /// Cleared at the end of each step/phase per CR 106.4.
    pub fn clear(&mut self) {
        todo!()
    }
}
