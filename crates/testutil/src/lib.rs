//! Fixture builders shared by the engine's integration tests: states, zones, objects and
//! snapshot assertions. See `docs/DESIGN.md`.

mod atomic;
mod card;
mod engine_ext;
mod object;
mod player;
mod state;

pub use atomic::*;
pub use card::*;
pub use engine_ext::*;
pub use object::*;
pub use player::*;
pub use state::*;
