//! `keywords.name` rows -> `KeywordSet` bits. The DB's `keywords` table
//! covers Scryfall's full ~140-entry CR 702 keyword list; `KeywordSet` only
//! models the subset that changes engine-checkpoint behavior (see that
//! type's own doc comment). Anything outside that subset is silently
//! dropped here, not an error - `KeywordSet` is explicitly documented as
//! "deliberately NOT meant to grow to cover all ~140 CR 702 keywords," so an
//! unrecognized name is expected input, not a bug to surface.

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
        "Can't be regenerated" => KeywordSet::CANT_BE_REGENERATED,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
