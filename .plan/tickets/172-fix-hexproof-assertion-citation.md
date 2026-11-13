---
id: 172
title: Fix the CR 702.16 citation in a hexproof test assertion
type: task
mode: AFK
status: open
blocked_by: []
claimed: false
---

## Question

The assertion message near `crates/engine/tests/hexproof_shroud.rs:872` cites CR 702.16d where 702.16c applies. Fix the string.

## Notes

- Found by 171. That ticket was comment-only, so it left this string alone because changing it is a code change.
- Check the line against `docs/comp_rules.md` before editing. Run `tools/check.sh --full`.

## Resolution

<Filled in at close.>
