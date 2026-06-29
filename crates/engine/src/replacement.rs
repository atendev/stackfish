use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId};

/// What `apply_replacements` produced for one proposed event. Not just a
/// `GameEventKind`: CR 616.1 has the *affected player* choose which
/// replacement applies first when more than one matches the same event —
/// a player decision, not something a pure transform can resolve on its
/// own — so this type makes that case a value the caller (`engine.rs`)
/// must handle rather than something `apply_replacements` could silently
/// resolve "for" the player or panic on.
pub enum ReplacementOutcome {
    /// Zero or exactly one `ReplacementEffect` matched (after however many
    /// rewrite/re-match passes CR 616's stabilization requirement needed) —
    /// this is the event's final form, ready for `CommitEvents`.
    Resolved(GameEventKind),
    /// More than one `ReplacementEffect` matches `event` and `affected_player`
    /// must choose which applies first (CR 616.1) before resolution can
    /// continue. `candidates` identifies the matching effects by their
    /// `ReplacementEffect::source` `ObjectId` (no dedicated id type exists
    /// yet). The engine must surface this as an actual pause — see
    /// `engine::ProcessState::AwaitingReplacementChoice` — not fold it back
    /// into a synchronous transform.
    NeedsOrderingChoice { affected_player: PlayerId, event: GameEventKind, candidates: Vec<ObjectId> },
}

/// Rewrites/prevents `proposed` per CR 614/615/616, applying every matching
/// `ReplacementEffect` in `state.replacement_effects` before the event is
/// allowed to commit, re-checking after each rewrite in case the rewritten
/// form matches a *different* replacement (CR 616.1's rewrite-stabilization
/// requirement).
pub fn apply_replacements(state: &mut GameState, proposed: GameEventKind) -> ReplacementOutcome {
    if state.replacement_effects.is_empty() {
        // Genuinely nothing registered to match against - safe to resolve
        // unchanged rather than hit the todo!() below, which only bites once
        // a ReplacementEffect actually exists.
        return ReplacementOutcome::Resolved(proposed);
    }
    // TODO: 1. Find every ReplacementEffect in state.replacement_effects
    //          where ReplacementEffect::matches(&proposed) is true (not
    //          implementable yet - needs a ScriptRuntime, see effect.rs).
    //       2. If zero match, return Resolved(proposed) unchanged.
    //       3. If exactly one matches, apply its rewrite (needs an
    //          "apply"/operation-equivalent callback on ReplacementEffect,
    //          which doesn't exist yet - only event_pattern (the matcher)
    //          is modeled so far, not the rewrite itself).
    //       4. If more than one matches: return NeedsOrderingChoice with
    //          the affected player (derived from the event - e.g. the
    //          target/controller it would hit; exact derivation per
    //          GameEventKind variant not worked out yet) and candidates =
    //          each matching effect's `source`. Do NOT guess an order.
    //       5. After any rewrite, re-run matching against the new form in
    //          case it matches a different replacement (CR 616's
    //          stabilization requirement) - loop until no further match or
    //          another NeedsOrderingChoice arises.
    //
    // `state` is `&mut` (was `&` in the Phase 2 draft): a replacement can
    // consume a one-shot shield (CR 614.13's "use up" effects, e.g. a
    // damage prevention shield with a counter budget) or otherwise mark
    // itself used (ConsumptionPolicy::LimitedUses) as a side effect of
    // applying it, which means mutating the matched ReplacementEffect's
    // state in `state.replacement_effects`, not just reading from it.
    todo!()
}
