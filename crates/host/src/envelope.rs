//! The message envelope: `v`, `kind` and `body` on every message, plus `seat` on a Seat's
//! request. One response shape, whose `status` says which optional fields to read.

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
    /// The engine returned an `Internal` error. The game is over, but a Seat can still
    /// `poll` for the events it was owed.
    PipelineError,
    /// The engine panicked. Every later message, a `poll` included, gets this code.
    EnginePanic,
    /// `advance` while a Decision is open.
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
            ErrorCode::EnginePanic => "engine_panic",
            ErrorCode::DecisionPending => "decision_pending",
        }
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// A request as it arrives. `body` stays raw until `kind` says how to parse it.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub v: u32,
    pub kind: String,
    pub seat: Option<u32>,
    pub body: serde_json::Value,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Created,
    DecisionOpen,
    StillRunning,
    GameOver,
    Error,
}

#[derive(Serialize, Clone, Debug)]
pub(crate) struct WireError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Serialize, Clone, Debug)]
pub(crate) struct Response {
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
        Response {
            status,
            events: Vec::new(),
            decision: None,
            error: None,
            winner: None,
            seats: None,
        }
    }

    pub(crate) fn created(seats: Vec<u32>) -> Self {
        Response {
            seats: Some(seats),
            ..Response::empty(Status::Created)
        }
    }

    pub(crate) fn decision_open(events: Vec<WireEvent>, decision: WireDecision) -> Self {
        Response {
            events,
            decision: Some(decision),
            ..Response::empty(Status::DecisionOpen)
        }
    }

    pub(crate) fn still_running(events: Vec<WireEvent>) -> Self {
        Response {
            events,
            ..Response::empty(Status::StillRunning)
        }
    }

    pub(crate) fn game_over(events: Vec<WireEvent>, winner: Option<u32>) -> Self {
        Response {
            events,
            winner: Some(winner),
            ..Response::empty(Status::GameOver)
        }
    }

    pub(crate) fn error(code: ErrorCode, message: impl Into<String>) -> Self {
        Response {
            error: Some(WireError {
                code,
                message: message.into(),
            }),
            ..Response::empty(Status::Error)
        }
    }

    /// A refusal that leaves a Decision open reports `decision_open` for it.
    pub(crate) fn refusal(
        code: ErrorCode,
        message: impl Into<String>,
        open: Option<WireDecision>,
    ) -> Self {
        let mut response = Response::error(code, message);
        if let Some(decision) = open {
            response.status = Status::DecisionOpen;
            response.decision = Some(decision);
        }
        response
    }

    /// A fatal error carries the events produced before it.
    pub(crate) fn fatal(events: Vec<WireEvent>, message: impl Into<String>) -> Self {
        Response {
            events,
            ..Response::error(ErrorCode::PipelineError, message)
        }
    }

    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        #[derive(Serialize)]
        struct Envelope<'a> {
            v: u32,
            kind: &'static str,
            body: &'a Response,
        }
        serde_json::to_vec(&Envelope {
            v: PROTOCOL_VERSION,
            kind: "response",
            body: self,
        })
        .expect("a response holds only strings, numbers and lists, which always serialize")
    }
}
