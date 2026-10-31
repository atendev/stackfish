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

impl Default for CounterSet {
    fn default() -> Self {
        Self::new()
    }
}

impl CounterSet {
    pub fn new() -> Self {
        CounterSet {
            counters: Vec::new(),
        }
    }

    pub fn count(&self, kind: &CounterKind) -> i32 {
        self.counters
            .iter()
            .filter(|(k, _, _)| k == kind)
            .map(|(_, n, _)| n)
            .sum()
    }

    /// A negative `amount` removes counters. `timestamp` is this addition's
    /// CR 613.7 timestamp and must come from `GameState::allocate_sequence_no`.
    pub fn add(
        &mut self,
        kind: CounterKind,
        amount: i32,
        timestamp: crate::types::ids::SequenceNo,
    ) {
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
    pub(crate) fn batches(
        &self,
        kind: &CounterKind,
    ) -> impl Iterator<Item = (i32, crate::types::ids::SequenceNo)> + '_ {
        let kind = kind.clone();
        self.counters
            .iter()
            .filter(move |(k, _, _)| *k == kind)
            .map(|(_, n, ts)| (*n, *ts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ids::SequenceNo;

    #[test]
    fn new_counter_set_is_empty() {
        let set = CounterSet::new();
        assert!(set.totals().is_empty());
        assert_eq!(set.count(&CounterKind::PlusOnePlusOne), 0);
    }

    #[test]
    fn count_sums_entries_of_one_kind() {
        let mut set = CounterSet::new();
        set.add(CounterKind::PlusOnePlusOne, 1, SequenceNo(0));
        set.add(CounterKind::PlusOnePlusOne, 1, SequenceNo(1));
        set.add(CounterKind::MinusOneMinusOne, 1, SequenceNo(2));
        assert_eq!(set.count(&CounterKind::PlusOnePlusOne), 2);
        assert_eq!(set.count(&CounterKind::MinusOneMinusOne), 1);
    }

    #[test]
    fn negative_amount_reduces_count() {
        let mut set = CounterSet::new();
        set.add(CounterKind::PlusOnePlusOne, 3, SequenceNo(0));
        set.add(CounterKind::PlusOnePlusOne, -1, SequenceNo(1));
        assert_eq!(set.count(&CounterKind::PlusOnePlusOne), 2);
    }

    #[test]
    fn totals_groups_by_kind_in_first_seen_order() {
        let mut set = CounterSet::new();
        set.add(CounterKind::Energy, 2, SequenceNo(0));
        set.add(CounterKind::Charge, 1, SequenceNo(1));
        set.add(CounterKind::Energy, 1, SequenceNo(2));
        let totals = set.totals();
        assert_eq!(
            totals,
            vec![(CounterKind::Energy, 3), (CounterKind::Charge, 1)]
        );
    }

    #[test]
    fn totals_returns_zero_for_kinds_with_only_removals() {
        let mut set = CounterSet::new();
        set.add(CounterKind::PlusOnePlusOne, 1, SequenceNo(0));
        set.add(CounterKind::PlusOnePlusOne, -1, SequenceNo(1));
        assert_eq!(set.totals(), vec![(CounterKind::PlusOnePlusOne, 0)]);
    }

    #[test]
    fn batches_filters_by_kind_and_preserves_timestamps() {
        let mut set = CounterSet::new();
        set.add(CounterKind::Poison, 1, SequenceNo(5));
        set.add(CounterKind::Poison, 2, SequenceNo(7));
        set.add(CounterKind::Loyalty, 3, SequenceNo(6));
        let batches: Vec<_> = set.batches(&CounterKind::Poison).collect();
        assert_eq!(batches, vec![(1, SequenceNo(5)), (2, SequenceNo(7))]);
    }
}
