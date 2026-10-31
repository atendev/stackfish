---
id: 066
title: Build the check script, pre-commit hook, toolchain pin and CI workflow
type: task
mode: HITL
status: closed
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

Built on `phase5/check-script-hook-ci` (`94bfc01`) and merged to `master`. `tools/check.sh` runs fmt and clippy by default and adds `cargo test --workspace` and the `tools` unittests with `--full`. `tools/hooks/pre-commit` runs the fast mode. `tools/install-hooks.sh` copies only that hook, refuses when `core.hooksPath` is set, never overwrites a backup and leaves the LFS hooks alone. `rust-toolchain.toml` pins 1.98.0, and `.github/workflows/ci.yml` runs `check.sh --full` on push to `master` and on manual dispatch.

CI installs the toolchain with `rustup toolchain install`, not `rustup show`, because `rustup show` stopped installing in rustup 1.28. A later step asserts the version. `check.sh --full` prints a `::notice::` when `db_scripts_smoke.rs` will skip. It uses the same check as the test. `tools/test_check_sh.py` covers the script with fake `cargo` and `python3` programs, and the installer and hook in throwaway repos. The notes are in `CLAUDE.md` under Checks and in `docs/implementation/29-checks.md`. The first CI run happens when `master` is pushed.
