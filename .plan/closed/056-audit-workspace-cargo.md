---
id: 056
title: Audit workspace Cargo.toml dependencies, lints, and formatting
type: research
mode: AFK
status: closed
blocked_by: []
claimed: false
---

## Question

What is the current state of workspace dependency hygiene, clippy warnings, and formatting drift across all crates in this workspace?

## Notes

- Skills: research, unslop. Follow the CLAUDE.md vertical slice workflow: `phase5/<name>` branch, requirements report, implementation, audit, commit with permission, push, watch CI, close ticket and update map.
- Read the workspace `Cargo.toml` and every crate `Cargo.toml`.
- Check `cargo fmt --check`, `cargo clippy --workspace`, and any unused-dependency tool available (`cargo machete`, `cargo udeps`).
- Record findings, not fixes: this is a research ticket unless the fixes are trivial.

## Resolution

- The workspace `Cargo.toml` has no `[workspace.dependencies]` section. Each crate pins its own versions.
- `cargo fmt --check` reports about 7,292 formatting diffs across many files. The codebase has not been run through `rustfmt`.
- `cargo clippy --workspace --all-targets -- -D warnings` reports 77 errors and 0 warnings, all in `stackfish-engine`. The most common lints are `collapsible_if` (46), `too_many_arguments` (8), `unnecessary_map_or` (4), `large_enum_variant` (4), `manual_contains` (3), `new_without_default` (2), and `doc_lazy_continuation` (2).
- `cargo tree --workspace --duplicates` shows only `bitflags` duplicated through transitive dependencies (`rusqlite` and `stackfish-engine`). There is no workspace-level redundant direct dependency.
- `cargo-machete` and `cargo-udeps` are not installed, so unused dependencies were not checked mechanically. A manual read of each crate's direct dependencies did not reveal an obvious unused crate.
- `cargo test --workspace` passes.

Next step: ticket 057 will centralize repeated dependencies (`serde`, `serde_json`, `rusqlite`), fix the clippy errors, run `rustfmt`, and install an unused-dependency checker to confirm the manual finding.
