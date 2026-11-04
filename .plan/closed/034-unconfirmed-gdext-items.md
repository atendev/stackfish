---
id: 034
title: Unconfirmed gdext items
type: research
mode: AFK
status: closed
blocked_by: []
claimed: false
---

## Question

Confirm three gdext behaviors that [008](008-gdext-constraints.md) and [015](015-godot-adapter.md) left unchecked.

## Notes

- Origin: tickets 008 and 015 above. A subagent resolves it with the `research` skill.
- Does mlua build and run on the web/emscripten target?
- How does a `#[func]` returning `Result` appear in GDScript?
- What does hot reload do to live Lua state?

## Resolution

Findings are in `docs/research/gdext-unconfirmed.md`. Nothing was built: the machine has no `emcc` or nightly Rust.

- Web build: mlua with vendored Lua 5.4 is documented for `wasm32-unknown-emscripten` in the mlua 0.9.9 README, and its workflow defines an emscripten job (pass status not checked). For this repo (mlua 0.9.9, `lua-src` 547, Rust 1.98.0) it is unknown, and a newer Rust broke the emscripten link for mlua 0.11.6 (mlua#690). gdext web export is experimental: nightly with `-Zbuild-std`, `api-custom` instead of `api-4-2`, a Godot template built with `dlink_enabled=yes`, and a panic aborts the page. One spike build would settle it. No follow-up ticket is filed: the destination does not need it, and web export is already out of scope ([closed/033-godot-smoke-test-ci.md](033-godot-smoke-test-ci.md)).
- `#[func]` returning `Result` (gdext 0.5.2 and later): the error type must implement `ErrorToGodot`. `Unexpected` logs an error and fails the call with code 1337. Per the gdext docs (not reproduced here), a typed call or a release build continues with the default value, and an untyped call in a debug or editor build aborts the calling GDScript function. `()` gives `null` and `global::Error` gives the Godot error enum, both without a log. The error envelope in the returned bytes stays the adapter's error path.
- Hot reload: editor only. Godot drops the Rust instance and rebuilds it from `init()`, so the Session and its Lua state are gone. `handle` returns an empty array and `last_error()` is `not_created` until the host calls `create` again. Exported games never reload, and F5 runs most likely do not (inference). Keep `reloadable = false` in the smoke project and shipped builds.
- Still unconfirmed: whether lua-src 547 links on Rust 1.98.0 with the book's flags, whether Lua's C++ exceptions work inside a side module, whether `-Z link-native-libraries=no` drops the static Lua link, whether Godot 4.2 reloads this crate cleanly, and whether allocations can outlive an unloaded library. The Godot engine C++ claims and the godot-rust book pages were read by the research subagent and not re-fetched during review.
