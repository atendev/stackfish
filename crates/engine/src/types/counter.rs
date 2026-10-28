/// Counters live on both `GameObject` and `PlayerState`. Kinds with direct
/// rules significance get a variant; the long tail (CR 122.3) is `Custom`.
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

/// Counter bookkeeping shared by `GameObject` and `PlayerState`.
///
/// Stores one timestamped entry per `add` call, not a total per kind.
/// `+1/+1` and `-1/-1` counters apply in layer 7c in timestamp order with
/// `ModifyPT` and `DoublePT` effects (CR 613.4c), so a counter added before a
/// `SetPT` must still apply after it (CR 613.5). `count` sums the entries.
#[derive(Clone, Debug, PartialEq)]
pub struct CounterSet {
    counters: Vec<(CounterKind, i32, crate::types::ids::SequenceNo)>,
}

impl CounterSet {
    pub fn new() -> Self {
        CounterSet { counters: Vec::new() }
    }

    pub fn count(&self, kind: &CounterKind) -> i32 {
        self.counters.iter().filter(|(k, _, _)| k == kind).map(|(_, n, _)| n).sum()
    }

    /// A negative `amount` removes counters. `timestamp` is this addition's
    /// CR 613.7 timestamp and must come from `GameState::allocate_sequence_no`.
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

    /// CR 613.4c: `kind`'s entries with their timestamps, so the layer 7c
    /// fold in `layers::effective_characteristics` can interleave them with
    /// `ModifyPT` and `DoublePT` effects.
    pub(crate) fn batches(&self, kind: &CounterKind) -> impl Iterator<Item = (i32, crate::types::ids::SequenceNo)> + '_ {
        let kind = kind.clone();
        self.counters.iter().filter(move |(k, _, _)| *k == kind).map(|(_, n, ts)| (*n, *ts))
    }
}
