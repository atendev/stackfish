---
id: 008
title: gdext constraints
type: research
mode: AFK
status: closed
blocked_by: []
claimed: false
---

## Question

What does `gdext` (godot-rust) require of a Rust library that a Godot project loads? Specifically: how to pass byte arrays and strings in both directions; whether a `!Send` value such as an `mlua::Lua` state can live inside a Godot object and which threads may touch it; how to build and ship a GDExtension (`.gdextension` file, crate type) from a crate inside a Cargo workspace on Linux, Windows and macOS; and which Godot 4 versions current `gdext` supports.

## Notes

- Answer from primary sources (godot-rust book, gdext docs and repo, Godot docs).
- Record findings in a markdown file on a `research/gdext-constraints` branch, and put a pointer here.

## Resolution

Findings are in `docs/research/gdext-constraints.md` on `master` (commit 6e8401d, saved by ticket 016). The `research/gdext-constraints` branch and its worktree hold only the original copy.

- `godot` crate 0.5.5 supports Godot 4.2 to 4.7. It needs Rust 1.94 and edition 2024, which applies to the whole workspace.
- Pass messages as `PackedByteArray`. Reading costs no copy; handing bytes to Godot costs one copy. Strings are converted between UTF-8 and UTF-32 on every crossing.
- A `!Send` `mlua::Lua` can live inside a gdext class, but nothing stops a GDScript thread from calling into it. The adapter must enforce main-thread use itself, or run the engine on its own thread and pass `Vec<u8>` over channels. mlua's `send` feature is not needed for either design.
- Shipping needs a `cdylib` crate, a `.gdextension` file, a copy step from the workspace-root `target/` into the Godot project, and a universal `.framework` on macOS.
- Not confirmed: mlua on the web/emscripten target, how a `#[func]` `Result` appears in GDScript, and what hot reload does to live Lua state.
