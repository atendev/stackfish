use serde::{Deserialize, Serialize};
use stackfish_engine::{AtomicManaSymbol, Color, ManaCost, ManaSymbol, ManaUnit};

use super::{WireCardType, WireColor, card_type_set, card_types};

fn color_letter(color: Color) -> char {
    match color {
        Color::White => 'W',
        Color::Blue => 'U',
        Color::Black => 'B',
        Color::Red => 'R',
        Color::Green => 'G',
    }
}

fn atom(symbol: &AtomicManaSymbol) -> String {
    match symbol {
        AtomicManaSymbol::Colored(color) => color_letter(*color).to_string(),
        AtomicManaSymbol::Generic(n) => n.to_string(),
        AtomicManaSymbol::Colorless => "C".to_string(),
        AtomicManaSymbol::Snow => "S".to_string(),
        AtomicManaSymbol::Phyrexian(color) => format!("{}/P", color_letter(*color)),
    }
}

/// A hybrid symbol. One with a Phyrexian half is written `{A/B/P}`, which is
/// the form `ManaCost::parse` reads back, and not `{A/P/B}`.
fn hybrid(a: &AtomicManaSymbol, b: &AtomicManaSymbol) -> String {
    match (a, b) {
        (AtomicManaSymbol::Phyrexian(p), AtomicManaSymbol::Colored(c))
        | (AtomicManaSymbol::Colored(c), AtomicManaSymbol::Phyrexian(p)) => {
            format!("{{{}/{}/P}}", color_letter(*p), color_letter(*c))
        }
        _ => format!("{{{}/{}}}", atom(a), atom(b)),
    }
}

/// A mana cost in the usual brace notation, such as `{2}{R}{G/W}`. An empty
/// cost is an empty string.
pub(crate) fn mana_cost_string(cost: &ManaCost) -> String {
    cost.symbols
        .iter()
        .map(|symbol| match symbol {
            ManaSymbol::Atom(a) => format!("{{{}}}", atom(a)),
            ManaSymbol::Hybrid(a, b) => hybrid(a, b),
            ManaSymbol::X => "{X}".to_string(),
        })
        .collect()
}

/// One unit of mana in a pool or a payment. A restricted unit can pay only for
/// spells of the listed types.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WireManaUnit {
    Colored { color: WireColor },
    Colorless,
    SnowColored { color: WireColor },
    SnowColorless,
    RestrictedColored { color: WireColor, types: Vec<WireCardType> },
    RestrictedColorless { types: Vec<WireCardType> },
}

impl From<ManaUnit> for WireManaUnit {
    fn from(unit: ManaUnit) -> Self {
        match unit {
            ManaUnit::Colored(color) => WireManaUnit::Colored { color: color.into() },
            ManaUnit::Colorless => WireManaUnit::Colorless,
            ManaUnit::SnowColored(color) => WireManaUnit::SnowColored { color: color.into() },
            ManaUnit::SnowColorless => WireManaUnit::SnowColorless,
            ManaUnit::RestrictedColored(color, types) => {
                WireManaUnit::RestrictedColored { color: color.into(), types: card_types(types) }
            }
            ManaUnit::RestrictedColorless(types) => WireManaUnit::RestrictedColorless { types: card_types(types) },
        }
    }
}

impl From<WireManaUnit> for ManaUnit {
    fn from(unit: WireManaUnit) -> Self {
        match unit {
            WireManaUnit::Colored { color } => ManaUnit::Colored(color.into()),
            WireManaUnit::Colorless => ManaUnit::Colorless,
            WireManaUnit::SnowColored { color } => ManaUnit::SnowColored(color.into()),
            WireManaUnit::SnowColorless => ManaUnit::SnowColorless,
            WireManaUnit::RestrictedColored { color, types } => {
                ManaUnit::RestrictedColored(color.into(), card_type_set(&types))
            }
            WireManaUnit::RestrictedColorless { types } => ManaUnit::RestrictedColorless(card_type_set(&types)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cost_prints_in_the_form_the_parser_reads() {
        for text in ["{2}{R}{G/W}", "{2/W}{C}{S}", "{X}{X}{B}", "{1}{B/P}{B/P}", "{W/U/P}", "{U/P}"] {
            let cost = ManaCost::parse(text).expect("valid cost");
            assert_eq!(mana_cost_string(&cost), text);
        }
    }
}
