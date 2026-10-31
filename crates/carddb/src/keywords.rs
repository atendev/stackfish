//! Maps `keywords.name` rows to `KeywordSet` bits. Names `KeywordSet` does not model are
//! dropped without an error, since the DB lists every CR 702 keyword.

use stackfish_engine::KeywordSet;

pub fn parse(names: &[String]) -> KeywordSet {
    let mut set = KeywordSet::empty();
    for name in names {
        if let Some(bit) = parse_one(name) {
            set |= bit;
        }
    }
    set
}

fn parse_one(name: &str) -> Option<KeywordSet> {
    Some(match name {
        "Flying" => KeywordSet::FLYING,
        "Reach" => KeywordSet::REACH,
        "Menace" => KeywordSet::MENACE,
        "Vigilance" => KeywordSet::VIGILANCE,
        "Haste" => KeywordSet::HASTE,
        "Defender" => KeywordSet::DEFENDER,
        "Flash" => KeywordSet::FLASH,
        "Deathtouch" => KeywordSet::DEATHTOUCH,
        "Indestructible" => KeywordSet::INDESTRUCTIBLE,
        "Lifelink" => KeywordSet::LIFELINK,
        "Wither" => KeywordSet::WITHER,
        "Infect" => KeywordSet::INFECT,
        "First strike" => KeywordSet::FIRST_STRIKE,
        "Double strike" => KeywordSet::DOUBLE_STRIKE,
        "Trample" => KeywordSet::TRAMPLE,
        "Fear" => KeywordSet::FEAR,
        "Intimidate" => KeywordSet::INTIMIDATE,
        "Shadow" => KeywordSet::SHADOW,
        "Horsemanship" => KeywordSet::HORSEMANSHIP,
        "Banding" => KeywordSet::BANDING,
        "Hexproof" => KeywordSet::HEXPROOF,
        "Shroud" => KeywordSet::SHROUD,
        "Attacks each combat if able" => KeywordSet::ATTACKS_EACH_COMBAT,
        "Can't be regenerated" => KeywordSet::CANT_BE_REGENERATED,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_must_attack_clause_sets_its_bit() {
        assert_eq!(
            parse(&["Attacks each combat if able".to_string()]),
            KeywordSet::ATTACKS_EACH_COMBAT
        );
    }

    #[test]
    fn recognized_keywords_set_bits() {
        let set = parse(&["Flying".to_string(), "Trample".to_string()]);
        assert_eq!(set, KeywordSet::FLYING | KeywordSet::TRAMPLE);
    }

    #[test]
    fn hexproof_and_shroud_set_bits() {
        let set = parse(&["Hexproof".to_string(), "Shroud".to_string()]);
        assert_eq!(set, KeywordSet::HEXPROOF | KeywordSet::SHROUD);
    }

    #[test]
    fn unrecognized_keywords_are_silently_dropped() {
        let set = parse(&["Flying".to_string(), "Cumulative upkeep".to_string()]);
        assert_eq!(set, KeywordSet::FLYING);
    }

    #[test]
    fn empty_input_is_empty_set() {
        assert_eq!(parse(&[]), KeywordSet::empty());
    }
}
