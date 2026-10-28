# Map: Host protocol

## Destination

A `crates/host` Session that talks only in bytes. A full game for any number of players (tested with two) plays end to end through it, with a bot answering every Decision. A thin `crates/godot` adapter loads in Godot and steps a game. The engine core stays independent of any UI. A real Godot UI is not part of this map.

## Notes

- Execution happens inside this map. Decision tickets come first. Each build ticket is a `phase5/<name>` slice that follows the CLAUDE.md slice workflow: a requirements report first, approval before every commit, a closing report, and the user merges.
- Build tickets graduate from Not yet specified only after the decisions they depend on are closed.
- Use the vocabulary in `CONTEXT.md` (Session, Seat, Decision, Answer). Grilling tickets use the `grilling` and `domain-modeling` skills. Update `CONTEXT.md` as terms settle.
- The protocol supports any number of players from the start. Exactly one Decision is open at a time, answered in APNAP order, even for secret choices.
- Proof-game cards are a fixture set inside this repo, gathered from the inline Lua test scripts. CI must not depend on `../stackfish-db`.
- `.plan/` is local only (git-ignored), so it is not committed.

## Decisions so far

- 008 gdext constraints. gdext 0.5.5 supports Godot 4.2 to 4.7. Messages pass as `PackedByteArray`, and the adapter must enforce main-thread use itself or run the engine on its own thread. [closed/008-gdext-constraints.md]
- 001 How a Session advances. Answer runs to the next Decision and returns one batch of events. Advance, with an optional step budget, runs the engine while no Decision is open. Replay is a host that sends Advance with budget 1, not a Session feature. A rejected Answer changes nothing. A pipeline error is fatal. [closed/001-session-advance.md]
- 002 Decision and Answer shape. The engine owns a Decision that has a reply shape, an explicit candidate list, and a Session counter id. Answers are a tagged enum that the Session routes through a facade over the existing `submit_*` methods. Rejections carry a code and a message. Order and Target replies are deferred to 005. [closed/002-decision-answer-shape.md]
- 003 What a priority Decision offers. The Decision lists playable entries with their announcement options (mode range, target slots with candidates, X flag, alternative costs, buyback, mana ability flag) and no affordability check. Targets stay in the priority Answer and payment stays a separate Decision, which gains a "cannot pay" Answer. Combat Decisions list options the same way. [closed/003-priority-decision-content.md]
- 004 What each Seat may see. One visibility rule over `(Seat, event)` and `(Seat, state)`. Player Seats see public zones and their own hand. Spectators get an `omniscient` flag set by the host. Replay of a finished game is omniscient. Secret Answers stay hidden. Hidden object ids depend on 005. [closed/004-seat-visibility.md]
- 005 How object identity crosses the boundary. Seats receive engine `ObjectId`s, with no per-Seat handles and no stable card id. A move event carries the old id, the new id and the identity only where the Seat can see each one. Opponents see a hand as a count. Library ids never leave without the identity, and search lists are sorted by id. Candidates and Pick/Target Answers use `Object(ObjectId) | Player(PlayerId)`. Order Answers are candidate indices. [closed/005-object-identity.md]
- 007 Message envelope and wire format. Direct calls only, with no framing. JSON through `serde_json`, with serde in `crates/host` and mirror types there. Every message has `v`, `kind`, and `body`, and requests from a Seat add `seat`. The Session rejects a mismatched `v` at Setup. Answers echo their Decision id. `u64` values are decimal strings. Responses have one type and a `status`. Ten stable error codes. [closed/007-wire-format.md]
- 006 How a host starts a game. A CardSource handed to the Session at creation turns `oracle_id` UUIDs into `CardDefinition`s, with a carddb-backed and a fixture implementation. The setup message holds a player list in turn order (the host picks who goes first), a decklist and optional commanders per player, a format preset with optional setting overrides, and a seed. The Session validates structure only. Mulligans are Decisions after the first Advance. [closed/006-game-setup.md]

- 009 Where the Godot adapter runs the Session. The Session lives on Godot's main thread, with no engine thread or channels. Each `#[func]` checks the creating thread, and a wrong-thread call returns an empty `PackedByteArray` and logs an error. One Godot class holds one Session. The adapter adds no time limit, so the host picks the Advance budget per frame. [closed/009-adapter-threading.md]
- 010 Build B1 Decision/Answer facade. `Engine::open_decision` and `Engine::answer` in `crates/engine/src/decision/` route every pause through five reply-shape checks and five rejection codes. Priority entries wait for B2, and `submit_action` still accepts illegal priority actions. [closed/010-decision-answer-facade.md]
- 011 Build B2 Engine::legal_actions(). `Engine::legal_actions`, `legal_attackers`, `legal_blockers` and `pending_damage_splits` list entries without an affordability check. `open_decision` offers typed `DeclareAttackers`, `DeclareBlockers` and `AssignDamage` Decisions ahead of priority. `answer` rejects any priority or combat Answer that no entry offers. Menace, must-attack and mode-combination rules still fail during `step()`. [closed/011-legal-actions.md]
- 012 Build B3 crates/host Session. `Session::create` and `Session::handle` speak JSON bytes over one Seat per player, with a `CardSource`, standard and commander presets, a `step_cap` setup field and mirror types for every event, shape and priority action. Events reach every Seat unfiltered until B4. [closed/012-host-session.md]
- 013 Build B4 per-Seat visibility. Each Seat has an outbox, a response carries the requesting Seat's events, and `poll` collects missed ones. One rule in `crates/host/src/view.rs` filters events and Decisions, with spectator Seats, a `Revealed` event and sorted search lists. The state snapshot, `Revealed` producers and a `v` bump are open. [closed/013-seat-visibility.md]
- 014 Build B5 proof game. `proof_cards()` (25 fixture cards), a byte-only random `Bot` and full-game tests in `crates/host`. 30 two-player games, three-player games and a 500-seed run finish with no rejected Answer, and the same seed gives the same bytes. The bot found three engine bugs, fixed in the same slice: library creatures attacking, `HashMap`-ordered deaths and a `script_error` on plain spells. [closed/014-proof-game-bot.md]
- 015 Build B6 Godot adapter. `crates/godot` has `StackfishSession` over the Session, a main-thread guard in a plain `Core` and a headless Godot smoke test that plays a proof game. gdext's own safeguards refuse a wrong-thread call first, so GDScript gets `null` and not the empty array from 009, unless the check is compiled out. Dropping off the main thread did not crash. [closed/015-godot-adapter.md]

## Not yet specified

- Carddb-backed CardSource: a thin wrapper over `CardDb::load_by_oracle_id` for `../stackfish-db/stackfish.db`. Its shape is settled in 006. The Godot adapter's `create` needs it to serve real cards.
- Wrong-thread behavior in the Godot adapter: tell hosts to expect `null`, or compile gdext's check out of release builds (found by 015).
- Engine atomicity on a failed command. Decided in ticket 024 (see `map-fixes.md`): validate before mutate, so a rejected input changes nothing. The build work is tracked there as 025, 035, 026 and 036.

- Audit of unordered `HashMap` walks over game state that can reach a host (found by 014, which fixed three). The 500-seed proof run found no more, but the proof set covers only part of the engine.

- CR 104.4b infinite-loop detection. The step budget from 001 only stops a runaway from freezing the host.

## Out of scope

- A playable Godot UI (scenes, card art, input handling). This is a separate effort after this map.
- Netcode, matchmaking and lobbies. These belong to the host application, per DESIGN.md.
- Answering several Decisions in parallel (for example, simultaneous mulligans). Ruled out in favour of one Decision at a time.

## Frontier

Open build tickets, unblocked and unclaimed:

- none

Blocked:

- none
