# Map: Host protocol

## Destination

A `crates/host` Session that talks only in bytes. A full game for any number of players (tested with two) plays end to end through it, with a bot answering every Decision. A thin `crates/godot` adapter loads in Godot and steps a game. The engine core stays independent of any UI. A real Godot UI is not part of this map.

## Notes

- Execution happens inside this map. Decision tickets come first. Each build ticket is a `phase5/<name>` slice that follows the CLAUDE.md slice workflow: a requirements report first, approval before every commit, a closing report, and the user merges.
- Build tickets graduate from Not yet specified only after the decisions they depend on are closed.
- Use the vocabulary in `CONTEXT.md` (Session, Seat, Decision, Answer). Grilling tickets use the `grilling` and `domain-modeling` skills. Update `CONTEXT.md` as terms settle.
- The protocol supports any number of players from the start. Exactly one Decision is open at a time, answered in APNAP order, even for secret choices.
- Proof-game cards are a fixture set inside this repo, gathered from the inline Lua test scripts. CI must not depend on `../stackfish-db`.
- `.plan/` is committed, as `README.md` says. Only `.agent-state.md` stays out of git.

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
- 026 Whole-declaration combat predicates. Menace and "attacks each combat if able" are one module, `commands/combat/predicate.rs`, called by `validate_declare_attackers`, `validate_declare_blockers`, `validate_pass_priority` and the two entry builders. Must-attack is the `ATTACKS_EACH_COMBAT` keyword bit, and `AttackerEntry` has `required`. [closed/026-whole-declaration-predicates.md]
- 036 Mode-combination predicate. `mode_can_join_others` in `commands/legality.rs` is the shared rule for the cast, the modal entries, modal triggers and the opponent's mode choice. `ModeRange` and `ReplyShape::Modes` carry `solo_only`, and the entries refuse a solo-only mode beside another. [closed/036-mode-combination-predicate.md]
- 048 Add unit tests for pure engine helpers. Added local `#[cfg(test)]` coverage for deterministic helpers in `color_identity.rs` and `types/{zone,object,player,counter,card,mana,cost}.rs`. [closed/048-add-unit-tests-pure-helpers.md]
- 029 CR 104.4b infinite-loop detection. A forced pass with a non-empty stack fingerprints the state, and a repeat ends the game as a draw. Ids are relabeled by rank and the ledger values read at priority are folded in. Persist and Undying returns no longer leave a stale graveyard object. `BEHAVIOUR_REV` is 2. [closed/029-infinite-loop-detection.md]
- 030 Bot coverage. Payment Decisions list candidate ids for object costs through one rule shared with validation, `may_cast` carries the cast's options, and a free cast takes only X = 0. The bot pays object costs and accepts casts. Phyrexian life and a byte-level `may_cast` game stay open. [closed/030-bot-coverage.md]
- 031 Remaining legal_actions gaps. A `DeclareBands` Decision lists attackers that can band and closes once per combat. A Payment Decision lists the payer's mana abilities and accepts an `Action` for one. Script picks list abilities on the stack. Timing grants moved to 076. [closed/031-legal-actions-gaps.md]
- 049 Add unit tests for engine helpers that need a small GameState or layer stubs. Added `#[cfg(test)]` coverage for `layers.rs`, `commands/legality.rs` and `commands/combat/predicate.rs` with minimal state fixtures. [closed/049-add-unit-tests-stubbed-helpers.md]
- 032 Host rejection text and payment cost filters. Engine answer errors have `Display` text in `engine/error_text.rs`, and a Seat gets that sentence in place of Debug output. 030's payment `candidates` closed the cost filter gap, so the filter stays off the wire. Engine terms in some `IllegalAction` strings are still a TODO. [closed/032-host-rejection-text-cost-filters.md]
- 033 Run the Godot smoke test in CI. The `godot-smoke` CI job runs `run.sh` in both safeguard modes on Godot 4.2.2 and 4.7.2, each download checked against its published SHA-512. Export and web builds were ruled out of scope. [closed/033-godot-smoke-test-ci.md]
- 034 Unconfirmed gdext items. Research in `docs/research/gdext-unconfirmed.md`. mlua on emscripten is unverified for this repo, and gdext web export is experimental. A `#[func]` `Result` with `Unexpected` logs and fails the call with code 1337, so the byte-level error envelope stays. Editor hot reload drops the Session and its Lua state. [closed/034-unconfirmed-gdext-items.md]
- 072 Ledger events the snapshot fold needs. Every life change writes `LifeChanged`, damage included, so life folds from `life_changed` alone. Infect writes `PlayerCounterAdded`, priority changes write `PrioritySet`, and CR 800.4a writes one `LeftGame` per object and passes priority on. A snapshot drops a revealed identity. `BEHAVIOUR_REV` is 3. [closed/072-ledger-events-for-snapshot-fold.md]
- 073 Make GameState objects a BTreeMap. Every walk over `objects` is in `ObjectId` order, which puts the Devour, Amplify, copy and entry-attach candidates in id order. The replay pin did not move. [closed/073-objects-btreemap.md]
- 074 Proof cards for ordered choices. Ravenous Ooze, Rat Herald, Mimic Pool and Armory Call put Devour, Amplify, Copy and Attach on entry into the proof decks, and the seed loop asserts all four open. They found a Devour creature offered as its own sacrifice, now excluded (CR 614.12). `BEHAVIOUR_REV` is 4. [closed/074-proof-cards-for-ordered-choices.md]
- 075 Lint against hash-ordered walks. Add `iter_over_hash_type` (workspace table, plus per-crate tables in engine, cli and testutil to keep the engine's `unreachable_pub` exemption) and `disallowed-methods` on hash walks in a root `clippy.toml`. The 9 sites are cleared by ordered containers, not allows. `into_iter`, `extend`, `from_iter` and Lua `pairs` stay accepted gaps. Build is 078. [closed/075-hash-walk-lint.md]
- 076 Timing permissions granted by effects. A Timing permission is a new list `state.timing_permissions` (player filter plus spell filter, flash only, removed when its source leaves), not FLASH through layers. One `can_cast_now` predicate serves cast and `cast_entries`, and land plays and Equip keep `is_sorcery_speed`. Zone permissions, end-of-turn grants and instant-speed equip are follow-ups. Build is 079. [closed/076-timing-grants.md]
- 077 A Clone offered as a copy of itself. `pending_copy_choice` never lists the entrant, in a first or chained round (CR 614.12a). A lone Clone enters with no Decision. 29 of 40 proof seeds moved but not the pinned seed 9, so `BEHAVIOUR_REV` is 5 with the same hash. [closed/077-clone-copies-itself.md]
- 078 Build the hash-walk lint and clear its sites. Clippy refuses a `for` loop or a walking method over a hash container in every crate. The 9 sites are ordered containers or keyed lookups with no allows, and the replay pin did not move. [closed/078-build-hash-walk-lint.md]
- 079 Build timing permissions (flash grants). `can_cast_now` reads `state.timing_permissions` for the cast and `cast_entries`. `Game.CreateTimingPermission` registers one, and `retire_object` and `remap_abilities` drop it with its source. The permission tests the caster. Vedalken Orrery is in the proof deck, `token_ceased` carries its zone and owner, and `BEHAVIOUR_REV` is 6. [closed/079-build-timing-permissions.md]
- 081 cost_modifiers outlive their source. `retire_object` purges a source's cost modifiers with the other static-ability lists (CR 611.3b), and a destroy-then-cast test pins it. The `remap_abilities` gap for hand-functioning modifiers stays a TODO. [closed/081-cost-modifiers-outlive-source.md]
- 080 Zone permissions. A Zone permission is a new list `state.zone_permissions` beside `timing_permissions` (source, affected, zone, owners, play-land or cast kind, card filter), not a zone field on `TimingPermission`. `can_cast_from_zone` and `can_play_land_from_zone` replace the hand tests in the cast command, `validate_play_land` and both entry scans. Flashback stays on the alternative cost, timing and the land limit are unchanged, graveyard and exile are in scope, and a permission is removed when its source leaves. Top of library, linked exile, end-of-turn grants and exile-instead riders are follow-ups. Build is 082. [closed/080-zone-permissions.md]
- 082 Build zone permissions. `can_cast_from_zone` and `can_play_land_from_zone` serve the cast and land commands and both entry scans. A card cast or played from another player's graveyard is controlled by the actor. `Game.CreateZonePermission` registers one, and the purges drop it with its source. Crucible of Worlds is in the red-green deck, a scenario covers the graveyard land play, and `BEHAVIOUR_REV` is 7. [closed/082-build-zone-permissions.md]
- 083 Library and linked-exile zone permissions. `ZonePermission` gains `library_top`, `linked` and `reveals_top`. The top card is found at check time, linked cards sit in a new `GameState.exiled_with` list, and a library id reaches only the permission holder unless the top is revealed (CR 401.5). Face-down linked cards are never offered. Future Sight joins the blue-black proof deck. Build is 086. [closed/083-library-linked-exile-zone-permissions.md]
- 084 Expiring timing and zone permissions. `TimingPermission` and `ZonePermission` each gain `duration: EffectDuration`. Static grants keep `WhileSourceOnBattlefield`, and Timed grants installed by a resolving effect last until end of turn, end of combat or your next turn, outlive their source and expire through the `engine/turn.rs` drains (CR 611.2a, 514.2). No proof card. Build is 087. [closed/084-expiring-permissions.md]
- 085 Zone permission riders. A rider is a per-cast fact on `StackObject`: `leave_stack_rider: Option<LeaveStackRider>` (`ExileInstead`, `ExileInsteadOfGraveyard`) replaces the flashback bool, `cast_zone_source` reports which permission allowed the cast, and one `leave_destination` helper serves resolve, fizzle and counter (CR 608.2n, 702.34a). Kess, Dissident Mage is a scenario fixture. Build is 088. [closed/085-zone-permission-riders.md]
- 086 Build library-top and linked-exile zone permissions. Built as 083 decided, with `Game.Ops.ExileLinked` in place of an exile-builder setter, the top-card helpers in `types/player.rs` and host visibility computed from the permissions at check time. A revealed top is snapshot-only, with no reveal event. [closed/086-build-library-linked-exile-zone-permissions.md]
- 087 Build expiring timing and zone permissions. Built as 084 decided, with the `AbilityEffect` rows boxed and `BEHAVIOUR_REV` left at 8, because the pin did not move. A Timed linked grant is accepted, and one from a spell matches nothing. [closed/087-build-expiring-permissions.md]
- 088 Build zone permission riders. Built as 085 decided. A rider-less permission wins over ones with riders, then list order, because permissions have no id. The Kess scenario casts Kess first, because battlefield setup does not register static scripts. `BEHAVIOUR_REV` stays 8. [closed/088-build-zone-permission-riders.md]
- 105 How the loader treats design non-goal card types. 518 planeswalkers, battles and sagas and 38 Class cards load and do nothing, with no error. 231 face-down-cast cards play face up, and 73 face-down producers do nothing. Research recommends rejecting at load with a new `CardLoadError`, except the 229 face-up-playable cards. The choice goes to 112. [closed/105-unsupported-card-types-in-loader.md]

## Not yet specified

- Carddb-backed CardSource: a thin wrapper over `CardDb::load_by_oracle_id` for `../stackfish-db/stackfish.db`. Its shape is settled in 006. The Godot adapter's `create` needs it to serve real cards.
- Wrong-thread behavior in the Godot adapter: tell hosts to expect `null`, or compile gdext's check out of release builds (found by 015).
- Engine atomicity on a failed command. Decided in ticket 024 (see `map-fixes.md`): validate before mutate, so a rejected input changes nothing. The build work is tracked there as 025, 035, 026 and 036.
- Chain operation leftovers. A word shared by two entries, a chain inside `ForEachPlayer`, a custom function as an entry and a chain as a mode are not built (`25-chain-operations.md`). A chain as a mode waits on the modal builder decision.
- Opponent-chosen modes with three or more players, where the controller picks the chooser (CR 700.2e), and repeatable pawprint-budget modes (CR 700.2i).
- Condescend's `{X}`. "Counter unless that player pays {X}" supports only a fixed printed amount, and an X announced on the countering spell has no representation yet.
- Devour and Amplify quality restrictions, and a creature with several independent Amplify instances.
- Enchanting or equipping a player. Attachment targets a permanent only.
- Splice (CR 601.2b, 702.47). `PlayerAction` and `StackObject` cannot say which cards are spliced.
- Restricted mana beyond a card-type filter (CR 106.11). Object-specific and activation restrictions are not modeled.
- The loader rejection build. Its card set and error shape wait on 112.
- Several pending kinds in one custom operation outcome. A `Chain` holds one pending kind today (`not-available-yet.md`).

## Out of scope

- A playable Godot UI (scenes, card art, input handling). This is a separate effort after this map.
- Netcode, matchmaking and lobbies. These belong to the host application, per DESIGN.md.
- Answering several Decisions in parallel (for example, simultaneous mulligans). Ruled out in favour of one Decision at a time.
- Godot export and web builds (a copy step into `res://`, a macOS `.framework`, an emscripten target). The destination only needs the adapter to load and step a game, so these go with the Godot UI effort. [closed/033-godot-smoke-test-ci.md]

## Frontier

Open tickets, unblocked and unclaimed. Tickets 089 to 111 were cut after 088 from the backlog and the "Not yet specified" list, so they are follow-up work past the destination above.

Decisions (HITL):

- 089 Lua event visibility for conditions
- 090 Modal builders in Lua
- 091 Divide damage or counters among targets
- 092 Effect-owned exile links
- 093 Scenario battlefield setup runs scripts
- 094 Library top reveal event
- 095 Scenario schema additions
- 096 Script-backed costs, or delete `invoke_cost`
- 097 Permission choice and once-per-turn limits
- 098 Timing permission follow-ups
- 099 Riders beyond leaving the stack
- 100 Reanimation options
- 101 Ward costs and player hexproof
- 103 Damage redirect to another target
- 104 Fight and bite target shapes
- 112 Which non-goal cards the loader rejects

Builds and audits:

- 106 Audit remaining order leaks (AFK)
- 107 `remap_abilities` purges cost modifiers
- 108 `new_game` loads alternative costs and buyback
- 109 Proof game coverage gaps
- 110 Stale note sweep (AFK)
- 111 Echo sacrifice checks control

Blocked:

- 102 Stun counters and tap hooks (by 089)
