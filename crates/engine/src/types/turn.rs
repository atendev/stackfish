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
/// independent of `Phase` (which step follows which depends on which phase
/// you're in), so "next step" is a question the engine loop has to answer
/// with both `Phase` and `Step` in hand, not something `Step` can answer
/// about itself.
///
/// `PreCombatMain`/`PostCombatMain` aren't real CR 500.4 "steps" (Main has
/// none) — added so `Step` itself disambiguates CR 505.1's "Main happens
/// twice per turn" instead of leaving `GameState.step` holding a stale
/// leftover value (whatever the prior real step was) while `Phase::Main` is
/// current. Previously resolved by a separate `GameState.postcombat_main:
/// bool` field; folded into `Step` instead so there's one place encoding
/// "where in the turn am I," not two fields that have to be kept in sync.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Untap,
    Upkeep,
    Draw,
    PreCombatMain,
    BeginningOfCombat,
    DeclareAttackers,
    DeclareBlockers,
    /// CR 510.1a/702.7: only inserted when at least one attacker or blocker
    /// has first strike or double strike - see
    /// `engine::Engine::advance_turn_step`'s `DeclareBlockers` arm. Skipped
    /// entirely otherwise, so combat with no first/double strike is exactly
    /// the one-step sequence it always was.
    FirstStrikeDamage,
    CombatDamage,
    EndOfCombat,
    PostCombatMain,
    End,
    Cleanup,
}
