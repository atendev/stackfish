---
id: 037
title: Real-card proof bot
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Now that `CardDbCardSource` exists, how do we run the random proof bot against real cards from `../stackfish-db/stackfish.db`?

## Notes

- Origin 019 / backlog item 2. Branch `phase5/real-card-proof-bot`.
- The DB has only 19 active `card_scripts`, so most real cards load as vanilla. That is fine for a first smoke test.
- The bot cannot pay hybrid, Phyrexian, snow or `{X}` costs yet, so the curated set must avoid those symbols.
- The test should be CI-safe: it skips if `../stackfish-db/stackfish.db` is missing.

## Resolution

`crates/host/tests/real_card_bot.rs` curates two simple 40-card decks and runs the bot through the real database. It skips if the DB is absent. `docs/real_card_set.md` lists the chosen cards and the scripts to add next. Merged in commit <TBD>.
