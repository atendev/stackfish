---
id: 067
title: Decide whether CI gets read access to stackfish-db and a strict missing-db mode
type: grilling
mode: HITL
status: closed
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

Deferred. CI does not check out `stackfish-db`, and there is no strict mode yet. The smoke test already runs on every local `tools/check.sh --full`, 059 accepted the CI skip as a known gap, and CI runs only on pushes to `master`. `stackfish.db` is 620,769,280 bytes, so each uncached run would pull about 0.58 GiB of LFS against the 10 GiB a month free quota. With a $0 budget, running over blocks LFS for the whole account until the next month, including local work on `stackfish-db`.

If this is reversed, it is one build ticket:
- Use a read-only deploy key on `stackfish-db`, stored as an Actions secret in `stackfish`. It has no expiry and reaches only that repo.
- Check out into the workspace with `path: stackfish-db` and `lfs: true`. `path:` resolves under `GITHUB_WORKSPACE`, so the sibling location the test hardcodes in `stackfish_db_dir()` is out of reach.
- Make `stackfish_db_dir()` honor `STACKFISH_DB_DIR`, which `tools/check.sh:32` already reads.
- Add `STACKFISH_DB_REQUIRED=1`, set in CI only when the secret exists. It turns the skips in `db_scripts_smoke.rs` and the `TODO` at `tools/check.sh:44` into failures.
- Cache `.git/lfs` between runs to save bandwidth.
- Write a `wizard.sh` for the key and secret setup.

Strict mode ships with that build, not alone, because without a CI checkout nothing would set it. Answered by a subagent from the code and the GitHub docs.
