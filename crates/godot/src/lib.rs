//! The Godot adapter for the Stackfish host. `StackfishSession` holds one
//! `Session` and passes bytes to it. The Session lives on the thread that made
//! the object (decision 009), and the rules for that are in `core.rs`.

mod core;

use godot::prelude::*;
use stackfish_host::{proof_cards, proof_setup_request};

use crate::core::{Core, Failure};

struct StackfishExtension;

#[gdextension]
unsafe impl ExtensionLibrary for StackfishExtension {}

/// One game. GDScript makes an instance with `StackfishSession.new()`, calls
/// `create` once with a `setup` request, then sends `advance`, `answer` and
/// `poll` requests through `handle`. Every call returns the response bytes.
/// An empty array means the call failed, and `last_error` says why.
///
/// Use it only on the thread that made it (decision 018):
/// - A call from another thread returns `null` and raises a script error.
///   gdext refuses it before the adapter runs, so `last_error` returns `null`
///   there too. Treat `null` as a programming error and do not retry.
///   The adapter's own `wrong_thread` code, with an empty array, is reached
///   only in a build with gdext's safeguards compiled out. Do not rely on it.
/// - `StackfishSession.new()` on another thread aborts the whole Godot process.
/// - Freeing the last reference on another thread ran without a crash in the
///   smoke test, but that is one run. Free it on the creating thread.
#[derive(GodotClass)]
#[class(init, base=RefCounted)]
struct StackfishSession {
    core: Core,
}

#[godot_api]
impl StackfishSession {
    /// Builds the game from a `setup` request. Returns the `created` response,
    /// or an error response if the setup was refused.
    // TODO(carddb): take a real CardSource. Until the carddb wrapper exists, only the proof card set resolves.
    #[func]
    fn create(&mut self, setup: PackedByteArray) -> PackedByteArray {
        let result = self.core.create(setup.as_slice(), &proof_cards());
        into_bytes(result)
    }

    /// Passes one `advance`, `answer` or `poll` request to the game.
    // TODO(snapshot): a Seat that joins late has no request that returns the current state.
    #[func]
    fn handle(&mut self, request: PackedByteArray) -> PackedByteArray {
        let result = self.core.handle(request.as_slice());
        into_bytes(result)
    }

    /// The reason the last call returned an empty array: `wrong_thread`,
    /// `panicked`, `not_created` or `already_created`. Empty if the last call
    /// worked.
    #[func]
    fn last_error(&self) -> GString {
        GString::from(self.core.last_error())
    }

    /// A `setup` request for the proof card set. The smoke test uses it to
    /// start a game without building JSON in GDScript.
    #[func]
    fn proof_setup(players: i64, seed: i64) -> PackedByteArray {
        let players = players.max(0) as usize;
        PackedByteArray::from(proof_setup_request(players, seed as u64))
    }

    /// How many sessions Godot has freed on a thread other than the one that
    /// made them. The smoke test reads this to see where Godot runs the
    /// destructor.
    #[func]
    fn off_thread_drops() -> i64 {
        core::off_thread_drops() as i64
    }
}

/// Turns a core result into bytes for GDScript. A failure logs and returns an
/// empty array.
fn into_bytes(result: Result<Vec<u8>, Failure>) -> PackedByteArray {
    match result {
        Ok(bytes) => PackedByteArray::from(bytes),
        Err(failure) => {
            godot_error!("StackfishSession call failed: {}", failure.code());
            PackedByteArray::new()
        }
    }
}
