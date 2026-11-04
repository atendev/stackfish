pub mod color_identity;
pub mod commands;
pub mod decision;
pub mod engine;
mod engine_context;
pub mod layers;
pub mod replacement;
pub mod sba;
pub mod script;
mod script_api;
pub mod triggers;
pub mod types;

pub use types::*;

/// Raised by hand in any change that alters the events or Decisions a given setup and
/// Answers produce, so a replay from an older engine is refused and not replayed wrong.
pub const BEHAVIOUR_REV: u32 = 4;
