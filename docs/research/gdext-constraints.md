# gdext (godot-rust) constraints for `crates/host` and `crates/godot`

Researched 2026-10-12 against primary sources: the godot-rust book, the gdext repository at tag `v0.5.5`, docs.rs, crates.io, the Godot docs (stable = 4.7), the mlua docs and repository, and the Cargo book. Every claim has a source link. Lines marked **UNCONFIRMED** are inferences or gaps that no primary source settled.

Source-code links point at tag `v0.5.5` of `godot-rust/gdext`, which matches the newest crates.io release.

## 1. Versions (question 4)

| Item | Value | Source |
|---|---|---|
| Newest `godot` crate | **0.5.5**, published 2026-08-09 | [crates.io API](https://crates.io/api/v1/crates/godot), [Changelog](https://github.com/godot-rust/gdext/blob/master/Changelog.md) |
| 0.5.x line | 0.5.0 (2026-03-27) through 0.5.5 | [Changelog](https://github.com/godot-rust/gdext/blob/master/Changelog.md) |
| MSRV | Rust **1.94**, edition 2024 | [crates.io 0.5.5](https://crates.io/api/v1/crates/godot/0.5.5), [Migrating to v0.5](https://godot-rust.github.io/book/migrate/v0.5.html) |
| License | MPL-2.0 | [crates.io 0.5.5](https://crates.io/api/v1/crates/godot/0.5.5) |
| Minimum Godot | **4.2** | [Compatibility](https://godot-rust.github.io/book/toolchain/compatibility.html); `ALL_VERSIONS` starts at `(4, 2, 0)` and `MIN_SUPPORTED_VERSION` derives from it ([import.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-bindings/src/import.rs)) |
| API-level features | `api-4-2`, `api-4-3`, `api-4-4`, `api-4-5`, `api-4-6`, `api-4-7`, plus `api-custom`, `api-custom-json` | [docs.rs features](https://docs.rs/crate/godot/latest/features) |
| Default API level (no `api-*` feature) | **4.6** (`pub use gdextension_api::version_4_6 as prebuilt`) | [import.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-bindings/src/import.rs), [Migrating to v0.5](https://godot-rust.github.io/book/migrate/v0.5.html) |
| Newest API level | 4.7 (`LATEST_API_VERSION = (4, 7, 0)`; `api-4-7` added in 0.5.4) | [import.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-bindings/src/import.rs), [Changelog](https://github.com/godot-rust/gdext/blob/master/Changelog.md) |
| Current Godot stable docs | 4.7 | [Thread-safe APIs (stable)](https://docs.godotengine.org/en/stable/tutorials/performance/thread_safe_apis.html) header |

Compatibility rules:

- An extension loads in any Godot whose runtime version is at least its API version. A 4.2-level build runs on 4.3, but a 4.3-level build does not run on 4.2.1 ([Compatibility](https://godot-rust.github.io/book/toolchain/compatibility.html)).
- Only one `api-*` feature may be active at a time ([docs.rs crate docs](https://docs.rs/godot/latest/godot/)).
- Since v0.5, API features name a minor version only. The `api-4-x-y` patch features are gone ([Migrating to v0.5](https://godot-rust.github.io/book/migrate/v0.5.html)). The [Selecting a Godot version](https://godot-rust.github.io/book/toolchain/godot-version.html) page still describes `api-4-2-1`, so that part of the book is out of date.
- The book's compatibility table still lists "0.4+ → 4.2" and does not mention 0.5 ([Compatibility](https://godot-rust.github.io/book/toolchain/compatibility.html)). The 0.5.5 source still ships `api-4-2`, which is consistent with a 4.2 minimum.
- Breaking changes are expected. crates.io releases follow SemVer, and support for older Godot versions typically lasts 1 to 2 years after their release ([Compatibility](https://godot-rust.github.io/book/toolchain/compatibility.html)).

Design note: pin `godot = "0.5.5"` and pick an explicit `api-4-x` feature that matches the oldest Godot you will ship on, rather than relying on the moving default.

## 2. Bytes and strings across the GDScript and Rust boundary (question 1)

### PackedByteArray

- In Rust, `PackedByteArray` is a type alias for `PackedArray<u8>` ([docs.rs](https://docs.rs/godot/latest/godot/builtin/type.PackedByteArray.html)).
- It has value semantics: "each copy will be independent of the original. Under the hood, Godot uses copy-on-write, so copies are still cheap to make" ([packed_array.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/builtin/collections/packed_array.rs), [book: built-in types](https://godot-rust.github.io/book/godot-api/builtins.html)).
- In GDScript, "Packed arrays are always passed by reference". Use `duplicate()` for an independent copy. Built-in properties and methods return copies ([Godot PackedByteArray](https://docs.godotengine.org/en/stable/classes/class_packedbytearray.html)).

Copy costs of each Rust conversion, read from the 0.5.5 source ([packed_array.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/builtin/collections/packed_array.rs)):

| Operation | Cost |
|---|---|
| `as_slice() -> &[u8]` | **Zero copy.** Borrows Godot's buffer through `slice::from_raw_parts`. |
| `as_mut_slice() -> &mut [u8]` | Zero copy if the buffer is unique. `ptr_mut()` "triggers a copy if non-unique" (copy-on-write). |
| `to_vec() -> Vec<u8>` | **One full copy** ("making a copy of its contents"). |
| `PackedByteArray::from(&[u8])` | Allocates a Godot buffer with `resize(n)`, then `clone_from_slice`. That is one allocation plus one copy, and `resize` may also initialize the buffer first (**UNCONFIRMED** whether Godot zero-fills on resize). |
| `PackedByteArray::from(Vec<u8>)` | **Still copies.** It allocates a Godot buffer with `resize(len)` and then `copy_from_nonoverlapping` from the Vec. Godot cannot adopt a Rust allocation. |
| `FromIterator<u8>` | Pre-allocates from `size_hint()`'s lower bound, then batches into a fixed buffer ([docs.rs PackedArray](https://docs.rs/godot/latest/godot/builtin/struct.PackedArray.html)). |
| `encode_*` / `decode_*` | Byte order is "an implementation detail". Use `as_slice()` or `as_mut_slice()` for portable byte handling ([docs.rs PackedArray](https://docs.rs/godot/latest/godot/builtin/struct.PackedArray.html)). |

Passing a `PackedByteArray` as a `#[func]` parameter or return value goes through `GodotFfi::from_arg_ptr` and `move_return_ptr`, which wrap the existing Godot value through `new_from_sys` ([godot_ffi.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-ffi/src/godot_ffi.rs)). **UNCONFIRMED** in detail: I did not trace whether this path does a refcount bump or a move, but nothing in it copies element bytes. The byte copy happens only when you convert to or from `Vec<u8>` or `&[u8]`.

In practice:
- Inbound (GDScript to Rust): take `PackedByteArray` and decode straight from `as_slice()`. That costs zero copies. If you need to own the bytes (for example, to send them to another thread), `to_vec()` costs one copy.
- Outbound (Rust to GDScript): build a `Vec<u8>` and convert with `PackedByteArray::from(vec)`. That costs one copy. Encoding straight into a pre-sized `PackedByteArray` through `as_mut_slice()` avoids it.

### Strings

- Godot's `String` is `GString` in Rust and uses **UTF-32**. Rust `String` is UTF-8 ([gstring.rs docs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/builtin/strings/gstring.rs)).
- In `#[func]` methods, both `String` and `GString` work as parameters and return types, "and conversions are done transparently" ([gstring.rs docs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/builtin/strings/gstring.rs)). Every crossing with a Rust `String` therefore re-encodes between UTF-8 and UTF-32.
- `String::from(&GString)` calls Godot's `string_to_utf8_chars` twice, once to get the length and once to fill a `Vec<u8>`, then runs `String::from_utf8` with validation ([gstring.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/builtin/strings/gstring.rs)). That is one allocation, one transcode, and one validation pass.
- The `GString` docs recommend `GString` "if your strings are very large, so you can avoid copying them" ([gstring.rs docs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/builtin/strings/gstring.rs)).
- Godot ignores everything after a NUL byte in a `GString` ([gstring.rs docs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/builtin/strings/gstring.rs)).
- `StringName` from `&str` is "rather expensive, since UTF-8 is re-encoded as UTF-32" ([book: built-in types](https://godot-rust.github.io/book/godot-api/builtins.html)).
- Godot `String` is reference-counted and copy-on-write, "so passing them around is cheap in resources" ([Godot String](https://docs.godotengine.org/en/stable/classes/class_string.html)).
- In GDScript, `String.to_utf8_buffer()` and `PackedByteArray.get_string_from_utf8()` are inverses. `get_string_from_utf8` returns an empty string on invalid UTF-8 ([Godot String](https://docs.godotengine.org/en/stable/classes/class_string.html), [Godot PackedByteArray](https://docs.godotengine.org/en/stable/classes/class_packedbytearray.html)).

Design note: if the protocol is UTF-8 text such as JSON, sending it as `PackedByteArray` (built with `to_utf8_buffer()` in GDScript and read with `as_slice()` in Rust) avoids the UTF-32 round trip that a `String` parameter forces. A binary protocol over `PackedByteArray` has the cheapest boundary.

## 3. Holding a `!Send` value such as `mlua::Lua` in a gdext class (question 2)

### What gdext requires of user classes

- `GodotClass` is declared as `pub trait GodotClass: Bounds + 'static`, with **no `Send` or `Sync` bound** ([traits.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/obj/traits.rs)). The derive macro adds none either: `derive_godot_class.rs` has no `Send` or thread references ([derive_godot_class.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-macros/src/class/derive_godot_class.rs)). A `#[derive(GodotClass)] #[class(base=RefCounted)]` struct with an `mlua::Lua` field therefore compiles. The same holds for `base=Node`.
- `Gd<T>` and `PackedArray<T>` are `!Send` and `!Sync` ([docs.rs PackedArray auto traits](https://docs.rs/godot/latest/godot/builtin/struct.PackedArray.html)). A `PackedArray` "is safe if used on a single thread only". Concurrent reads are fine, but writes need external synchronization (same page).

### Which threads may touch it

- gdext records the thread that loads the library as the main thread (`MAIN_THREAD_ID`, set in `initialize`) ([godot-ffi lib.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-ffi/src/lib.rs)).
- In the default build, without `experimental-threads`, instance storage uses `godot_cell::panicking::GdCell`, which panics on a conflicting borrow rather than blocking ([single_threaded.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/storage/single_threaded.rs), [godot-cell lib.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-cell/src/lib.rs)).
- Several APIs assert main-thread use: `run_deferred`, `Base::to_init_gd()` for ref-counted objects, autoload fetching, and `godot_task()` ([gd.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/obj/gd.rs), [base_weak_initialization.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/obj/base_weak_initialization.rs), [autoload.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/tools/autoload.rs), [async_runtime.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/task/async_runtime.rs)). A comment in `base_weak_initialization.rs` says: "At the moment, all Godot objects must be accessed from the main thread, because their deferred destruction (`Drop`) runs on the main thread."
- **UNCONFIRMED and important:** I found no runtime check that rejects a `#[func]` call arriving on a non-main thread. That would happen if GDScript calls the object from a `Thread` or `WorkerThreadPool`. A search of the 0.5.5 `meta/` and `registry/` call path found no thread-ID validation. The tracking issue [gdext#18 "Deal with cross-thread access to user-defined GodotClass instances"](https://github.com/godot-rust/gdext/issues/18) is still **open**. In it, the maintainers note that `GodotClass` has no `Send` requirement, so a non-`Send` field makes cross-thread access unsound, and they describe thread-ID validation only as a plan. Treat "only call the Session from the main thread" as a rule you must enforce yourself, for example by asserting `godot::init::is_main_thread()` or comparing `std::thread::current().id()` in each `#[func]`.
- `experimental-threads` "adds synchronization to access the user instance in `Gd<T>` and disables several single-thread checks". The docs warn: "The safety aspects are not ironed out yet; there is a high risk of unsoundness at the moment." It cannot be combined with `lazy-function-tables` ([godot lib.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot/src/lib.rs), [docs.rs](https://docs.rs/godot/latest/godot/)). Even with it enabled, the user class still has no `Send` bound. It would serialize access to a `!Send` `Lua` across threads, which Rust's rules do not allow (**inference**, based on the missing bound in [traits.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/obj/traits.rs)).
- On the Godot side, the active scene tree is not thread-safe. Use `call_deferred` and `set_deferred` from threads ([Thread-safe APIs](https://docs.godotengine.org/en/stable/tutorials/performance/thread_safe_apis.html)).

### mlua and its `send` feature

- Stackfish currently depends on `mlua = { version = "0.9", features = ["lua54", "vendored"] }` (`crates/engine/Cargo.toml`). The newest mlua is **0.12.2** (2026-10-03) ([mlua Cargo.toml](https://raw.githubusercontent.com/mlua-rs/mlua/main/Cargo.toml), [mlua CHANGELOG](https://raw.githubusercontent.com/mlua-rs/mlua/main/CHANGELOG.md)).
- "By default `mlua` is `!Send`" ([docs.rs mlua 0.9.9](https://docs.rs/mlua/0.9.9/mlua/), [docs.rs mlua latest](https://docs.rs/mlua/latest/mlua/)).
- **mlua 0.9 with `send`:** `Lua` becomes `Send` through a manual impl ("Requires `feature = "send"`"), but stays **`!Sync`**. The feature "adds `Send` requirement to `mlua::Function` and `mlua::UserData`" ([docs.rs mlua 0.9.9 Lua](https://docs.rs/mlua/0.9.9/mlua/struct.Lua.html), [mlua 0.9.9 README](https://raw.githubusercontent.com/mlua-rs/mlua/v0.9.9/README.md)).
- **mlua 0.10 and later with `send`:** "Make Lua types truly `Send` and `Sync`" (0.10.0-beta.1). Access to the VM "is synchronized using a reentrant mutex that can be locked many times within the same thread". UserData must be `Send + Sync`, and `Send`-only userdata must be wrapped, for example in a `Mutex`. `send` also turns on `error-send`. `send` is disabled in module mode ([mlua CHANGELOG](https://raw.githubusercontent.com/mlua-rs/mlua/main/CHANGELOG.md), [docs.rs mlua latest](https://docs.rs/mlua/latest/mlua/), [mlua README](https://raw.githubusercontent.com/mlua-rs/mlua/main/README.md), [mlua Cargo.toml](https://raw.githubusercontent.com/mlua-rs/mlua/main/Cargo.toml)).
- `Lua` is `!UnwindSafe` and `!RefUnwindSafe` in both 0.9.9 and latest ([0.9.9](https://docs.rs/mlua/0.9.9/mlua/struct.Lua.html), [latest](https://docs.rs/mlua/latest/mlua/struct.Lua.html)). gdext runs `#[func]` bodies inside `std::panic::catch_unwind` (`handle_panic` in [private.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/private.rs)), so the engine state may be left half-updated after a panic. The Session should treat a caught panic as fatal for that game (**inference**).
- mlua converts panics inside Rust callbacks into Lua errors ([mlua README](https://raw.githubusercontent.com/mlua-rs/mlua/main/README.md)).

### Options for the Session

1. **Main-thread only (simplest, no feature changes).** The gdext class owns `host::Session`, which owns the engine and `mlua::Lua`. GDScript calls only from the main thread, and each `#[func]` asserts that. The docs and source above support this pattern. It does not need mlua's `send` feature.
2. **Dedicated engine thread.** `host::Session` lives on a Rust `std::thread` it spawned. The gdext class holds only a channel pair that carries `Vec<u8>`. The channel endpoints are `Send`, and `Lua` never leaves its thread, so `send` is still not needed. The gdext wrapper copies `PackedByteArray` to `Vec<u8>` (one copy) because `PackedArray` is `!Send`. Results come back through polling from `_process` or a `call_deferred` signal.
3. **Moving the Session between threads** (for example, running it on `WorkerThreadPool`) needs `Lua: Send`, which means mlua's `send` feature. In 0.10 and later that also means `Send + Sync` userdata and an internal mutex. It also runs into gdext's open threading questions above. Not recommended.

## 4. Building and shipping from a Cargo workspace (question 3)

### Crate setup

- `crates/godot/Cargo.toml` needs `[lib] crate-type = ["cdylib"]` and the `godot` dependency ([book: Hello World](https://godot-rust.github.io/book/intro/hello-world.html), [Cargo targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html)).
- The entry point is one `#[gdextension] unsafe impl ExtensionLibrary for MyExtension {}`, and the `.gdextension` file uses `entry_symbol = "gdext_rust_init"` ([book: Hello World](https://godot-rust.github.io/book/intro/hello-world.html)).
- `crate-type` is an array, so a library can produce several crate types ([Cargo targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html)). **UNCONFIRMED:** whether adding `"rlib"` alongside `"cdylib"` is needed or harmful for gdext. Keep the logic in `crates/host`, which needs no Godot dependency, so the adapter needs no Rust-side tests.
- Keeping `godot` as a dependency of `crates/godot` only keeps the core UI-agnostic. Nothing in gdext requires the other crates to know about Godot.

### Workspace pitfalls

- **Output path:** "All packages share a common output directory, which defaults to a directory named `target` in the workspace root" ([Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)). The library lands in `<repo>/target/{debug,release}/`, not in `crates/godot/target/`. The book's example paths (`res://../rust/target/...`) assume a standalone crate ([book: Hello World](https://godot-rust.github.io/book/intro/hello-world.html)), so rewrite them relative to the Godot project's location.
- **Library name:** the library target name is the package name with dashes replaced by underscores ([Cargo targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html)). A package named `stackfish-godot` produces `libstackfish_godot.so`, `stackfish_godot.dll`, and `libstackfish_godot.dylib`. Rust's convention is a `lib` prefix on Unix and none on Windows, as the book's paths show ([book: Hello World](https://godot-rust.github.io/book/intro/hello-world.html)). Setting `[lib] name = "..."` pins the name.
- **Profiles:** `[profile.*]` (and `[patch]`) are only read from the workspace root manifest ([Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)). Any release tuning or `panic` setting for the extension must go in the root `Cargo.toml` and applies to all members.
- **Lockfile:** one `Cargo.lock` sits at the workspace root ([Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)). Raising the workspace MSRV to 1.94 for gdext affects every member that shares the toolchain.
- Build only the adapter with `cargo build -p <adapter-package>` (standard Cargo; the book does not cover workspaces).
- **Cross-compile paths:** using a custom `--target` adds a target-triple directory (`target/<triple>/release/`), and the `.gdextension` paths must match ([book: Hello World](https://godot-rust.github.io/book/intro/hello-world.html), [book: Web export](https://godot-rust.github.io/book/toolchain/export-web.html)).

### The `.gdextension` file

From the book ([Hello World](https://godot-rust.github.io/book/intro/hello-world.html)), adapted to a workspace where the Godot project sits at `<repo>/godot/`:

```ini
[configuration]
entry_symbol = "gdext_rust_init"
compatibility_minimum = 4.2
reloadable = true

[libraries]
linux.debug.x86_64 =     "res://../target/debug/lib{crate}.so"
linux.release.x86_64 =   "res://../target/release/lib{crate}.so"
windows.debug.x86_64 =   "res://../target/debug/{crate}.dll"
windows.release.x86_64 = "res://../target/release/{crate}.dll"
macos.debug =            "res://../target/debug/lib{crate}.dylib"
macos.release =          "res://../target/release/lib{crate}.dylib"
macos.debug.arm64 =      "res://../target/debug/lib{crate}.dylib"
macos.release.arm64 =    "res://../target/release/lib{crate}.dylib"
```

The book uses `compatibility_minimum = 4.1`. I wrote `4.2` above to match gdext's minimum; that is my adaptation.

Key reference ([Godot: The .gdextension file, 4.4 docs](https://docs.godotengine.org/en/4.4/tutorials/scripting/gdextension/gdextension_file.html)):
- `entry_symbol` is required. `compatibility_minimum` needs Godot 4.1 or later. `compatibility_maximum` needs 4.3 or later. `reloadable` is meant for development. `android_aar_plugin` also exists.
- `[libraries]` keys combine an OS tag (`windows`, `macos`, `linux`, `linuxbsd`, `android`, `ios`, `web`), a build tag (`debug`, `release`, `editor`), and an arch tag (`x86_64`, `arm64`, `rv64`, `wasm32`, and others). Entries are evaluated top to bottom, and every tag must match for an exported game.
- `[dependencies]` lists extra shared libraries to copy on export. On macOS they must go in `Game.app/Contents/Frameworks`. This matters only if the extension links a separate shared library. Vendored Lua is linked statically (**UNCONFIRMED** for every target; mlua's `vendored` feature "will automatically build and link" Lua per the [mlua README](https://raw.githubusercontent.com/mlua-rs/mlua/main/README.md)).
- `[icons]` maps class names to 16x16 SVGs.
- The current Godot docs URL for this page returned 404 under `/stable/` and `/latest/`, so I used the 4.4 copy. **UNCONFIRMED** that 4.7 added no keys.

Other notes:
- For export, library paths "must stay inside `res://`" ([book: Hello World](https://godot-rust.github.io/book/intro/hello-world.html)). Paths like `res://../target/...` work in the editor, but shipping needs a step that copies the built libraries into the Godot project, for example `godot/bin/`, with `.gdextension` paths pointing there.
- `res://.godot/extension_list.cfg` must list the `.gdextension` file. The editor creates it ([book: Hello World](https://godot-rust.github.io/book/intro/hello-world.html)).
- Remove `reloadable` if Godot crashes ([book: Hello World](https://godot-rust.github.io/book/intro/hello-world.html)).

### Per-platform notes

- **Linux / Windows:** build natively and point the `.gdextension` paths at the `.so` or `.dll` ([book: Hello World](https://godot-rust.github.io/book/intro/hello-world.html)).
- **macOS:** for distribution, build `x86_64-apple-darwin` and `aarch64-apple-darwin`, merge them with `lipo` into a universal dylib, and wrap it in `lib{crate}.macos.framework/` with `Resources/Info.plist`. `CFBundleExecutable` must match the dylib name, and `CFBundlePackageType` is `FMWK`. Then point `macos.release` at the `.framework`. Code signing and notarization need an Apple Developer account and are optional when exporting the whole game ([book: macOS and iOS export](https://godot-rust.github.io/book/toolchain/export-mac-and-ios.html)). Godot's docs also use `.framework` entries for macOS ([Godot .gdextension file](https://docs.godotengine.org/en/4.4/tutorials/scripting/gdextension/gdextension_file.html)).
- **Web (experimental):** target `wasm32-unknown-emscripten` with nightly `-Zbuild-std` and the `api-custom` and `experimental-wasm` features. Panics abort, since they cannot be caught in Wasm. Rust 1.93+ needs `-Z emscripten-wasm-eh=false`. Name the Rust library differently from the Godot project ([book: Web export](https://godot-rust.github.io/book/toolchain/export-web.html)). **UNCONFIRMED:** whether mlua's vendored Lua 5.4 builds for `wasm32-unknown-emscripten`.
- **Android / iOS:** experimental ([gdext ReadMe](https://raw.githubusercontent.com/godot-rust/gdext/master/ReadMe.md)). Not researched further.

## 5. Other constraints on a bytes-in/bytes-out Session (question 5)

- **No Godot types in `crates/host`.** `PackedByteArray` is `!Send` ([docs.rs](https://docs.rs/godot/latest/godot/builtin/struct.PackedArray.html)), and Godot objects belong to the main thread ([base_weak_initialization.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/obj/base_weak_initialization.rs)). A host API of `fn handle(&mut self, input: &[u8]) -> Vec<u8>` maps cleanly: `as_slice()` gives zero-copy input, and `PackedByteArray::from(vec)` costs one copy on output (section 2).
- **Write output straight into Godot memory** (optional optimization). An API shaped like `fn handle_into(&mut self, input: &[u8], out: &mut impl Write)` or a size-then-fill pair would let the adapter `resize()` a `PackedByteArray` and write through `as_mut_slice()`, avoiding the `Vec` copy. This is only worth doing for large snapshots.
- **Panics.** `#[func]` bodies run under `catch_unwind` ([private.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/private.rs)), but on Web they abort ([book: Web export](https://godot-rust.github.io/book/toolchain/export-web.html)). The protocol should report engine errors as error messages in the byte stream rather than relying on panics.
- **Error returns.** The 0.5.5 source keeps a thread-local `CallError` for "`#[func]` methods returning `Result<T, E>`" ([private.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/private.rs)). **UNCONFIRMED:** how GDScript observes that error. A plain error envelope inside the returned bytes is simpler and portable.
- **Choice of base class.** A `RefCounted` subclass needs no scene tree and is freed when its last GDScript reference drops. A `Node` subclass gets `_process` for polling a background engine thread. gdext supports both ([book: Hello World](https://godot-rust.github.io/book/intro/hello-world.html)). On the main thread, `Base::to_init_gd()` works for ref-counted objects only ([base.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/obj/base.rs)).
- **Re-entrancy.** In the default build, a second borrow of the same instance while one is active panics ("`Gd<T>::bind_mut()` failed, already bound") ([storage/mod.rs](https://github.com/godot-rust/gdext/blob/v0.5.5/godot-core/src/storage/mod.rs)). If the Session emits a Godot signal while holding `&mut self`, and a GDScript handler calls back into the Session synchronously, the call panics. Return events in the output bytes, or emit signals after the borrow ends, for example with `call_deferred`.
- **Hot reload.** `reloadable = true` reloads the library when the editor regains focus ([book: Hello World](https://godot-rust.github.io/book/intro/hello-world.html)). **UNCONFIRMED:** what happens to live Session instances and their `Lua` state across a reload. Plan for sessions to be recreated.
- **Determinism and floats.** `double-precision` switches `real` to `f64` and needs a Godot build with `precision=double` ([docs.rs crate docs](https://docs.rs/godot/latest/godot/)). This is irrelevant if the protocol carries no Godot math types, which is another reason to keep it to plain bytes.
- **Lua limits (mlua latest).** `set_memory_limit` makes allocations past a byte limit fail with `Error::MemoryError`, and `set_hook` with `every_nth_instruction` can bound execution ([docs.rs mlua Lua](https://docs.rs/mlua/latest/mlua/struct.Lua.html)). Both are useful if the GUI can load untrusted card scripts. **UNCONFIRMED** whether 0.9 has the same APIs.
