use bitflags::bitflags;

use crate::types::card::CardTypeSet;

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

impl ColorSet {
    /// Whether `color` is in the set, used by `AddManaChoice` to validate a
    /// chosen color.
    pub fn contains_color(&self, color: Color) -> bool {
        let bit = match color {
            Color::White => ColorSet::WHITE,
            Color::Blue => ColorSet::BLUE,
            Color::Black => ColorSet::BLACK,
            Color::Red => ColorSet::RED,
            Color::Green => ColorSet::GREEN,
        };
        self.contains(bit)
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
    /// CR 202.3, 202.3g: this atom's contribution to mana value. `Generic(n)`
    /// is `n`, and every other atom is 1 however it is paid.
    pub(crate) fn mana_value(&self) -> u32 {
        match self {
            AtomicManaSymbol::Generic(n) => *n,
            AtomicManaSymbol::Colored(_)
            | AtomicManaSymbol::Colorless
            | AtomicManaSymbol::Snow
            | AtomicManaSymbol::Phyrexian(_) => 1,
        }
    }

    /// Colors this atom contributes to a card's color (CR 202.2) — empty for
    /// `Generic`/`Colorless`/`Snow`.
    pub(crate) fn colors(&self) -> ColorSet {
        match self {
            AtomicManaSymbol::Colored(c) | AtomicManaSymbol::Phyrexian(c) => {
                let mut set = ColorSet::empty();
                set.insert(match c {
                    Color::White => ColorSet::WHITE,
                    Color::Blue => ColorSet::BLUE,
                    Color::Black => ColorSet::BLACK,
                    Color::Red => ColorSet::RED,
                    Color::Green => ColorSet::GREEN,
                });
                set
            }
            AtomicManaSymbol::Generic(_) | AtomicManaSymbol::Colorless | AtomicManaSymbol::Snow => {
                ColorSet::empty()
            }
        }
    }
}

/// One symbol in a mana cost. A hybrid is exactly two atoms, "pay either":
/// color/color (`{W/U}`), generic/color (`{2/W}`) and hybrid Phyrexian
/// (`Hybrid(Phyrexian(Blue), Colored(Green))`). Two slots suffice because
/// `Phyrexian(Color)` already bundles two ways to pay. Plain `{W/P}` is
/// `Atom(Phyrexian(_))`, not a hybrid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManaSymbol {
    Atom(AtomicManaSymbol),
    Hybrid(AtomicManaSymbol, AtomicManaSymbol),
    X,
}

impl ManaSymbol {
    /// CR 202.3e, 202.3f: this symbol's contribution to mana value. A hybrid
    /// takes the larger atom (`{2/W}` is 2), and a raw `X` is 0. An on-stack
    /// `{X}` is substituted first with `ManaCost::resolve_x`; see
    /// `ManaCost::mana_value`.
    pub(crate) fn mana_value(&self) -> u32 {
        match self {
            ManaSymbol::Atom(a) => a.mana_value(),
            ManaSymbol::Hybrid(a, b) => a.mana_value().max(b.mana_value()),
            ManaSymbol::X => 0,
        }
    }

    /// Union of the symbol's atom colors. A hybrid has all its component
    /// colors (CR 107.4e, 107.4f).
    pub(crate) fn colors(&self) -> ColorSet {
        match self {
            ManaSymbol::Atom(a) => a.colors(),
            ManaSymbol::Hybrid(a, b) => a.colors() | b.colors(),
            ManaSymbol::X => ColorSet::empty(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ManaCost {
    pub symbols: Vec<ManaSymbol>,
}

impl ManaCost {
    /// Union of colors across all symbols (CR 202.2).
    pub fn colors(&self) -> ColorSet {
        self.symbols
            .iter()
            .fold(ColorSet::empty(), |acc, s| acc | s.colors())
    }

    /// CR 202.3: sum of each symbol's contribution, which folds in the hybrid
    /// and Phyrexian rules (CR 202.3f, 202.3g). An empty cost is 0
    /// (CR 202.3a). A raw `{X}` counts 0 (CR 202.3e), so an on-stack spell
    /// must call `resolve_x` first (`StackObject::x_value`). Back faces,
    /// melded permanents and split cards (CR 202.3b, 202.3c, 202.3d) are
    /// non-goals in `DESIGN.md`.
    pub fn mana_value(&self) -> u32 {
        self.symbols.iter().map(|s| s.mana_value()).sum()
    }

    /// Whether the cost has an `{X}` (CR 107.3a), so an X value must be
    /// announced before payment.
    pub(crate) fn has_x(&self) -> bool {
        self.symbols.iter().any(|s| matches!(s, ManaSymbol::X))
    }

    /// Substitutes the announced X for every `{X}` (CR 107.3a, 107.3i: one
    /// shared value). `X` is always a bare top-level symbol, never inside a
    /// `Hybrid`, so the substitution is flat. The result has no `X` left, so
    /// `PaymentPlan::validate` treats it like any other cost.
    pub(crate) fn resolve_x(&self, x: u32) -> ManaCost {
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

    /// CR 601.2f, 118.9: applies a signed `delta` to the generic component
    /// only (a bare `Atom(Generic(_))`, not colored or hybrid), clamped at
    /// `{0}`. A positive `delta` creates a generic component if none exists
    /// (Thalia on an `{R}` spell). A non-positive `delta` with nothing to
    /// reduce is a no-op.
    ///
    /// `apply_cost_modifiers` calls this once per spell with the sum of every
    /// matching modifier's `amount`. Applying them one by one depends on
    /// order: base `{1}` with `-2` then `+1` gives `{1}`, but `+1` then `-2`
    /// gives `{0}`. Summing first and clamping once is order-independent.
    pub(crate) fn apply_cost_delta(&self, delta: i32) -> ManaCost {
        let current: i32 = self
            .symbols
            .iter()
            .filter_map(|s| match s {
                ManaSymbol::Atom(AtomicManaSymbol::Generic(n)) => Some(*n as i32),
                _ => None,
            })
            .sum();
        let adjusted = (current + delta).max(0) as u32;

        let mut symbols: Vec<ManaSymbol> = self
            .symbols
            .iter()
            .filter(|s| !matches!(s, ManaSymbol::Atom(AtomicManaSymbol::Generic(_))))
            .copied()
            .collect();
        if adjusted > 0 {
            symbols.push(ManaSymbol::Atom(AtomicManaSymbol::Generic(adjusted)));
        }
        ManaCost { symbols }
    }

    /// CR 118.7b, 118.7c: adjusts the plain `Colored(color)` component by
    /// `delta` mana of `color`. Returns the cost and a spillover (always
    /// `<= 0`) for the caller's generic reduction: all of `delta` when the
    /// cost has none of that color (CR 118.7b), the excess when `delta`
    /// exceeds it (CR 118.7c). Matches only a bare `Atom(Colored(color))`,
    /// not a hybrid or Phyrexian symbol, as with `apply_cost_delta`. A
    /// positive `delta` just adds pips; see `CostModifierTarget`. Reductions
    /// that interact with hybrid and Phyrexian symbols (CR 118.7e, 118.7f)
    /// are not modeled.
    pub(crate) fn apply_colored_cost_delta(&self, color: Color, delta: i32) -> (ManaCost, i32) {
        self.apply_specific_cost_delta(
            delta,
            |a| matches!(a, AtomicManaSymbol::Colored(c) if *c == color),
            ManaSymbol::Atom(AtomicManaSymbol::Colored(color)),
        )
    }

    /// CR 118.7d: `apply_colored_cost_delta` for the colorless (`{C}`)
    /// component.
    pub(crate) fn apply_colorless_cost_delta(&self, delta: i32) -> (ManaCost, i32) {
        self.apply_specific_cost_delta(
            delta,
            |a| matches!(a, AtomicManaSymbol::Colorless),
            ManaSymbol::Atom(AtomicManaSymbol::Colorless),
        )
    }

    /// Shared removal and spillover logic. `matches_atom` picks the bare
    /// atoms that count as the component, and `insert` is what a positive
    /// `delta` appends. A negative `delta` removes up to `-delta` matching
    /// symbols in any order (CR 118.7). The unconsumed remainder is the
    /// spillover.
    fn apply_specific_cost_delta(
        &self,
        delta: i32,
        matches_atom: impl Fn(&AtomicManaSymbol) -> bool,
        insert: ManaSymbol,
    ) -> (ManaCost, i32) {
        if delta == 0 {
            return (self.clone(), 0);
        }
        if delta > 0 {
            let mut symbols = self.symbols.clone();
            for _ in 0..delta {
                symbols.push(insert);
            }
            return (ManaCost { symbols }, 0);
        }
        let mut to_remove = (-delta) as usize;
        let mut symbols = Vec::with_capacity(self.symbols.len());
        for s in &self.symbols {
            let is_match = matches!(s, ManaSymbol::Atom(a) if matches_atom(a));
            if is_match && to_remove > 0 {
                to_remove -= 1;
                continue;
            }
            symbols.push(*s);
        }
        (ManaCost { symbols }, -(to_remove as i32))
    }

    /// Parses a printed mana-cost string (CR 107.4, 202.3) such as `"{1}{R}"`,
    /// `"{X}{G}"`, `"{W/U}"`, `"{2/W}"`, `"{W/P}"`, `"{B/G/P}"` or `"{S}"`. A
    /// pure string-to-data parser, so payment reuses the existing machinery.
    /// `CardDefinition.mana_cost` is untouched, since it arrives structured
    /// from the card database.
    ///
    /// Per `{...}` group: digits are `Generic`, a WUBRG letter is `Colored`,
    /// `C` is `Colorless` (parses, but is unpayable; see `atom_branches`), `S`
    /// is `Snow`, `X` is bare `ManaSymbol::X`, `<letter>/P` is plain
    /// `Phyrexian`, `<A>/<B>` is `Hybrid`, and `<A>/<B>/P` is
    /// `Hybrid(Phyrexian(A), Colored(B))`, matching the
    /// `hybrid_phyrexian_offers_three_real_branches` test. Text outside a
    /// group, an empty or unterminated group, or an unknown shape is a hard
    /// error.
    pub fn parse(s: &str) -> Result<ManaCost, ManaCostParseError> {
        let mut symbols = Vec::new();
        let mut chars = s.char_indices().peekable();
        while let Some((start, c)) = chars.next() {
            if c != '{' {
                return Err(ManaCostParseError(format!(
                    "expected '{{' at position {start} in \"{s}\", found '{c}'"
                )));
            }
            let mut inner = String::new();
            let mut closed = false;
            for (_, c) in chars.by_ref() {
                if c == '}' {
                    closed = true;
                    break;
                }
                inner.push(c);
            }
            if !closed {
                return Err(ManaCostParseError(format!("unterminated '{{' in \"{s}\"")));
            }
            symbols.push(parse_symbol_group(&inner, s)?);
        }
        Ok(ManaCost { symbols })
    }
}

/// `ManaCost::parse`'s error type, a plain message like
/// `ScriptError::LoadError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaCostParseError(pub String);

/// Parses one `{...}` group's inner text. `whole` is the full string, for
/// error messages.
fn parse_symbol_group(inner: &str, whole: &str) -> Result<ManaSymbol, ManaCostParseError> {
    if inner.is_empty() {
        return Err(ManaCostParseError(format!(
            "empty \"{{}}\" group in \"{whole}\""
        )));
    }
    if inner.chars().all(|c| c.is_ascii_digit()) {
        let n: u32 = inner.parse().map_err(|_| {
            ManaCostParseError(format!(
                "generic amount \"{inner}\" out of range in \"{whole}\""
            ))
        })?;
        return Ok(ManaSymbol::Atom(AtomicManaSymbol::Generic(n)));
    }
    if inner == "X" {
        return Ok(ManaSymbol::X);
    }
    if inner == "C" {
        return Ok(ManaSymbol::Atom(AtomicManaSymbol::Colorless));
    }
    if inner == "S" {
        return Ok(ManaSymbol::Atom(AtomicManaSymbol::Snow));
    }
    if let Ok(color) = parse_color_letter(inner) {
        return Ok(ManaSymbol::Atom(AtomicManaSymbol::Colored(color)));
    }

    let parts: Vec<&str> = inner.split('/').collect();
    match parts.as_slice() {
        [a, b] if *b == "P" => {
            let color = parse_color_letter(a).map_err(|_| {
                ManaCostParseError(format!(
                    "\"{{{inner}}}\" needs a color before /P in \"{whole}\""
                ))
            })?;
            Ok(ManaSymbol::Atom(AtomicManaSymbol::Phyrexian(color)))
        }
        [a, b] => {
            let atom_a = parse_hybrid_atom(a, inner, whole)?;
            let atom_b = parse_hybrid_atom(b, inner, whole)?;
            Ok(ManaSymbol::Hybrid(atom_a, atom_b))
        }
        [a, b, c] if *c == "P" => {
            let color_a = parse_color_letter(a).map_err(|_| {
                ManaCostParseError(format!(
                    "\"{{{inner}}}\" needs two colors before /P in \"{whole}\""
                ))
            })?;
            let color_b = parse_color_letter(b).map_err(|_| {
                ManaCostParseError(format!(
                    "\"{{{inner}}}\" needs two colors before /P in \"{whole}\""
                ))
            })?;
            Ok(ManaSymbol::Hybrid(
                AtomicManaSymbol::Phyrexian(color_a),
                AtomicManaSymbol::Colored(color_b),
            ))
        }
        _ => Err(ManaCostParseError(format!(
            "unrecognized mana symbol \"{{{inner}}}\" in \"{whole}\""
        ))),
    }
}

/// A 2-part hybrid's atom: a generic amount or a single color, never `P`,
/// which `parse_symbol_group` handles first.
fn parse_hybrid_atom(
    part: &str,
    inner: &str,
    whole: &str,
) -> Result<AtomicManaSymbol, ManaCostParseError> {
    if part.chars().all(|c| c.is_ascii_digit()) && !part.is_empty() {
        let n: u32 = part.parse().map_err(|_| {
            ManaCostParseError(format!(
                "generic amount \"{part}\" out of range in \"{whole}\""
            ))
        })?;
        return Ok(AtomicManaSymbol::Generic(n));
    }
    parse_color_letter(part)
        .map(AtomicManaSymbol::Colored)
        .map_err(|_| {
            ManaCostParseError(format!(
                "unrecognized hybrid component \"{{{inner}}}\" in \"{whole}\""
            ))
        })
}

/// `pub` because the scenario crate's TOML loader reuses this WUBRG-letter
/// vocabulary for `chosen_colors`.
pub fn parse_color_letter(s: &str) -> Result<Color, ManaCostParseError> {
    match s {
        "W" => Ok(Color::White),
        "U" => Ok(Color::Blue),
        "B" => Ok(Color::Black),
        "R" => Ok(Color::Red),
        "G" => Ok(Color::Green),
        other => Err(ManaCostParseError(format!(
            "unknown color letter \"{other}\""
        ))),
    }
}

/// A player's floating mana pool, emptied at the end of each step and phase
/// (CR 106.4). The `snow_*` fields are subcounts of the color totals, not a
/// seventh type (CR 107.4h), so `snow_white <= white`. They let a `{S}`
/// symbol tell snow-sourced mana from ordinary mana of the same color; see
/// `ManaUnit`.
#[derive(Default, Clone, PartialEq, Debug)]
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
    /// CR 106.11 restricted mana, kept as a flat multiset because a
    /// restriction cannot be bucketed by color. One entry is consumed per unit
    /// spent; see `cost::spend` and `PaymentPlan::validate`.
    pub restricted: Vec<ManaUnit>,
}

/// One literal unit of floating mana. Narrower than `AtomicManaSymbol`,
/// which also has `Generic` and `Phyrexian` cost shapes that cannot sit in a
/// pool. `CostPayment::Mana` records the exact units spent (CR 118.13a).
///
/// `SnowColored` and `SnowColorless` are the snow-sourced twins of `Colored`
/// and `Colorless` (CR 107.4h). They are variants, not a `snow` field, so
/// existing construction sites stay unchanged. A snow unit counts as its
/// color everywhere, and the tag matters only for paying `{S}`.
///
/// `RestrictedColored` and `RestrictedColorless` are CR 106.11's "spend this
/// mana only to...", narrowed to a card-type filter on what it can cast
/// (Mishra's Workshop). The restriction rides on the unit as data. These
/// cannot live in `ManaPool`'s per-color counters, which cannot tell them from
/// ordinary mana; see `ManaPool::restricted`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManaUnit {
    Colored(Color),
    Colorless,
    SnowColored(Color),
    SnowColorless,
    RestrictedColored(Color, CardTypeSet),
    RestrictedColorless(CardTypeSet),
}

impl ManaUnit {
    /// The unit as produced by a snow source (CR 106.3, 107.4h). A restricted
    /// unit stays restricted, because the pool has no snow-restricted unit.
    pub fn from_snow_source(self) -> ManaUnit {
        match self {
            ManaUnit::Colored(color) => ManaUnit::SnowColored(color),
            ManaUnit::Colorless => ManaUnit::SnowColorless,
            other => other,
        }
    }
}

impl ManaPool {
    /// CR 500.5, 106.4: empties the pool as a step or phase ends.
    pub(crate) fn empty(&mut self) {
        *self = ManaPool::default();
    }

    /// Adds one literal unit, the inverse of `cost::spend`. CR 605 mana
    /// abilities add exact units, not a `ManaCost` shape. A snow unit bumps
    /// both its color total and its snow subcount, keeping `snow_* <= *`.
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
            // CR 106.11: restricted units stay out of the aggregate counts, which hold only freely spendable mana.
            unit @ (ManaUnit::RestrictedColored(..) | ManaUnit::RestrictedColorless(..)) => {
                self.restricted.push(unit);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_x_true_and_false_cases() {
        let with_x = ManaCost {
            symbols: vec![
                ManaSymbol::X,
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
            ],
        };
        assert!(with_x.has_x());
        let without_x = ManaCost {
            symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))],
        };
        assert!(!without_x.has_x());
    }

    #[test]
    fn mana_value_sums_generic_and_colored_pips() {
        let cost = ManaCost {
            symbols: vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
            ],
        };
        assert_eq!(cost.mana_value(), 3);
    }

    #[test]
    fn mana_value_no_mana_cost_is_zero() {
        assert_eq!(
            ManaCost {
                symbols: Vec::new()
            }
            .mana_value(),
            0
        );
    }

    #[test]
    fn mana_value_hybrid_uses_the_larger_component() {
        // CR 202.3f
        let cost = ManaCost {
            symbols: vec![ManaSymbol::Hybrid(
                AtomicManaSymbol::Generic(2),
                AtomicManaSymbol::Colored(Color::White),
            )],
        };
        assert_eq!(cost.mana_value(), 2);
        let color_hybrid = ManaCost {
            symbols: vec![ManaSymbol::Hybrid(
                AtomicManaSymbol::Colored(Color::White),
                AtomicManaSymbol::Colored(Color::Blue),
            )],
        };
        assert_eq!(color_hybrid.mana_value(), 1);
    }

    #[test]
    fn mana_value_phyrexian_counts_as_one() {
        let cost = ManaCost {
            symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Phyrexian(Color::Black))],
        };
        assert_eq!(cost.mana_value(), 1);
    }

    #[test]
    fn mana_value_unresolved_x_is_zero_off_the_stack() {
        // CR 202.3e
        let cost = ManaCost {
            symbols: vec![
                ManaSymbol::X,
                ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
            ],
        };
        assert_eq!(cost.mana_value(), 1);
    }

    #[test]
    fn mana_value_resolved_x_counts_the_announced_value() {
        // CR 202.3e
        // resolve_x must be applied first while on the stack.
        let cost = ManaCost {
            symbols: vec![ManaSymbol::X],
        };
        assert_eq!(cost.resolve_x(3).mana_value(), 3);
    }

    #[test]
    fn resolve_x_substitutes_generic_and_leaves_other_symbols_alone() {
        let cost = ManaCost {
            symbols: vec![
                ManaSymbol::X,
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
            ],
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
        // CR 107.3i
        let cost = ManaCost {
            symbols: vec![ManaSymbol::X, ManaSymbol::X],
        };
        let resolved = cost.resolve_x(2);
        assert_eq!(
            resolved.symbols,
            vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(2)),
                ManaSymbol::Atom(AtomicManaSymbol::Generic(2)),
            ]
        );
    }

    #[test]
    fn apply_colored_cost_delta_reduces_an_exact_match_with_no_spillover() {
        // CR 118.7
        let cost = ManaCost {
            symbols: vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White)),
            ],
        };
        let (adjusted, spillover) = cost.apply_colored_cost_delta(Color::White, -1);
        assert_eq!(spillover, 0);
        assert_eq!(
            adjusted.symbols,
            vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White)),
            ]
        );
    }

    #[test]
    fn apply_colored_cost_delta_over_reduction_spills_the_excess() {
        // CR 118.7c
        let cost = ManaCost {
            symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White))],
        };
        let (adjusted, spillover) = cost.apply_colored_cost_delta(Color::White, -2);
        assert_eq!(spillover, -1);
        assert!(adjusted.symbols.is_empty());
    }

    #[test]
    fn apply_colored_cost_delta_with_no_matching_color_spills_entirely() {
        // CR 118.7b
        let cost = ManaCost {
            symbols: vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))],
        };
        let (adjusted, spillover) = cost.apply_colored_cost_delta(Color::White, -1);
        assert_eq!(spillover, -1);
        assert_eq!(
            adjusted.symbols,
            vec![ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red))]
        );
    }

    #[test]
    fn apply_colorless_cost_delta_reduces_the_colorless_component() {
        let cost = ManaCost {
            symbols: vec![AtomicManaSymbol::Colorless, AtomicManaSymbol::Colorless]
                .into_iter()
                .map(ManaSymbol::Atom)
                .collect(),
        };
        let (adjusted, spillover) = cost.apply_colorless_cost_delta(-3);
        assert_eq!(
            spillover, -1,
            "only 2 {{C}} were there to consume, the third spills"
        );
        assert!(adjusted.symbols.is_empty());
    }

    #[test]
    fn parse_generic_and_colored() {
        let cost = ManaCost::parse("{1}{R}").unwrap();
        assert_eq!(
            cost.symbols,
            vec![
                ManaSymbol::Atom(AtomicManaSymbol::Generic(1)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)),
            ]
        );
    }

    #[test]
    fn parse_repeated_colored_pips() {
        let cost = ManaCost::parse("{W}{W}").unwrap();
        assert_eq!(
            cost.symbols,
            vec![
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White)),
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::White)),
            ]
        );
    }

    #[test]
    fn parse_color_color_hybrid() {
        let cost = ManaCost::parse("{W/U}").unwrap();
        assert_eq!(
            cost.symbols,
            vec![ManaSymbol::Hybrid(
                AtomicManaSymbol::Colored(Color::White),
                AtomicManaSymbol::Colored(Color::Blue),
            )]
        );
    }

    #[test]
    fn parse_generic_color_hybrid() {
        let cost = ManaCost::parse("{2/W}").unwrap();
        assert_eq!(
            cost.symbols,
            vec![ManaSymbol::Hybrid(
                AtomicManaSymbol::Generic(2),
                AtomicManaSymbol::Colored(Color::White),
            )]
        );
    }

    #[test]
    fn parse_plain_phyrexian() {
        let cost = ManaCost::parse("{W/P}").unwrap();
        assert_eq!(
            cost.symbols,
            vec![ManaSymbol::Atom(AtomicManaSymbol::Phyrexian(Color::White))]
        );
    }

    #[test]
    fn parse_hybrid_phyrexian() {
        let cost = ManaCost::parse("{B/G/P}").unwrap();
        assert_eq!(
            cost.symbols,
            vec![ManaSymbol::Hybrid(
                AtomicManaSymbol::Phyrexian(Color::Black),
                AtomicManaSymbol::Colored(Color::Green),
            )]
        );
    }

    #[test]
    fn parse_snow_and_colorless() {
        let cost = ManaCost::parse("{S}{C}").unwrap();
        assert_eq!(
            cost.symbols,
            vec![
                ManaSymbol::Atom(AtomicManaSymbol::Snow),
                ManaSymbol::Atom(AtomicManaSymbol::Colorless),
            ]
        );
    }

    #[test]
    fn parse_x() {
        let cost = ManaCost::parse("{X}{G}").unwrap();
        assert_eq!(
            cost.symbols,
            vec![
                ManaSymbol::X,
                ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Green))
            ]
        );
    }

    #[test]
    fn parse_rejects_malformed_strings() {
        assert!(ManaCost::parse("1R").is_err(), "missing braces");
        assert!(ManaCost::parse("{1").is_err(), "unterminated brace");
        assert!(ManaCost::parse("{Q}").is_err(), "unknown letter");
        assert!(ManaCost::parse("{}").is_err(), "empty group");
        assert!(
            ManaCost::parse("{W/U/B}").is_err(),
            "3-part group not ending in P"
        );
    }

    #[test]
    fn color_set_contains_color_for_each_color() {
        assert!(ColorSet::all().contains_color(Color::White));
        assert!(ColorSet::all().contains_color(Color::Blue));
        assert!(ColorSet::all().contains_color(Color::Black));
        assert!(ColorSet::all().contains_color(Color::Red));
        assert!(ColorSet::all().contains_color(Color::Green));
        assert!(!ColorSet::empty().contains_color(Color::Red));
    }

    #[test]
    fn atomic_mana_symbol_mana_value() {
        assert_eq!(AtomicManaSymbol::Generic(5).mana_value(), 5);
        assert_eq!(AtomicManaSymbol::Colored(Color::White).mana_value(), 1);
        assert_eq!(AtomicManaSymbol::Colorless.mana_value(), 1);
        assert_eq!(AtomicManaSymbol::Snow.mana_value(), 1);
        assert_eq!(AtomicManaSymbol::Phyrexian(Color::Black).mana_value(), 1);
    }

    #[test]
    fn atomic_mana_symbol_colors() {
        assert_eq!(
            AtomicManaSymbol::Colored(Color::Red).colors(),
            ColorSet::RED
        );
        assert_eq!(
            AtomicManaSymbol::Phyrexian(Color::White).colors(),
            ColorSet::WHITE
        );
        assert_eq!(AtomicManaSymbol::Generic(2).colors(), ColorSet::empty());
        assert_eq!(AtomicManaSymbol::Colorless.colors(), ColorSet::empty());
        assert_eq!(AtomicManaSymbol::Snow.colors(), ColorSet::empty());
    }

    #[test]
    fn mana_symbol_colors() {
        assert_eq!(
            ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Green)).colors(),
            ColorSet::GREEN
        );
        let hybrid = ManaSymbol::Hybrid(
            AtomicManaSymbol::Colored(Color::White),
            AtomicManaSymbol::Colored(Color::Blue),
        );
        assert_eq!(hybrid.colors(), ColorSet::WHITE | ColorSet::BLUE);
        assert_eq!(ManaSymbol::X.colors(), ColorSet::empty());
    }

    #[test]
    fn mana_cost_colors_union_across_symbols() {
        let cost = ManaCost::parse("{W}{U}{B}{R}{G}{X}").unwrap();
        assert_eq!(cost.colors(), ColorSet::all());

        let generic_only = ManaCost::parse("{1}{C}").unwrap();
        assert_eq!(generic_only.colors(), ColorSet::empty());
    }

    #[test]
    fn apply_cost_delta_positive_negative_and_zero() {
        // CR 118.9: generic reduction floors at {0}; generic increase adds a new symbol.
        let base = ManaCost::parse("{1}").unwrap();
        assert!(base.apply_cost_delta(-1).symbols.is_empty());
        assert!(base.apply_cost_delta(-2).symbols.is_empty()); // floor at {0}

        let colored = ManaCost::parse("{R}").unwrap();
        let increased = colored.apply_cost_delta(1);
        assert_eq!(increased.symbols.len(), 2);
        assert!(
            increased
                .symbols
                .contains(&ManaSymbol::Atom(AtomicManaSymbol::Generic(1)))
        );
        assert!(
            increased
                .symbols
                .contains(&ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Red)))
        );

        assert_eq!(base.apply_cost_delta(0).symbols, base.symbols);
    }

    #[test]
    fn apply_cost_delta_order_independence_from_sum() {
        // Base {1} with -2 then +1 would give {1} if applied one by one, but summing first gives {0}.
        let base = ManaCost::parse("{1}").unwrap();
        assert!(base.apply_cost_delta(-2 + 1).symbols.is_empty());
    }

    #[test]
    fn mana_pool_empty_resets_all_fields() {
        let mut pool = ManaPool {
            white: 1,
            blue: 1,
            black: 1,
            red: 1,
            green: 1,
            colorless: 1,
            snow_white: 1,
            snow_blue: 1,
            snow_black: 1,
            snow_red: 1,
            snow_green: 1,
            snow_colorless: 1,
            restricted: vec![ManaUnit::Colored(Color::White)],
        };
        pool.empty();
        assert_eq!(pool, ManaPool::default());
    }

    #[test]
    fn mana_pool_add_unit_bumps_each_bucket() {
        let mut pool = ManaPool::default();
        pool.add_unit(ManaUnit::Colored(Color::Red));
        assert_eq!(pool.red, 1);

        pool.add_unit(ManaUnit::SnowColored(Color::Blue));
        assert_eq!(pool.blue, 1);
        assert_eq!(pool.snow_blue, 1);

        pool.add_unit(ManaUnit::Colorless);
        assert_eq!(pool.colorless, 1);

        pool.add_unit(ManaUnit::SnowColorless);
        assert_eq!(pool.colorless, 2);
        assert_eq!(pool.snow_colorless, 1);

        pool.add_unit(ManaUnit::RestrictedColored(
            Color::Black,
            crate::types::card::CardTypeSet::CREATURE,
        ));
        assert_eq!(pool.restricted.len(), 1);
    }

    #[test]
    fn mana_unit_from_snow_source_tags_colored_and_colorless() {
        assert_eq!(
            ManaUnit::Colored(Color::White).from_snow_source(),
            ManaUnit::SnowColored(Color::White)
        );
        assert_eq!(
            ManaUnit::Colorless.from_snow_source(),
            ManaUnit::SnowColorless
        );
        // Restricted units stay restricted.
        let restricted =
            ManaUnit::RestrictedColored(Color::Red, crate::types::card::CardTypeSet::CREATURE);
        assert_eq!(restricted.from_snow_source(), restricted);
    }
}
