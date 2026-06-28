/// CR 500, plus `PreGame` — not a CR-defined phase, but the engine needs a
/// state to sit in for setup (shuffling, mulligans, commander/partner
/// reveal, opening hand) before the turn loop's first `Beginning` phase
/// starts. The host steps the engine out of it explicitly once setup
/// resolves.
pub enum Phase {
    PreGame,
    Beginning,
    Main,
    Combat,
    Ending,
}

/// CR 500-514.
pub enum Step {
    Untap,
    Upkeep,
    Draw,
    BeginningOfCombat,
    DeclareAttackers,
    DeclareBlockers,
    CombatDamage,
    EndOfCombat,
    End,
    Cleanup,
}
