---
id: 066
title: Build the check script, pre-commit hook, toolchain pin and CI workflow
type: task
mode: HITL
status: open
blocked_by: [059]
claimed: false
---

## Question

Build the 059 decision: `tools/check.sh`, a versioned pre-commit hook with an installer, `rust-toolchain.toml` pinned to 1.98.0, and `.github/workflows/ci.yml` on push to `master`.

## Notes

- Read the 059 Resolution first. It fixes which checks run in the hook (fmt, clippy) and which run in CI (fmt, clippy, `cargo test --workspace`, the `tools` unittest).
- The installer copies only `pre-commit` into `.git/hooks/`. It must not set `core.hooksPath` or touch the LFS hooks (`post-checkout`, `post-commit`, `post-merge`, `pre-push`).
- `comment_diff.py` is not part of any automatic check.
- CI does not check out `stackfish-db`. The workflow should report that `db_scripts_smoke.rs` skipped.
- Update `CLAUDE.md` (a short "Checks" note, and the audit subagent runs `tools/check.sh`) and add an entry to `docs/implementation/index.md` naming the smoke test gap.
- Pushing the workflow to `master` is outward-facing. The merge step asks first.

## Resolution

<Filled in at close: the decision and the reason for it.>
