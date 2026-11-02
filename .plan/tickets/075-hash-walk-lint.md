---
id: 075
title: Decide a lint against hash-ordered walks
type: grilling
mode: HITL
status: open
blocked_by: [073]
claimed: false
---

## Question

Should the repo add a clippy rule against walking hash containers, and with which config?

## Notes

- Origin 028. Probe results (clippy 0.1.98, rustc 1.98.0): `clippy::iter_over_hash_type` catches `for x in &map` but no method form; `disallowed-methods` catches `.iter()`, `.values()`, `.keys()`, `.drain()`, `.retain()` but needs a new `clippy.toml`; neither catches `.into_iter()` method calls, `extend` or `from_iter`. `disallowed-types` flags every mention and so forces a decision at each declaration.
- After 073 about 8 order-irrelevant hash walks remain (`commands/combat/damage.rs`, `commands/combat/entries.rs`, `scenario/src/report.rs`); each would need an `#[allow]` with a reason.
- A new config file needs the user's approval. `tools/check.sh` lints tests too (`--all-targets -D warnings`).
- Use the `grilling` and `domain-modeling` skills.

## Resolution

<Filled in at close: the decision and the reason for it.>
