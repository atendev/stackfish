//! Shared fixture builders for `crates/engine/tests/*.rs` — a single home
//! for the `two_player_state`/`card_object`/`drain`-style helpers most of
//! those test files otherwise define for themselves.

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
