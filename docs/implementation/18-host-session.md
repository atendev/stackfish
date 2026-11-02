# 18-host-session

`crates/host` (`stackfish-host`) is the layer a host application talks to. A `Session` wraps one `Engine` and speaks
JSON bytes. The design is in `.plan/closed/001`, `002`, `005`, `006` and `007`.

## API

- `Session::create(request: &[u8], cards: &dyn CardSource) -> Result<(Session, Vec<u8>), Vec<u8>>` parses a `setup`
  request. `Ok` holds the Session and the `created` response. `Err` holds an error response. Nothing runs at
  creation, and the Session does not keep `cards`.
- `Session::handle(&mut self, request: &[u8]) -> Vec<u8>` takes an `advance`, `answer`, `poll`, `snapshot` or
  `export_replay` request and returns one response.
- `Session::from_replay(replay: &[u8], cards: &dyn CardSource) -> Result<(Session, Vec<u8>), Vec<u8>>` loads an exported
  replay and plays every recorded Answer. `Replay::parse`, `verify`, `start` and `answer_request(i)` let a host step it.
- `CardSource::load(key) -> Result<CardDefinition, CardSourceError>` and `CardSource::fingerprint(key) -> Result<String,
  CardSourceError>`. `FixtureCardSource` serves cards from a map built in code. `CardDbCardSource` reads `CardDb`.
- `REPLAY_VERSION` (1) and `engine_build_id()`.

## Module map

- `setup.rs`: `SetupBody`, the two format presets, `SettingOverrides`, and `build`, which turns a body into a
  `GameConfig`. The source is asked once per distinct key. Library order is the decklist order.
- `envelope.rs`: `Request`, `Response`, `ErrorCode`, `PROTOCOL_VERSION`.
- `replay.rs`: `ReplayFile` (the `replay` field), `RecordedAnswer`, `Replay`, `REPLAY_VERSION`, `engine_build_id`.
- `fingerprint.rs`: the canonical card encoding and FNV-1a, shared with the bot's transcript hash.
  `CardDb::record_by_oracle_id` gives the raw row, sorted keywords and active script it hashes for a database card.
- `session.rs`: the dispatch, the step loop, the per-Seat outboxes and the fatal state.
- `view.rs`: the visibility rule (B4). `Viewer`, `Observer` (reads each ledger entry once into an `Observed` that
  knows every viewer's copy), `decision_for` and `snapshot_for`.
- `wire/`: mirror types. `event.rs` has one variant per `GameEventKind`, with no wildcard arm, so a new engine event
  fails to compile until it is mirrored. `decision.rs` mirrors the twelve `ReplyShape`s and the `Entry` types.
  `reply.rs` parses Answers. `snapshot.rs` holds the snapshot types. `mod.rs` holds `U64` (a decimal string), `WireTarget` and `WireCandidate`.

## Wire details settled in B3

- A request is `{v, kind, seat?, body}` with unknown fields rejected. `v` is checked on every request, not only at
  setup. Seat `i` answers for `PlayerId(i)`, and `setup` returns the seat ids. `answer` needs a seat. `advance`
  takes an optional one, which since B4 chooses whose events and view of the Decision the response carries.
- A response is `{v, kind: "response", body}`. The body has `status`, `events`, and optionally `decision`, `error`,
  `winner` (`game_over` only, `null` for a draw) and `seats` (`created` only).
- Any refusal that leaves a Decision open returns `decision_open` with that Decision and an empty `events` list. The
  rest return status `error`. Only `pipeline_error` and `engine_panic` are fatal.
- Engine rejection codes map to wire codes: `wrong_seat` to `not_your_decision`, `not_open` to `no_open_decision`,
  `stale_decision` to `stale_decision`, and `invalid_choice` and `wrong_answer_shape` to `invalid_answer`.
- `decision_pending` is an eleventh error code. 007 had none for an `advance` sent while a Decision is open.
- `budget` is a plain integer of at least 1. `step_cap` in setup (default 100000) bounds an `answer` and an `advance`
  without a budget. `outbox_cap` in setup (default 4096, at least 1) bounds each Seat's outbox, as described under
  "Outbox cap" below.
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

## Seat snapshot (ticket 021)

- `snapshot` (`{v, kind: "snapshot", seat, body: {}}`, seat required) returns the `poll` response for that Seat with
  an empty `events` list and a `snapshot` field. It drops the Seat's outbox, because the snapshot already holds what
  those events said, and the Seat resumes from events after `through_sequence_no`. It works after the game ends or
  halts, like `poll`. A late spectator is a spectator Seat declared at setup that has not polled yet.
- The snapshot holds `through_sequence_no` (`null` before the first event), `turn_number`, `active_player`, `phase`,
  `step`, `priority`, and `players` (life, counters, commander damage by name, `eliminated`, `hand_count`,
  `library_count`). `zones` lists every zone the Seat may look into in engine order, each object as `{object, owner,
  controller, tapped, face_down, counters, attached_to?, attached_to_player?, identity?}`. `identity` is absent for a
  face-down object the Seat may not identify. Hands and libraries are sorted by id, and only an omniscient Seat gets
  libraries. `stack` lists the
  entries, top last, with `kind`, `owner`, `controller`, `targets`, `chosen_modes`, `x_value`, the spell's `card` and a
  `source` that is absent where the Seat cannot see it. The open Decision is the response's `decision`, built by
  `decision_for`.
- The fold tests in `seat_visibility.rs` rebuild zones, counts, tapped, turn, phase and step from a first snapshot plus
  the Seat's events and compare them with a later snapshot, over a bear game for all four Seats and 400 answers of five
  proof games. Damage to a player is a `damage_dealt` event, not `life_changed`.
- The fold found event gaps, each a TODO in the engine: life gain (the `GainLife` effect, the script `GainLife` result
  and lifelink) writes no `life_changed`, poison from infect damage to a player writes no event, nothing writes
  `priority_set`, and CR 800.4a removes a leaving player's objects without events. The proof-game fold skips life and
  stops before an elimination until those land.
- TODO: a card a `revealed` event showed keeps its identity in the events but not in a later snapshot while it stays
  in a hand or library. The fold tests do not fold identities.

## Replay export (ticket 022)

- `export_replay` (`{v, kind: "export_replay", body: {}}`, no `seat`) returns status `exported` with a `replay` field:
  `{replay_v, engine_build, cards, setup, answers}`. `setup` is the setup request as a string, byte for byte.
  `cards` maps every key the setup names to its fingerprint, taken at creation, so a source that cannot fingerprint a
  key refuses the setup with `setup_invalid`. `answers` holds `{seat, decision, reply}` for each accepted Answer, the
  body fields as sent. Export works after the game ends or halts. A dead Session answers `engine_panic`.
- `Session::from_replay` checks `replay_v`, the build id and every fingerprint before any card loads, and names every
  key that differs. It then runs to each Decision with `step_cap` batches and applies the next Answer. A recorded
  Answer that does not fit (no Decision open, or the engine refuses it) gives `setup_invalid` naming its index. After
  the last Answer it runs on to the next Decision or the end, so a replay exported mid-run loads further ahead. It
  never drains an outbox and lifts `outbox_cap` while it plays, so every Seat can `poll` the whole game. The cap
  applies again from the first event after loading, and a Seat whose outbox is at or past the cap then lapses.
- A database fingerprint covers the `cards` columns, sorted keyword names and the active script's version and source.
  A fixture fingerprint covers the printed characteristics, mode choice and script source.
- TODO: an omniscient stream digest in the file, so a loader catches an Answer swapped for another legal one.
- TODO: the fixture fingerprint skips `effect`, `modes`, `alternative_costs`, `buyback_cost` and `target_requirement`.
- Determinism also rests on the `HashMap` walk audit (ticket 028) and on Lua `pairs` order, which is not seeded.

## Outbox cap and wire version (ticket 023)

- `outbox_cap` (setup, `u64`, default `DEFAULT_OUTBOX_CAP` = 4096, `0` refused with `setup_invalid`) counts events per
  Seat. The check runs when an event is queued (`Seat::queue`): an outbox already holding the cap is dropped whole,
  that event is not queued, and the Seat lapses. A Seat may hold exactly the cap without lapsing.
- A lapsed Seat gets nothing queued. Every response to a request naming it (`poll`, `advance`, `answer`, refusals) has
  empty `events` and `resync: true`, and still shows the open Decision. A lapsed player still answers as usual. Only
  `snapshot` clears the mark, and its response has no `resync`. `resync` is absent from every other response.
- The default came from full bot games over seeds 0 to 39, taking the largest outbox at the moment it was drained:
  58 events with 2 players, 77 with 4 in the commander preset, 108 with 6. Eliminated players' Seats were left out,
  since the bot never polls them and the cap is meant to lapse them. The rest of the headroom is for real cards and
  was not measured.
- The cap never changes the game: `outbox_cap.rs` replays a bot game's Answers under a cap of 1 and gets the same
  Answers, the same winner and the same snapshots.
- `v` stays 1 under the pre-release rule in 007: until the first tagged `crates/host` release or the first host outside
  this repository, breaking changes land without a bump. `v` is checked on every request.

## Engine changes

- `CounterSet::totals()` returns the total per counter kind, so the wire can list counters.
- The Decision id counter stays on `Engine`. The Session is its only caller.

## Open items

- `Revealed` producers (ticket 027): `Game.Ops.Search` with `reveal = true` (CR 701.23e) and both amplify answers
  (CR 702.38a), through `Engine::reveal_event`. The host tests still use constructed events.
- TODO: nothing sets `face_down`, because face-down mechanics are a design non-goal, so that rule is only tested with
  constructed objects.
- TODO: ids inside other events (`damage_dealt.source`, `became_target`, and so on) are assumed to lie in public zones.
  `no_seat_ever_receives_a_hidden_id` checks that over one bot game.
- The free-text rule (ticket 027) is in `DESIGN.md`, "Writing events". Every `script_error` message comes from
  `GameState::script_error_message`, which leaves out the id and detail of a library or hand source. Before this, a
  library card's broken script logged its id and Lua error text to every Seat at game start.
- A Payment cost's `sacrifice`, `discard`, `tap_permanents` and `exile_from_graveyard` entries carry `candidates`, the
  ids that may pay them. The filter itself is not sent. See `30-payment-candidates.md`.
- A `may_cast` Decision carries `options` (`modes`, `targets` in the cast-entry form, `x`) when a cast can be announced.
  `x: true` asks for X, and 0 is the only legal value (CR 107.3b).
- Rejection messages are the engine's `Debug` text, passed through.
- Ticket 035 (`22-error-split.md`): a refusal that gets past the facade is a normal `invalid_answer` and the Decision stays
  open. An engine `Internal` error halts the Session as `pipeline_error`, and a panic kills it as `engine_panic`. Both send a
  fixed message; the detail is in `Session::internal_error()`. `engine_panic` is a twelfth error code.
- CR 104.4b loop detection is not done. `step_cap` only stops a runaway from freezing the host.

## Tests

`crates/host/tests/seat_visibility.rs` (16) drives a bot game over four Seats (two players, a spectator and an
omniscient spectator): draws, a cast, the hidden-id scan, opaque Decisions, `poll`, spectators that cannot answer, and
the snapshot (fold equality, hidden ids, the dropped outbox and request errors).
`crates/host/tests/replay.rs` (13) replays bot games and compares an omniscient Seat's events, re-exports, loads with a
Decision open and mid-run, refuses changed cards, a changed card list, another build, a dropped Answer and a wrong
seat, checks refused Answers are not recorded, steps a started
replay, and pins the proof-game hash to `BEHAVIOUR_REV`.
Unit tests in `session.rs` (4) cover `revealed`, scry, surveil, `rng_drawn`, `cycled` and `trigger_queued`.
`crates/host/tests/phase5_host_session.rs` (11) drives a real two-player game in bytes: creation failures,
envelope and body rejection, the first priority Decision, mulligan order and the bottom-card choice, rejected
Answers that keep the same Decision, requests outside their window, game over as a draw, budget slicing against one
uncapped run, byte-identical transcripts for the same seed, and passing priority through the turn. Unit tests in
`setup.rs` (6) cover presets, overrides, library expansion and one source call per key. A unit test in `session.rs`
covers the fatal state.
