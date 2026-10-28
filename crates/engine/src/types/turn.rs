/// CR 500. `PreGame` is not a CR phase; the engine waits in it during setup
/// until the host steps it into the first `Beginning` phase.
///
/// There is no `next()`: `Main` occurs twice per turn (CR 505.1) with
/// different successors, so only the engine loop can advance the turn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    PreGame,
    Beginning,
    Main,
    Combat,
    Ending,
}

/// CR 500-514. There is no `next()`: the successor depends on `Phase` too,
/// so the engine loop decides.
///
/// `PreCombatMain` and `PostCombatMain` are not CR steps (CR 500.4). They
/// tell the two Main phases apart (CR 505.1) so `GameState.step` never holds
/// a stale step from before the phase.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Untap,
    Upkeep,
    Draw,
    PreCombatMain,
    BeginningOfCombat,
    DeclareAttackers,
    DeclareBlockers,
    /// CR 510.1a, 702.7: inserted only when an attacker or blocker has first
    /// strike or double strike.
    FirstStrikeDamage,
    CombatDamage,
    EndOfCombat,
    PostCombatMain,
    End,
    Cleanup,
}
