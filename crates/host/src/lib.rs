//! The host protocol for the Stackfish engine. A `Session` talks only in bytes:
//! the host sends a `setup` request to create it, then `advance`, `answer` and
//! `poll` requests, and reads one response per request. A response shows only
//! what the requesting Seat may see. The wire format is JSON; see
//! `.plan/closed/007-wire-format.md` for the envelope and error codes.

mod bot;
mod card_source;
mod envelope;
mod proof_cards;
mod session;
mod setup;
mod view;
mod wire;

pub use bot::{Bot, BotError, Outcome, Progress};
pub use card_source::{CardSource, CardSourceError, FixtureCardSource};
pub use envelope::{ErrorCode, PROTOCOL_VERSION};
pub use proof_cards::{proof_cards, proof_setup_request};
pub use session::Session;
pub use setup::DEFAULT_STEP_CAP;
