use crate::types::event::GameEvent;
use crate::types::game::GameState;

/// One pass of CR 704's state-based actions: finds every applicable SBA
/// (lethal damage, 0 toughness, player at 0 life, 10+ poison, legend rule,
/// etc.) and applies all of them simultaneously, mutating `state` and
/// returning the `GameEvent`s that resulted (destroys, life-loss-induced
/// losses, etc) for the caller to append to the ledger and feed into
/// trigger collection.
///
/// This is a single pass, not the fixed-point loop — CR 704.3 requires
/// repeating until no SBA applies. The engine loop (`engine.rs`) is
/// responsible for calling this in a loop until it returns empty, not this
/// function itself, so a caller stepping through ledger entries one at a
/// time can observe each pass separately rather than the loop being opaque.
pub fn check_state_based_actions(state: &mut GameState) -> Vec<GameEvent> {
    let _ = state;
    // TODO: check each CR 704.5 SBA against current state and collect the
    // ones that apply (simultaneously, not short-circuiting on the first
    // found):
    //   - player at 0 or less life loses (704.5a)
    //   - player who attempted to draw from an empty library loses (704.5b)
    //   - player with 10+ poison counters loses (704.5c)
    //   - creature with toughness 0 (via effective_characteristics,
    //     layers.rs) goes to graveyard (704.5f)
    //   - creature with damage_marked >= effective toughness, or that's
    //     been dealt deathtouch damage, is destroyed (704.5g/h)
    //   - legend rule: two+ legendary permanents with the same name under
    //     one controller, owner chooses one to keep (704.5j)
    //   - planeswalker with loyalty 0 goes to graveyard (704.5i) - not v1
    //     scope per DESIGN.md non-goals, skip until planeswalkers exist
    //   - commander damage >= 21 from a single source - PlayerState::
    //     commander_damage_from already exposed for this exact check
    // None of these are implemented yet - each needs machinery (layers.rs,
    // combat-damage tracking) that doesn't exist. Returning empty is
    // correct for now (no SBA can actually apply with no creatures/damage
    // ever tracked), not a shortcut around real logic - revisit once any
    // prerequisite above lands. Caller (engine.rs) loops this until it
    // returns empty (CR 704.3).
    Vec::new()
}
