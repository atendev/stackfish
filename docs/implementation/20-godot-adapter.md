# 20-godot-adapter

`crates/godot` (`stackfish-godot`, a `cdylib`) loads in Godot 4.2 or later and gives GDScript one class,
`StackfishSession`. It holds one `Session` and passes bytes to it. The design is in `.plan/closed/008`, `009` and `018`.

## API

`StackfishSession` is a `RefCounted`. Make one with `StackfishSession.new()` and call it only from the thread that made it.

- `create(setup: PackedByteArray) -> PackedByteArray` builds the Session. It returns the `created` response, or an error
  response if the setup was refused. A refused setup leaves the object without a Session, so `create` can be tried again.
- `handle(request: PackedByteArray) -> PackedByteArray` passes an `advance`, `answer` or `poll` request to the Session.
- `last_error() -> String` is the reason for the last empty return: `panicked`, `not_created` or `already_created`. It is
  empty if the last call worked. It also has a `wrong_thread` code, but a host does not see it in a default build (see
  the thread contract).
- `proof_setup(players, seed)` (static) is a `setup` request for the proof card set.
- `off_thread_drops()` (static) counts sessions freed on a thread other than the one that made them. The smoke test reads it.

## Module map

- `core.rs`: `Core` holds the Session, the owner `ThreadId`, a dead flag and the last failure. It has no Godot type, so unit tests run it
  from any thread. A call from another thread fails without writing to `self`. Each call runs under `catch_unwind`. After a
  panic the instance is dead, the Session is leaked instead of dropped, and every later call fails with `panicked`. Since ticket
  035 the Session catches an engine panic first and answers with an `engine_panic` response, so this guard is a backstop for a
  panic outside the Session's own guard (`22-error-split.md`).
- `lib.rs`: the class, a thin wrapper over `Core`. A failure logs through `godot_error!` and returns an empty array.
- `smoke/`: a Godot project, `smoke.gd` and `run.sh`.

## Smoke test

`GODOT=/path/to/godot crates/godot/smoke/run.sh` builds the library and runs three modes, each in its own headless Godot
process. `SAFEGUARDS_OFF=1` repeats the run with gdext's own thread check compiled out (release build with
`godot/safeguards-release-disengaged`, in a copy of the project under `target/smoke-release`).

- `play` must pass. It plays a two-player proof game through the class (1140 answers to `game_over`, passing priority), then calls
  `handle` and `last_error` from a second `Thread`. It requires the result in the thread contract: `null` from both by
  default, an empty array and `wrong_thread` with `SAFEGUARDS_OFF=1`. Then it checks the session still works.
- `drop` must pass. It frees the last reference on a second `Thread` and reads `off_thread_drops()`.
- `thread_owned` is recorded only. It makes and uses a session on a second `Thread`.

Results on Godot 4.7.2, Linux, gdext 0.5.5 with `api-4-2`:

| | safeguards on (default) | safeguards off |
|---|---|---|
| `handle` from another thread | gdext panics first. GDScript gets `null` and a script error "Invalid call error code 1337" | the adapter answers with an empty array and logs `wrong_thread` |
| `last_error()` from another thread | `null`, same error | `wrong_thread` |
| free the last reference off the main thread | works, Rust `Drop` ran on that thread | same |
| `StackfishSession.new()` off the main thread | aborts the whole Godot process | works, the session belongs to that thread |

## Thread contract (decision 018)

gdext's safeguards stay on in every build. The adapter does not use `godot/safeguards-release-disengaged`, because that
would make a wrong-thread call return `null` in debug builds and an empty array in release builds.

- Use a `StackfishSession` only on the thread that made it.
- A `handle` or `last_error` call from another thread returns `null` and raises a script error. Treat `null` from any call as
  a programming error and do not retry. A host reads `last_error()` only after an empty array, where it is `panicked`,
  `not_created` or `already_created`.
- The adapter's guard (`wrong_thread`, empty array) is a backstop for builds with gdext's check compiled out. A host does not
  rely on it.
- `StackfishSession.new()` on another thread aborts the whole Godot process.
- Freeing the last reference on another thread ran without a crash in one smoke run. That is not proof it is safe, so free
  it on the creating thread.

## Findings

- Decision 009 said a wrong-thread call returns an empty array. That holds only when gdext's own check is compiled out. gdext
  checks the main thread on every call that touches its binding, in debug builds (strict) and in release builds (balanced),
  and the release-disengaged feature is the only way to remove it. Decision 018 kept the check, so what a host sees is `null`
  plus a script error.
- Dropping a session off the main thread did not crash. Godot ran the Rust destructor on the dropping thread, and mlua's `Lua`
  dropped there without trouble. One run is not proof that it is safe in general.
- Creating the object off the main thread aborts the process while the safeguards are on, so `thread_owned` cannot be used
  by a host in a default build.
- The first `--import` of a fresh project aborts once, and the second run works. `run.sh` runs it twice.

## Open items

- TODO(carddb): `create` uses `proof_cards()`. It should take a real `CardSource` once the carddb wrapper exists.
- TODO(snapshot): a late-joining Seat has no request that returns the current state (from B4).
- TODO(export): the `.gdextension` paths point at the workspace `target/`. Shipping needs a copy step into `res://` and a macOS
  `.framework`.
- TODO(web): mlua's vendored Lua on emscripten is untested, and panics abort on Web.
- TODO(hot-reload): `reloadable` is false in the smoke project. What a reload does to live Sessions is unconfirmed.
- The smoke test needs a Godot binary, so CI does not run it. `cargo test --workspace` runs the seven `Core` tests and builds the
  `cdylib`.
