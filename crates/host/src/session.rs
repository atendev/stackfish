//! The host-facing wrapper around one game: request bytes in, response bytes out. Each
//! Seat has an outbox of events filtered for its view. See `docs/DESIGN.md`.

use std::any::Any;
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};

use serde::Deserialize;
use stackfish_engine::decision::{Answer, DecisionId, Rejection, RejectionCode};
use stackfish_engine::engine::{Engine, EngineSnapshot, StepStatus};
use stackfish_engine::PlayerId;

use crate::card_source::CardSource;
use crate::envelope::{ErrorCode, PROTOCOL_VERSION, Request, Response};
use crate::setup::{self, SetupBody};
use crate::view::{self, Observer, Viewer};
use crate::wire::{AnswerBody, WireDecision, WireEvent};

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct AdvanceBody {
    budget: Option<u64>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct PollBody {}

/// How a run of engine steps ended.
enum Outcome {
    Decision,
    StillRunning,
    GameOver(Option<PlayerId>),
    /// The engine returned an `Internal` error. Its text is in `Session::internal`.
    Fatal,
}

enum Phase {
    Playing,
    Over(Option<PlayerId>),
    /// The engine returned an `Internal` error. Every later message but `poll` is refused.
    Halted,
    /// The engine panicked. Every later message, `poll` included, is refused, and the
    /// engine is never touched again.
    Dead,
}

// The detail can name a hidden card, so Seats get this text and the host reads
// `Session::internal_error`.
const INTERNAL_MESSAGE: &str = "the engine hit an internal error and the game cannot continue";

/// One point of view on the game and the events it has not received yet.
struct Seat {
    viewer: Viewer,
    outbox: Vec<WireEvent>,
}

pub struct Session {
    engine: Engine,
    /// Player Seats come first: Seat `i` answers for `PlayerId(i)`. Spectator
    /// Seats follow.
    seats: Vec<Seat>,
    players: u32,
    step_cap: u64,
    /// Ledger entries already turned into outbox events.
    observed: usize,
    observer: Observer,
    phase: Phase,
    /// Why the Session halted or died, for the host's log and never for a Seat.
    internal: Option<String>,
    // TODO: keep the setup bytes and the accepted Answers in order, for replay export
    // TODO: bound a Seat's outbox; one that never sends a request grows without limit
}

fn refuse_start(code: ErrorCode, message: impl Into<String>) -> Vec<u8> {
    Response::error(code, message).to_bytes()
}

#[allow(clippy::result_large_err)]
fn parse_request(bytes: &[u8]) -> Result<Request, Response> {
    let request: Request = serde_json::from_slice(bytes)
        .map_err(|e| Response::error(ErrorCode::MalformedMessage, format!("not a valid request: {e}")))?;
    if request.v != PROTOCOL_VERSION {
        return Err(Response::error(
            ErrorCode::UnsupportedVersion,
            format!("protocol version {} is not supported, this Session speaks {PROTOCOL_VERSION}", request.v),
        ));
    }
    Ok(request)
}

#[allow(clippy::result_large_err)]
fn parse_body<T: for<'de> Deserialize<'de>>(request: &Request) -> Result<T, Response> {
    serde_json::from_value(request.body.clone()).map_err(|e| {
        Response::error(ErrorCode::MalformedMessage, format!("bad {} body: {e}", request.kind))
    })
}

fn rejection_code(code: RejectionCode) -> ErrorCode {
    match code {
        RejectionCode::WrongSeat => ErrorCode::NotYourDecision,
        RejectionCode::NotOpen => ErrorCode::NoOpenDecision,
        RejectionCode::StaleDecision => ErrorCode::StaleDecision,
        RejectionCode::InvalidChoice | RejectionCode::WrongAnswerShape => ErrorCode::InvalidAnswer,
        RejectionCode::Internal => ErrorCode::PipelineError,
    }
}

fn panic_text(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no text".to_string())
}

impl Session {
    /// Builds a Session from a `setup` request. Nothing runs until the first `advance`.
    /// `Err` holds the error response to send back. `cards` is read here and not kept.
    pub fn create(request: &[u8], cards: &dyn CardSource) -> Result<(Session, Vec<u8>), Vec<u8>> {
        // A script that panics during setup must give an error response, as it would in play.
        match catch_unwind(AssertUnwindSafe(|| Self::build_session(request, cards))) {
            Ok(result) => result,
            Err(_) => Err(Response::error(ErrorCode::EnginePanic, INTERNAL_MESSAGE).to_bytes()),
        }
    }

    fn build_session(request: &[u8], cards: &dyn CardSource) -> Result<(Session, Vec<u8>), Vec<u8>> {
        let request = parse_request(request).map_err(|r| r.to_bytes())?;
        if request.kind != "setup" {
            return Err(refuse_start(
                ErrorCode::MalformedMessage,
                format!("expected a setup request to create the Session, got {:?}", request.kind),
            ));
        }
        if request.seat.is_some() {
            return Err(refuse_start(ErrorCode::MalformedMessage, "setup comes from the host, so it has no seat"));
        }
        let body: SetupBody = parse_body(&request).map_err(|r| r.to_bytes())?;
        let built = setup::build(&body, cards).map_err(|e| refuse_start(ErrorCode::SetupInvalid, e.0))?;
        let players = built.config.players.len() as u32;
        let seats: Vec<Seat> = (0..players)
            .map(|i| Viewer::Player(PlayerId(i)))
            .chain(body.spectators.iter().map(|s| Viewer::Spectator { omniscient: s.omniscient }))
            .map(|viewer| Seat { viewer, outbox: Vec::new() })
            .collect();
        let ids = (0..seats.len() as u32).collect();
        let mut session = Session {
            engine: Engine::new_game(built.config),
            seats,
            players,
            step_cap: built.step_cap,
            observed: 0,
            observer: Observer::default(),
            phase: Phase::Playing,
            internal: None,
        };
        session.observe();
        Ok((session, Response::created(ids).to_bytes()))
    }

    /// The engine state a rejected request must leave unchanged. Only tests need it.
    pub fn snapshot(&self) -> EngineSnapshot {
        self.engine.snapshot()
    }

    /// Handles one `advance`, `answer` or `poll` request.
    ///
    /// An engine panic is caught here. The Session marks itself dead and answers this
    /// request and every later one with `engine_panic`.
    pub fn handle(&mut self, request: &[u8]) -> Vec<u8> {
        if matches!(self.phase, Phase::Dead) {
            return Response::error(ErrorCode::EnginePanic, INTERNAL_MESSAGE).to_bytes();
        }
        self.guarded(|session| session.dispatch(request))
    }

    fn guarded(&mut self, f: impl FnOnce(&mut Session) -> Response) -> Vec<u8> {
        match catch_unwind(AssertUnwindSafe(|| f(self).to_bytes())) {
            Ok(bytes) => bytes,
            Err(payload) => {
                self.phase = Phase::Dead;
                self.internal = Some(panic_text(payload.as_ref()));
                Response::error(ErrorCode::EnginePanic, INTERNAL_MESSAGE).to_bytes()
            }
        }
    }

    /// Why the Session stopped, if it did. For the host's log: it can name a hidden card,
    /// so never pass it to a player.
    pub fn internal_error(&self) -> Option<&str> {
        self.internal.as_deref()
    }

    fn dispatch(&mut self, bytes: &[u8]) -> Response {
        let request = match parse_request(bytes) {
            Ok(request) => request,
            Err(response) => return self.refuse(None, response),
        };
        if let Some(seat) = request.seat
            && seat as usize >= self.seats.len()
        {
            return self.refuse_with(None, ErrorCode::UnknownSeat, format!("seat {seat} does not exist"));
        }
        // A halted Session still lets a Seat collect its last events.
        if request.kind == "poll" {
            return self.poll(&request);
        }
        if matches!(self.phase, Phase::Halted) {
            return Response::error(ErrorCode::PipelineError, "the Session stopped after a pipeline error");
        }
        match request.kind.as_str() {
            "advance" => self.advance(&request),
            "answer" => self.answer(&request),
            "setup" => self.refuse_with(request.seat, ErrorCode::MalformedMessage, "setup only creates a Session"),
            other => {
                self.refuse_with(request.seat, ErrorCode::MalformedMessage, format!("unknown request kind {other:?}"))
            }
        }
    }

    // Without a `seat` the response carries no events, so a host that drives the Session
    // collects them with `poll`.
    fn advance(&mut self, request: &Request) -> Response {
        let seat = request.seat;
        let body: AdvanceBody = match parse_body(request) {
            Ok(body) => body,
            Err(response) => return self.refuse(seat, response),
        };
        if body.budget == Some(0) {
            return self.refuse_with(seat, ErrorCode::MalformedMessage, "budget must be at least 1");
        }
        if matches!(self.phase, Phase::Over(_)) {
            return self.refuse_with(seat, ErrorCode::GameOver, "the game is over");
        }
        if self.engine.open_decision().is_some() {
            return self.refuse_with(seat, ErrorCode::DecisionPending, "a Decision is open, so send an answer");
        }
        let outcome = self.run(body.budget.unwrap_or(self.step_cap));
        self.finish(seat, outcome)
    }

    fn answer(&mut self, request: &Request) -> Response {
        let body: AnswerBody = match parse_body(request) {
            Ok(body) => body,
            Err(response) => return self.refuse(request.seat, response),
        };
        let Some(seat) = request.seat else {
            return self.refuse_with(None, ErrorCode::MalformedMessage, "an answer needs a seat");
        };
        if matches!(self.phase, Phase::Over(_)) {
            return self.refuse_with(Some(seat), ErrorCode::GameOver, "the game is over");
        }
        if seat >= self.players {
            return self.refuse_with(Some(seat), ErrorCode::NotYourDecision, "a spectator Seat cannot answer");
        }
        let player = PlayerId(seat);
        let answer = Answer { decision: DecisionId(body.decision.0), reply: body.reply.into_reply(player) };
        if let Err(Rejection { code, message }) = self.engine.answer(player, answer) {
            // An `Internal` rejection means the engine broke, which ends the game.
            if code == RejectionCode::Internal {
                self.observe();
                let events = self.take_events(Some(seat));
                return self.halt(events);
            }
            return self.refuse_with(Some(seat), rejection_code(code), message);
        }
        self.observe();
        let outcome = self.run(self.step_cap);
        self.finish(Some(seat), outcome)
    }

    fn poll(&mut self, request: &Request) -> Response {
        let _: PollBody = match parse_body(request) {
            Ok(body) => body,
            Err(response) => return self.refuse(request.seat, response),
        };
        let Some(seat) = request.seat else {
            return self.refuse_with(None, ErrorCode::MalformedMessage, "a poll needs a seat");
        };
        let events = self.take_events(Some(seat));
        if let Phase::Over(winner) = &self.phase {
            return Response::game_over(events, winner.map(|p| p.0));
        }
        if matches!(self.phase, Phase::Halted) {
            return Response::fatal(events, INTERNAL_MESSAGE);
        }
        match self.open_decision_for(Some(seat)) {
            Some(decision) => Response::decision_open(events, decision),
            None => Response::still_running(events),
        }
    }

    // Each step is observed before the next one, while the objects it moved still exist.
    fn run(&mut self, budget: u64) -> Outcome {
        for _ in 0..budget {
            let status = self.engine.step().status;
            self.observe();
            match status {
                StepStatus::Progressed => {}
                StepStatus::GameOver { winner } => return Outcome::GameOver(winner),
                StepStatus::Error(internal) => {
                    self.internal = Some(internal.0);
                    return Outcome::Fatal;
                }
                _ => return Outcome::Decision,
            }
        }
        Outcome::StillRunning
    }

    fn finish(&mut self, seat: Option<u32>, outcome: Outcome) -> Response {
        let events = self.take_events(seat);
        match outcome {
            Outcome::Decision => match self.open_decision_for(seat) {
                Some(decision) => Response::decision_open(events, decision),
                None => {
                    self.internal = Some("the engine paused without opening a Decision".to_string());
                    self.halt(events)
                }
            },
            Outcome::StillRunning => Response::still_running(events),
            Outcome::GameOver(winner) => {
                self.phase = Phase::Over(winner);
                Response::game_over(events, winner.map(|p| p.0))
            }
            Outcome::Fatal => self.halt(events),
        }
    }

    fn halt(&mut self, events: Vec<WireEvent>) -> Response {
        self.phase = Phase::Halted;
        Response::fatal(events, INTERNAL_MESSAGE)
    }

    fn observe(&mut self) {
        let state = self.engine.state();
        for event in &state.ledger[self.observed..] {
            let observed = self.observer.observe(state, event);
            for seat in &mut self.seats {
                if let Some(kind) = observed.kind_for(seat.viewer) {
                    seat.outbox.push(WireEvent::with_kind(event, kind));
                }
            }
        }
        self.observed = state.ledger.len();
    }

    fn take_events(&mut self, seat: Option<u32>) -> Vec<WireEvent> {
        seat.map(|s| mem::take(&mut self.seats[s as usize].outbox)).unwrap_or_default()
    }

    // Without a seat the Decision is shown as a non-choosing spectator sees it.
    fn open_decision_for(&mut self, seat: Option<u32>) -> Option<WireDecision> {
        let viewer = seat.map_or(Viewer::Spectator { omniscient: false }, |s| self.seats[s as usize].viewer);
        let decision = self.engine.open_decision()?;
        Some(view::decision_for(viewer, self.engine.state(), &decision))
    }

    fn refuse(&mut self, seat: Option<u32>, response: Response) -> Response {
        let Some(error) = response.error else { return response };
        self.refuse_with(seat, error.code, error.message)
    }

    fn refuse_with(&mut self, seat: Option<u32>, code: ErrorCode, message: impl Into<String>) -> Response {
        let open = match self.phase {
            Phase::Playing => self.open_decision_for(seat),
            Phase::Over(_) | Phase::Halted | Phase::Dead => None,
        };
        Response::refusal(code, message, open)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card_source::FixtureCardSource;
    use serde_json::{Value, json};
    use stackfish_engine::{
        CardTypeSet, EventId, GameEvent, GameEventKind, ManaCost, ObjectId, RevealedCard, SequenceNo, ZoneKind,
    };
    use stackfish_testutil::minimal_card_definition;

    #[test]
    fn a_pipeline_error_is_fatal_and_every_later_message_is_refused() {
        let card = minimal_card_definition(1, "A", "a", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE);
        let cards = FixtureCardSource::new().with("a", card);
        let setup = json!({
            "v": 1, "kind": "setup",
            "body": { "players": [{ "deck": [{ "card": "a", "count": 3 }] }, { "deck": [{ "card": "a", "count": 3 }] }],
                      "format": "standard", "seed": "1" },
        });
        let (mut session, _) = Session::create(&serde_json::to_vec(&setup).unwrap(), &cards).unwrap();

        session.engine.poison(stackfish_engine::commands::Internal::new("boom names the card Black Lotus"));
        let advance = serde_json::to_vec(&json!({ "v": 1, "kind": "advance", "seat": 0, "body": {} })).unwrap();
        let first: Value = serde_json::from_slice(&session.handle(&advance)).unwrap();
        assert_eq!(first["body"]["status"], "error");
        assert_eq!(first["body"]["error"]["code"], "pipeline_error");
        // A Seat must get only the generic text.
        assert!(!first.to_string().contains("Black Lotus"), "{first}");
        assert_eq!(session.internal_error(), Some("boom names the card Black Lotus"));

        let later: Value = serde_json::from_slice(&session.handle(&advance)).unwrap();
        assert_eq!(later["body"]["error"]["code"], "pipeline_error");
        assert!(!later.to_string().contains("Black Lotus"), "{later}");
    }

    #[test]
    fn a_panic_in_the_engine_kills_the_session_with_a_stable_code_and_no_detail() {
        let mut session = started();
        let poll = serde_json::to_vec(&json!({ "v": 1, "kind": "poll", "seat": 0, "body": {} })).unwrap();
        let advance = serde_json::to_vec(&json!({ "v": 1, "kind": "advance", "body": {} })).unwrap();

        // The engine is hard to break from outside, so a closure stands in for a panic in a step.
        let first: Value = serde_json::from_slice(&session.guarded(|_| panic!("seat 1 holds Black Lotus"))).unwrap();
        assert_eq!(first["body"]["status"], "error");
        assert_eq!(first["body"]["error"]["code"], "engine_panic");
        assert_eq!(session.internal_error(), Some("seat 1 holds Black Lotus"), "the text is kept for the host's log");
        assert!(!first.to_string().contains("Black Lotus"), "a Seat never sees the panic text: {first}");

        // A dead Session refuses even a poll.
        for request in [&advance, &poll] {
            let later: Value = serde_json::from_slice(&session.handle(request)).unwrap();
            assert_eq!(later["body"]["error"]["code"], "engine_panic");
        }
    }

    #[test]
    fn a_panic_while_building_the_game_is_an_error_response() {
        struct Panicking;
        impl CardSource for Panicking {
            fn load(&self, _key: &str) -> Result<stackfish_engine::CardDefinition, crate::card_source::CardSourceError> {
                panic!("the card source broke");
            }
        }
        let setup = json!({
            "v": 1, "kind": "setup",
            "body": { "players": [{ "deck": [{ "card": "a", "count": 3 }] }, { "deck": [{ "card": "a", "count": 3 }] }],
                      "format": "standard", "seed": "1" },
        });
        let refused = Session::create(&serde_json::to_vec(&setup).unwrap(), &Panicking).err().expect("create fails");
        let refused: Value = serde_json::from_slice(&refused).unwrap();
        assert_eq!(refused["body"]["error"]["code"], "engine_panic");
    }
    fn started() -> Session {
        let card = minimal_card_definition(1, "A", "a", ManaCost { symbols: Vec::new() }, CardTypeSet::CREATURE);
        let cards = FixtureCardSource::new().with("a", card);
        let setup = json!({
            "v": 1, "kind": "setup",
            "body": { "players": [{ "deck": [{ "card": "a", "count": 20 }] }, { "deck": [{ "card": "a", "count": 20 }] }],
                      "format": "standard", "seed": "1", "overrides": { "skip_mulligan": true } },
        });
        let (mut session, _) = Session::create(&serde_json::to_vec(&setup).unwrap(), &cards).unwrap();
        let advance = serde_json::to_vec(&json!({ "v": 1, "kind": "advance", "body": {} })).unwrap();
        session.handle(&advance);
        session
    }

    const VIEWERS: [Viewer; 4] = [
        Viewer::Player(PlayerId(0)),
        Viewer::Player(PlayerId(1)),
        Viewer::Spectator { omniscient: false },
        Viewer::Spectator { omniscient: true },
    ];

    fn a_card_in_player_zero_s_hand(session: &Session) -> stackfish_engine::GameObject {
        let state = session.engine.state();
        let id = *state.zone(ZoneKind::Hand, Some(PlayerId(0))).unwrap().contents.front().unwrap();
        state.object(id).unwrap().clone()
    }

    fn kind_json(observed: &view::Observed, viewer: Viewer) -> Option<Value> {
        observed.kind_for(viewer).map(|kind| serde_json::to_value(kind).unwrap())
    }

    fn observe(session: &mut Session, kind: GameEventKind) -> view::Observed {
        let event = GameEvent::new(EventId(9000), SequenceNo(9000), kind);
        session.observer.observe(session.engine.state(), &event)
    }

    #[test]
    fn a_revealed_card_shows_its_identity_to_everyone_and_its_id_only_where_the_card_is_visible() {
        let mut session = started();
        let card = a_card_in_player_zero_s_hand(&session);
        let revealed = RevealedCard { object: card.object_id, snapshot: card.clone() };
        let observed = observe(
            &mut session,
            GameEventKind::Revealed { player: PlayerId(0), zone: ZoneKind::Hand, cards: vec![revealed] },
        );
        for (viewer, sees_id) in VIEWERS.into_iter().zip([true, false, false, true]) {
            let event = kind_json(&observed, viewer).expect("a reveal reaches every Seat");
            assert_eq!(event["cards"][0]["identity"]["name"], "A", "{viewer:?}");
            assert_eq!(event["cards"][0].get("object").is_some(), sees_id, "{viewer:?}");
        }
    }

    #[test]
    fn scry_and_surveil_results_go_to_the_controller_and_rng_draws_to_omniscient_seats() {
        let mut session = started();
        let scried = observe(
            &mut session,
            GameEventKind::CardsScried { player: PlayerId(0), kept_on_top: Vec::new(), put_bottom: Vec::new() },
        );
        let surveiled = observe(
            &mut session,
            GameEventKind::CardsSurveiled { player: PlayerId(0), kept_on_top: Vec::new(), put_graveyard: Vec::new() },
        );
        let rng = observe(&mut session, GameEventKind::RngDrawn { purpose: "test".to_string(), value: 5 });
        // Indexes follow VIEWERS.
        let gets_scry = [true, false, false, true];
        let gets_rng = [false, false, false, true];
        for (i, viewer) in VIEWERS.into_iter().enumerate() {
            assert_eq!(scried.kind_for(viewer).is_some(), gets_scry[i], "scry, {viewer:?}");
            assert_eq!(surveiled.kind_for(viewer).is_some(), gets_scry[i], "surveil, {viewer:?}");
            assert_eq!(rng.kind_for(viewer).is_some(), gets_rng[i], "rng, {viewer:?}");
        }
    }

    #[test]
    fn a_cycling_marker_hides_the_hand_snapshot_from_the_opponent_but_not_the_card() {
        let mut session = started();
        let card = a_card_in_player_zero_s_hand(&session);
        let observed = observe(
            &mut session,
            GameEventKind::Cycled { object: ObjectId(9001), last_known_info: card },
        );
        let owner = kind_json(&observed, VIEWERS[0]).unwrap();
        let opponent = kind_json(&observed, VIEWERS[1]).unwrap();
        assert!(owner.get("last_known_info").is_some());
        assert!(opponent.get("last_known_info").is_none(), "the snapshot names the old hand id");
        assert_eq!(opponent["identity"]["name"], "A", "the card now lies in the graveyard");
        assert_eq!(opponent["object"], "9001");
    }

    // CR 111.7
    // The token's hand id was never sent to the opponent, so the cease event must not leak it.
    #[test]
    fn a_token_that_ceases_in_a_hand_is_reported_only_to_its_owner() {
        let mut session = started();
        let card = a_card_in_player_zero_s_hand(&session);
        let token = stackfish_engine::GameObject { is_token: true, zone: ZoneKind::Battlefield, ..card.clone() };
        observe(
            &mut session,
            GameEventKind::ZoneChange {
                object: card.object_id,
                from: ZoneKind::Battlefield,
                to: ZoneKind::Hand,
                last_known_info: Some(token),
            },
        );
        // The token is already gone from the state when the event is read.
        let state = session.engine.state_mut();
        state.zone_mut(ZoneKind::Hand, Some(PlayerId(0))).unwrap().remove(card.object_id);
        state.objects.remove(&card.object_id);

        let ceased = observe(&mut session, GameEventKind::TokenCeased { object: card.object_id });
        for (viewer, receives) in VIEWERS.into_iter().zip([true, false, false, true]) {
            assert_eq!(ceased.kind_for(viewer).is_some(), receives, "{viewer:?}");
        }
    }

    #[test]
    fn a_token_that_ceases_in_a_public_zone_is_reported_to_everyone() {
        let mut session = started();
        let ceased = observe(&mut session, GameEventKind::TokenCeased { object: ObjectId(9002) });
        for viewer in VIEWERS {
            assert!(ceased.kind_for(viewer).is_some(), "{viewer:?}");
        }
    }

    #[test]
    fn a_trigger_whose_source_sits_in_a_hand_hides_the_source_from_the_opponent() {
        let mut session = started();
        let card = a_card_in_player_zero_s_hand(&session);
        let observed = observe(
            &mut session,
            GameEventKind::TriggerQueued { source: card.object_id, controller: PlayerId(0) },
        );
        assert!(kind_json(&observed, VIEWERS[0]).unwrap().get("source").is_some());
        assert!(kind_json(&observed, VIEWERS[1]).unwrap().get("source").is_none());
        assert!(kind_json(&observed, VIEWERS[2]).unwrap().get("source").is_none());
        assert!(kind_json(&observed, VIEWERS[3]).unwrap().get("source").is_some());
    }
}
