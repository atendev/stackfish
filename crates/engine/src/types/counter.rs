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
