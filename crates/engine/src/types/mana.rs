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

impl ColorSet {
    /// Membership test against a single `Color` — used by
    /// `AbilityEffect::AddManaChoice` to validate a player's chosen color is
    /// actually among the ability's allowed set.
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
    /// Colors this atom contributes to a card's color (CR 202.2) — empty for
    /// `Generic`/`Colorless`/`Snow`.
    pub fn colors(&self) -> ColorSet {
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
        match self {
            ManaSymbol::Atom(a) => a.colors(),
            ManaSymbol::Hybrid(a, b) => a.colors() | b.colors(),
            ManaSymbol::X => ColorSet::empty(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ManaCost {
    pub symbols: Vec<ManaSymbol>,
}

impl ManaCost {
    /// Union of colors across all symbols (CR 202.2).
    pub fn colors(&self) -> ColorSet {
        self.symbols.iter().fold(ColorSet::empty(), |acc, s| acc | s.colors())
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

    /// CR 601.2f/118.9: applies one cost-modification effect's signed
    /// `delta` to this cost's generic component only (a plain `Atom(Generic(_))`
    /// symbol — same "Generic/numeric only, not colored, not inside a
    /// Hybrid" scope `generic_total`'s own doc comment already draws) —
    /// negative `delta` reduces, positive increases, and the result is
    /// clamped at `{0}` (CR 118.9: generic can't go negative; nothing here
    /// ever removes a colored pip to compensate). If this cost has no bare
    /// `Generic` symbol yet, a positive `delta` still creates one (a cost
    /// increase can introduce a generic component where none existed, e.g.
    /// Thalia taxing an otherwise-`{R}`-only spell) — a non-positive delta
    /// with nothing to reduce is simply a no-op.
    ///
    /// `cost.rs`'s `apply_cost_modifiers` calls this exactly once per spell,
    /// with the *sum* of every matching `CostModifier`'s `amount` — not
    /// once per modifier. An earlier version of this codebase called it
    /// once per modifier in registration order instead, on the claim that
    /// "applying one at a time still gets the common case (at most one
    /// reduction and one increase interacting) right, since clamping is
    /// idempotent past zero" — that claim was wrong (`phase5/cost-modifier-
    /// order`): base `{1}` with deltas `-2` then `+1` clamps to `{1}`,
    /// while the same two deltas in the opposite order clamp to `{0}` —
    /// registration order silently changed the result. Summing first and
    /// clamping once (what this function still does, unchanged — only its
    /// caller changed) is order-independent by construction, since addition
    /// commutes and there's only one floor applied at the very end.
    pub fn apply_cost_delta(&self, delta: i32) -> ManaCost {
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

    /// Parses a printed mana-cost string (CR 107.4/202.3's `{...}`-group
    /// templating, e.g. `"{1}{R}"`, `"{X}{G}"`, `"{W/U}"`, `"{2/W}"`,
    /// `"{W/P}"`, `"{B/G/P}"`, `"{S}"`) into every symbol shape this engine
    /// already models (`AtomicManaSymbol`/`ManaSymbol`) — a pure string→data
    /// parser, no new payment logic: once a real `ManaCost` exists, the
    /// existing `PaymentPlan`/`ManaCost` machinery (6a's Phyrexian/snow/
    /// hybrid payment) handles it exactly like a spell's cost, unmodified.
    ///
    /// Closes the "no Lua-facing mana-cost-string parser exists anywhere
    /// yet" gap `ActivatedAbilityBuilder` (script.rs) previously flagged —
    /// `CardDefinition.mana_cost` (spells) is untouched by this: it's still
    /// sourced as a structured `ManaCost` from the external card database,
    /// never parsed from a string.
    ///
    /// Grammar per `{...}` group: digits → `Generic`; a single WUBRG letter →
    /// `Colored`; `C` → `Colorless` (parses fine, still unpayable — see
    /// `atom_branches`'s own doc comment, a pre-existing gap this doesn't
    /// fix); `S` → `Snow`; `X` → bare `ManaSymbol::X`; `<letter>/P` → plain
    /// `Phyrexian` (not a `Hybrid` — see this type's own doc comment on why);
    /// `<A>/<B>` (color/color or generic/color) → `Hybrid`; `<A>/<B>/P`
    /// (both `A`/`B` colors) → hybrid-Phyrexian `Hybrid(Phyrexian(A),
    /// Colored(B))`, matching the convention `cost.rs`'s own
    /// `hybrid_phyrexian_offers_three_real_branches` test already documents.
    /// Anything outside a `{...}` group, an empty/unterminated group, or an
    /// unrecognized shape inside one is a hard parse error, not guessed at.
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

/// `ManaCost::parse`'s error type — a plain human-readable message, same
/// "a String is enough, no fancy variant taxonomy needed" stance this
/// codebase already takes for script-load errors (`ScriptError::LoadError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaCostParseError(pub String);

/// Parses one `{...}` group's inner text (already stripped of braces) —
/// shared by every call `ManaCost::parse` makes per symbol. `whole` is the
/// original full string, kept only for error messages.
fn parse_symbol_group(inner: &str, whole: &str) -> Result<ManaSymbol, ManaCostParseError> {
    if inner.is_empty() {
        return Err(ManaCostParseError(format!("empty \"{{}}\" group in \"{whole}\"")));
    }
    if inner.chars().all(|c| c.is_ascii_digit()) {
        let n: u32 = inner
            .parse()
            .map_err(|_| ManaCostParseError(format!("generic amount \"{inner}\" out of range in \"{whole}\"")))?;
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
            let color = parse_color_letter(a)
                .map_err(|_| ManaCostParseError(format!("\"{{{inner}}}\" needs a color before /P in \"{whole}\"")))?;
            Ok(ManaSymbol::Atom(AtomicManaSymbol::Phyrexian(color)))
        }
        [a, b] => {
            let atom_a = parse_hybrid_atom(a, inner, whole)?;
            let atom_b = parse_hybrid_atom(b, inner, whole)?;
            Ok(ManaSymbol::Hybrid(atom_a, atom_b))
        }
        [a, b, c] if *c == "P" => {
            let color_a = parse_color_letter(a).map_err(|_| {
                ManaCostParseError(format!("\"{{{inner}}}\" needs two colors before /P in \"{whole}\""))
            })?;
            let color_b = parse_color_letter(b).map_err(|_| {
                ManaCostParseError(format!("\"{{{inner}}}\" needs two colors before /P in \"{whole}\""))
            })?;
            Ok(ManaSymbol::Hybrid(
                AtomicManaSymbol::Phyrexian(color_a),
                AtomicManaSymbol::Colored(color_b),
            ))
        }
        _ => Err(ManaCostParseError(format!("unrecognized mana symbol \"{{{inner}}}\" in \"{whole}\""))),
    }
}

/// A 2-part hybrid's own atom: either a generic amount or a single color —
/// never `P` on its own (plain Phyrexian is handled as its own case in
/// `parse_symbol_group` before this is ever called on a `/P` pair).
fn parse_hybrid_atom(part: &str, inner: &str, whole: &str) -> Result<AtomicManaSymbol, ManaCostParseError> {
    if part.chars().all(|c| c.is_ascii_digit()) && !part.is_empty() {
        let n: u32 = part
            .parse()
            .map_err(|_| ManaCostParseError(format!("generic amount \"{part}\" out of range in \"{whole}\"")))?;
        return Ok(AtomicManaSymbol::Generic(n));
    }
    parse_color_letter(part)
        .map(AtomicManaSymbol::Colored)
        .map_err(|_| ManaCostParseError(format!("unrecognized hybrid component \"{{{inner}}}\" in \"{whole}\"")))
}

/// `pub` rather than crate-private — the scenario crate's TOML loader
/// (`chosen_colors: Vec<String>` on its `ActivateAbility` action spec) reuses
/// this same WUBRG-letter vocabulary rather than inventing a second parser.
pub fn parse_color_letter(s: &str) -> Result<Color, ManaCostParseError> {
    match s {
        "W" => Ok(Color::White),
        "U" => Ok(Color::Blue),
        "B" => Ok(Color::Black),
        "R" => Ok(Color::Red),
        "G" => Ok(Color::Green),
        other => Err(ManaCostParseError(format!("unknown color letter \"{other}\""))),
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
        assert_eq!(cost.symbols, vec![ManaSymbol::Atom(AtomicManaSymbol::Phyrexian(Color::White))]);
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
            vec![ManaSymbol::X, ManaSymbol::Atom(AtomicManaSymbol::Colored(Color::Green))]
        );
    }

    #[test]
    fn parse_rejects_malformed_strings() {
        assert!(ManaCost::parse("1R").is_err(), "missing braces");
        assert!(ManaCost::parse("{1").is_err(), "unterminated brace");
        assert!(ManaCost::parse("{Q}").is_err(), "unknown letter");
        assert!(ManaCost::parse("{}").is_err(), "empty group");
        assert!(ManaCost::parse("{W/U/B}").is_err(), "3-part group not ending in P");
    }
}
