---
id: 006
title: How a host starts a game
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

What does a host send to start a game? Who turns card information into `CardDefinition`s for each deck: the host, the Session (as the middle man), or a pluggable card source? How are the seed, settings, player count, turn order and Commander setup given, and where do mulligans happen?

## Notes

- The user's intent: a middle man takes card information, builds the `CardDefinition`s for each deck and hands them to the engine. That middle man may be the Session.
- Today: `Engine::new_game`, `engine/pregame.rs`, `PlayerConfig`; `stackfish-carddb` builds `CardDefinition`s from SQLite (`docs/implementation/15-card-database-loader.md`).
- The proof game uses fixture cards in this repo; the real database is later work but the shape chosen here must allow it.

## Resolution

**Card source.** A CardSource is the middle layer. It is an interface with one job: given a card key, return a `CardDefinition`. Whoever creates the Session passes it in at creation. It never travels in a message. There are two implementations:

- A carddb-backed source that opens the DB file through `stackfish-carddb`.
- A fixture source that serves the proof cards from inside this repo, so CI never reads `../stackfish-db`.

The engine never sees keys, decklists or the database.

**Card key.** A key is the card's `oracle_id` UUID. The UI or host owns the mapping from readable slugs (`script_id`) or names to UUIDs. The Session and CardSource never see a slug. `CardDb::load_by_oracle_id` already does the lookup.

**Setup message.** The host sends:

- A player list in turn order. The first entry plays first (CR 103.1), and the host decides who that is. The Session assigns `PlayerId`s 0..n from list position.
- For each player: a decklist (a list of `oracle_id` and count) and an optional list of commander `oracle_id`s.
- A format name, plus optional overrides of individual `GameSettings` fields.
- The seed, a `u64`.

**Expansion.** The Session asks the CardSource once per distinct key, repeats each definition by its count, and builds the `GameConfig` that `Engine::new_game` already takes. Library order is the decklist order. The engine shuffles it when the settings say so.

**Formats.** A format is a preset that fills `GameSettings`. Two exist for now: standard (20 life, no free mulligan) and commander (40 life, 1 free mulligan per CR 103.5c, commander slots allowed). The host's overrides apply on top of the preset. Tests use overrides for no shuffle and skip mulligan.

**Validation at creation.** The Session checks structure only. Creation fails with a named error if:

- a card key does not resolve (the error names the key),
- there are fewer than 2 players,
- a commander slot is used in a format that has none,
- the format name is unknown.

Deck legality (size, singleton, color identity, bans) stays with the deck builder, which matches the trust-the-setup stance in DESIGN.md.

**Mulligans.** Setup carries no mulligan field beyond the settings. Mulligans are Decisions inside the Session. The first Advance (ticket 001) runs shuffle and opening draws, then opens the keep or mulligan Decision (yes or no, per 002) for each player in turn order. Bottoming after a paid mulligan is a pick-from-candidates Decision.

**Replay.** The Session exports the setup message and the seed with the accepted Answers (ticket 001). A recorded setup holds `oracle_id`s, so a replay needs a CardSource that resolves the same cards. Tracking the card data version is left to 007 or later.

Handed to other tickets:

- 007: the wire encoding of the setup message, and the creation error codes.
- 009: whether the adapter passes the DB path to a carddb-backed CardSource when Godot creates a Session.

Handed to build work:

- B3 includes the CardSource trait, the setup types, format presets and creation validation in `crates/host`.
- B5 uses the fixture source.
- The carddb-backed source is a thin wrapper over `load_by_oracle_id` and needs no new decision.
