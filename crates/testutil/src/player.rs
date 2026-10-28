use stackfish_engine::*;

/// A `PlayerState` at game start: 20 life, empty mana pool, no mulligans taken.
/// Override fields with struct update syntax: `PlayerState { mana_pool: pool, ..testutil::player_state(p0) }`
pub fn player_state(player_id: PlayerId) -> PlayerState {
    PlayerState {
        player_id,
        life: 20,
        mana_pool: ManaPool::default(),
        counters: CounterSet::new(),
        emblems: Vec::new(),
        commander_damage_taken: Vec::new(),
        eliminated: false,
        drew_from_empty_library: false,
        mulligans_taken: 0,
        hand_kept: false,
        commander_casts: std::collections::HashMap::new(),
    }
}
