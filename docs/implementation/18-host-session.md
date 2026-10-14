# 18-host-session

`crates/host` (`stackfish-host`) is the layer a host application talks to. A `Session` wraps one `Engine` and speaks
JSON bytes. The design is in `.plan/closed/001`, `002`, `005`, `006` and `007`.

## API

- `Session::create(request: &[u8], cards: &dyn CardSource) -> Result<(Session, Vec<u8>), Vec<u8>>` parses a `setup`
  request. `Ok` holds the Session and the `created` response. `Err` holds an error response. Nothing runs at
  creation, and the Session does not keep `cards`.
- `Session::handle(&mut self, request: &[u8]) -> Vec<u8>` takes an `advance`, `answer` or `poll` request and returns
  one response.
- `CardSource::load(key) -> Result<CardDefinition, CardSourceError>`. `FixtureCardSource` serves cards from a map built
  in code.

## Module map

- `setup.rs`: `SetupBody`, the two format presets, `SettingOverrides`, and `build`, which turns a body into a
  `GameConfig`. The source is asked once per distinct key. Library order is the decklist order.
- `envelope.rs`: `Request`, `Response`, `ErrorCode`, `PROTOCOL_VERSION`.
- `session.rs`: the dispatch, the step loop, the per-Seat outboxes and the fatal state.
- `view.rs`: the visibility rule (B4). `Viewer`, `Observer` (reads each ledger entry once into an `Observed` that
  knows every viewer's copy) and `decision_for`.
- `wire/`: mirror types. `event.rs` has one variant per `GameEventKind`, with no wildcard arm, so a new engine event
  fails to compile until it is mirrored. `decision.rs` mirrors the twelve `ReplyShape`s and the `Entry` types.
  `reply.rs` parses Answers. `mod.rs` holds `U64` (a decimal string), `WireTarget` and `WireCandidate`.

## Wire details settled in B3

- A request is `{v, kind, seat?, body}` with unknown fields rejected. `v` is checked on every request, not only at
  setup. Seat `i` answers for `PlayerId(i)`, and `setup` returns the seat ids. `answer` needs a seat. `advance`
  takes an optional one, which since B4 chooses whose events and view of the Decision the response carries.
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

## Per-Seat visibility (B4)

- Seats: player Seat `i` answers for `PlayerId(i)`. Spectator Seats follow, in the order of the setup's `spectators`
  list (`[{ "omniscient": bool }]`, default empty). `created` lists every Seat id. A spectator cannot answer
  (`not_your_decision`), and the omniscient flag exists only on spectators.
- Delivery: after every `engine.step()` and after an accepted Answer, the Session reads the new ledger entries and
  queues each Seat's copy in that Seat's outbox. A response carries the outbox of the request's `seat`. `advance`
  without a seat runs the game, returns no events and shows the Decision as a non-chooser sees it. `poll`
  (`{v, kind: "poll", seat, body: {}}`, seat required) drains a Seat's outbox and returns `decision_open` with that
  Seat's view, `still_running`, `game_over` or the fatal error. It also works after the game ends or halts.
- One rule (`view.rs`): the hand is visible to its owner, the library to nobody, every other zone to all, and an
  omniscient Seat sees everything. A face-down object's identity goes to its controller only (no engine producer sets
  `face_down` yet).
- `zone_change` per Seat: `object` (new id) and `previous` (old id) appear only where the Seat could see the object in
  that zone. `owner` appears on any move that touches a hand or library, so a Seat can keep counts. `identity`
  (printed characteristics, no id) appears when the Seat could tell the card on either side. `last_known_info` (the old
  object's full snapshot, with its old id) appears when the Seat could see the old object. The Session finds the old
  id, owner and identity by reading the state right after the step that wrote the event, because the engine event
  holds only the new id. If the lookup fails it hides rather than guesses.
- `cycled` and `madness_exiled` always carry `identity`. Their `last_known_info` goes only to a Seat that could see
  the hand.
- `trigger_queued` and `replacement_applied` drop `source` for a Seat that cannot see the source, including a card
  that has since left a hand (the Observer remembers the old hidden ids).
- `revealed` (new engine event `GameEventKind::Revealed { player, zone, cards }`): every Seat gets each card's
  `identity`. `object` appears only where the Seat could see the card. Nothing in the engine produces it yet.
- `cards_scried` and `cards_surveiled` go to the controller and omniscient Seats. `rng_drawn` goes to omniscient Seats
  only. `library_shuffled` is public.
- Decisions: the chooser and omniscient Seats get the full Decision plus `identities` (`[{object, identity}]`) for
  offered cards in a library or another player's hand (search and scry). Everyone else gets
  `{shape: "opaque", reply, count, candidates?}`. `candidates` lists the Pick and Targets options that lie in public
  zones. A refusal shows the Decision in the sender's view.
- Search Decisions list candidates sorted by `ObjectId` (`decision/open.rs`), never in library order.

## Engine changes

- `CounterSet::totals()` returns the total per counter kind, so the wire can list counters.
- The Decision id counter stays on `Engine`. The Session is its only caller.

## Open items

- TODO(snapshot): ticket 004's `(Seat, state)` view is not built, and no request asks for a snapshot. A Seat that
  joins late has no way to learn the current state. `view.rs` is written so a snapshot can reuse its rule.
- TODO: nothing in the engine writes a `Revealed` event, and nothing sets `face_down`, so both rules are only tested
  with constructed events.
- TODO(outbox): a Seat that never sends a request keeps its outbox growing. The Session never drops events.
- TODO: ids inside other events (`damage_dealt.source`, `became_target`, and so on) are assumed to lie in public zones.
  `no_seat_ever_receives_a_hidden_id` checks that over one bot game.
- TODO: ticket 004 asks for a rule in the engine's event-writing notes that free-text fields (`script_error`,
  `state_based_action_applied`) never name a hidden card.
- Replay export (the setup bytes and the accepted Answers in order, from 001) is not built.
- The carddb-backed `CardSource` is not built. It wraps `CardDb::load_by_oracle_id`.
- The Payment cost lists additional costs without their filters (`sacrifice`, `discard`, `tap_permanents`,
  `exile_from_graveyard`), because the engine does not list eligible objects for a Payment Decision.
- Rejection messages are the engine's `Debug` text, passed through.
- A pipeline error is fatal and the engine is not rolled back (engine atomicity item in `map.md`). A panic inside the
  engine is not caught. The Godot adapter (B6) has to decide what a panic does at the FFI boundary.
- CR 104.4b loop detection is not done. `step_cap` only stops a runaway from freezing the host.

## Tests

`crates/host/tests/phase5_seat_visibility.rs` (10) drives a bot game over four Seats (two players, a spectator and an
omniscient spectator): draws, a cast, the hidden-id scan, opaque Decisions, `poll`, and spectators that cannot answer.
Unit tests in `session.rs` (4) cover `revealed`, scry, surveil, `rng_drawn`, `cycled` and `trigger_queued`.
`crates/host/tests/phase5_host_session.rs` (11) drives a real two-player game in bytes: creation failures,
envelope and body rejection, the first priority Decision, mulligan order and the bottom-card choice, rejected
Answers that keep the same Decision, requests outside their window, game over as a draw, budget slicing against one
uncapped run, byte-identical transcripts for the same seed, and passing priority through the turn. Unit tests in
`setup.rs` (6) cover presets, overrides, library expansion and one source call per key. A unit test in `session.rs`
covers the fatal state.
