//! The host-facing facade over the engine's pauses: `Decision`, `Answer` and the
//! mapping from `ProcessState` and engine errors to reply shapes and rejection
//! codes. See `DESIGN.md` for the rest.

mod answer;
mod check;
mod entry;
mod legal_actions;
mod open;

pub use entry::{
    ActivateEntry, AttackerEntry, BlockerEntry, CastEntry, ColorChoice, DamageSplitEntry, Entry, EntryTargets, ModeRange,
    TargetSlot,
};

use crate::replacement::ReplacementCandidate;
use crate::triggers::PendingTrigger;
use crate::types::command::PlayerAction;
use crate::types::cost::{CostSpec, PaymentPlan};
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::target::TargetSpec;

/// Identifies one open Decision. Starts at 1, goes up by one for each new
/// Decision, and is never reused.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct DecisionId(pub u64);

/// What a Pick Decision offers and what a Pick Answer names. Most picks are
/// objects or players, so those reuse the engine's `Target`. A CR 616.1
/// replacement choice picks between effects, which are not targets. `Target`
/// includes `Ability` (CR 113.9) because a trigger or script can legally target
/// an ability on the stack.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Candidate {
    Target(Target),
    Replacement(ReplacementCandidate),
}

/// Where the cards a Partition Decision does not keep on top go.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PartitionRest {
    /// Scry (CR 701.22): bottom of the library.
    LibraryBottom,
    /// Surveil (CR 701.25): graveyard.
    Graveyard,
}

/// The kind of reply a Decision asks for. An Answer's `Reply` variant must
/// match.
#[derive(Clone, Debug)]
pub enum ReplyShape {
    /// Choose between `min` and `max` of `candidates`. The order of the
    /// chosen list matters only for the mulligan bottom-card Decision, where
    /// it is the order the cards go on the bottom of the library.
    Pick { candidates: Vec<Candidate>, min: usize, max: usize },
    /// A yes or no. `true` means the optional thing happens: take the
    /// mulligan, return the commander, say yes to a script's `MayChoose`.
    YesNo,
    /// Put `items` in an order. The reply is a permutation of indices into
    /// `items`, because two pending triggers can be identical.
    Order { items: Vec<PendingTrigger> },
    /// Choose between `min` and `max` mode indices out of `mode_count`.
    /// `repeatable` allows the same index twice (CR 700.2d). `solo_only`
    /// lists the ascending indices that can only be chosen alone.
    Modes { mode_count: usize, min: u32, max: u32, repeatable: bool, solo_only: Vec<u32> },
    /// Split `cards` into a list kept on top, in order, and the rest, which
    /// go to `rest`.
    Partition { cards: Vec<ObjectId>, rest: PartitionRest },
    /// Pay `cost` with a `PaymentPlan`. A `None` reply means the player
    /// cannot pay (CR 601.6 unwinds the cast) or declines to pay an
    /// unless-paid cost (CR 118.12).
    Payment { cost: CostSpec },
    /// Choose targets for a trigger: one candidate list per target word of
    /// `spec`, in order. The reply is one list per word.
    Targets { spec: TargetSpec, candidates: Vec<Vec<Target>> },
    /// Cast `card` as the resolving ability allows (Madness CR 702.35a,
    /// Cascade CR 702.85a), or decline with `None`. Cascade with no
    /// qualifying card has `card: None` and only accepts a decline.
    MayCast { card: Option<ObjectId> },
    /// The priority decision. The reply is a `PlayerAction` that matches one
    /// of `entries`: pass, a land play, a cast or an activation.
    Action { entries: Vec<Entry> },
    /// The active player declares attackers (CR 508.1). The reply may be empty.
    DeclareAttackers { attackers: Vec<AttackerEntry> },
    /// A defending player declares blockers (CR 509.1). The reply may be empty.
    DeclareBlockers { blockers: Vec<BlockerEntry> },
    /// Split combat damage (CR 510.1c). The reply covers every entry.
    AssignDamage { splits: Vec<DamageSplitEntry> },
}

/// A point where the engine cannot continue until `player` chooses.
#[derive(Clone, Debug)]
pub struct Decision {
    pub id: DecisionId,
    pub player: PlayerId,
    /// Display only, such as "Devour" or "Legend rule". Hosts must not
    /// branch on it.
    pub mechanic: &'static str,
    pub shape: ReplyShape,
}

/// What a player announces to cast a card from a Madness or Cascade
/// Decision.
pub struct CastAnnouncement {
    pub modes: Vec<u32>,
    pub targets: Vec<Target>,
    pub x_value: Option<u32>,
}

/// One variant per `ReplyShape`.
pub enum Reply {
    Pick(Vec<Candidate>),
    YesNo(bool),
    Order(Vec<usize>),
    Modes(Vec<u32>),
    Partition { top: Vec<ObjectId>, rest: Vec<ObjectId> },
    Payment(Option<PaymentPlan>),
    Targets(Vec<Vec<Target>>),
    MayCast(Option<CastAnnouncement>),
    Action(PlayerAction),
    DeclareAttackers(Vec<(ObjectId, Target)>),
    DeclareBlockers(Vec<(ObjectId, ObjectId)>),
    AssignDamage(Vec<(ObjectId, Vec<(ObjectId, u32)>)>),
}

/// A player's reply to the open Decision, tagged with the Decision's id.
pub struct Answer {
    pub decision: DecisionId,
    pub reply: Reply,
}

/// Why `Engine::answer` refused an Answer. A rejected Answer changes
/// nothing: the Decision stays open and no events are emitted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RejectionCode {
    /// The Answer came from a player the Decision does not name.
    WrongSeat,
    /// No Decision is open.
    NotOpen,
    /// The reply names something illegal: not a candidate, a bad count, a
    /// duplicate, a plan that does not cover the cost.
    InvalidChoice,
    /// The Answer names a Decision id that is not the open one.
    StaleDecision,
    /// The reply variant does not match the open Decision's shape.
    WrongAnswerShape,
    /// The engine broke while carrying out the Answer. Unlike every other
    /// code this one gives no guarantee that nothing changed: the engine is
    /// fatal (`Engine::fatal_error`) and `step()` only reports the error.
    Internal,
}

impl RejectionCode {
    /// The snake_case name hosts see.
    pub fn as_str(self) -> &'static str {
        match self {
            RejectionCode::WrongSeat => "wrong_seat",
            RejectionCode::NotOpen => "not_open",
            RejectionCode::InvalidChoice => "invalid_choice",
            RejectionCode::StaleDecision => "stale_decision",
            RejectionCode::WrongAnswerShape => "wrong_answer_shape",
            RejectionCode::Internal => "internal",
        }
    }
}

/// A refused Answer: a stable `code` for hosts to branch on and a `message`
/// for people.
#[derive(Clone, Debug)]
pub struct Rejection {
    pub code: RejectionCode,
    pub message: String,
}

impl Rejection {
    pub(crate) fn new(code: RejectionCode, message: impl Into<String>) -> Self {
        Rejection { code, message: message.into() }
    }
}

/// Hands out Decision ids and remembers which one is open. The host `Session`
/// is the only caller, so replay from the same seed rebuilds the same ids.
/// Mixing direct `submit_*` calls with `answer` on one `Engine` leaves the
/// counter out of step.
#[derive(Debug)]
pub(crate) struct DecisionTracker {
    next: u64,
    open: Option<DecisionId>,
}

impl DecisionTracker {
    pub(crate) fn new() -> Self {
        DecisionTracker { next: 1, open: None }
    }

    /// The id of the open Decision, allocating a fresh one the first time
    /// it is asked for after the previous Decision was answered.
    pub(crate) fn open_id(&mut self) -> DecisionId {
        *self.open.get_or_insert_with(|| {
            let id = DecisionId(self.next);
            self.next += 1;
            id
        })
    }

    /// An accepted Answer closes the open Decision. The next pause gets a
    /// new id even if it is the same kind of pause.
    pub(crate) fn close(&mut self) {
        self.open = None;
    }
}
