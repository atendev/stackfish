/// Shared by `GameObject` and `PlayerState` — counters live on both (e.g.
/// +1/+1 on a creature vs. poison/energy/experience on a player). Common
/// kinds with direct rules significance get a variant; the long tail of
/// flavor-named counters (CR 122.3 lists 100+) falls back to `Custom`.
#[derive(Clone, PartialEq, Eq, Debug)]
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
#[derive(Clone, Debug)]
pub struct CounterSet {
    counters: Vec<(CounterKind, i32)>,
}

impl CounterSet {
    pub fn new() -> Self {
        // Implemented ahead of phase order: the Phase 4 scenario tooling
        // needs this to construct PlayerState/GameObject without going
        // through their own (still todo!()) constructors, and `counters`
        // is the one private field blocking a plain struct literal from
        // outside this module. See crates/scenario's module doc.
        CounterSet { counters: Vec::new() }
    }

    pub fn count(&self, kind: &CounterKind) -> i32 {
        self.counters.iter().find(|(k, _)| k == kind).map(|(_, n)| *n).unwrap_or(0)
    }

    /// Negative `amount` removes counters (e.g. -1/-1 cancelling +1/+1 —
    /// CR 121.3-121.4 style annihilation is handled elsewhere; this just
    /// adjusts the named kind's count).
    pub fn add(&mut self, kind: CounterKind, amount: i32) {
        match self.counters.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, n)) => *n += amount,
            None => self.counters.push((kind, amount)),
        }
    }
}
