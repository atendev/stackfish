//! CR 303.4d: an Aura's controller chooses which object it becomes attached to when an effect would attach it to several.
//! `commands::propose_attach` decides between zero, one (attach at once) and two or more candidates (a choice).
//! The `AwaitingAttachChoice` pause is tested in the engine's own unit tests, since `process_state` is private.
// TODO: no effect yet produces several attach candidates for one Aura, so `propose_attach` is never reached with more than one

use stackfish_engine::ObjectId;
use stackfish_engine::commands::{AttachProposal, propose_attach};

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
    assert_eq!(
        propose_attach(&[a, b]),
        AttachProposal::NeedsChoice(vec![a, b])
    );
    assert_eq!(
        propose_attach(&[a, b, c]),
        AttachProposal::NeedsChoice(vec![a, b, c])
    );
}
