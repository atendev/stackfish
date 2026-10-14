//! The host-facing wrapper around one game. A Session takes request bytes in
//! and gives response bytes out. Hosts never touch the engine.

use serde::Deserialize;
use stackfish_engine::decision::{Answer, DecisionId, Rejection, RejectionCode};
use stackfish_engine::engine::{Engine, StepStatus};
use stackfish_engine::PlayerId;

use crate::card_source::CardSource;
use crate::envelope::{ErrorCode, PROTOCOL_VERSION, Request, Response};
use crate::setup::{self, SetupBody};
use crate::wire::{AnswerBody, WireDecision, WireEvent};

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct AdvanceBody {
    budget: Option<u64>,
}

/// How a run of engine steps ended.
enum Outcome {
    Decision,
    StillRunning,
    GameOver(Option<PlayerId>),
    Fatal(String),
}

enum Phase {
    Playing,
    Over,
    /// A pipeline error happened. Every later message is refused.
    Halted,
}

pub struct Session {
    engine: Engine,
    /// One Seat per player: Seat `i` answers for `PlayerId(i)`.
    seats: u32,
    step_cap: u64,
    /// Ledger entries already sent in a response.
    sent: usize,
    phase: Phase,
    // TODO(replay export): keep the setup bytes and the accepted Answers in
    // order, as ticket 001 describes. Not in the B3 ticket text.
    // TODO(B4): every Seat sees every event until per-Seat visibility exists.
    // `take_events` is the one place events leave the Session.
}

fn refuse_start(code: ErrorCode, message: impl Into<String>) -> Vec<u8> {
    Response::error(code, message).to_bytes()
}

/// Parses the envelope and checks the version, so every entry point reports
/// these two failures alike.
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
    }
}

impl Session {
    /// Builds a Session from a `setup` request. Nothing runs yet: the host's
    /// first `advance` produces the pregame events and the first Decision.
    /// `Err` holds the error response to send back.
    ///
    /// `cards` is read here and not kept.
    pub fn create(request: &[u8], cards: &dyn CardSource) -> Result<(Session, Vec<u8>), Vec<u8>> {
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
        let seats = built.config.players.len() as u32;
        let session = Session {
            engine: Engine::new_game(built.config),
            seats,
            step_cap: built.step_cap,
            sent: 0,
            phase: Phase::Playing,
        };
        let response = Response::created((0..seats).collect()).to_bytes();
        Ok((session, response))
    }

    /// Handles one `advance` or `answer` request.
    pub fn handle(&mut self, request: &[u8]) -> Vec<u8> {
        self.dispatch(request).to_bytes()
    }

    fn dispatch(&mut self, bytes: &[u8]) -> Response {
        if matches!(self.phase, Phase::Halted) {
            return Response::error(ErrorCode::PipelineError, "the Session stopped after a pipeline error");
        }
        let request = match parse_request(bytes) {
            Ok(request) => request,
            Err(response) => return self.refuse(response),
        };
        if let Some(seat) = request.seat
            && seat >= self.seats
        {
            return self.refuse_with(ErrorCode::UnknownSeat, format!("seat {seat} does not exist"));
        }
        match request.kind.as_str() {
            "advance" => self.advance(&request),
            "answer" => self.answer(&request),
            "setup" => self.refuse_with(ErrorCode::MalformedMessage, "setup only creates a Session"),
            other => self.refuse_with(ErrorCode::MalformedMessage, format!("unknown request kind {other:?}")),
        }
    }

    fn advance(&mut self, request: &Request) -> Response {
        let body: AdvanceBody = match parse_body(request) {
            Ok(body) => body,
            Err(response) => return self.refuse(response),
        };
        if body.budget == Some(0) {
            return self.refuse_with(ErrorCode::MalformedMessage, "budget must be at least 1");
        }
        if matches!(self.phase, Phase::Over) {
            return self.refuse_with(ErrorCode::GameOver, "the game is over");
        }
        if self.engine.open_decision().is_some() {
            return self.refuse_with(ErrorCode::DecisionPending, "a Decision is open, so send an answer");
        }
        let outcome = self.run(body.budget.unwrap_or(self.step_cap));
        self.finish(outcome)
    }

    fn answer(&mut self, request: &Request) -> Response {
        let body: AnswerBody = match parse_body(request) {
            Ok(body) => body,
            Err(response) => return self.refuse(response),
        };
        let Some(seat) = request.seat else {
            return self.refuse_with(ErrorCode::MalformedMessage, "an answer needs a seat");
        };
        if matches!(self.phase, Phase::Over) {
            return self.refuse_with(ErrorCode::GameOver, "the game is over");
        }
        let player = PlayerId(seat);
        let answer = Answer { decision: DecisionId(body.decision.0), reply: body.reply.into_reply(player) };
        if let Err(Rejection { code, message }) = self.engine.answer(player, answer) {
            return self.refuse_with(rejection_code(code), message);
        }
        let outcome = self.run(self.step_cap);
        self.finish(outcome)
    }

    /// Steps the engine until a Decision opens, the game ends, an error stops
    /// it or `budget` steps have run.
    fn run(&mut self, budget: u64) -> Outcome {
        for _ in 0..budget {
            match self.engine.step().status {
                StepStatus::Progressed => {}
                StepStatus::GameOver { winner } => return Outcome::GameOver(winner),
                StepStatus::Error(error) => return Outcome::Fatal(format!("{error:?}")),
                _ => return Outcome::Decision,
            }
        }
        Outcome::StillRunning
    }

    fn finish(&mut self, outcome: Outcome) -> Response {
        let events = self.take_events();
        match outcome {
            Outcome::Decision => match self.engine.open_decision() {
                Some(decision) => Response::decision_open(events, WireDecision::from(&decision)),
                None => self.halt(events, "the engine paused without opening a Decision".to_string()),
            },
            Outcome::StillRunning => Response::still_running(events),
            Outcome::GameOver(winner) => {
                self.phase = Phase::Over;
                Response::game_over(events, winner.map(|p| p.0))
            }
            Outcome::Fatal(message) => self.halt(events, message),
        }
    }

    fn halt(&mut self, events: Vec<WireEvent>, message: String) -> Response {
        self.phase = Phase::Halted;
        Response::fatal(events, message)
    }

    /// The ledger entries added since the last response.
    fn take_events(&mut self) -> Vec<WireEvent> {
        let ledger = &self.engine.state().ledger;
        let events = ledger[self.sent..].iter().map(WireEvent::from).collect();
        self.sent = ledger.len();
        events
    }

    /// Adds the open Decision, if any, to a refusal, so a host can retry.
    fn refuse(&mut self, response: Response) -> Response {
        let Some(error) = response.error else { return response };
        self.refuse_with(error.code, error.message)
    }

    fn refuse_with(&mut self, code: ErrorCode, message: impl Into<String>) -> Response {
        let open = match self.phase {
            Phase::Playing => self.engine.open_decision().map(|d| WireDecision::from(&d)),
            Phase::Over | Phase::Halted => None,
        };
        Response::refusal(code, message, open)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card_source::FixtureCardSource;
    use serde_json::{Value, json};
    use stackfish_engine::{CardTypeSet, ManaCost};
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

        let response = session.halt(Vec::new(), "boom".to_string());
        assert_eq!(response.error.as_ref().unwrap().code, ErrorCode::PipelineError);

        let advance = serde_json::to_vec(&json!({ "v": 1, "kind": "advance", "body": {} })).unwrap();
        let later: Value = serde_json::from_slice(&session.handle(&advance)).unwrap();
        assert_eq!(later["body"]["status"], "error");
        assert_eq!(later["body"]["error"]["code"], "pipeline_error");
    }
}
