use serde::Serialize;
use stackfish_engine::{CounterSet, GameObject, StackEntryKind};

use super::event::{WireCounter, WireIdentity, WirePhase, WireStep};
use super::{U64, WireTarget, WireZone};

pub(crate) fn counters(set: &CounterSet) -> Vec<WireCounter> {
    set.totals()
        .iter()
        .map(|(kind, amount)| WireCounter {
            counter: kind.into(),
            amount: *amount,
        })
        .collect()
}

/// One Seat's view of the current state through `through_sequence_no`, which is `null`
/// before the first event. Zones, counts, turn, phase, step, priority, life and poison
/// equal the fold of the events that Seat receives; a revealed identity in a hidden zone
/// is not kept.
#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireSnapshot {
    pub through_sequence_no: Option<U64>,
    pub turn_number: u32,
    pub active_player: u32,
    pub phase: WirePhase,
    pub step: WireStep,
    pub priority: u32,
    pub players: Vec<WirePlayerView>,
    pub zones: Vec<WireZoneView>,
    /// Top of the stack last, as `stack_object_pushed` events arrive.
    pub stack: Vec<WireStackEntry>,
}

/// A player as every Seat sees them. Hidden zones show only their counts here.
#[derive(Serialize, Clone, Debug)]
pub(crate) struct WirePlayerView {
    pub player: u32,
    pub life: i32,
    pub counters: Vec<WireCounter>,
    pub commander_damage: Vec<WireCommanderDamage>,
    pub eliminated: bool,
    pub hand_count: u32,
    pub library_count: u32,
}

/// Combat damage taken from one commander, keyed by its name (CR 903.10a).
#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireCommanderDamage {
    pub commander: String,
    pub amount: u32,
}

/// A zone the Seat may look into. A public zone is in engine order with index 0 on top.
/// A hand or library is sorted by id, and a library is listed only for an omniscient Seat.
#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireZoneView {
    pub zone: WireZone,
    /// `null` for the shared battlefield and exile.
    pub owner: Option<u32>,
    pub objects: Vec<WireObjectView>,
}

/// An object as it stands now. `identity` is absent for a face-down object the Seat may
/// not identify. Only copy effects apply, as in `WireObjectSummary`.
#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireObjectView {
    pub object: U64,
    pub owner: u32,
    pub controller: u32,
    pub tapped: bool,
    pub face_down: bool,
    pub counters: Vec<WireCounter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attached_to: Option<U64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attached_to_player: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<WireIdentity>,
}

impl WireObjectView {
    pub(crate) fn new(object: &GameObject, sees_identity: bool) -> Self {
        WireObjectView {
            object: object.object_id.into(),
            owner: object.owner.0,
            controller: object.controller.0,
            tapped: object.tapped,
            face_down: object.face_down,
            counters: counters(&object.counters),
            attached_to: object.attached_to.map(U64::from),
            attached_to_player: object.attached_to_player.map(|p| p.0),
            identity: sees_identity.then(|| WireIdentity::from(object)),
        }
    }
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WireStackKind {
    Spell,
    ActivatedAbility,
    TriggeredAbility,
    Copy,
}

impl From<&StackEntryKind> for WireStackKind {
    fn from(kind: &StackEntryKind) -> Self {
        match kind {
            StackEntryKind::Spell => WireStackKind::Spell,
            StackEntryKind::ActivatedAbility => WireStackKind::ActivatedAbility,
            StackEntryKind::TriggeredAbility => WireStackKind::TriggeredAbility,
            StackEntryKind::Copy { .. } => WireStackKind::Copy,
        }
    }
}

/// CR 707.10: the kind a copy copies, `None` for an entry that is not a copy.
/// A copy of a copy reports the original's kind, so this is never `Copy`.
// TODO: no spell or ability copy is ever put on the stack, so a copy's text and index are never set (CR 707.10)
pub(crate) fn copy_of(kind: &StackEntryKind) -> Option<WireStackKind> {
    fn original(kind: &StackEntryKind) -> WireStackKind {
        match kind {
            StackEntryKind::Copy { of } => original(of),
            other => other.into(),
        }
    }
    match kind {
        StackEntryKind::Copy { of } => Some(original(of)),
        _ => None,
    }
}

/// One stack entry. `card` is the spell's object on the stack, absent for an ability.
/// `source` is absent where the Seat cannot see it, as in `trigger_queued`.
#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireStackEntry {
    pub stack_id: U64,
    pub kind: WireStackKind,
    /// Set only when `kind` is `copy`: the kind of what was copied, never `copy`.
    /// A Seat labels a copy of an ability with it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy_of: Option<WireStackKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<U64>,
    pub owner: u32,
    pub controller: u32,
    pub targets: Vec<WireTarget>,
    /// CR 601.2d: the announced shares, index for index with `targets`. Empty when
    /// the effect is undivided or has one target, which takes the whole total.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub division: Vec<u32>,
    pub chosen_modes: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x_value: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<WireObjectView>,
    /// CR 405.4: an ability's text, absent for a spell or an ability whose script
    /// set none. Every Seat gets it, because the stack is public (CR 400.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ability_text: Option<String>,
    /// The ability's position among its source's abilities when it was put on the
    /// stack, as `find_ability` counts them. Absent for a spell or a departed source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ability_index: Option<u32>,
    /// The source's identity, sent only when `source` is withheld from this Seat and
    /// the source was not face down. Absent for a spell, whose `card` already has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_identity: Option<WireIdentity>,
}
