---
id: 009
title: Where the Godot adapter runs the Session
type: grilling
mode: HITL
status: closed
blocked_by: [001]
claimed: false
---

## Question

Does `crates/godot` keep the Session on Godot's main thread and refuse calls from other threads, or run the engine on a dedicated thread and exchange `Vec<u8>` messages over channels? What does the chosen model mean for a long engine step (for example a big SBA loop or a Lua script) blocking a frame?

## Notes

- Background: closed ticket 008 (`closed/008-gdext-constraints.md`). gdext does not stop a GDScript `Thread` from calling a `#[func]` on an object that holds a `!Send` `mlua::Lua`.
- Depends on 001: whether a Session call runs to the next Decision or advances one step changes how long a call can block.

## Resolution

- The adapter keeps the Session on Godot's main thread. It does not run a dedicated engine thread and uses no channels. Calls are synchronous and return the response bytes.
- Each `#[func]` records the creating thread's id and checks it on every call. The check lives in `crates/godot`, so the Session stays plain Rust and tests can call it from any thread.
- A call from the wrong thread returns an empty `PackedByteArray` and logs through Godot's error macro. This adds no wire error code, so 007 stays closed. Superseded in part by 018: with gdext's safeguards on, a wrong-thread call gives `null` and a script error, not an empty array.
- One Godot class holds one Session per instance. There is no registry and no global state. GDScript creates as many instances as it needs. Each Session owns its own `Lua` state.
- The adapter adds no wall-clock limit and no budget parameter of its own. A host that wants smooth frames sends Advance with a small budget each frame. The Session's default cap (ticket 001) is the only guard on Answer, and a capped Answer returns "still running" so the host sends Advance.
- A host that later needs the engine off the main thread can wrap the adapter in its own thread, because the Session API is bytes in and bytes out.

Handed to other tickets:

- B3 should expose the Session's default cap as a Setup override, so a Godot host can set a low one. This touches 006 and 007, which are closed, so B3's requirements report should raise it.
- B6's smoke test calls the adapter from a second thread to confirm the guard fires. It also records whether dropping the Godot object on a non-main thread is safe. That stays unconfirmed until the test runs.

