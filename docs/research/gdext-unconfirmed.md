# gdext items left unconfirmed: web build, `Result` returns, hot reload

Researched 2026-11-03 against primary sources: the godot-rust book, the gdext sources and changelog at tag `v0.5.5` (read from the crates.io source of `godot-core`, `godot-macros`, `godot` 0.5.5), the mlua repository and the `mlua` 0.9.9 and `lua-src` 547.0.0 sources (the versions in `Cargo.lock`), the Godot docs, the Godot engine source at the 4.2, 4.3, 4.5 and 4.7 tags, and the rustc unstable book. Claims carry a source link or a file reference. Lines marked **UNCONFIRMED** or **inference** are gaps that no primary source settled. Nothing here was built or run: the machine has no `emcc` and no nightly toolchain.

Companion to [gdext-constraints.md](gdext-constraints.md), which holds the version table and threading rules.

## Verdicts

| # | Question | Verdict |
|---|---|---|
| 1a | mlua with vendored Lua 5.4 builds for `wasm32-unknown-emscripten` | **Documented** by the mlua 0.9.9 README, and its workflow defines an emscripten job (pass status not checked). **Unknown** for this repo's mlua 0.9.9 on Rust 1.98.0 and for mlua inside a gdext side module. |
| 1b | gdext supports Godot web export | **Documented** as experimental, with constraints from the book (nightly, `build-std`, `api-custom`, panics abort). One Rust extension per game is reported in open issue gdext#968, not a documented rule. |
| 2 | `#[func]` returning `Result<T, E>` | **Confirmed** (since gdext 0.5.2). `E` must implement `ErrorToGodot<T>`: `strat::Unexpected`, `()`, `global::Error`, or your own type. `Result<T, String>` does not compile (**inference** from the trait bound). The GDScript-side behavior of `Unexpected` comes from gdext docs and was not reproduced here. |
| 3 | Hot reload and live Rust state | **Confirmed** for the editor: the Rust instance is dropped and rebuilt from `init()`. A `Lua` field is lost and the session must be created again. Not applicable to an exported game. |

## 1. mlua and gdext on the web

### mlua on `wasm32-unknown-emscripten`

- The mlua 0.9.9 README says: "WebAssembly (WASM) is supported through `wasm32-unknown-emscripten` target for all Lua versions excluding JIT" ([0.9.9 README](https://raw.githubusercontent.com/mlua-rs/mlua/v0.9.9/README.md), line 31 of the copy in the crates.io source). The same text is in v0.11.6 ([README](https://raw.githubusercontent.com/mlua-rs/mlua/v0.11.6/README.md)).
- mlua 0.9.9 has a CI job `test_wasm32_emscripten` that installs Emscripten from apt and runs `cargo test --tests --features "lua54,vendored"` on that target for Lua 5.4, 5.3, 5.2, 5.1 and Luau ([0.9.9 main.yml](https://raw.githubusercontent.com/mlua-rs/mlua/v0.9.9/.github/workflows/main.yml)). The `main` branch still has the job ([main.yml](https://raw.githubusercontent.com/mlua-rs/mlua/main/.github/workflows/main.yml)).
- `mlua-sys` 0.6.8 refuses a non-vendored wasm32 build: "Please enable `vendored` feature to build for wasm32" (`build/find_normal.rs`). This repo already uses `vendored`.
- `lua-src` 547.0.0 has an emscripten branch. It defines `LUA_USE_POSIX`, compiles Lua **as C++** (`.cpp(true)`, sources copied and the Lua headers wrapped in `extern "C"`), and passes `-fexceptions` "to be caught" (`src/lib.rs` lines 107 to 133). Lua 5.4's `ldo.c` then throws C++ exceptions instead of using `longjmp`, because it is built under `__cplusplus` without `LUA_USE_LONGJMP`.
- The current mlua README no longer mentions emscripten. It describes `wasm32-unknown-unknown` with a WASI C runtime and the `wasm32-wasip*` targets ([README](https://raw.githubusercontent.com/mlua-rs/mlua/main/README.md), [docs/wasm32.md](https://raw.githubusercontent.com/mlua-rs/mlua/main/docs/wasm32.md)). That path needs the WASI SDK and does not fit Godot's emscripten side-module model.

Known breakage on newer Rust:

- [mlua#690](https://github.com/mlua-rs/mlua/issues/690) (closed 2026-03-28): with Rust 1.93 (this repo pins 1.98.0, past that point), `wasm32-unknown-emscripten` and mlua 0.11.6, linking fails with `undefined symbol: llvm_eh_typeid_for` and `__resumeException`, for all Lua versions. Rust 1.93 moved the emscripten unwind ABI from JS exceptions to wasm exceptions.
- The maintainer's resolution: the `cc` crate adds `-fno-exceptions` for wasm32 targets, so `lua-src` needs an explicit option. The Luau fix was in `luau-src-rs` commit `47d424a`. The non-Luau fix was in `lua-src-rs` commit `5be36b6`, which was "not included in the published `v550.0.0`". No Cargo-level workaround is documented.
- `lua-src` 547.0.0 (what mlua 0.9.9 resolves to in `Cargo.lock`) predates that commit. Newer `lua-src` releases exist (551.1.0 on 2026-10-24, [crates.io](https://crates.io/api/v1/crates/lua-src)), but mlua 0.9 pins the 547 line. **UNCONFIRMED** whether 547 fails the same way on Rust 1.98.0. The code path is the same `-fexceptions` plus C++ branch, so failure is likely unless the Rust side uses the JS exception ABI (see the flags below).

### gdext and Godot web export

- The book calls web export experimental and says it "should be understood as such" ([Export to Web](https://godot-rust.github.io/book/toolchain/export-web.html)). The gdext ReadMe says the same: "experimental support for Wasm, Android and iOS, but documentation and tooling is still lacking" ([ReadMe](https://raw.githubusercontent.com/godot-rust/gdext/v0.5.5/ReadMe.md)).
- Toolchain from the book: nightly `rustc` with `-Zbuild-std`, the `wasm32-unknown-emscripten` target, `rust-src`, and Emscripten on `PATH` (3.1.74 recommended for Godot 4.3+, up to 3.1.39 for Godot 4.2 and earlier templates). Build: `cargo +nightly build -Zbuild-std --target wasm32-unknown-emscripten`.
- Features: "At the moment, you'll need to use `api-custom` to export to the web", plus `experimental-wasm`, optionally `lazy-function-tables`, and `experimental-wasm-nothreads` for single-threaded builds ([book](https://godot-rust.github.io/book/toolchain/export-web.html), [godot lib.rs](https://raw.githubusercontent.com/godot-rust/gdext/v0.5.5/godot/src/lib.rs)). `api-custom` needs `GDRUST_GODOT_BIN` to point at a Godot binary. This repo uses `api-4-2` in `crates/godot/Cargo.toml`, so a web build needs a different feature set.
- Rustflags from the book: `-C link-args=-sSIDE_MODULE=2`, `-C llvm-args=-enable-emscripten-cxx-exceptions=0`, `-Z default-visibility=hidden`, `-Z link-native-libraries=no`, `-Z emscripten-wasm-eh=false`, plus `-pthread` and `+atomics` for threaded builds. The `emscripten-wasm-eh=false` flag fixes a `__cpp_exception` LinkError on Rust 1.93+. Setting `RUSTFLAGS` replaces everything in `.cargo/config.toml`.
- GDExtension on web is dynamic linking. The book says: "The compiled Wasm binary is a _side module_ which is dynamically loaded by Godot's _main module_ at runtime." The book never uses the word "dlink". On the Godot side, the default web export templates do not include GDExtension. They must be built with `dlink_enabled=yes` ([Compiling for the Web](https://docs.godotengine.org/en/stable/engine_details/development/compiling/compiling_for_web.html)). The export preset needs "Extensions Support" enabled ([Exporting for the Web, stable](https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_web.html), [4.2](https://docs.godotengine.org/en/4.2/tutorials/export/exporting_for_web.html)).
- Threads:
  - Godot 4.2: web exports always use `SharedArrayBuffer`, which needs the `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp` headers, or "the project will not run" ([4.2 docs](https://docs.godotengine.org/en/4.2/tutorials/export/exporting_for_web.html)).
  - Godot 4.3 and later: a single-threaded export exists and is the default ([stable docs](https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_web.html)). gdext handles it with a second `nothreads` build (gdext 0.3 or later). An extension that ships both modes needs two `.wasm` files, `{Crate}.threads.wasm` and `{Crate}.wasm`, and the Wasm file must match the crate name ([book](https://godot-rust.github.io/book/toolchain/export-web.html), [Changelog, `nothreads` PR #794](https://raw.githubusercontent.com/godot-rust/gdext/v0.5.5/Changelog.md)).
- Panics: "godot-rust cannot catch panics in Wasm yet due to limitations in the Rust compiler, so your game will abort" ([book](https://godot-rust.github.io/book/toolchain/export-web.html)).
- Reported, not documented: only one gdext extension can load per web game. [gdext#968](https://github.com/godot-rust/gdext/issues/968) (open, a user report) says that "it doesn't seem to be possible to use two extensions using `gdext` in web builds at all", because side modules share global symbols. `-Z default-visibility=hidden` is the proposed and book-recommended mitigation. This repo ships one extension, so it is not blocked.

### What it means for this repo

- Web export is possible in principle and not covered by CI. Nothing in the sources shows mlua running inside a gdext side module. **UNCONFIRMED** end to end.
- Risks to check, in this order:
  1. Does mlua 0.9.9 with `lua-src` 547 link on Rust 1.98.0 with the book's flags (`emscripten-wasm-eh=false` gives the Rust side the JS exception ABI that `-fexceptions` C++ code expects)? Inference, not shown anywhere.
  2. Do Lua errors thrown as C++ exceptions work in a side module built with `-enable-emscripten-cxx-exceptions=0` and `SIDE_MODULE=2`? That flag affects Rust codegen, and Lua's C++ is compiled separately with `-fexceptions`. **UNCONFIRMED**. A failing `luaL_error` would break every Lua `error()` and `pcall`.
  3. Does `-Z link-native-libraries=no` drop the `cargo:rustc-link-lib=static=lua5.4` that `lua-src` emits (`src/lib.rs` line 246)? The unstable book says only that it ignores libraries "specified in `#[link]` attributes" and does not say whether `-l` flags from build scripts are affected ([unstable book](https://doc.rust-lang.org/nightly/unstable-book/compiler-flags/link-native-libraries.html)). **UNCONFIRMED**.
- The toolchain cost is large. `rust-toolchain.toml` pins one compiler. A web build needs nightly with `-Zbuild-std`, so it would need its own workflow outside the pin.
- `Core` relies on `catch_unwind` and a dead flag after a panic (see `docs/implementation/20-godot-adapter.md`). On web a panic aborts the page, so neither that `catch_unwind` nor the `engine_panic` response runs there. A panic on web is not recoverable.
- Cheapest way to settle all three risks: one spike that builds `stackfish-godot` for `wasm32-unknown-emscripten` with the book's flags and loads it in a Godot web template built with `dlink_enabled=yes`. It needs `emcc`, nightly and a custom template, so it was not attempted here.

## 2. `#[func]` returning `Result<T, E>`

Support arrived in gdext 0.5.2: "Support `Result<T, E>` as return type in `#[func]` (#1544, #1564)". 0.5.4 added "Suppress Godot errors in fallible functions (#1637)" ([Changelog](https://raw.githubusercontent.com/godot-rust/gdext/v0.5.5/Changelog.md)). The book's [functions page](https://godot-rust.github.io/book/register/functions.html) does not cover it, so the source and API docs are the reference. No `api-4-*` gate was found on the `Result` path in `meta/error` or `signature.rs`, so it should apply to the repo's `api-4-2` build (**UNCONFIRMED**, not compiled).

### Allowed error types

A `#[func]` return type is valid if it is `ToGodot`, or `Result<T, E>` where `T: ToGodot` and `E: ErrorToGodot<T>` (`FuncReturn` in [signature.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/meta/signature.rs)). The in-crate `E` types are:

| `E` | What GDScript sees as the return type | On `Ok(v)` | On `Err` |
|---|---|---|---|
| `strat::Unexpected` | `T` (keeps static typing) | `v` | Godot logs the message and the call fails (see below). Accepts anything via `?` that converts to `Box<dyn Error + Send + Sync>`, plus `String` and `&str`. `func_bail!("...")` returns early. |
| `()` | `Variant` | `v` | `null`. Nothing is logged. |
| `global::Error` | `global::Error` enum | `OK` (the value is discarded) | the `ERR_*` constant. Nothing is logged. |
| your own type | whatever `Mapped` you choose | your choice | your choice |

Sources: [strat/mod.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/meta/error/strat/mod.rs), [strat/unexpected.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/meta/error/strat/unexpected.rs), [error_to_godot.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/meta/error/error_to_godot.rs). The module docs list "Dictionary `ok`/`err`", "Array 0/1 elems", "Variant" and "Custom class" as strategies that are not provided but can be built with `impl<T: ToGodot> ErrorToGodot<T> for MyError` returning `CallOutcome::Return(...)`. Rust's orphan rule allows that impl for a type owned by this repo.

`Result<T, String>` and `Result<T, serde_json::Error>` do not satisfy the bound and fail to compile (**inference** from the trait bound and the three in-crate impls found by searching the 0.5.5 source).

### What GDScript receives on `Err(strat::Unexpected)`

- `Unexpected` returns `CallOutcome::CallFailed("Err(Unexpected) in #[func]: <msg>")`. gdext builds a `CallError` and prints it with `godot_error!` unless an out-call from Rust is in progress ([private.rs `handle_fallible_call`](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/private.rs)). The printed text starts with "godot-rust function call failed: Class::func()" and carries the reason.
- The call then fails at the FFI level with error code `1337`. The source comment says the GDScript VM treats any non-OK code as "call failed, abort calling function", and that a "Bug: Invalid call error code 1337." print "is unavoidable at the VM level". `docs/implementation/20-godot-adapter.md` documents the same line for wrong-thread calls; the smoke test checks the null result in the default build and the empty-array result in the `SAFEGUARDS_OFF` build (`crates/godot/smoke/smoke.gd`, `run.sh`), and never checks the printed 1337 text.
- What the GDScript caller sees depends on the call kind. This is from the gdext docs ([unexpected.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/meta/error/strat/unexpected.rs)), not reproduced here. The source comment at line 81 says the varcall abort case is "Not yet reproduced in itest":
  - Varcall (untyped `Variant` receiver, `Object.call()`) in a Godot debug or editor build: the calling GDScript function aborts. The value of the call is never written.
  - Everything else (ptrcall through a typed variable such as `var s: StackfishSession`, and release builds): the caller continues with the default value of the declared return type, for example an empty `PackedByteArray`. The error is still printed. `signature.rs` says the same: ptrcalls "can't abort the calling GDScript function. Instead, this results in a Godot error print + default value".
  - Rust callers using `Object::try_call()` always get `Err`.
  - Godot's own discussion of why callers cannot reliably handle this is [godot-proposals#7751](https://github.com/godotengine/godot-proposals/discussions/7751).
- The docs say `Unexpected` is for bugs "fixed during development", not for expected runtime errors. "The Rust object and its state remain valid after an error."
- A panic inside the `#[func]` takes the same failure path (code `1337`), after the panic hook prints the message ([private.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/private.rs), `handle_fallible_varcall` and `handle_fallible_call`; `CallError::failed_by_user_panic`).

### What it means for this repo

- The existing design (error envelope inside the returned bytes, empty array plus `last_error()` for adapter faults) needs no `Result`. The `Result` form does not fit it: `Unexpected` gives a script error and a default value, with no way to carry a machine-readable reason to the host.
- If a typed error path is ever wanted, the useful choices are `Result<PackedByteArray, ()>` (host gets `null`, return type becomes `Variant`) or a custom `ErrorToGodot` that maps `Err` to a `Dictionary` such as `{"err": "..."}`. Both return normally, so they never abort the calling script and never log.
- The `null` the smoke test sees for a wrong-thread call is the `CallFailed` path described above, seen through an untyped (varcall) `var session` in a debug build. A typed caller would get an empty `PackedByteArray` instead.

## 3. Hot reload and live Rust state

### What the engine does

`GDExtension::prepare_reload()` and `finish_reload()` in Godot ([4.2 gdextension.cpp](https://raw.githubusercontent.com/godotengine/godot/4.2/core/extension/gdextension.cpp), [4.7 gdextension.cpp](https://raw.githubusercontent.com/godotengine/godot/4.7/core/extension/gdextension.cpp)):

1. For every live instance of every class the extension registered, Godot saves each property whose usage flags include `PROPERTY_USAGE_STORAGE`, skipping values equal to the class default.
2. `Object::clear_internal_extension()` calls the extension's `free_instance` for each object. The Rust instance is destroyed (its `Drop` runs) and the Godot object turns into its base class (a `RefCounted` here) ([4.2 object.cpp](https://raw.githubusercontent.com/godotengine/godot/4.2/core/object/object.cpp), same function in 4.3, 4.5 and 4.7).
3. The library reloads and classes register again.
4. `Object::reset_internal_extension()` calls the class's `recreate_instance` function. If it returns null, Godot logs "Unable to recreate GDExtension instance - does this extension support hot reloading?" and the object stays a plain base-class object.
5. Saved properties are restored through `set()`, then each object receives `NOTIFICATION_EXTENSION_RELOADED`.

The 4.2 and 4.7 versions of `prepare_reload` both filter on `PROPERTY_USAGE_STORAGE`. I read 4.2 in full and confirmed the same filter in 4.7 only.

### What gdext does with it

- For a `#[class(init)]` class, gdext registers `callbacks::recreate::<T>`, which runs the class's `init` again on the existing Godot object ([callbacks.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/registry/callbacks.rs), [shard.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/registry/shard.rs)). If `init` panics it returns null.
- The `#[derive(GodotClass)]` docs state it directly: "Editor reload ("hot reload") reconstructs your Rust instances from scratch." Godot saves `STORAGE` properties, destroys the old Rust instance, builds a fresh one with `init()`, and restores the saved properties through their setters. "Plain `#[var]` fields are not restored" because `#[var]` lacks `STORAGE`. `#[export]` includes it ([godot-macros lib.rs, "Editor reload and instance state"](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-macros/src/lib.rs)). Persisting a `#[var]` serializes it into the `.tscn`.
- Non-`#[var]` Rust fields cannot be saved by this mechanism at all. They are dropped and rebuilt from `init()`.
- `Gd<T>` signal connections made in Rust are not carried over and must be rebuilt on `EXTENSION_RELOADED` ([signal_connections_registry.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/signal/signal_connections_registry.rs)). Callables from `#[export_tool_button]` become invalid.
- Relevant fixes: 0.3.2 "Make hot-reload work with `#[class(no_init)]`" (gdext#1197, closing [#874](https://github.com/godot-rust/gdext/issues/874)), 0.4.1 macOS crash fix, 0.5.0 "Address unsoundness of typed signals after hot reload", 0.5.4 "Fix Linux hot-reload panic when TLS destructors fire before init" ([#1611](https://github.com/godot-rust/gdext/pull/1611)) ([Changelog](https://raw.githubusercontent.com/godot-rust/gdext/v0.5.5/Changelog.md)).
- There is no gdext feature named `editor_hot_reload`. Reload is controlled by the `.gdextension` key `reloadable` alone. The Godot docs say it "Reloads the extension upon recompilation", "mainly used for developing or debugging" ([4.2 docs](https://docs.godotengine.org/en/4.2/tutorials/scripting/gdextension/gdextension_file.html)). The book says to remove `reloadable` if Godot crashes.

### Where it applies

- `reloadable` only takes effect when `Engine::is_extension_reloading_enabled()` is true (`gdextension.cpp`, `set_reloadable(... && is_extension_reloading_enabled())`). Godot sets that only for the editor process (`main.cpp`: `if (editor) { ... set_extension_reloading_enabled(true) }`, in `#ifdef TOOLS_ENABLED`; same code in 4.2 and 4.7). An exported game, a project run from the command line without `--editor`, and the smoke test never reload.
- If a class cannot be recreated, Godot disables reload for the whole extension: "Extension marked as reloadable, but attempted to register class '%s' which doesn't support reloading ... Reloading disabled for this extension." (`gdextension.cpp`, 4.2 and 4.7).
- `StackfishSession` is `#[class(init, base=RefCounted)]`, so it has a recreate function.

### What it means for this repo

- A reload in the editor drops the `Core`. The owned `Session` and its `mlua::Lua` are dropped as part of `free_instance` (a normal `Drop`, not a leak). The fresh instance comes from `init()`, so it has no Session. Every later `handle` call returns an empty array, with `last_error()` set to `not_created`, until the host calls `create` again. Game state is lost.
- `Core` holds the owner `ThreadId`. After a reload the new instance records the thread that runs `init`. **UNCONFIRMED** which thread that is; the editor reloads from its main loop, so it is most likely the main thread.
- A dylib reload also unloads the library. Lua is linked statically into the cdylib, so the Lua heap should go away with the instances and library (inference). A Session kept alive in a Rust `static`, or leaked by the adapter's panic path (`Core` leaks the Session after a panic), would leave a dangling allocation in an unloaded library. **UNCONFIRMED**: I found no primary source on allocations that outlive an unloaded library. Keep sessions only inside instances.
- A scripted game started with F5 is, by inference from the editor-only flag above, a separate non-editor process and is not reloaded. The only effect is on a Session that a tool script or editor plugin holds inside the editor process.
- Leave `reloadable = false` in the smoke project and in shipped builds. For editor development, plan for sessions to be recreated: the host should treat `last_error() == "not_created"` as "rebuild the game", or a tool script can handle `NOTIFICATION_EXTENSION_RELOADED` to recreate it. Restoring a game would need a snapshot request (already a TODO in the adapter notes), since no Rust field can survive the reload.
- **UNCONFIRMED:** whether reload works with gdext on Godot 4.2 specifically with this crate. The Godot 4.2 reload machinery is the same as shown above, but nothing tested it here. The `reloadable` key is documented in the 4.2 `.gdextension` docs linked above, and it is off in the smoke project.
