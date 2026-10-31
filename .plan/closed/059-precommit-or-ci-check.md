---
id: 059
title: Decide on a pre-commit or CI check for fmt, clippy and comment_diff.py
type: grilling
mode: HITL
status: closed
blocked_by: [057]
claimed: false
---

## Question

Should `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `python3 tools/comment_diff.py` run automatically, and if so, where (a git pre-commit hook, a GitHub Actions workflow, or both), on which triggers, and which of the three checks belong in each?

## Notes

- 057 left the workspace clean under `cargo fmt --all` and clippy with `-D warnings`, and `cargo test --workspace` passes.
- `comment_diff.py` compares the working tree with `HEAD` and only applies to comment-only passes, so it may not fit an always-on check.
- The repo has no `.github/workflows` today.
- After the decision, any hook or workflow is a build slice for a new ticket.

## Resolution

Run the checks automatically in two layers. A versioned pre-commit hook runs `cargo fmt --check` (0.7 s) and `cargo clippy --workspace --all-targets -- -D warnings` (2.7 s warm). An installer copies it into `.git/hooks/` and leaves `core.hooksPath` unset, so the Git LFS hooks keep running. A GitHub Actions workflow on push to `master` runs those two plus `cargo test --workspace` (79 s) and `python3 -m unittest discover -s tools`, because only `master` reaches origin. The tests stay out of the hook because they are too slow for every commit.

`comment_diff.py` stays a manual step after comment-only passes, as `CLAUDE.md` requires, because it fails on any code change. A `rust-toolchain.toml` pins 1.98.0 with `rustfmt` and `clippy` so a stable release with new lints cannot break CI under `-D warnings`. The pin moves only in its own commit.

CI does not check out `atendev/stackfish-db` for now. That repo is private and its `stackfish.db` is LFS-tracked, so CI would need a read token. `db_scripts_smoke.rs` skips in CI, and the docs record that as a known gap. A token and a strict mode where a missing db fails are deferred to 067.

One script, `tools/check.sh`, is the entry point for the hook, CI and the audit subagent. `CLAUDE.md` and `docs/implementation/index.md` get a short note. The build is 066.

Measured with rustc 1.98.0. The log showed no recurring fmt or clippy fixup commits, so the gate is cheap insurance, not a fix for a known problem.
