//! The Lua embedding bridge. `runtime` holds `ScriptRuntime`, the
//! script-facing API; `tags`, `types`, `decode`, `builders`, `globals` and
//! `encode` hold the rest. See `docs/DESIGN.md`.

mod builders;
mod decode;
mod encode;
mod globals;
mod runtime;
mod tags;
mod types;

// Re-exports keep `crate::script::X` paths resolving for the rest of the crate and the integration tests.
pub(crate) use decode::check_chain_words;
pub use runtime::ScriptRuntime;
pub use types::{OperationOutcome, ScriptChoiceRequest, ScriptContext, ScriptError, ScriptZoneMoveKind};
