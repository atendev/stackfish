---
id: 069
title: Move duplicated engine test helpers into stackfish_testutil
type: task
mode: HITL
status: open
blocked_by: [060]
claimed: false
---

## Question

Pull the near-identical setup helpers (`creature_definition`, `vanilla_creature`, `bolt_definition`, `base_state` and similar) that each protection and targeting test file declares into `stackfish_testutil`, without changing what any test asserts.

## Notes

- Optional. Found by 060.
- Clusters: `crates/engine/tests/protection_from_*.rs` (about 50 tests) and `crates/engine/tests/targeting_*.rs`. `layer_engine_*`, `modal_*` and `script_ops_*` may share helpers too.
- Only move a helper when the copies are actually the same. Keep test-specific variants local.

## Resolution

<Filled in at close: the decision and the reason for it.>
