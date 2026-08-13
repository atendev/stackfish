# VSCode intellisense for card scripts

[← index](index.md)

The `Game` table card scripts use is a Rust/mlua global — no Lua tool knows its shape on its
own. [stubs/game.lua](stubs/game.lua) is a hand-written EmmyLua type stub (declarations only,
never executed) covering every `Game.*` entry from [game-constants.md](game-constants.md) and the
per-kind pages. It's wired up via [.luarc.json](/.luarc.json) at the repo root.

## Setup

1. Install the **Lua** extension (`sumneko.lua` / LuaLS) in VSCode.
2. Open the repo — `.luarc.json` is picked up automatically, no per-user config needed.
3. Open or create a `.lua` file anywhere in the repo and you get autocomplete/hover docs for
   `Game.CreateStaticAbility`, `Game.Conditions.*`, etc.

## The catch: scripts don't live in `.lua` files today

Card scripts are currently authored as Lua source embedded in Rust raw strings (`r#"..."#` in
test files) or TOML triple-quoted strings (`script = """..."""` in scenario files). LuaLS attaches
to `.lua` files, not to string literals inside another language, so there's no intellisense
in-place inside a `.rs` or `.toml` file.

Workaround: draft/edit the script body in a standalone scratch `.lua` file (anywhere under the
repo, so `.luarc.json` applies) to get full intellisense, then paste the finished body into the
Rust raw string or TOML block. Delete the scratch file when done, or keep a `scratch.lua` around
and just overwrite it each time — it's gitignored-worthy but not currently in `.gitignore`.

## Keeping the stub honest

The stub is hand-maintained, not generated. If you add a new `Game.*` primitive or change a
signature, update both the relevant `docs/lua/*.md` page and [stubs/game.lua](stubs/game.lua) in
the same change — they're supposed to agree, but nothing enforces that automatically.
