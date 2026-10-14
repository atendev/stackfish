//! Who may see what (ticket 004). One rule decides it, and it is applied to
//! each ledger event and to each open Decision.
//!
//! A hand is visible to its owner and a library to nobody, so those two zones
//! are the hidden ones. Every other zone is public. A Seat that is omniscient
//! sees everything. The engine ids of hidden objects are never sent to a Seat
//! that cannot see them (ticket 005).
//!
//! TODO(snapshot): ticket 004 also wants a `(Seat, state)` view that equals the
//! fold of the events a Seat may see. No request asks for a state snapshot yet,
//! so only the event and Decision halves exist. A snapshot would reuse
//! `sees_object` and `sees_identity`.

use std::collections::HashMap;

use stackfish_engine::decision::{Candidate, Decision};
use stackfish_engine::{GameEvent, GameEventKind, GameObject, GameState, ObjectId, PlayerId, Target, ZoneKind};

use crate::wire::{
    U64, WireCardIdentity, WireDecision, WireEventKind, WireIdentity, WireObjectSummary, WireRevealedCard, WireZone,
    offered_objects,
};

/// One Seat's point of view.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Viewer {
    /// A Seat tied to a player. It is never omniscient.
    Player(PlayerId),
    /// A Seat tied to no player. The host sets `omniscient` when it creates it.
    Spectator { omniscient: bool },
}

impl Viewer {
    fn omniscient(self) -> bool {
        matches!(self, Viewer::Spectator { omniscient: true })
    }

    fn is(self, player: Option<PlayerId>) -> bool {
        matches!((self, player), (Viewer::Player(a), Some(b)) if a == b)
    }
}

/// A zone, and the player whose hand or library it is when that matters.
#[derive(Clone, Copy, Debug)]
pub struct Place {
    zone: ZoneKind,
    owner: Option<PlayerId>,
}

fn is_hidden(zone: ZoneKind) -> bool {
    matches!(zone, ZoneKind::Hand | ZoneKind::Library)
}

/// Whether the viewer can see that an object lies at `place`, and so its id.
fn sees_object(viewer: Viewer, place: Place) -> bool {
    viewer.omniscient()
        || !is_hidden(place.zone)
        || (place.zone == ZoneKind::Hand && viewer.is(place.owner))
}

/// Whether the viewer can also tell which card it is. Only the controller sees
/// a face-down object's identity.
fn sees_identity(viewer: Viewer, place: Place, face_down: bool, controller: Option<PlayerId>) -> bool {
    sees_object(viewer, place) && (!face_down || viewer.omniscient() || viewer.is(controller))
}

/// One side of a move: where the object was or went, and how it looked there.
#[derive(Clone, Copy, Debug)]
struct Side {
    place: Place,
    face_down: bool,
    controller: Option<PlayerId>,
}

/// What the Session learned about a `ZoneChange` while the new object still
/// existed. The engine event holds only the new id, so the old id, the owner
/// and the identity come from the state right after the step that made it.
#[derive(Debug)]
pub struct Move {
    object: ObjectId,
    previous: Option<ObjectId>,
    from: Side,
    to: Side,
    identity: Option<WireIdentity>,
    last_known_info: Option<WireObjectSummary>,
}

impl Move {
    fn new(
        state: &GameState,
        object: ObjectId,
        from: ZoneKind,
        to: ZoneKind,
        last_known_info: &Option<GameObject>,
    ) -> Move {
        let now = state.object(object);
        let lki = last_known_info.as_ref();
        let owner = lki.map(|o| o.owner).or(now.map(|o| o.owner));
        Move {
            object,
            previous: lki.map(|o| o.object_id).or(now.and_then(|o| o.lineage.previous)),
            from: Side {
                place: Place { zone: from, owner },
                face_down: lki.is_some_and(|o| o.face_down),
                controller: lki.map(|o| o.controller),
            },
            to: Side {
                place: Place { zone: to, owner },
                face_down: now.is_some_and(|o| o.face_down),
                controller: now.map(|o| o.controller),
            },
            identity: lki.or(now).map(WireIdentity::from),
            last_known_info: lki.map(WireObjectSummary::from),
        }
    }

    /// The rule of ticket 005. Each id is sent where the viewer could see the
    /// object, and the identity where the viewer could tell the card on either
    /// side. `owner` is always sent for a hidden move, so a Seat can count.
    fn kind_for(&self, viewer: Viewer) -> WireEventKind {
        let sees_old = sees_object(viewer, self.from.place);
        let sees_new = sees_object(viewer, self.to.place);
        let identity_old = sees_identity(viewer, self.from.place, self.from.face_down, self.from.controller);
        let identity_new = sees_identity(viewer, self.to.place, self.to.face_down, self.to.controller);
        let hidden_move = is_hidden(self.from.place.zone) || is_hidden(self.to.place.zone);
        WireEventKind::ZoneChange {
            object: sees_new.then_some(U64::from(self.object)),
            previous: self.previous.filter(|_| sees_old).map(U64::from),
            from: self.from.place.zone.into(),
            to: self.to.place.zone.into(),
            owner: self.from.place.owner.filter(|_| hidden_move).map(|p| p.0),
            identity: self.identity.clone().filter(|_| identity_old || identity_new),
            last_known_info: self.last_known_info.clone().filter(|_| identity_old),
        }
    }
}

/// `Cycled` and `MadnessExiled`: a discard whose card now lies in a public zone.
#[derive(Debug)]
pub struct Marker {
    cycled: bool,
    object: ObjectId,
    in_hand: Side,
    identity: WireIdentity,
    last_known_info: WireObjectSummary,
}

impl Marker {
    fn kind_for(&self, viewer: Viewer) -> WireEventKind {
        let from = self.in_hand;
        let sees = sees_identity(viewer, from.place, from.face_down, from.controller);
        let last_known_info = sees.then(|| self.last_known_info.clone());
        let (object, identity) = (self.object.into(), self.identity.clone());
        if self.cycled {
            WireEventKind::Cycled { object, identity, last_known_info }
        } else {
            WireEventKind::MadnessExiled { object, identity, last_known_info }
        }
    }
}

/// One ledger event, with the facts a viewer's copy depends on.
#[derive(Debug)]
pub enum Observed {
    /// Nothing in it depends on who looks. Its ids lie in public zones.
    Public(WireEventKind),
    /// Carries the id of a source that may sit in a hidden zone, such as a
    /// card in hand with a cycling trigger.
    Located { kind: WireEventKind, source: Option<Place> },
    /// Only the player it names, and omniscient Seats, receive it.
    Controller { player: PlayerId, kind: WireEventKind },
    /// Only omniscient Seats receive it.
    Omniscient(WireEventKind),
    Move(Move),
    Marker(Marker),
    Revealed { player: PlayerId, zone: ZoneKind, cards: Vec<(ObjectId, PlayerId, WireIdentity)> },
}

fn without_source(kind: &WireEventKind) -> WireEventKind {
    match kind.clone() {
        WireEventKind::TriggerQueued { controller, .. } => WireEventKind::TriggerQueued { source: None, controller },
        WireEventKind::ReplacementApplied { original_event, .. } => {
            WireEventKind::ReplacementApplied { source: None, original_event }
        }
        other => other,
    }
}

impl Observed {
    /// The event as `viewer` receives it, or `None` when it stays hidden.
    pub fn kind_for(&self, viewer: Viewer) -> Option<WireEventKind> {
        match self {
            Observed::Public(kind) => Some(kind.clone()),
            Observed::Located { kind, source } => match source {
                Some(place) if !sees_object(viewer, *place) => Some(without_source(kind)),
                _ => Some(kind.clone()),
            },
            Observed::Controller { player, kind } => {
                (viewer.omniscient() || viewer.is(Some(*player))).then(|| kind.clone())
            }
            Observed::Omniscient(kind) => viewer.omniscient().then(|| kind.clone()),
            Observed::Move(mv) => Some(mv.kind_for(viewer)),
            Observed::Marker(marker) => Some(marker.kind_for(viewer)),
            Observed::Revealed { player, zone, cards } => Some(WireEventKind::Revealed {
                player: player.0,
                zone: WireZone::from(*zone),
                // A reveal shows the card to everyone. Its id goes only to a
                // Seat that could already see it where it lies.
                cards: cards
                    .iter()
                    .map(|(id, owner, identity)| WireRevealedCard {
                        object: sees_object(viewer, Place { zone: *zone, owner: Some(*owner) })
                            .then_some(U64::from(*id)),
                        identity: identity.clone(),
                    })
                    .collect(),
            }),
        }
    }
}

/// Reads the ledger one step at a time. It remembers where objects sat in a
/// hidden zone after they left, because their old ids can still turn up in a
/// later event (a trigger whose source was the card in hand).
#[derive(Default)]
pub struct Observer {
    retired: HashMap<ObjectId, Place>,
}

impl Observer {
    /// Where `id` sits or sat, when that is a hidden zone. Public objects and
    /// ids the Observer never saw in a hidden zone give `None`.
    fn hidden_place(&self, state: &GameState, id: ObjectId) -> Option<Place> {
        match state.object(id) {
            Some(object) => Some(Place { zone: object.zone, owner: Some(object.owner) }),
            None => self.retired.get(&id).copied(),
        }
        .filter(|place| is_hidden(place.zone))
    }

    /// Reads `event`. Call it once per ledger entry, in order, against the
    /// state as it is right after the step that wrote the entry.
    pub fn observe(&mut self, state: &GameState, event: &GameEvent) -> Observed {
        use GameEventKind as K;
        let plain = || WireEventKind::plain(&event.kind).expect("every event not built in the match below has a plain form");
        match &event.kind {
            K::ZoneChange { object, from, to, last_known_info } => {
                let mv = Move::new(state, *object, *from, *to, last_known_info);
                if is_hidden(*from)
                    && let Some(previous) = mv.previous
                {
                    self.retired.insert(previous, mv.from.place);
                }
                Observed::Move(mv)
            }
            K::Cycled { object, last_known_info } | K::MadnessExiled { object, last_known_info } => {
                Observed::Marker(Marker {
                    cycled: matches!(event.kind, K::Cycled { .. }),
                    object: *object,
                    in_hand: Side {
                        place: Place { zone: last_known_info.zone, owner: Some(last_known_info.owner) },
                        face_down: last_known_info.face_down,
                        controller: Some(last_known_info.controller),
                    },
                    identity: last_known_info.into(),
                    last_known_info: last_known_info.into(),
                })
            }
            K::Revealed { player, zone, cards } => Observed::Revealed {
                player: *player,
                zone: *zone,
                cards: cards.iter().map(|c| (c.object, c.snapshot.owner, WireIdentity::from(&c.snapshot))).collect(),
            },
            // CR 701.22 and 701.25: what a player does with the cards it looked at.
            K::CardsScried { player, .. } | K::CardsSurveiled { player, .. } => {
                Observed::Controller { player: *player, kind: plain() }
            }
            // Ticket 004: replay only. Drawing from the seeded stream says nothing a live Seat needs.
            K::RngDrawn { .. } => Observed::Omniscient(plain()),
            K::TriggerQueued { source, .. } | K::ReplacementApplied { source, .. } => {
                Observed::Located { kind: plain(), source: self.hidden_place(state, *source) }
            }
            // The ids in these events lie on the battlefield, the stack, in a
            // graveyard or in exile when they happen. Free text in them never
            // names a hidden card (ticket 004).
            K::LifeChanged { .. }
            | K::DamageDealt { .. }
            | K::CounterAdded { .. }
            | K::Tapped { .. }
            | K::Untapped { .. }
            | K::ManaAdded { .. }
            | K::AttackerDeclared { .. }
            | K::BlockerDeclared { .. }
            | K::CostPaid { .. }
            | K::PrioritySet { .. }
            | K::PriorityPassed { .. }
            | K::StackObjectPushed { .. }
            | K::StackObjectResolved { .. }
            | K::Fizzled { .. }
            | K::Countered { .. }
            | K::CastingUnwound { .. }
            | K::StateBasedActionApplied { .. }
            | K::ScriptError { .. }
            | K::TurnStarted { .. }
            | K::PhaseStarted { .. }
            | K::StepStarted { .. }
            | K::PlayerEliminated { .. }
            | K::LibraryShuffled { .. }
            | K::BecameCopy { .. }
            | K::TextBoxesExchanged { .. }
            | K::Regenerated { .. }
            | K::TokenCeased { .. }
            | K::Attached { .. }
            | K::AttachedToPlayer { .. }
            | K::Unattached { .. }
            | K::UmbraArmorConsumed { .. }
            | K::SpellCast { .. }
            | K::BecameTarget { .. } => Observed::Public(plain()),
        }
    }
}

/// The open Decision as `viewer` sees it. The chooser sees every option, with
/// the identity of each offered card that lies in a hidden zone. Anyone else
/// sees the kind of reply and a count, plus the options in public zones.
pub fn decision_for(viewer: Viewer, state: &GameState, decision: &Decision) -> WireDecision {
    if viewer.omniscient() || viewer == Viewer::Player(decision.player) {
        let identities = offered_objects(&decision.shape)
            .into_iter()
            .filter_map(|id| {
                let object = state.object(id)?;
                let place = Place { zone: object.zone, owner: Some(object.owner) };
                // Events already told the viewer about its own hand and the public zones.
                let known = !is_hidden(place.zone) || (place.zone == ZoneKind::Hand && viewer.is(place.owner));
                (!known).then(|| WireCardIdentity { object: id.into(), identity: object.into() })
            })
            .collect();
        WireDecision::full(decision, identities)
    } else {
        WireDecision::opaque(decision, |candidate| match candidate {
            Candidate::Target(Target::Object(id)) => state.object(*id).is_none_or(|o| !is_hidden(o.zone)),
            _ => true,
        })
    }
}
