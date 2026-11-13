# Ticket 150 decision report: non-goal card rejection in the loader

Branch `phase5/non-goal-card-rejection`. Audit verdict: APPROVE in round 1. The decision itself comes from the closed ticket 112.

## What was built

- `CardLoadError::NonGoal { name, kind }` and a public `NonGoalKind` (Planeswalker, Battle, Saga, Class, FaceDownProducer) with a `Display`, in `crates/carddb/src/lib.rs`.
- The check runs in `build_definition` after the `MultiFace` check, the mana cost parse and `types_line::parse`, and before keywords load. The first match wins, in the order above.
- Planeswalker and Battle come from the type bits. Saga and Class come from the subtype, because only the text left of the em dash reaches `parse_card_type`. Producers come from the `card_keywords` names Manifest, Manifest dread and Cloak.
- `record_by_oracle_id` reuses the keyword names that `build_definition` already queried, so the producer lookup adds no extra query.
- The `TypeLine` error text no longer lists Class as a type word.

## Decisions

| Question | Options | Choice | Made by | Verdict |
|---|---|---|---|---|
| Keep the Class kind? The ticket said to drop it if a real Class card already errors with `TypeLine`. | Keep or drop | Keep. "Enchantment — Class" loads today because Class is a subtype, so nothing else rejects it. All 38 Class cards loaded before the change. | Implementer (main thread), checked against the real database | APPROVE |
| Where to query the producer keywords | A separate lookup beside `fetch_row`, or share the existing keyword query | Share `keyword_names`. `build_definition` queries once and passes the names to the check and to `keywords::parse`. A private helper returns the names so `record_by_oracle_id` does not query twice. | Implementer | APPROVE |
| Test cards | Invented names, or real names checked against the database | Real names, each checked against `stackfish.db` first: Jace, the Mind Sculptor, The Aetherspark (artifact planeswalker), Occupation of Kulrath, The Eldest Reborn, Artificer Class, Veiled Ascension (Cloak), Manifest Dread, Roalesk, Prime Specimen (Disguise plus Cloak), Venomspout Brackus (Morph), Ire Shaman (Megamorph), Mistway Spy (Disguise only), Bruna, the Fading Light (meld), Ixidron. The fixture rows copy their name, cost, type line and keywords. | Implementer | APPROVE |
| Real-database checks | Run only the fixture tests, or also run the real database | Both. The host tests resolve the database three directories above `crates/host`, so in this worktree they skip. They ran in a temporary copy of the tree beside a link to `stackfish-db`, and the copy was thrown away. | Implementer | APPROVE |
| Backlog item 9 wording | Leave it, or point it at the new item 12 | Reworded, after the auditor's nit. | Auditor raised it, implementer applied it | APPROVE |

## Results against the real database

- Bulk load of every `cards` row: 323 Planeswalker, 2 Battle, 193 Saga, 38 Class and 71 FaceDownProducer rejections, 32,685 loads, 3,200 `MultiFace`, 1,826 `TypeLine`, 6 mana cost parse errors that already existed. The 71 producers are the 73 from the research note minus 2 that stop at `MultiFace` or `TypeLine` first.
- `stackfish-cli db-tests`: 35 of 35 tests had no hard failures.
- `crates/host/tests/carddb_source.rs` and `real_card_bot.rs` pass. No `real_card_bot` deck holds a rejected card: a query of the deck oracle ids found no planeswalker, battle, saga or Class card and no producer keyword.
- `tools/check.sh --full`: fmt, clippy, 2,608 workspace tests and 38 tools unittests all pass. `db_scripts_smoke.rs` skips in this worktree.

## Docs changed

- Backlog item 12 now covers producers only, notes that the 229 face-down cast cards load, and records the Ixidron-type gap, the unflagged layouts, and the empty-string loyalty and defense gap with a note to bump the definition fingerprint version when it is fixed.
- `15-card-database-loader.md`, `DESIGN.md`, `implementation/index.md` and `research/non-goal-card-types.md` describe the new rejection. The `TODO` in `card.rs` about the reserved slots is untouched.
- No Lua API change, so the docs game stub needed no update.

## Process notes

- The requirements report and its audit (CLAUDE.md implementation steps 1 and 2) were skipped. The parent wayfinder brief and ticket 112 already fixed the scope.
- The auditor was a read-only `sonnet` subagent, as the repo loop specifies. The runner's `haiku` default did not apply.
- The branch does not merge. The master merges.

## Surfaced tickets

None.
