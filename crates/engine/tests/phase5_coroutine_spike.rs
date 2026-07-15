//! Phase 5 vertical slice: backlog item 9, spike. Proves the raw `mlua`
//! coroutine mechanism this whole slice's design depends on actually works
//! against the real vendored mlua 0.9, before any `Engine`-level plumbing
//! gets built on top of it:
//!
//! 1. A Lua coroutine can `coroutine.yield(...)` mid-script and get resumed
//!    later with a value standing in for the yield's return (`Thread::resume`).
//! 2. A suspended `Thread` survives a round-trip through `Lua`'s registry
//!    (`create_registry_value`/`registry_value`) - the mechanism
//!    `ScriptRegistry` will use to hold a paused script across separate
//!    `Engine::step()` calls, mirroring exactly how it already stores
//!    `Function`s via `RegistryKey`.
//! 3. `Thread::status()` reports `Resumable` while suspended and
//!    `Unresumable` once the coroutine has returned for good - the signal
//!    `Engine`'s resume logic will use to decide "pause again" vs "finished,
//!    continue the pipeline."
//!
//! No engine code depends on this file - it's a standalone confirmation this
//! slice's design (docs/implementation.md) is building on solid ground, not
//! a guess about mlua's API.

use mlua::{Lua, Thread, ThreadStatus};

#[test]
fn a_coroutine_yields_and_resumes_with_real_values() {
    let lua = Lua::new();
    let thread: Thread = lua
        .load(
            r#"
            coroutine.create(function(arg)
                assert(arg == 42, "expected 42, got " .. tostring(arg))
                local yielded = coroutine.yield(123)
                assert(yielded == 43, "expected 43, got " .. tostring(yielded))
                return 987
            end)
            "#,
        )
        .eval()
        .expect("loads and evaluates to a coroutine.create(...) thread");

    assert_eq!(thread.status(), ThreadStatus::Resumable, "freshly created, not yet started");

    let yielded: i64 = thread.resume(42).expect("first resume starts the coroutine's main function");
    assert_eq!(yielded, 123, "coroutine.yield(123) should hand back 123 to the resumer");
    assert_eq!(thread.status(), ThreadStatus::Resumable, "suspended at the yield, can resume again");

    let returned: i64 = thread.resume(43).expect("second resume delivers 43 as coroutine.yield's own return value");
    assert_eq!(returned, 987, "the coroutine's own `return 987` should come back from this resume");
    assert_eq!(thread.status(), ThreadStatus::Unresumable, "finished for good - no more yields pending");

    let result: mlua::Result<i64> = thread.resume(());
    assert!(result.is_err(), "resuming a finished coroutine must error, not silently return garbage");
}

#[test]
fn a_suspended_thread_survives_a_registry_round_trip_and_resumes_correctly() {
    // One Lua instance throughout, exactly like ScriptRuntime owns one `Lua`
    // for the engine's whole lifetime - RegistryKey is tied to the specific
    // Lua instance it was created from, so real usage always retrieves
    // through that same long-lived instance, never a fresh one.
    let lua = Lua::new();
    let thread: Thread = lua
        .load(
            r#"
            coroutine.create(function()
                local answer = coroutine.yield("what should I do?")
                return "did: " .. answer
            end)
            "#,
        )
        .eval()
        .unwrap();

    let question: String = thread.resume(()).unwrap();
    assert_eq!(question, "what should I do?");

    let key = lua.create_registry_value(thread).unwrap();

    // ... time passes, other Engine::step() calls happen in between ...

    let retrieved: Thread = lua.registry_value(&key).expect("a stashed Thread round-trips through the registry");
    assert_eq!(retrieved.status(), ThreadStatus::Resumable, "still paused at its yield after the round trip");

    let result: String = retrieved.resume("go left").expect("resuming the retrieved handle continues the same coroutine");
    assert_eq!(result, "did: go left", "the coroutine's own logic ran with the real resumed value, not a stale one");
    assert_eq!(retrieved.status(), ThreadStatus::Unresumable);

    lua.remove_registry_value(key).expect("cleanup - same convention a finished ScriptHandle entry would need");
}
