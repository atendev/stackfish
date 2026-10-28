//! Wire twins of the engine types a message carries, since the engine has no serde. Names
//! are snake_case, every `u64` is a decimal string and `PlayerId` is a JSON number.
//! See `docs/DESIGN.md`.

mod decision;
mod event;
mod mana;
mod reply;

pub use decision::{WireCardIdentity, WireDecision, offered_objects};
pub use event::{WireEvent, WireEventKind, WireIdentity, WireObjectSummary, WireRevealedCard};
pub use reply::AnswerBody;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use stackfish_engine::decision::Candidate;
use stackfish_engine::replacement::ReplacementCandidate;
use stackfish_engine::{CardTypeSet, Color, ObjectId, Target, ZoneKind};

/// A `u64` written as a decimal string, because a JSON number loses precision
/// above 2^53 in some hosts. A JSON number is rejected on input.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct U64(pub u64);

impl Serialize for U64 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for U64 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(D::Error::custom(format!("{text:?} is not a decimal string")));
        }
        text.parse().map(U64).map_err(D::Error::custom)
    }
}

impl From<ObjectId> for U64 {
    fn from(id: ObjectId) -> Self {
        U64(id.0)
    }
}

impl From<U64> for ObjectId {
    fn from(id: U64) -> Self {
        ObjectId(id.0)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum WireColor {
    White,
    Blue,
    Black,
    Red,
    Green,
}

impl From<Color> for WireColor {
    fn from(color: Color) -> Self {
        match color {
            Color::White => WireColor::White,
            Color::Blue => WireColor::Blue,
            Color::Black => WireColor::Black,
            Color::Red => WireColor::Red,
            Color::Green => WireColor::Green,
        }
    }
}

impl From<WireColor> for Color {
    fn from(color: WireColor) -> Self {
        match color {
            WireColor::White => Color::White,
            WireColor::Blue => Color::Blue,
            WireColor::Black => Color::Black,
            WireColor::Red => Color::Red,
            WireColor::Green => Color::Green,
        }
    }
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum WireZone {
    Library,
    Hand,
    Battlefield,
    Graveyard,
    Stack,
    Exile,
    Command,
}

impl From<ZoneKind> for WireZone {
    fn from(zone: ZoneKind) -> Self {
        match zone {
            ZoneKind::Library => WireZone::Library,
            ZoneKind::Hand => WireZone::Hand,
            ZoneKind::Battlefield => WireZone::Battlefield,
            ZoneKind::Graveyard => WireZone::Graveyard,
            ZoneKind::Stack => WireZone::Stack,
            ZoneKind::Exile => WireZone::Exile,
            ZoneKind::Command => WireZone::Command,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum WireCardType {
    Artifact,
    Creature,
    Enchantment,
    Instant,
    Land,
    Sorcery,
    Kindred,
    Planeswalker,
    Battle,
}

const CARD_TYPES: [(CardTypeSet, WireCardType); 9] = [
    (CardTypeSet::ARTIFACT, WireCardType::Artifact),
    (CardTypeSet::CREATURE, WireCardType::Creature),
    (CardTypeSet::ENCHANTMENT, WireCardType::Enchantment),
    (CardTypeSet::INSTANT, WireCardType::Instant),
    (CardTypeSet::LAND, WireCardType::Land),
    (CardTypeSet::SORCERY, WireCardType::Sorcery),
    (CardTypeSet::KINDRED, WireCardType::Kindred),
    (CardTypeSet::PLANESWALKER, WireCardType::Planeswalker),
    (CardTypeSet::BATTLE, WireCardType::Battle),
];

pub(crate) fn card_types(set: CardTypeSet) -> Vec<WireCardType> {
    CARD_TYPES.iter().filter(|(flag, _)| set.contains(*flag)).map(|(_, wire)| *wire).collect()
}

pub(crate) fn card_type_set(types: &[WireCardType]) -> CardTypeSet {
    CARD_TYPES.iter().filter(|(_, wire)| types.contains(wire)).fold(CardTypeSet::empty(), |set, (flag, _)| set | *flag)
}

/// An object, a player or an ability on the stack (CR 113.9).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WireTarget {
    Object { id: U64 },
    Player { id: u32 },
    Ability { id: U64 },
}

impl From<Target> for WireTarget {
    fn from(target: Target) -> Self {
        match target {
            Target::Object(id) => WireTarget::Object { id: id.into() },
            Target::Player(player) => WireTarget::Player { id: player.0 },
            Target::Ability(id) => WireTarget::Ability { id: id.into() },
        }
    }
}

impl From<WireTarget> for Target {
    fn from(target: WireTarget) -> Self {
        match target {
            WireTarget::Object { id } => Target::Object(id.into()),
            WireTarget::Player { id } => Target::Player(stackfish_engine::PlayerId(id)),
            WireTarget::Ability { id } => Target::Ability(id.into()),
        }
    }
}

/// What a Pick Decision offers and what a Pick Answer names. `Replacement` is a
/// CR 616.1 choice between effects, so it names the effect's source, and
/// `protection` marks the CR 702.16e protection prevention.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WireCandidate {
    Object { id: U64 },
    Player { id: u32 },
    Ability { id: U64 },
    Replacement { source: U64, protection: bool },
}

impl From<Candidate> for WireCandidate {
    fn from(candidate: Candidate) -> Self {
        match candidate {
            Candidate::Target(Target::Object(id)) => WireCandidate::Object { id: id.into() },
            Candidate::Target(Target::Player(player)) => WireCandidate::Player { id: player.0 },
            Candidate::Target(Target::Ability(id)) => WireCandidate::Ability { id: id.into() },
            Candidate::Replacement(ReplacementCandidate::Registered(id)) => {
                WireCandidate::Replacement { source: id.into(), protection: false }
            }
            Candidate::Replacement(ReplacementCandidate::Protection(id)) => {
                WireCandidate::Replacement { source: id.into(), protection: true }
            }
        }
    }
}

impl From<WireCandidate> for Candidate {
    fn from(candidate: WireCandidate) -> Self {
        match candidate {
            WireCandidate::Object { id } => Candidate::Target(Target::Object(id.into())),
            WireCandidate::Player { id } => Candidate::Target(Target::Player(stackfish_engine::PlayerId(id))),
            WireCandidate::Ability { id } => Candidate::Target(Target::Ability(id.into())),
            WireCandidate::Replacement { source, protection: false } => {
                Candidate::Replacement(ReplacementCandidate::Registered(source.into()))
            }
            WireCandidate::Replacement { source, protection: true } => {
                Candidate::Replacement(ReplacementCandidate::Protection(source.into()))
            }
        }
    }
}
