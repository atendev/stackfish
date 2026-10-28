# Map: Host protocol fixes

## Destination

Every item left open by the closed Host protocol tickets (001 to 015) is either fixed, decided against, or recorded as out of scope. After that, a host can create a Session from a real card database, join or replay a game from a snapshot, and trust that a bad Answer cannot corrupt the engine.

## Notes

- This map follows `map.md` (Host protocol). That map is complete. This one collects its leftovers, found by reading all 15 closed tickets on 2026-10-17.
- Ticket numbers continue from 016. No ticket files exist yet. Create each file in `tickets/` from `TICKET.template.md` when it is picked up.
- Build tickets are `phase5/<name>` slices that follow the CLAUDE.md slice workflow: a requirements report first, approval before every commit, a closing report, and the user merges.
- Use the vocabulary in `CONTEXT.md` (Session, Seat, Decision, Answer).
- When a fix changes the Lua API, check the game stub in `docs/`.
- "Origin" names the closed ticket that left the item open.

## Decisions so far

- 016 Save the gdext research file. The file is on `master` as 6e8401d and the 008 pointer is fixed. [closed/016-save-gdext-research.md]
- 018 Wrong-thread behavior in the Godot adapter. gdext's safeguards stay on, so a wrong-thread call gives `null` and a script error. 009 is not rewritten and points to 018. The smoke test pins `null` by default. [closed/018-wrong-thread-behavior.md]

- 024 How the engine recovers from a failed command. A rejected input changes nothing, and the engine keeps that by validating before it mutates, not by snapshot and restore. Script errors stay "log and continue", and an engine bug stays fatal. Build work is 025, 035, 026 and 036, in that order. [closed/024-engine-atomicity.md]
- 019 Carddb-backed CardSource. A thin wrapper over `stackfish_carddb::CardDb::load_by_oracle_id` in `crates/host/src/carddb_source.rs`, with a `create_with_db` entry point in the Godot adapter. CI stays independent of `../stackfish-db` by using in-memory fixture DBs in tests. [closed/019-carddb-card-source.md]
- 037 Real-card proof bot. `crates/host/tests/real_card_bot.rs` curates two 40-card decks from `../stackfish-db/stackfish.db` and runs the random bot through the host protocol. It skips when the DB is absent. [closed/037-real-card-proof-bot.md]

## Not yet specified

- Wire `v` bump policy. 013 changed `zone_change.object`, which 007 calls breaking, and left `v` at 1. Ticket 020 decides whether to bump now or to treat it as pre-release.
- Whether the Lua API needs changes for any item below. Check at each requirements report.

## Tickets

Ticket types and modes follow `README.md`.

### A. Housekeeping

- 016 Save the gdext research file. Type task, HITL. Origin 008.
  - `docs/research/gdext-constraints.md` exists only as an untracked file in the worktree `.claude/worktrees/agent-ac916927a9e12b438`, on branch `research/gdext-constraints`. It is not on master. Commit it (with permission) and fix the pointer in `closed/008-gdext-constraints.md`.
- 017 Reconcile stale notes and policy conflicts. Type task, HITL. Origin 007, 009, 010, 011, 001.
  - `map.md` says `.plan/` is git-ignored. `README.md` and the global instructions say to commit it. Pick one.
  - 010 and 011 say "Not pushed yet", but `master` matches `origin/master`. The feature branches for 010 to 015 exist neither locally nor on origin, so confirm that is intended.
  - 009 says the drop-off-main-thread behavior is unconfirmed and that creating the object off-thread works. 015 found it safe to drop and found that creation aborts Godot with the safeguards on.
  - 007's rejected-Answer response shape and `seat`-less `setup` request were derived and never confirmed. Confirm them with the user.
  - DESIGN.md says "one ledger entry" where it should say "one state transition" (001). Its snapshot question stays open until 020.

### B. Decisions

- 018 Wrong-thread behavior in the Godot adapter. Type grilling, HITL. Origin 009, 015.
  - 009 promised an empty `PackedByteArray`. With gdext's safeguards on, GDScript gets `null` and a script error instead. Decide whether to document `null`, or compile the check out of release builds with `godot/safeguards-release-disengaged`. This is the only open item that contradicts a closed decision.
- 020 Late join, replay and wire versioning. Type grilling, HITL. Origin 001, 006, 012, 013, 015.
  - Shape of the `(Seat, state)` snapshot. 004 requires it to equal the fold of that Seat's events.
  - Shape of the replay export: seed, setup and accepted Answers in order.
  - How to record the card data version, so a replay resolves the same cards. 006 left this open.
  - Whether to bump `v` for the `zone_change.object` change.
  - A bound or a drain rule for the per-Seat outbox, which grows without limit for a Seat that never polls.
- 024 How the engine recovers from a failed command. Type grilling, HITL. Origin 001, 002, 010, 011.
  - `ProcessState::Error` leaves partial mutation. 001 left rollback to this item, and 002 said to revisit it. Choose between rolling back state, validating the whole command before it mutates anything, or leaving it fatal. Menace, must-attack and mode-combination rules depend on the answer.

### C. Build

- 019 Carddb-backed CardSource. Type task, HITL. Origin 006, 015.
  - A thin wrapper over `CardDb::load_by_oracle_id` for `../stackfish-db/stackfish.db`. 006 says it needs no new decision. The Godot `create` serves only the proof cards until it exists. CI must still not depend on `../stackfish-db`.
- 021 Seat state snapshot. Type task, HITL. Blocked by 020. Origin 013, 015.
- 022 Replay export and card data version. Type task, HITL. Blocked by 020. Origin 001, 006, 012.
- 023 Wire version bump and outbox bound. Type task, HITL. Blocked by 020. Origin 013.
- 025 Atomicity harness and audit. Type task, HITL. Origin 001, 010. Decided in 024.
  - Add test-only `Clone` and `PartialEq` on the `GameState` type graph (fallback: a state fingerprint) and a harness that feeds illegal inputs and asserts state and ledger are unchanged.
  - Audit every `execute_command` branch and every `submit_*` method for mutate-then-`Err`, and fix what the audit finds.
  - Update the "no rollback" doc comments on `StepStatus::Error` and in `engine/mod.rs`.
- 035 Error split and Session mapping. Type task, HITL. Blocked by 025. Origin 001, 024.
  - Split `CommandError` into `Rejected` (buildable only in the validation phase) and `Internal`.
  - The Session answers `Rejected` with a rejection response and continues. It treats `Internal` as fatal.
  - `Session` catches a panic, marks itself dead and reports a stable error code.
  - `submit_action` and the other `submit_*` methods validate on their own, so the result does not depend on the caller.
- 026 Whole-declaration combat predicates. Type task, HITL. Blocked by 035. Origin 011. Decided in 024.
  - Menace and must attack, as one predicate shared by `legal_attackers`, `legal_blockers` and command validation.
  - The check runs before the command mutates anything.
- 036 Mode-combination predicate. Type task, HITL. Blocked by 035. Origin 011. Decided in 024.
  - An impossible mode pair (`combine_chosen_modes` cannot combine it) is rejected at announcement and not listed in the modal entries.
  - Shares the predicate between the entries and the cast validation.
- 027 Revealed and face-down producers, and the event-writing rule. Type task, HITL. Origin 004, 013.
  - The engine never emits `Revealed` or `face_down`.
  - Write the rule "never put a hidden identity in a free-text event field" into the engine's event-writing guidelines (`ScriptError`, `StateBasedActionApplied`, `RngDrawn`).
- 028 Audit unordered `HashMap` walks over game state. Type task, HITL. Origin 014.
  - 014 fixed three. The 500-seed run found no more, but the proof cards cover only part of the engine.
- 029 CR 104.4b infinite-loop detection. Type task, HITL. Origin 001.
  - The step budget only stops a runaway from freezing the host. The requirements report must choose how a mandatory loop is detected.
- 030 Bot coverage. Type task, HITL. Origin 014.
  - The bot declines `may_cast` Decisions. It cannot pay sacrifice, discard, tap or graveyard-exile costs, or hybrid, Phyrexian, snow and `{X}` costs.
- 031 Remaining `legal_actions` gaps. Type task, HITL. Origin 010, 011.
  - Target candidates ignore X.
  - `DeclareBands` is not offered.
  - Timing grants from continuous effects are missing.
  - Nested mana abilities during a Payment Decision.
  - `Target::Ability` candidates for script picks.
- 032 Host rejection text and payment cost filters. Type task, HITL. Origin 010, 012.
- 033 Run the Godot smoke test in CI. Type task, HITL. Origin 015.
  - It needs a Godot binary. Also covers export and web builds, which are untouched.

### D. Research

- 034 Unconfirmed gdext items. Type research, AFK. Origin 008, 015.
  - mlua on the web/emscripten target.
  - How a `#[func]` `Result` appears in GDScript.
  - What hot reload does to live Lua state.

## Out of scope

- A playable Godot UI. This stays a separate effort, as in `map.md`.
- Netcode, matchmaking and lobbies.
- Answering several Decisions in parallel.
- New card mechanics. Those follow their own `phase5/<feature-name>` slices.

## Frontier

Open tickets, unblocked and unclaimed. Suggested first picks are 019, 020 and 025. 020 unblocks 021 to 023, and 025 starts the atomicity chain.

- 017, 020, 027, 028, 029, 030, 031, 032, 033, 034

Blocked:

- 021, 022, 023 wait on 020.

In review, claimed: 026 on `phase5/combat-predicates` and 036 on `phase5/mode-combination`. 025 and 035 are built (`phase5/atomicity-harness`, `phase5/error-split`).
