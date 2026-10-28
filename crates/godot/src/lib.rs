//! Godot adapter over the host `Session`: `StackfishSession` passes bytes to one Session.
//! Thread rules live in `core.rs`; see `docs/DESIGN.md`.

mod core;

use godot::prelude::*;
use stackfish_host::{proof_cards, proof_setup_request};

use crate::core::{Core, Failure};

struct StackfishExtension;

#[gdextension]
unsafe impl ExtensionLibrary for StackfishExtension {}

/// One game. Call `create` once with a `setup` request, then send `advance`, `answer` and
/// `poll` requests through `handle`. An empty array means the call failed, and `last_error`
/// says why.
///
/// Use it only on the thread that made it:
/// - A call from another thread returns `null` and raises a script error, because gdext
///   refuses it before the adapter runs. `last_error` returns `null` there too. The
///   adapter's `wrong_thread` code is reached only with gdext's safeguards compiled out.
/// - `StackfishSession.new()` on another thread aborts the Godot process.
/// - Freeing the last reference on another thread did not crash in the smoke test, but
///   free it on the creating thread anyway.
#[derive(GodotClass)]
#[class(init, base=RefCounted)]
struct StackfishSession {
    core: Core,
}

#[godot_api]
impl StackfishSession {
    /// Builds the game from a `setup` request using the proof-card fixture set, which
    /// needs no card database. Returns the `created` response or an error response.
    #[func]
    fn create(&mut self, setup: PackedByteArray) -> PackedByteArray {
        let result = self.core.create(setup.as_slice(), &proof_cards());
        into_bytes(result)
    }

    /// Builds the game from a `setup` request using the card database at
    /// `../stackfish-db/stackfish.db`, relative to the process's working directory.
    /// Returns the `created` response, or an error response if the database is missing.
    // TODO: take the database path as a parameter instead of assuming the launch directory
    #[func]
    fn create_with_db(&mut self, setup: PackedByteArray) -> PackedByteArray {
        match stackfish_host::CardDbCardSource::open("../stackfish-db/stackfish.db") {
            Ok(source) => into_bytes(self.core.create(setup.as_slice(), &source)),
            Err(e) => {
                // No Session exists to build the error response, so build one by hand.
                let body = serde_json::json!({
                    "v": stackfish_host::PROTOCOL_VERSION,
                    "kind": "response",
                    "body": {
                        "status": "error",
                        "error": { "code": "setup_error", "message": format!("could not open card database: {e}") }
                    }
                });
                PackedByteArray::from(body.to_string().into_bytes())
            }
        }
    }

    /// Passes one `advance`, `answer` or `poll` request to the game.
    // TODO: a Seat that joins late has no request that returns the current state
    #[func]
    fn handle(&mut self, request: PackedByteArray) -> PackedByteArray {
        let result = self.core.handle(request.as_slice());
        into_bytes(result)
    }

    /// Why the last call returned an empty array (`wrong_thread`, `panicked`, `not_created`
    /// or `already_created`), or an empty string if it worked.
    #[func]
    fn last_error(&self) -> GString {
        GString::from(self.core.last_error())
    }

    /// A `setup` request for the proof card set, so GDScript need not build the JSON.
    #[func]
    fn proof_setup(players: i64, seed: i64) -> PackedByteArray {
        let players = players.max(0) as usize;
        PackedByteArray::from(proof_setup_request(players, seed as u64))
    }

    /// How many sessions Godot freed on a thread other than their creator's.
    #[func]
    fn off_thread_drops() -> i64 {
        core::off_thread_drops() as i64
    }
}

fn into_bytes(result: Result<Vec<u8>, Failure>) -> PackedByteArray {
    match result {
        Ok(bytes) => PackedByteArray::from(bytes),
        Err(failure) => {
            godot_error!("StackfishSession call failed: {}", failure.code());
            PackedByteArray::new()
        }
    }
}
