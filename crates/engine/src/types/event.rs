use crate::types::counter::CounterKind;
use crate::types::ids::{CommandId, EventId, ObjectId, PlayerId, SequenceNo, Target};
use crate::types::object::GameObject;
use crate::types::zone::ZoneKind;

/// Ledger granularity (DESIGN.md open question): logs every micro-step, not
/// just card-level moves, so the replay viewer can scrub at fine resolution
/// and the live action log can filter/group from a single underlying stream.
/// This variant list is a first pass — expect additions as Phase 5 dry runs
/// surface gaps.
#[derive(Debug)]
pub enum GameEventKind {
    /// `last_known_info` (CR 603.10): a snapshot of `object` exactly as it
    /// was in `from`, captured by whoever produces this event *before*
    /// mutating anything — `None` only when the producer is certain nothing
    /// downstream needs it (rare; default to capturing it). Without this,
    /// a leaves-the-battlefield trigger evaluated after `CommitEvents`/
    /// `SbaLoop` settle would have to look `object` up live in
    /// `GameState.objects` — but a battlefield -> graveyard move mints a
    /// *new* `ObjectId` (see `ObjectLineage`), so the old id may already be
    /// gone, or whatever still answers to it may have already lost
    /// characteristics (e.g. an aura-granted ability) that justified the
    /// trigger in the first place. Same shape of gap `StackObject.
    /// source_lki` (CR 608.2b) already solves on the stack side - this is
    /// the zone-change-event equivalent.
    ZoneChange { object: ObjectId, from: ZoneKind, to: ZoneKind, last_known_info: Option<GameObject> },
    LifeChanged { player: PlayerId, delta: i32 },
    DamageDealt { source: ObjectId, target: Target, amount: u32 },
    CounterAdded { object: ObjectId, kind: CounterKind, amount: i32 },
    /// First real tap producer (CR 602.5a's tap-cost activation, e.g. a
    /// mana ability) — nothing minted this event before now.
    Tapped { object: ObjectId },
    /// CR 502.3: one per permanent untapped as the untap step's turn-based
    /// action. Same shape as `Tapped` — first real producer on the untap
    /// side.
    Untapped { object: ObjectId },
    /// `units`: the exact literal mana added (CR 605), same "literal
    /// units, not a cost shape" vocabulary `CostPaid`/`CostPayment::Mana`
    /// already use — retargeted from a `ManaCost`-shaped `amount` (unused
    /// anywhere until this slice) for consistency.
    ManaAdded { player: PlayerId, units: Vec<crate::mana::ManaUnit> },
    /// CR 508.1m/508.2a: one per creature declared as an attacker. Exists
    /// so a future trigger-collection pass has something concrete to match
    /// "whenever X attacks"-style conditions against — `collect_triggers`
    /// itself stays `todo!()` for `Triggered`-kind abilities regardless
    /// (same script-bridge gap as everywhere else).
    AttackerDeclared { attacker: ObjectId, target: Target },
    /// CR 509.1i: one per blocker/attacker pairing declared.
    BlockerDeclared { blocker: ObjectId, attacker: ObjectId },
    /// CR 601.2i/118: `payer` committed a `PaymentPlan` covering
    /// `stack_id`'s cost - the ledger fact that completes the cast process
    /// `CastSpell`'s own events started (card moved to stack), distinct from
    /// those since payment is its own decision point (see command.rs's
    /// `SubmitPayment` doc).
    CostPaid { stack_id: ObjectId, payer: PlayerId },
    PrioritySet { player: PlayerId },
    PriorityPassed { player: PlayerId },
    StackObjectPushed { stack_id: ObjectId },
    StackObjectResolved { stack_id: ObjectId },
    /// CR 608.2b: every target of `stack_id`, for its one instance of the
    /// word "target," was illegal by the time it would resolve - it doesn't
    /// resolve at all. Emitted instead of (not alongside) `StackObjectResolved`
    /// - 608.2b is explicit the spell/ability "doesn't resolve." A fizzled
    /// spell still leaves the stack (see the accompanying `ZoneChange` to its
    /// owner's graveyard, pushed just before this); a fizzled ability just
    /// stops existing, no `ZoneChange` for it.
    Fizzled { stack_id: ObjectId },
    /// CR 601.6: `Engine::unwind_payment` abandoned `stack_id`'s cast/
    /// activation rather than paying its cost - the game "returns to the
    /// moment before [it] was proposed." Same "emitted instead of/alongside
    /// a `ZoneChange` depending on what's leaving the stack" shape `Fizzled`
    /// already establishes: a spell's `stack_id` gets an accompanying
    /// `ZoneChange` back to hand (pushed just before this), an activated
    /// ability's doesn't (it just stops existing, same as a fizzled
    /// ability). Not a true CR 733 state rewind (this engine's ledger is
    /// append-only, no undo primitive exists anywhere) - an honest forward
    /// zone-change/removal instead, flagged not silently guessed.
    CastingUnwound { stack_id: ObjectId },
    TriggerQueued { source: ObjectId, controller: PlayerId },
    ReplacementApplied { source: ObjectId, original_event: EventId },
    StateBasedActionApplied { description: String },
    TurnStarted { player: PlayerId, turn_number: u32 },
    PhaseStarted { phase: crate::turn::Phase },
    StepStarted { step: crate::turn::Step },
    PlayerEliminated { player: PlayerId },
    RngDrawn { purpose: String, value: u64 },
    /// CR 103.3: logged once per player whose library was shuffled during
    /// pregame setup. Useful for replay (marks where the randomness happened)
    /// and for any future "whenever a library is shuffled" triggers.
    LibraryShuffled { player: PlayerId },
    /// CR 701.22a/701.22d: `player` finished a `Game.Ops.Scry(n)` pause
    /// (`Engine::submit_scry_choice`) — `kept_on_top`/`put_bottom` are the
    /// two chosen piles, each in the player's chosen order. One event per
    /// scry (not per card), same granularity `LibraryShuffled` already uses
    /// — the reorder itself isn't a `ZoneChange` (nothing left the library).
    CardsScried { player: PlayerId, kept_on_top: Vec<ObjectId>, put_bottom: Vec<ObjectId> },
    /// CR 701.25a/701.25d: `player` finished a `Game.Ops.Surveil(n)` pause
    /// (`Engine::submit_surveil_choice`). `put_graveyard` holds the *new*
    /// `ObjectId`s minted for the real library -> graveyard move (CR 400.7);
    /// a separate `ZoneChange` per card is logged alongside this summary
    /// event, same "summary event plus the real per-object `ZoneChange`s"
    /// shape `submit_discard` already establishes.
    CardsSurveiled { player: PlayerId, kept_on_top: Vec<ObjectId>, put_graveyard: Vec<ObjectId> },
}

/// Immutable fact. Once committed, never mutated or removed.
#[derive(Debug)]
pub struct GameEvent {
    pub event_id: EventId,
    pub sequence_no: SequenceNo,
    pub kind: GameEventKind,
    pub caused_by_command: Option<CommandId>,
    pub parent_event: Option<EventId>,
}

impl GameEvent {
    pub fn new(event_id: EventId, sequence_no: SequenceNo, kind: GameEventKind) -> Self {
        // TODO: GameEvent { event_id, sequence_no, kind, caused_by_command: None,
        // parent_event: None } - caller sets the causal-ref fields after
        // construction if applicable, or this may need optional params added.
        todo!()
    }
}
