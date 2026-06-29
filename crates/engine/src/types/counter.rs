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
        // Implemented ahead of phase order: the Phase 4 scenario tooling
        // needs this to construct PlayerState/GameObject without going
        // through their own (still todo!()) constructors, and `counters`
        // is the one private field blocking a plain struct literal from
        // outside this module. See crates/scenario's module doc.
        CounterSet { counters: Vec::new() }
    }

    pub fn count(&self, kind: &CounterKind) -> i32 {
        // TODO: find self.counters entry matching kind (note: CounterKind
        // needs PartialEq derived/implemented for this to compile -
        // currently missing), return its count or 0 if absent.
        todo!()
    }

    /// Negative `amount` removes counters (e.g. -1/-1 cancelling +1/+1 —
    /// CR 121.3-121.4 style annihilation is handled elsewhere; this just
    /// adjusts the named kind's count).
    pub fn add(&mut self, kind: CounterKind, amount: i32) {
        // TODO: find or insert the (kind, count) entry, add amount. Decide
        // whether a count reaching 0 should remove the entry or just sit at
        // 0 - matters for CounterSet::count's "absent" vs "present at 0"
        // distinction if that's ever observable.
        todo!()
    }
}
