use crate::types::counter::CounterKind;
use crate::types::ids::{CommandId, EventId, ObjectId, PlayerId, SequenceNo, Target};
use crate::types::object::GameObject;
use crate::types::zone::ZoneKind;

/// Ledger events, one per micro-step, so a replay viewer can scrub finely and
/// the action log can group from a single stream.
#[derive(Debug, Clone, PartialEq)]
pub enum GameEventKind {
    /// `last_known_info` (CR 603.10) is a snapshot of `object` as it was in
    /// `from`, taken before anything mutates. Leaves-the-battlefield triggers
    /// need it, because the move mints a new `ObjectId` and the old one may be
    /// gone. `None` only when no consumer needs it.
    ZoneChange { object: ObjectId, from: ZoneKind, to: ZoneKind, last_known_info: Option<GameObject> },
    LifeChanged { player: PlayerId, delta: i32 },
    /// `is_combat_damage` (CR 903.10a) is set only by `AssignCombatDamage`.
    /// Commander combat damage to a player counts toward the 21-damage loss,
    /// but ability damage from the same source does not. `apply_damage_event`
    /// is the only reader.
    DamageDealt { source: ObjectId, target: Target, amount: u32, is_combat_damage: bool },
    CounterAdded { object: ObjectId, kind: CounterKind, amount: i32 },
    /// Produced by a tap cost such as a mana ability (CR 602.5a).
    Tapped { object: ObjectId },
    /// CR 502.3: one per permanent untapped by the untap step.
    Untapped { object: ObjectId },
    /// `units` are the literal mana added (CR 605), the same vocabulary as
    /// `CostPayment::Mana`.
    ManaAdded { player: PlayerId, units: Vec<crate::mana::ManaUnit> },
    /// CR 508.1m, 508.2a: one per declared attacker, for "whenever X attacks"
    /// conditions.
    AttackerDeclared { attacker: ObjectId, target: Target },
    /// CR 509.1i: one per blocker/attacker pairing declared.
    BlockerDeclared { blocker: ObjectId, attacker: ObjectId },
    /// CR 601.2i, 118: `payer` committed a `PaymentPlan` for `stack_id`'s cost,
    /// completing the cast that `CastSpell` started. Payment is its own
    /// decision point (see `SubmitPayment`).
    CostPaid { stack_id: ObjectId, payer: PlayerId },
    PrioritySet { player: PlayerId },
    PriorityPassed { player: PlayerId },
    StackObjectPushed { stack_id: ObjectId },
    StackObjectResolved { stack_id: ObjectId },
    /// CR 608.2b: every target of `stack_id` for one "target" word was
    /// illegal, so it doesn't resolve. Emitted instead of
    /// `StackObjectResolved`. A fizzled spell's `ZoneChange` to the graveyard
    /// comes just before, and an ability just ceases to exist.
    Fizzled { stack_id: ObjectId },
    /// CR 701.6a: `AbilityEffect::Counter` removed `stack_id` without
    /// resolving it. Emitted instead of `StackObjectResolved`, with a
    /// preceding `ZoneChange` only for a spell. Distinct from `Fizzled`
    /// because cards care about the difference.
    /// TODO: no `ConditionPrimitive` matches this event, and "would be countered" replacements are not modeled
    Countered { stack_id: ObjectId },
    /// CR 601.6: `Engine::unwind_payment` abandoned `stack_id`'s cast or
    /// activation. A spell gets a preceding `ZoneChange` back to hand and an
    /// ability just ceases to exist. This is a forward removal, not a CR 733
    /// rewind, because the ledger is append-only.
    CastingUnwound { stack_id: ObjectId },
    TriggerQueued { source: ObjectId, controller: PlayerId },
    ReplacementApplied { source: ObjectId, original_event: EventId },
    StateBasedActionApplied { description: String },
    /// A Lua call failed mid-resolution and the engine used the no-effect or
    /// "condition false" default. A free-text diagnostic, not a gameplay fact.
    /// The pure query path in `layers.rs` cannot log it, since it holds no
    /// `&mut Engine`.
    ScriptError { message: String },
    TurnStarted { player: PlayerId, turn_number: u32 },
    PhaseStarted { phase: crate::turn::Phase },
    StepStarted { step: crate::turn::Step },
    PlayerEliminated { player: PlayerId },
    RngDrawn { purpose: String, value: u64 },
    /// CR 103.3: one per library shuffled during setup, marking where
    /// randomness happened for replay.
    LibraryShuffled { player: PlayerId },
    /// CR 701.22a, 701.22d: `player` finished a `Game.Ops.Scry(n)` pause
    /// (`Engine::submit_scry_choice`). One event per scry, each pile in the
    /// chosen order. The reorder is not a `ZoneChange`.
    CardsScried { player: PlayerId, kept_on_top: Vec<ObjectId>, put_bottom: Vec<ObjectId> },
    /// CR 701.25a, 701.25d: `player` finished a `Game.Ops.Surveil(n)` pause.
    /// `put_graveyard` holds the new ids from the library-to-graveyard moves
    /// (CR 400.7), each also logged as a `ZoneChange`.
    CardsSurveiled { player: PlayerId, kept_on_top: Vec<ObjectId>, put_graveyard: Vec<ObjectId> },
    /// CR 707.2, 613.1a: `object` became a copy of `copied`'s current copiable
    /// values (`AbilityEffect::BecomeCopy`). No trigger condition matches it.
    BecameCopy { object: ObjectId, copied: ObjectId },
    /// CR 612.5 (layer 3): `a` and `b` exchanged text boxes
    /// (`AbilityEffect::ExchangeTextBoxes`). A record of a mutation already
    /// made. No trigger condition matches it.
    TextBoxesExchanged { a: ObjectId, b: ObjectId },
    /// CR 614.8, 701.19: a regeneration shield replaced `object`'s destruction
    /// (CR 614.6), emitted by `ReplacementEffect::apply` in place of the
    /// `ZoneChange`. The permanent stays, so it keeps its `ObjectId`
    /// (CR 400.7). `sba::commit_regeneration` is the only producer, and it also
    /// removes damage, taps the permanent and removes it from combat
    /// (CR 701.19a, 701.19b).
    Regenerated { object: ObjectId },
    /// CR 704.5d: a token outside the battlefield ceased to exist. Not a
    /// `ZoneChange`, since there is nowhere to move it. Produced by
    /// `sba::collect_ceasing_tokens` and `commit_token_cessation`, which skip
    /// a token that died this same pass so its triggers fire first (CR 111.7).
    TokenCeased { object: ObjectId },
    /// CR 303.4b, 702.6a: `object`, an Aura or Equipment, became attached to
    /// `attached_to`, as an Aura entered or an Equip resolved. Not a
    /// `ZoneChange`. Logged beside `commands::attach_permanent`, which has no
    /// event pipeline of its own. No trigger condition matches it.
    Attached { object: ObjectId, attached_to: ObjectId },
    /// CR 303.4b: the player-attached twin of `Attached`, for an Aura
    /// enchanting a player (CR 301.5a, 301.6). Produced beside
    /// `commands::attach_permanent_to_player`.
    AttachedToPlayer { object: ObjectId, player: PlayerId },
    /// CR 704.5n: `object`, an Equipment or Fortification, became unattached
    /// from `was_attached_to`, which is no longer a legal host. It stays on the
    /// battlefield, unlike an illegal Aura (CR 704.5m). Not a `ZoneChange`.
    /// Produced by `sba::commit_unattach`.
    Unattached { object: ObjectId, was_attached_to: ObjectId },
    /// CR 702.89a: `aura`'s umbra armor replaced `enchanted`'s destruction
    /// (CR 614.6), emitted by `ReplacementEffect::apply` in place of the
    /// `ZoneChange`. `enchanted` stays and keeps its id. `aura` is destroyed
    /// separately, so `sba::commit_umbra_armor` returns several events.
    UmbraArmorConsumed { enchanted: ObjectId, aura: ObjectId },
    /// CR 702.29c, 702.29f: `object` was discarded to pay a Cycling or
    /// Typecycling `AdditionalCost::DiscardSelf`. Logged beside the ordinary
    /// discard `ZoneChange`. `last_known_info` (CR 603.10) is needed because
    /// the card's own cycling trigger has already left `state.abilities`.
    /// Produced by `Engine::continue_cost_zone_moves`.
    Cycled { object: ObjectId, last_known_info: GameObject },
    /// CR 702.35a: `object` was discarded and exiled by its madness redirect.
    /// Logged beside the discard `ZoneChange`. `last_known_info` (CR 603.10)
    /// is needed because the card's madness trigger has already left
    /// `state.abilities`. Produced by `replacement::madness_marker_event`.
    MadnessExiled { object: ObjectId, last_known_info: GameObject },
    /// CR 601.2i: `object` has been cast, with every step of CR 601.2a-h done.
    /// It has no `last_known_info` because the spell is still on the stack.
    /// Logged once per cast from the two commit points in
    /// `Engine::maybe_log_spell_cast`. A general "when you cast a spell" hook,
    /// used by Cascade (CR 702.85).
    SpellCast { object: ObjectId },
    /// CR 603.2, 702.21a: permanent `object` became the target of `by` (its
    /// `StackObject::stack_id`), controlled by `by_controller`. One event per
    /// distinct object per spell or ability (CR 603.2c). Logged once the cast
    /// completes (CR 601.2i), the cost is paid (CR 602.2) or the trigger goes
    /// on the stack (CR 603.3d), so an unwound cast never fires it.
    /// `by_controller` is carried because a `ConditionPrimitive` sees only the
    /// event, and Ward needs it. Only `Target::Object` produces it.
    BecameTarget { object: ObjectId, by: ObjectId, by_controller: PlayerId },
    /// CR 701.20a: `player` revealed `cards` from `zone`. The cards stay where
    /// they are, so this is not a `ZoneChange`. Each card carries a snapshot,
    /// because a host decides per Seat whether it may also see the card's id
    /// (a card revealed from a hand or library is still hidden afterwards).
    /// TODO: no effect produces this yet. Reveal effects such as Wheel of Sun
    /// and Moon's replacement are unmodeled, so only tests build it.
    Revealed { player: PlayerId, zone: ZoneKind, cards: Vec<RevealedCard> },
}

/// One card of a `GameEventKind::Revealed`: its id and what it looked like.
#[derive(Debug, Clone, PartialEq)]
pub struct RevealedCard {
    pub object: ObjectId,
    pub snapshot: GameObject,
}

/// Immutable fact. Once committed, never mutated or removed.
#[derive(Debug, Clone, PartialEq)]
pub struct GameEvent {
    pub event_id: EventId,
    pub sequence_no: SequenceNo,
    pub kind: GameEventKind,
    pub caused_by_command: Option<CommandId>,
    pub parent_event: Option<EventId>,
}

impl GameEvent {
    /// Set `caused_by_command` and `parent_event` afterwards with struct
    /// update syntax.
    pub fn new(event_id: EventId, sequence_no: SequenceNo, kind: GameEventKind) -> Self {
        GameEvent { event_id, sequence_no, kind, caused_by_command: None, parent_event: None }
    }
}
