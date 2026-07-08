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

    /// Whether this cost has an `{X}` in it (CR 107.3a) — the caller's cue
    /// that an X value must be announced before this cost can be paid.
    pub fn has_x(&self) -> bool {
        self.symbols.iter().any(|s| matches!(s, ManaSymbol::X))
    }

    /// Substitutes the announced value of X for every `{X}` in this cost
    /// (CR 107.3a/107.3i: X is chosen once and every instance shares that
    /// same value) — `X` is only ever a bare top-level `ManaSymbol`, never
    /// nested inside a `Hybrid` (see that type's own doc comment), so this
    /// is a flat per-symbol substitution, not a recursive one. Once
    /// resolved, the cost has no `X` symbols left, so `PaymentPlan::validate`
    /// can treat it exactly like any ordinary cost.
    pub fn resolve_x(&self, x: u32) -> ManaCost {
        ManaCost {
            symbols: self
                .symbols
                .iter()
                .map(|s| match s {
                    ManaSymbol::X => ManaSymbol::Atom(AtomicManaSymbol::Generic(x)),
                    other => *other,
                })
                .collect(),
        }
    }
}

/// A player's floating mana pool. Cleared at the end of each step/phase per
/// CR 106.4. The `snow_*` fields are subcounts, not a seventh mana type —
/// snow is neither a color nor a type of mana (CR 107.4h), just a tag some
/// floating mana carries because of what produced it. `snow_white` is always
/// `<= white`, etc — the snow-tagged mana of a color is part of that color's
/// total, not separate from it. Needed so a `{S}` cost symbol (payable with
/// *any* type of mana, as long as it came from a snow source) can be told
/// apart from an ordinary mana of the same color — see `ManaUnit`'s own doc
/// comment for the matching per-unit tag.
#[derive(Default)]
pub struct ManaPool {
    pub white: u32,
    pub blue: u32,
    pub black: u32,
    pub red: u32,
    pub green: u32,
    pub colorless: u32,
    pub snow_white: u32,
    pub snow_blue: u32,
    pub snow_black: u32,
    pub snow_red: u32,
    pub snow_green: u32,
    pub snow_colorless: u32,
}

/// One literal unit of floating mana, as it actually exists in a `ManaPool`
/// — deliberately a narrower type than `AtomicManaSymbol`: that type also
/// has a `Generic`/`Phyrexian` variant, which describe *cost shapes* or
/// *payment choices*, not things that can sit in a pool waiting to be spent.
/// Used by `CostPayment::Mana` to record exactly which units a `PaymentPlan`
/// spends — see that type's doc comment in `cost.rs` for why this replaced
/// an earlier design that only recorded the cost's *shape*, not the payer's
/// actual choice of units (CR 118.13a).
///
/// `SnowColored`/`SnowColorless` are the snow-sourced counterpart of
/// `Colored`/`Colorless` (CR 107.4h) — a separate pair of variants, not a
/// `snow: bool` field wrapping the other two, so the 28-or-so existing call
/// sites building plain non-snow units don't need touching. A snow unit is
/// still exactly that color for every other purpose (colored pips, generic)
/// — the tag only matters for paying a `{S}` symbol.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManaUnit {
    Colored(Color),
    Colorless,
    SnowColored(Color),
    SnowColorless,
}

impl ManaPool {
    /// Inverse of `cost::spend` (`cost.rs`) — adds one literal unit instead
    /// of removing one. Used by CR 605 mana abilities, which add exact
    /// units to the pool rather than a `ManaCost`-shaped amount (see
    /// `ManaUnit`'s own doc comment for why this is the narrower vocabulary).
    /// A snow unit bumps both its color's total and its snow subcount —
    /// the two always move together, keeping `snow_* <= *` an invariant.
    pub fn add_unit(&mut self, unit: ManaUnit) {
        match unit {
            ManaUnit::Colored(Color::White) => self.white += 1,
            ManaUnit::Colored(Color::Blue) => self.blue += 1,
            ManaUnit::Colored(Color::Black) => self.black += 1,
            ManaUnit::Colored(Color::Red) => self.red += 1,
            ManaUnit::Colored(Color::Green) => self.green += 1,
            ManaUnit::Colorless => self.colorless += 1,
            ManaUnit::SnowColored(Color::White) => {
                self.white += 1;
                self.snow_white += 1;
            }
            ManaUnit::SnowColored(Color::Blue) => {
                self.blue += 1;
                self.snow_blue += 1;
            }
            ManaUnit::SnowColored(Color::Black) => {
                self.black += 1;
                self.snow_black += 1;
            }
            ManaUnit::SnowColored(Color::Red) => {
                self.red += 1;
                self.snow_red += 1;
            }
            ManaUnit::SnowColored(Color::Green) => {
                self.green += 1;
                self.snow_green += 1;
            }
            ManaUnit::SnowColorless => {
                self.colorless += 1;
                self.snow_colorless += 1;
            }
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_x_true_and_false_cases() {
        let with_x = ManaCost {
            symbols: vec![ManaSymbol::X, ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))],
        };
        assert!(with_x.has_x());
        let without_x =
            ManaCost { symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))] };
        assert!(!without_x.has_x());
    }

    #[test]
    fn resolve_x_substitutes_generic_and_leaves_other_symbols_alone() {
        let cost = ManaCost {
            symbols: vec![ManaSymbol::X, ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))],
        };
        let resolved = cost.resolve_x(3);
        assert_eq!(
            resolved.symbols,
            vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(3)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
            ]
        );
    }

    #[test]
    fn resolve_x_gives_every_x_the_same_value() {
        // CR 107.3i: all instances of X in one cost share the same value -
        // not independently chosen per occurrence.
        let cost = ManaCost { symbols: vec![ManaSymbol::X, ManaSymbol::X] };
        let resolved = cost.resolve_x(2);
        assert_eq!(
            resolved.symbols,
            vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(2)),
                ManaSymbol::Atom(AtomicManaSymbol::Generic(2)),
            ]
        );
    }
}
