//! Plain data types with no rules behavior. The rules processor and script
//! bridge live in sibling modules; see `docs/DESIGN.md`.

pub mod ability;
pub(crate) mod card;
pub(crate) mod combat;
pub(crate) mod command;
pub mod config;
pub mod cost;
pub mod counter;
pub(crate) mod effect;
pub(crate) mod event;
pub(crate) mod game;
pub(crate) mod ids;
pub mod mana;
pub(crate) mod object;
pub(crate) mod player;
pub(crate) mod stack;
pub(crate) mod target;
pub mod turn;
pub(crate) mod zone;

pub use ability::*;
pub use card::*;
pub use combat::*;
pub use command::*;
pub use config::*;
pub use cost::*;
pub use counter::*;
pub use effect::*;
pub use event::*;
pub use game::*;
pub use ids::*;
pub use mana::*;
pub use object::*;
pub use player::*;
pub use stack::*;
pub use target::*;
pub use turn::*;
pub use zone::*;
