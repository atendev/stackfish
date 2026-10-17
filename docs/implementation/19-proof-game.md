# 19-proof-game

B5 of the host protocol map (`phase5/proof-game-bot`). A random bot plays whole games through the byte interface of
`crates/host`, using a fixture set of 25 cards. The design is in `.plan/closed/014-proof-game-bot.md`.

## API

- `stackfish_host::proof_cards() -> FixtureCardSource` serves the 25 cards by slug key (`grizzly_bear`, `shock`, ...).
- `stackfish_host::proof_setup_request(players, seed) -> Vec<u8>` is a `setup` request for the standard preset. Player
  `i` gets the red-green deck when `i` is even and the blue-black deck when it is odd, 40 cards each.
- `stackfish_host::Bot` is a client of a `Session`. `Bot::new(seed).play(&mut session, max_answers)` plays to
  `game_over` and returns an `Outcome` (`winner`, `answers`, `rejected`, `transcript_hash`). `step` answers one
  Decision, which is what a host that runs a game across frames would call.
- `Bot::recording(&[spectator seats])` keeps every event each Seat receives (`events(seat)`). `keeping_transcript()`
  keeps every request and response (`transcript()`).

## The card set

Four basic lands with a `{T}` mana ability, eight creatures, and thirteen other cards. All scripts are inline Lua that
uses documented primitives, except Fork in the Road, whose modes are declared in Rust because a card script cannot
declare modes.

- Creatures: Grizzly Bear, Stone Giant (vanilla), Wind Drake (flying), Cinder Duelist (first strike), Scholar Imp
  (enters, draw), Grave Rat (dies, gain 3), Ember Pinger (`{T}`: 1 damage to a creature), Mirror Sage (enters, may
  draw).
- Instants and sorceries: Shock, Divination, Counterspell, Bounce, Fork in the Road (modal), Rampant Growth (search to
  the battlefield), Raise Militia (token), Peek (scry), Drain Touch, Doom Blade.
- Permanents: Giant Growth Aura, Spark Stone (mana artifact), Iron Blade (equipment with equip `{1}`).
- Every card carries an inline script. A card with no abilities gets `local s = {} return s`, because a `ByKey` script
  fails to load on every zone change (the engine does not resolve keys).

## The bot

- It reads a Decision as its chooser, builds a reply from that Decision's own candidates, and sends it. It finds the
  open Decision with `advance` (which shows the Decision as a non-chooser sees it), then `poll`s the chooser's Seat.
- Priority: a land play has weight 12 and a pass has weight 6, or 2 while a card is within reach, or 1 while a card is
  affordable. It learns each card's mana cost from the identity in the move event that put the card in its hand, tracks
  its pool from `mana_added` events (emptied on every `step_started` and `phase_started`), casts a card the pool
  covers, and taps mana sources when a card needs more mana than the pool holds.
- Payment: mana first, then the additional costs in the listed order. `tap_self`, `sacrifice_self` and `discard_self`
  name the source of the last ability the bot activated. Any other cost, and any hybrid, Phyrexian, snow or `{X}`
  cost, gets "cannot pay".
- Combat: each attacker attacks with probability 1/2 at a random defender, each blocker blocks with probability 1/3, and
  a creature gives all its damage to its first recipient.
- A rejected Answer is replaced once by a fallback that is always legal (pass priority, or "cannot pay") and counted
  in `Outcome::rejected`. Any other rejection, a pipeline error, a Decision id that does not go up, and an answer
  count over the limit are errors.

## Engine bugs the bot found

- Creatures outside the battlefield were listed as attackers and blockers, and planeswalkers and battles outside it as
  defenders. A creature still in a library fought and dealt damage. `attacker_can_attack`, `block_is_legal` and
  `attack_target_is_legal` now check the zone (CR 508.1a, 509.1a, 508.1b).
- Creatures that die together left the battlefield in `HashMap` order, so they took their new ids in a different order
  on each run and the same seed gave different transcripts. `collect_dying_creatures` now sorts by id, and so do the
  two target-candidate builders (`legal_target_candidates`, `legal_aura_entry_candidates`).
- A script without `register_abilities` (every plain spell, per `docs/lua/card-script-basics.md`) raised a
  `script_error` event on each zone change. The runtime now treats a missing function as "registers nothing". A
  `register_abilities` that is present but not a function is still an error.

## Tests

`crates/host/tests/phase5_proof_game.rs` (5, plus 1 ignored):

- 30 two-player games reach `game_over` with no rejected Answer and no `script_error`, and both players win some.
- The 30 games cast all 21 non-land cards and play all four lands.
- Eight seeds each give byte-identical transcripts on two runs.
- Three-player games finish.
- No Seat receives a library id or an opponent's hand id over a whole game (the scan skips `power`, `toughness`,
  `loyalty` and `defense`, which are digit strings too).
- Ignored: 500 seeds (`cargo test --release -p stackfish-host --test phase5_proof_game -- --ignored`).

Engine regressions: `phase5_legal_actions.rs` (3 zone tests), `phase5_sba_basics.rs` (one test, `creatures_that_die_together_leave_in_id_order`), `phase5_operation_only_script.rs` (2).

## Open items

- TODO(proof bot): the bot never accepts a `may_cast` Decision (madness, cascade), because the Decision does not list
  the cast's modes or targets.
- TODO(proof bot): it cannot pay a sacrifice, discard, tap or graveyard-exile cost, because a Payment Decision does not
  list the eligible objects. The proof set has no such card.
- TODO(proof bot): a pawprint mode budget (CR 700.2i) is read as a plain mode count, and attackers and blockers are
  chosen at random with no regard for the board.
- The bot gives "cannot pay" for hybrid, Phyrexian, snow and `{X}` costs.
- Other `HashMap` walks over `state.objects` may still decide an order that reaches a host. The 500-seed run found
  none, but the proof set covers only part of the engine.
