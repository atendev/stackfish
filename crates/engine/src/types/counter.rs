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
///
/// Internally one entry *per `add` call* (a "batch"), each tagged with the
/// `SequenceNo` it was added at, rather than one coalesced running total per
/// kind — CR 613.4c's literal text has `+1/+1`/`-1/-1` counters apply in
/// layer 7c *together with*, and timestamp-ordered against, `ModifyPT`/
/// `DoublePT` effects (`layers::effective_characteristics`'s own layer-7c
/// fold reads `batches` for exactly this — see that function's own doc
/// comment for the worked CR 613.5 example this makes correct: a counter
/// added before a `SetPT` override must still apply *after* it, in 7c, not
/// get baked into a 7a base that the override throws away). `count` stays a
/// plain sum across all of a kind's batches — every other caller (Poison/
/// Energy/Charge/Loyalty, none of which ever interact with layer 7) sees no
/// behavior change at all, only P/T-affecting kinds' *relative timing*
/// against other layer-7c effects is new.
#[derive(Clone, Debug, PartialEq)]
pub struct CounterSet {
    counters: Vec<(CounterKind, i32, crate::types::ids::SequenceNo)>,
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
        self.counters.iter().filter(|(k, _, _)| k == kind).map(|(_, n, _)| n).sum()
    }

    /// Negative `amount` removes counters (e.g. -1/-1 cancelling +1/+1 —
    /// CR 121.3-121.4 style annihilation is handled elsewhere; this just
    /// adjusts the named kind's count). `timestamp` is this addition's own
    /// CR 613.7 timestamp (irrelevant to every kind but `PlusOnePlusOne`/
    /// `MinusOneMinusOne` — see `batches`) — always a fresh, real value from
    /// `GameState::allocate_sequence_no`, never reused/guessed at by a
    /// caller, same as every other timestamped construct in this engine.
    pub fn add(&mut self, kind: CounterKind, amount: i32, timestamp: crate::types::ids::SequenceNo) {
        self.counters.push((kind, amount, timestamp));
    }

    /// The total per counter kind, in the order each kind was first added.
    /// A host reads this to show what an object carried.
    pub fn totals(&self) -> Vec<(CounterKind, i32)> {
        let mut totals: Vec<(CounterKind, i32)> = Vec::new();
        for (kind, amount, _) in &self.counters {
            match totals.iter_mut().find(|(k, _)| k == kind) {
                Some((_, total)) => *total += amount,
                None => totals.push((kind.clone(), *amount)),
            }
        }
        totals
    }

    /// CR 613.4c: `kind`'s individual addition batches (not the coalesced
    /// `count`), each with the timestamp it was added at — read by
    /// `layers::effective_characteristics`'s layer 7c fold to interleave
    /// `+1/+1`/`-1/-1` counters with real `ModifyPT`/`DoublePT` effects by
    /// timestamp, instead of pre-netting them into the layer 7a base.
    pub(crate) fn batches(&self, kind: &CounterKind) -> impl Iterator<Item = (i32, crate::types::ids::SequenceNo)> + '_ {
        let kind = kind.clone();
        self.counters.iter().filter(move |(k, _, _)| *k == kind).map(|(_, n, ts)| (*n, *ts))
    }
}
