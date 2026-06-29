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
pub fn apply_replacements(state: &mut GameState, proposed: GameEventKind) -> GameEventKind {
    // TODO: 1. Find every ReplacementEffect in state.replacement_effects
    //          where ReplacementEffect::matches(&proposed) is true (not
    //          implementable yet - needs a ScriptRuntime, see effect.rs).
    //       2. If zero match, return proposed unchanged.
    //       3. If exactly one matches, apply its rewrite (needs an
    //          "apply"/operation-equivalent callback on ReplacementEffect,
    //          which doesn't exist yet - only event_pattern (the matcher)
    //          is modeled so far, not the rewrite itself).
    //       4. If more than one matches: CR 616.1 gap noted in the doc
    //          comment above - needs player choice, can't resolve here yet.
    //       5. After any rewrite, re-run matching against the new form in
    //          case it matches a different replacement (CR 616's
    //          stabilization requirement) - loop until no further match.
    //
    // `state` is `&mut` (was `&` in the Phase 2 draft): a replacement can
    // consume a one-shot shield (CR 614.13's "use up" effects, e.g. a
    // damage prevention shield with a counter budget) or otherwise mark
    // itself used (ConsumptionPolicy::LimitedUses) as a side effect of
    // applying it, which means mutating the matched ReplacementEffect's
    // state in `state.replacement_effects`, not just reading from it.
    todo!()
}
