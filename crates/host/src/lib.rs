//! The host protocol: a `Session` talks only in JSON bytes, one response per request, and
//! shows each Seat only what it may see. See `docs/DESIGN.md`.

mod bot;
mod bot_payment;
mod card_source;
mod carddb_source;
mod envelope;
mod illegal;
mod proof_cards;
mod session;
mod setup;
mod view;
mod wire;

pub use bot::{Bot, BotError, Outcome, Progress};
pub use card_source::{CardSource, CardSourceError, FixtureCardSource};
pub use carddb_source::CardDbCardSource;
pub use envelope::{ErrorCode, PROTOCOL_VERSION};
pub use proof_cards::{proof_cards, proof_setup_request};
pub use session::Session;
pub use setup::DEFAULT_STEP_CAP;
