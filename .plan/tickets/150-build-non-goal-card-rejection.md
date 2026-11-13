---
id: 150
title: Build non-goal card rejection in the loader
type: task
mode: AFK
status: open
blocked_by: [112]
claimed: true
---

## Question

Build what "Which non-goal cards the loader rejects" decided: `CardLoadError::NonGoal`, its checks in `build_definition`, the tests and the docs.

## Notes

- Origin 112. The full decision is in `closed/112-which-non-goal-cards-the-loader-rejects.md`. Follow the CLAUDE.md implementation loop on a `phase5/` branch.
- Add `CardLoadError::NonGoal { name, kind }` and a public, re-exported `NonGoalKind` (Planeswalker, Battle, Saga, Class, FaceDownProducer) with a `Display`. Check in `build_definition` after the `MultiFace` check and `types_line::parse`, before `load_keywords`, in the order Planeswalker, Battle, Saga, Class, FaceDownProducer. The first match wins.
- Planeswalker and Battle come from the type bits, which stay mapped in `types_line.rs`. Saga and Class come from the subtype, because only the left of the em dash goes through `parse_card_type`.
- Producers (Manifest, Manifest dread, Cloak) come from the `card_keywords` rows, with a lookup beside `fetch_row`, because `keywords::parse` drops those names. `record_by_oracle_id` already queries those rows, so share the query.
- Start with a test that loads a real Class card. If it already errors with `TypeLine`, drop the Class kind.
- Tests: one real card per kind, a card with Disguise and a producer keyword reporting FaceDownProducer, an artifact or land planeswalker reporting Planeswalker, and a Morph card, a Megamorph card, a Disguise-only card, a meld card and an Ixidron-type card still loading. The 229 cast cards must keep loading.
- The host already turns a load error into a setup refusal naming the card (`crates/host/src/carddb_source.rs`), so no host change is expected. The fingerprint path and `db_script_id` now refuse these cards too. Run `db-tests`, `crates/carddb/tests/load_card.rs`, `crates/host/tests/carddb_source.rs` and `crates/host/tests/real_card_bot.rs` against the real database, and confirm no deck in `real_card_bot` holds a rejected card.
- Do not copy the three card names in `docs/research/non-goal-card-types.md` line 13 into code or tests until they are checked against the database.
- Docs: amend backlog item 12 in `docs/implementation/99-backlog.md` to producers only, and note that face-down cast cards load. Update the `TypeLine` error text if it still lists Class as a type word. Check `docs/DESIGN.md`, `docs/research/non-goal-card-types.md` and `docs/implementation/index.md`. Keep the `TODO` in `card.rs` about the reserved slots.
- Record the Ixidron-type gap and the unflagged layouts in the backlog as well as the map. Record the empty-string loyalty and defense gap with a note to bump the definition-fingerprint version when it is fixed.

## Resolution

<Filled in at close.>
