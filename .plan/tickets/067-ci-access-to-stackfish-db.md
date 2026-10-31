---
id: 067
title: Decide whether CI gets read access to stackfish-db and a strict missing-db mode
type: grilling
mode: HITL
status: open
blocked_by: [066]
claimed: false
---

## Question

Should CI check out the private, LFS-tracked `atendev/stackfish-db` (deploy key or token secret, `lfs: true`) so `db_scripts_smoke.rs` runs on real scripts, and should a strict mode (for example `STACKFISH_DB_REQUIRED=1`) turn its skip into a failure in CI?

## Notes

- Optional. 059 accepted the skip as a known gap for the first CI version.
- The secret setup is a human-only step and needs a `wizard.sh`.
- LFS bandwidth on a private repo counts against the account's GitHub quota.

## Resolution

<Filled in at close: the decision and the reason for it.>
