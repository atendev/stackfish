//! The message envelope. Every message has `v`, `kind` and `body`, and a
//! request from a Seat adds `seat`. A response has one shape, with a `status`
//! that says which of its optional fields to read.

use serde::{Deserialize, Serialize, Serializer};

use crate::wire::{WireDecision, WireEvent};

/// The protocol version in every message. A breaking change to a field's name,
/// type or meaning, or to a kind's required fields, bumps it.
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrorCode {
    UnsupportedVersion,
    MalformedMessage,
    UnknownSeat,
    NotYourDecision,
    NoOpenDecision,
    StaleDecision,
    InvalidAnswer,
    GameOver,
    SetupInvalid,
    PipelineError,
    /// `advance` while a Decision is open. Added after ticket 007's ten codes,
    /// which had none for this case.
    DecisionPending,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::UnsupportedVersion => "unsupported_version",
            ErrorCode::MalformedMessage => "malformed_message",
            ErrorCode::UnknownSeat => "unknown_seat",
            ErrorCode::NotYourDecision => "not_your_decision",
            ErrorCode::NoOpenDecision => "no_open_decision",
            ErrorCode::StaleDecision => "stale_decision",
            ErrorCode::InvalidAnswer => "invalid_answer",
            ErrorCode::GameOver => "game_over",
            ErrorCode::SetupInvalid => "setup_invalid",
            ErrorCode::PipelineError => "pipeline_error",
            ErrorCode::DecisionPending => "decision_pending",
        }
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// A request as it arrives. Unknown fields are rejected, and `body` is parsed
/// once `kind` is known.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub v: u32,
    pub kind: String,
    pub seat: Option<u32>,
    pub body: serde_json::Value,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Created,
    DecisionOpen,
    StillRunning,
    GameOver,
    Error,
}

#[derive(Serialize, Clone, Debug)]
pub struct WireError {
    pub code: ErrorCode,
    pub message: String,
}

/// The body of every response.
#[derive(Serialize, Clone, Debug)]
pub struct Response {
    pub status: Status,
    pub events: Vec<WireEvent>,
    /// Present when a Decision is open, including after a refused request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<WireDecision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
    /// With `game_over` only. `null` is a draw.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub winner: Option<Option<u32>>,
    /// With `created` only: the Seat ids, one per player.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seats: Option<Vec<u32>>,
}

impl Response {
    fn empty(status: Status) -> Self {
        Response { status, events: Vec::new(), decision: None, error: None, winner: None, seats: None }
    }

    pub fn created(seats: Vec<u32>) -> Self {
        Response { seats: Some(seats), ..Response::empty(Status::Created) }
    }

    pub fn decision_open(events: Vec<WireEvent>, decision: WireDecision) -> Self {
        Response { events, decision: Some(decision), ..Response::empty(Status::DecisionOpen) }
    }

    pub fn still_running(events: Vec<WireEvent>) -> Self {
        Response { events, ..Response::empty(Status::StillRunning) }
    }

    pub fn game_over(events: Vec<WireEvent>, winner: Option<u32>) -> Self {
        Response { events, winner: Some(winner), ..Response::empty(Status::GameOver) }
    }

    pub fn error(code: ErrorCode, message: impl Into<String>) -> Self {
        Response {
            error: Some(WireError { code, message: message.into() }),
            ..Response::empty(Status::Error)
        }
    }

    /// A refusal that leaves a Decision open reports `decision_open` for it.
    pub fn refusal(code: ErrorCode, message: impl Into<String>, open: Option<WireDecision>) -> Self {
        let mut response = Response::error(code, message);
        if let Some(decision) = open {
            response.status = Status::DecisionOpen;
            response.decision = Some(decision);
        }
        response
    }

    /// A fatal error carries the events produced before it.
    pub fn fatal(events: Vec<WireEvent>, message: impl Into<String>) -> Self {
        Response { events, ..Response::error(ErrorCode::PipelineError, message) }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        #[derive(Serialize)]
        struct Envelope<'a> {
            v: u32,
            kind: &'static str,
            body: &'a Response,
        }
        serde_json::to_vec(&Envelope { v: PROTOCOL_VERSION, kind: "response", body: self })
            .expect("a response holds only strings, numbers and lists, which always serialize")
    }
}
