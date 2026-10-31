//! Spike for the `mlua` coroutine mechanism that script pauses depend on, against the vendored mlua 0.9.
//! A script can `coroutine.yield` and resume with a value, a suspended `Thread` survives a round trip through the Lua registry, and `Thread::status()` separates `Resumable` from `Unresumable`.

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

    assert_eq!(
        thread.status(),
        ThreadStatus::Resumable,
        "freshly created, not yet started"
    );

    let yielded: i64 = thread
        .resume(42)
        .expect("first resume starts the coroutine's main function");
    assert_eq!(
        yielded, 123,
        "coroutine.yield(123) should hand back 123 to the resumer"
    );
    assert_eq!(
        thread.status(),
        ThreadStatus::Resumable,
        "suspended at the yield, can resume again"
    );

    let returned: i64 = thread
        .resume(43)
        .expect("second resume delivers 43 as coroutine.yield's own return value");
    assert_eq!(
        returned, 987,
        "the coroutine's own `return 987` should come back from this resume"
    );
    assert_eq!(
        thread.status(),
        ThreadStatus::Unresumable,
        "finished for good - no more yields pending"
    );

    let result: mlua::Result<i64> = thread.resume(());
    assert!(
        result.is_err(),
        "resuming a finished coroutine must error, not silently return garbage"
    );
}

#[test]
fn a_suspended_thread_survives_a_registry_round_trip_and_resumes_correctly() {
    // One Lua instance throughout, as ScriptRuntime owns one, because a RegistryKey is tied to the instance that created it.
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

    let retrieved: Thread = lua
        .registry_value(&key)
        .expect("a stashed Thread round-trips through the registry");
    assert_eq!(
        retrieved.status(),
        ThreadStatus::Resumable,
        "still paused at its yield after the round trip"
    );

    let result: String = retrieved
        .resume("go left")
        .expect("resuming the retrieved handle continues the same coroutine");
    assert_eq!(
        result, "did: go left",
        "the coroutine's own logic ran with the real resumed value, not a stale one"
    );
    assert_eq!(retrieved.status(), ThreadStatus::Unresumable);

    lua.remove_registry_value(key)
        .expect("cleanup - same convention a finished ScriptHandle entry would need");
}
