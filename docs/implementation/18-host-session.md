# 18-host-session

`crates/host` (`stackfish-host`) is the layer a host application talks to. A `Session` wraps one `Engine` and speaks
JSON bytes. The design is in `.plan/closed/001`, `002`, `005`, `006` and `007`.

## API

- `Session::create(request: &[u8], cards: &dyn CardSource) -> Result<(Session, Vec<u8>), Vec<u8>>` parses a `setup`
  request. `Ok` holds the Session and the `created` response. `Err` holds an error response. Nothing runs at
  creation, and the Session does not keep `cards`.
- `Session::handle(&mut self, request: &[u8]) -> Vec<u8>` takes an `advance` or `answer` request and returns one
  response.
- `CardSource::load(key) -> Result<CardDefinition, CardSourceError>`. `FixtureCardSource` serves cards from a map built
  in code.

## Module map

- `setup.rs`: `SetupBody`, the two format presets, `SettingOverrides`, and `build`, which turns a body into a
  `GameConfig`. The source is asked once per distinct key. Library order is the decklist order.
- `envelope.rs`: `Request`, `Response`, `ErrorCode`, `PROTOCOL_VERSION`.
- `session.rs`: the dispatch, the step loop and the fatal state.
- `wire/`: mirror types. `event.rs` has one variant per `GameEventKind`, with no wildcard arm, so a new engine event
  fails to compile until it is mirrored. `decision.rs` mirrors the twelve `ReplyShape`s and the `Entry` types.
  `reply.rs` parses Answers. `mod.rs` holds `U64` (a decimal string), `WireTarget` and `WireCandidate`.

## Wire details settled in B3

- A request is `{v, kind, seat?, body}` with unknown fields rejected. `v` is checked on every request, not only at
  setup. Seat `i` answers for `PlayerId(i)`, and `setup` returns the seat ids. `answer` needs a seat. `advance`
  takes an optional one, which is only checked.
- A response is `{v, kind: "response", body}`. The body has `status`, `events`, and optionally `decision`, `error`,
  `winner` (`game_over` only, `null` for a draw) and `seats` (`created` only).
- Any refusal that leaves a Decision open returns `decision_open` with that Decision and an empty `events` list. The
  rest return status `error`. Only `pipeline_error` is fatal.
- Engine rejection codes map to wire codes: `wrong_seat` to `not_your_decision`, `not_open` to `no_open_decision`,
  `stale_decision` to `stale_decision`, and `invalid_choice` and `wrong_answer_shape` to `invalid_answer`.
- `decision_pending` is an eleventh error code. 007 had none for an `advance` sent while a Decision is open.
- `budget` is a plain integer of at least 1. `step_cap` in setup (default 100000) bounds an `answer` and an `advance`
  without a budget.
- Replies are tagged by `kind`, matching the Decision's `shape`: `pick`, `yes_no`, `order`, `modes`, `partition`,
  `payment`, `targets`, `may_cast`, `action`, `declare_attackers`, `declare_blockers`, `assign_damage`. An `action`
  reply carries only the four priority actions, because every other `PlayerActionKind` has a shape of its own.
- `ZoneChange`, `Cycled` and `MadnessExiled` carry last known information as `WireObjectSummary`: printed
  characteristics from the definition plus tapped, token, commander and counters. Layer effects are not applied.
- Format presets: `standard` (20 life) and `commander` (40 life, 1 free mulligan, commander slots). Setup overrides
  replace individual `GameSettings` fields. `max_mulligans: null` means no limit.

## Engine changes

- `CounterSet::totals()` returns the total per counter kind, so the wire can list counters.
- The Decision id counter stays on `Engine`. The Session is its only caller.

## Open items

- Events go to every Seat unfiltered. TODO(B4): per-Seat visibility, applied where `Session::take_events` builds a
  response. `RngDrawn` and the identity of hidden cards are the first things it has to hide.
- Replay export (the setup bytes and the accepted Answers in order, from 001) is not built.
- The carddb-backed `CardSource` is not built. It wraps `CardDb::load_by_oracle_id`.
- The Payment cost lists additional costs without their filters (`sacrifice`, `discard`, `tap_permanents`,
  `exile_from_graveyard`), because the engine does not list eligible objects for a Payment Decision.
- Rejection messages are the engine's `Debug` text, passed through.
- A pipeline error is fatal and the engine is not rolled back (engine atomicity item in `map.md`). A panic inside the
  engine is not caught. The Godot adapter (B6) has to decide what a panic does at the FFI boundary.
- CR 104.4b loop detection is not done. `step_cap` only stops a runaway from freezing the host.
- Spectator Seats do not exist yet. B4 adds them with the visibility rule.

## Tests

`crates/host/tests/phase5_host_session.rs` (11) drives a real two-player game in bytes: creation failures,
envelope and body rejection, the first priority Decision, mulligan order and the bottom-card choice, rejected
Answers that keep the same Decision, requests outside their window, game over as a draw, budget slicing against one
uncapped run, byte-identical transcripts for the same seed, and passing priority through the turn. Unit tests in
`setup.rs` (6) cover presets, overrides, library expansion and one source call per key. A unit test in `session.rs`
covers the fatal state.
