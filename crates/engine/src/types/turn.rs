/// CR 500, plus `PreGame` — not a CR-defined phase, but the engine needs a
/// state to sit in for setup (shuffling, mulligans, commander/partner
/// reveal, opening hand) before the turn loop's first `Beginning` phase
/// starts. The host steps the engine out of it explicitly once setup
/// resolves.
/// No `next()` helper here either, for the same reason as `Step`: per CR
/// 505.1, `Main` occurs twice per turn (precombat and postcombat) with
/// different successors each time (precombat Main -> Combat, postcombat
/// Main -> Ending) — the same `Phase` value, two different "next"s depending
/// on where in the turn you are. That position isn't recoverable from
/// `Phase` alone, so turn advancement has to be the engine loop's job.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    PreGame,
    Beginning,
    Main,
    Combat,
    Ending,
}

/// CR 500-514. No `next()` helper here on purpose: step order isn't linear
/// independent of `Phase` (Main has no steps at all; which step follows
/// which depends on which phase you're in), so "next step" is a question
/// the engine loop has to answer with both `Phase` and `Step` in hand, not
/// something `Step` can answer about itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
