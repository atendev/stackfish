/// Shared by `GameObject` and `PlayerState` — counters live on both (e.g.
/// +1/+1 on a creature vs. poison/energy/experience on a player). Common
/// kinds with direct rules significance get a variant; the long tail of
/// flavor-named counters (CR 122.3 lists 100+) falls back to `Custom`.
pub enum CounterKind {
    PlusOnePlusOne,
    MinusOneMinusOne,
    Loyalty,
    Poison,
    Energy,
    Experience,
    Charge,
    Custom(String),
}

/// The counter-tracking behavior `GameObject` and `PlayerState` both need
/// (count of a given kind, adjusting a kind's count) lived as duplicate
/// methods over a duplicate `Vec<(CounterKind, i32)>` field on each. Pulled
/// out once, here, so both just hold one `CounterSet` field instead.
pub struct CounterSet {
    counters: Vec<(CounterKind, i32)>,
}

impl CounterSet {
    pub fn new() -> Self {
        todo!()
    }

    pub fn count(&self, kind: &CounterKind) -> i32 {
        todo!()
    }

    /// Negative `amount` removes counters (e.g. -1/-1 cancelling +1/+1 —
    /// CR 121.3-121.4 style annihilation is handled elsewhere; this just
    /// adjusts the named kind's count).
    pub fn add(&mut self, kind: CounterKind, amount: i32) {
        todo!()
    }
}
