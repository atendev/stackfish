---
id: 033
title: Run the Godot smoke test in CI
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Run the headless Godot smoke test in CI, and decide what export and web builds need.

## Notes

- Origin 015.
- CI needs a Godot binary.
- Export (copy step into `res://`, macOS `.framework`) and web builds are untouched.

## Resolution

CI runs the smoke test. The `godot-smoke` job in `.github/workflows/ci.yml` runs on Godot 4.2.2 and 4.7.2. 4.2 is the adapter's `compatibility_minimum` and API level (`api-4-2`), and 4.2.2 is its last patch. 4.7.2 is the newest Godot that gdext 0.5.5 supports. The job downloads each official Linux build, checks the zip against Godot's published SHA-512, and runs `crates/godot/smoke/run.sh` twice: as shipped, and with `SAFEGUARDS_OFF=1`. All four combinations passed locally and in CI run 41501272207. `thread_owned` still aborts with safeguards on, and `run.sh` records that without failing.

Export and web builds are out of scope for this map (user decision). The destination needs the adapter to load in Godot and step a game, and nothing more. Shipping builds belong to the Godot UI effort, which is already out of scope. `TODO(export)` and `TODO(web)` stay in `docs/implementation/20-godot-adapter.md` and the `.gdextension`. Ticket 034 still answers whether mlua builds for emscripten.
