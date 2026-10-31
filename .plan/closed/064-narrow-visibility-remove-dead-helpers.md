---
id: 064
title: Narrow pub visibility and remove dead helpers
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Should `EngineCommand`, `CommandOutcome` and `ProcessState` stay nameable outside `stackfish-engine`, and once that is settled, which of the ~424 `pub` items and 15 dead or test-only helpers listed in 055 get narrowed or removed?

## Notes

- Split from 057. The visibility half of the original 057 needs the nameability decision first, as 055 recorded.
- 055 counts: about 424 items and fields can narrow to `pub(crate)` (engine 85, scenario 137, host 187), 9 items are dead (engine 5 plus the `invoke_cost` stub, host 3), and 6 more are test-only (engine 5, host 1).
- After the decision, the narrowing and removal is a build slice for a new ticket.

## Resolution

`EngineCommand`, `CommandOutcome` and `ProcessState` stop being nameable outside `stackfish-engine`. They narrow to `pub(crate)` together with `commands::execute_command` (the only public signature that leaked them), `CommandError`, `PriorityHandoff`, `PendingPaymentRequest` and `PaymentResolution`. `commands` stays public for `Rejected`, `Internal`, `AttachProposal` and `propose_attach`.

The other items narrow only when the compiler proves it. Engine items use 055's scratch-copy method, regenerated at build time. Host, godot and carddb items use rustc's `unreachable_pub` lint. Scenario `schema.rs` stays `pub` because it is the documented TOML format. Fields on types that stay public or serde-shaped stay `pub`. `private_interfaces` is fixed by narrowing the leaking function, never by `#[allow]`. There is no curated engine facade.

Delete: `RejectionCode::as_str`, `enum Keyword`, `NeedsOrderingChoice.is_destroy`, `ScriptContext.chosen_targets`, `StepResult.events_added`, `Bot::keeping_transcript` and `transcript`, `is_permanent`, `ZoneContents::top`. Keep: `invoke_cost` (TODO), `Bot::with_starting_life` (documented 40-life override), `Engine::fatal_error` and `Session::internal_error` (diagnostic accessors), and `AwaitingAttachChoice` (documented dormant machinery, held with `#[cfg_attr(not(test), expect(dead_code, reason = ...))]`). `script_api` becomes a private `mod`. Items that narrowing newly exposes as dead are deleted if they have no TODO, no doc marking them intentional and no non-test caller; otherwise they are kept with `expect`. No `TODO` is deleted.

`unreachable_pub = "warn"` becomes a workspace lint as a last commit, for host, godot, carddb and scenario. The engine is exempt because its modules are public.

The build is one slice on `phase5/narrow-visibility`, built in 065.
