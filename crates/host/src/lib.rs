//! The host protocol for the Stackfish engine. A `Session` talks only in bytes:
//! the host sends a `setup` request to create it, then `advance` and `answer`
//! requests, and reads one response per request. The wire format is JSON; see
//! `.plan/closed/007-wire-format.md` for the envelope and error codes.

mod card_source;
mod envelope;
mod session;
mod setup;
mod wire;

pub use card_source::{CardSource, CardSourceError, FixtureCardSource};
pub use envelope::{ErrorCode, PROTOCOL_VERSION};
pub use session::Session;
pub use setup::DEFAULT_STEP_CAP;
