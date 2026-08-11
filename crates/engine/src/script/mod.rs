//! The Lua/mlua embedding bridge — split out of a single `script.rs` into
//! this directory for readability: `runtime` keeps `ScriptRuntime` itself
//! (the crate's real script-facing API, called from `engine.rs`/
//! `commands/`/`layers.rs`/`replacement.rs`/`sba.rs`/`triggers.rs`), while
//! `tags` (the `UserData` wrapper types a Lua script holds an opaque Rust
//! enum through), `types` (the cross-boundary error/context/registry
//! types), `decode` (Lua value/table -> Rust type decoders), `builders`
//! (the `Game.Create*Ability`/`Game.CreateReplacementEffect`/
//! `Game.CreateCostModifier` fluent builders), `globals` (the `Game.*` Lua
//! DSL registration), and `encode` (Rust -> Lua encoders) each hold one
//! slice of what used to be one file. See each submodule for the real
//! definitions and their own doc comments.

mod builders;
mod decode;
mod encode;
mod globals;
mod runtime;
mod tags;
mod types;

// Re-exports so `crate::script::X` keeps resolving for every path
// `engine.rs`/`commands/`/`layers.rs`/`replacement.rs`/`sba.rs`/
// `triggers.rs`/`types/cost.rs`/`types/effect.rs` (and, for `ScriptRuntime`,
// every `crates/engine/tests/phase5_*.rs` integration test) already reach
// through this module - see `runtime`/`types` for the real definitions.
pub use runtime::ScriptRuntime;
pub use types::{OperationOutcome, ScriptChoiceRequest, ScriptContext, ScriptError, ScriptZoneMoveKind};
