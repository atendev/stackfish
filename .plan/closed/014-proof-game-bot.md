---
id: 014
title: Build B5 proof card set, random bot and full-game byte test
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Build the proof card set as a fixture CardSource and a random legal-move bot, plus a test that plays a full game over bytes.

## Notes

- Branch: `phase5/proof-game-bot`. Follow the CLAUDE.md slice workflow: requirements report first, approval before every commit, closing report, user merges.
- Waits on 010 to 013 (B1 to B4). Decision 006 is closed.
- Proof-game cards are a fixture set inside this repo, gathered from the inline Lua test scripts. CI must not depend on `../stackfish-db`.
- The bot answers every Decision. Exactly one Decision is open at a time, answered in APNAP order, even for secret choices.
- The full game must run for two players end to end through the bytes interface. Other player counts are covered by the protocol but tested with two.
- Mark any stubbed parts with TODO.

## Resolution

Built the proof game on `phase5/proof-game-bot` and merged it (`6422249`).

- `proof_cards()` serves 25 cards as inline Lua (4 lands, 8 creatures, 13 other cards), and `proof_setup_request(players, seed)` builds a standard setup with a red-green and a blue-black deck. Fork in the Road is declared in Rust, because a card script cannot declare modes.
- `Bot` is a byte-only client of a Session. It answers all twelve reply shapes from each Decision's own candidates, tracks its mana pool from events, learns card costs from move identities, and passes more often than it acts. It is uniform otherwise, and a small in-crate PRNG seeds it.
- `phase5_proof_game.rs`: 30 two-player games finish with no rejected Answer and no `script_error`, the games cast all 21 non-land cards, eight seeds give byte-identical transcripts, three-player games finish, and no Seat receives a hidden id. A 500-seed run is ignored by default and passes in release mode.
- The bot found three engine bugs, fixed in the same slice: creatures outside the battlefield could attack and block, simultaneous deaths took ids in `HashMap` order, and a script with no `register_abilities` raised a `script_error`. Each has a regression test.
- Left open: the bot declines `may_cast` Decisions and cannot pay sacrifice, discard, tap or graveyard-exile costs or hybrid, Phyrexian, snow and `{X}` costs. Other unordered `HashMap` walks may still reach a host. Notes are in `docs/implementation/19-proof-game.md`.
