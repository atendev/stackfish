//! Phase 5 vertical slice: CR 303.4d's third clause - "An Aura can't
//! enchant more than one object or player. If a spell or ability would
//! cause an Aura to become attached to more than one object or player, the
//! Aura's controller chooses which object or player it becomes attached
//! to." The first two clauses (an Aura can't enchant itself; a creature-Aura
//! can't enchant anything) were already real (`phase5/aura-self-enchant`).
//! This one was left open since - it needed a "player picks one of several
//! candidates" mechanism, not another state-based action.
//!
//! `commands::propose_attach` is the pure decision function tested here:
//! zero candidates, one candidate (attach immediately, no choice), or two
//! or more (needs a choice). It's dormant as of this slice - no effect,
//! spell, or ability anywhere in this engine ever computes more than one
//! simultaneous attach candidate for one Aura, since both real attach call
//! sites (`ResolveStackTop`'s Aura-ETB-attach, `Equip`'s own resolution)
//! already narrow to a single legal target via `TargetRequirement { count: 1
//! }` before this would ever run. The rule is genuinely enforced *if* a
//! future effect ever produces multiple candidates, not because anything
//! does today.
//!
//! The other half of this slice - `ProcessState::AwaitingAttachChoice`/
//! `Engine::submit_attach_choice`, the actual "controller picks one" pause -
//! is tested inside `engine.rs`'s own `#[cfg(test)] mod tests`, not here:
//! `process_state` is a private field, so only an in-crate unit test can
//! construct the pause directly (the same "no real producer, drive it by
//! hand" precedent `AwaitingReplacementChoice`'s own unit tests already set
//! for a different pause).

use stackfish_engine::commands::{propose_attach, AttachProposal};
use stackfish_engine::ObjectId;

#[test]
fn zero_candidates_proposes_nothing() {
    assert_eq!(propose_attach(&[]), AttachProposal::None);
}

#[test]
fn one_candidate_proposes_an_immediate_attach() {
    let only = ObjectId(1);
    assert_eq!(propose_attach(&[only]), AttachProposal::Attach(only));
}

#[test]
fn two_or_more_candidates_needs_a_choice() {
    let a = ObjectId(1);
    let b = ObjectId(2);
    let c = ObjectId(3);
    assert_eq!(propose_attach(&[a, b]), AttachProposal::NeedsChoice(vec![a, b]));
    assert_eq!(propose_attach(&[a, b, c]), AttachProposal::NeedsChoice(vec![a, b, c]));
}
