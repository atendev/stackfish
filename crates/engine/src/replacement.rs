use crate::types::event::GameEventKind;
use crate::types::game::GameState;

/// Rewrites/prevents `proposed` per CR 614/615/616, applying every matching
/// `ReplacementEffect` in `state.replacement_effects` before the event is
/// allowed to commit, re-checking after each rewrite in case the rewritten
/// form matches a *different* replacement (CR 616.1's rewrite-stabilization
/// requirement).
///
/// Known gap, not solved by this signature yet: CR 616.1 has the *affected
/// player* choose the order when multiple replacements could apply to the
/// same event — that's a player decision, not something a pure
/// `&GameState -> GameEventKind` function can resolve on its own. Likely
/// needs to return a choice request in the ambiguous case rather than
/// always a resolved event; left as a single resolved-event return for this
/// first pass, expect this signature to change once that's worked out.
pub fn apply_replacements(state: &GameState, proposed: GameEventKind) -> GameEventKind {
    todo!()
}
