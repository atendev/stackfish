//! Card fingerprints: a canonical byte encoding of a card and its FNV-1a hash, so a replay
//! can tell that a key still resolves to the same card. See `docs/DESIGN.md`.

use stackfish_carddb::CardRecord;
use stackfish_engine::{
    AtomicManaSymbol, CardDefinition, Color, ManaCost, ManaSymbol, ModeChoice, PTValue,
    ScriptReference,
};

pub(crate) const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;

/// FNV-1a. It detects drift in card data and transcripts; it is not a security hash.
pub(crate) fn fnv(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

// Every field carries a length or a presence byte, so `("ab", "c")` and `("a", "bc")`
// encode differently. `Debug` text is never used: it can change with the toolchain.
struct Encoder(Vec<u8>);

impl Encoder {
    fn new(tag: &str) -> Self {
        let mut encoder = Encoder(Vec::new());
        encoder.str(tag);
        encoder
    }

    fn str(&mut self, s: &str) {
        self.u64(s.len() as u64);
        self.0.extend_from_slice(s.as_bytes());
    }

    fn opt_str(&mut self, s: Option<&str>) {
        match s {
            Some(s) => {
                self.0.push(1);
                self.str(s);
            }
            None => self.0.push(0),
        }
    }

    fn u64(&mut self, n: u64) {
        self.0.extend_from_slice(&n.to_le_bytes());
    }

    fn finish(&self) -> String {
        format!("{:016x}", fnv(FNV_OFFSET, &self.0))
    }
}

/// The fingerprint of a database card: every `cards` column the loader reads, the
/// sorted keyword names and the active script's version and source.
pub(crate) fn of_record(record: &CardRecord) -> String {
    let mut e = Encoder::new("carddb/1");
    e.str(&record.oracle_id);
    e.str(&record.name);
    e.opt_str(record.mana_cost.as_deref());
    e.str(&record.type_line);
    e.str(&record.oracle_text);
    for value in [
        &record.power,
        &record.toughness,
        &record.loyalty,
        &record.defense,
    ] {
        e.opt_str(value.as_deref());
    }
    e.u64(record.keywords.len() as u64);
    for keyword in &record.keywords {
        e.str(keyword);
    }
    match &record.script {
        Some(script) => {
            e.0.push(1);
            e.u64(script.version as u64);
            e.str(&script.source);
        }
        None => e.0.push(0),
    }
    e.finish()
}

/// The fingerprint of a definition built in code, over its printed characteristics and
/// its script source.
// TODO: fingerprint effect, modes, alternative_costs, buyback_cost and target_requirement
pub(crate) fn of_definition(card: &CardDefinition) -> String {
    let mut e = Encoder::new("definition/1");
    e.u64(card.definition_id);
    e.str(&card.name);
    e.str(&mana_cost_text(&card.mana_cost));
    match card.color_indicator {
        Some(colors) => {
            e.0.push(1);
            e.u64(u64::from(colors.bits()));
        }
        None => e.0.push(0),
    }
    e.u64(u64::from(card.supertypes.bits()));
    e.u64(u64::from(card.card_types.bits()));
    e.u64(card.subtypes.len() as u64);
    for subtype in &card.subtypes {
        e.str(&subtype.0);
    }
    e.str(&card.oracle_text);
    for value in [&card.power, &card.toughness, &card.loyalty, &card.defense] {
        pt(&mut e, value.as_ref());
    }
    script(&mut e, &card.script_ref);
    e.u64(u64::from(card.keywords.bits()));
    let (kind, n) = match card.mode_choice {
        ModeChoice::Exactly(n) => ("exactly", n),
        ModeChoice::UpTo(n) => ("up_to", n),
        ModeChoice::ExactlyRepeatable(n) => ("exactly_repeatable", n),
        ModeChoice::UpToRepeatable(n) => ("up_to_repeatable", n),
        ModeChoice::ExactlyChosenByOpponent(n) => ("exactly_chosen_by_opponent", n),
        ModeChoice::UpToChosenByOpponent(n) => ("up_to_chosen_by_opponent", n),
        ModeChoice::PawprintBudget(n) => ("pawprint_budget", n),
    };
    e.str(kind);
    e.u64(u64::from(n));
    e.finish()
}

fn pt(e: &mut Encoder, value: Option<&PTValue>) {
    match value {
        None => e.str("none"),
        Some(PTValue::Fixed(n)) => {
            e.str("fixed");
            e.str(&n.to_string());
        }
        Some(PTValue::CharacteristicDefining { display, script: s }) => {
            e.str("cda");
            e.str(display);
            match s {
                Some(s) => script(e, s),
                None => e.str("none"),
            }
        }
    }
}

fn script(e: &mut Encoder, script: &ScriptReference) {
    match script {
        ScriptReference::ByKey(key) => {
            e.str("key");
            e.str(key);
        }
        ScriptReference::Inline(source) => {
            e.str("inline");
            e.str(source);
        }
    }
}

fn mana_cost_text(cost: &ManaCost) -> String {
    let atom = |a: &AtomicManaSymbol| match a {
        AtomicManaSymbol::Colored(c) => color_letter(*c).to_string(),
        AtomicManaSymbol::Generic(n) => n.to_string(),
        AtomicManaSymbol::Colorless => "C".to_string(),
        AtomicManaSymbol::Snow => "S".to_string(),
        AtomicManaSymbol::Phyrexian(c) => format!("{}/P", color_letter(*c)),
    };
    cost.symbols
        .iter()
        .map(|symbol| match symbol {
            ManaSymbol::Atom(a) => format!("{{{}}}", atom(a)),
            ManaSymbol::Hybrid(a, b) => format!("{{{}/{}}}", atom(a), atom(b)),
            ManaSymbol::X => "{X}".to_string(),
        })
        .collect()
}

fn color_letter(color: Color) -> char {
    match color {
        Color::White => 'W',
        Color::Blue => 'U',
        Color::Black => 'B',
        Color::Red => 'R',
        Color::Green => 'G',
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stackfish_carddb::ScriptRecord;

    fn record() -> CardRecord {
        CardRecord {
            oracle_id: "o".to_string(),
            name: "Bear".to_string(),
            mana_cost: Some("{1}{G}".to_string()),
            type_line: "Creature — Bear".to_string(),
            oracle_text: String::new(),
            power: Some("2".to_string()),
            toughness: Some("2".to_string()),
            loyalty: None,
            defense: None,
            keywords: vec!["Trample".to_string()],
            script: Some(ScriptRecord {
                version: 1,
                source: "return {}".to_string(),
            }),
        }
    }

    #[test]
    fn a_record_fingerprint_is_pinned_so_a_change_to_the_encoding_is_noticed() {
        assert_eq!(of_record(&record()), "96825daa2428f7c6");
    }

    #[test]
    fn fields_are_length_prefixed_so_moving_a_boundary_changes_the_fingerprint() {
        let mut a = record();
        a.name = "ab".to_string();
        a.type_line = "c".to_string();
        let mut b = record();
        b.name = "a".to_string();
        b.type_line = "bc".to_string();
        assert_ne!(of_record(&a), of_record(&b));
    }

    #[test]
    fn an_absent_field_differs_from_an_empty_one() {
        let mut a = record();
        a.loyalty = Some(String::new());
        assert_ne!(of_record(&a), of_record(&record()));
    }

    #[test]
    fn the_script_version_and_source_are_both_part_of_the_fingerprint() {
        let mut version = record();
        version.script.as_mut().unwrap().version = 2;
        let mut source = record();
        source.script.as_mut().unwrap().source = "return { }".to_string();
        let mut none = record();
        none.script = None;
        let base = of_record(&record());
        for other in [version, source, none] {
            assert_ne!(of_record(&other), base);
        }
    }

    #[test]
    fn a_mana_cost_is_written_in_its_printed_form() {
        let cost = ManaCost::parse("{2}{W/U}{G/P}{X}{C}{S}").unwrap();
        assert_eq!(mana_cost_text(&cost), "{2}{W/U}{G/P}{X}{C}{S}");
    }
}
