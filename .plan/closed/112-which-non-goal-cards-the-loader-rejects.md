---
id: 112
title: Which non-goal cards the loader rejects
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Which cards should `stackfish-carddb` refuse to load, and how? Ticket 105 found 518 planeswalkers, battles and sagas, 38 Class cards and 302 face-down keyword cards that load today and break silently or partly.

## Notes

- Findings: `docs/research/non-goal-card-types.md`, and the Resolution of `closed/105-unsupported-card-types-in-loader.md`.
- Questions to settle:
  - Face-down: reject only the 73 producers (Manifest, Manifest dread, Cloak) and let the 229 morph, megamorph and disguise cards load face up, or reject all 302 as backlog item 12 says.
  - Keyword-less face-down cards (Ixidron and similar) and the 147 face-down exile cards: reject by oracle text or a hand list, or allow for now.
  - Melds (20 besides Urza, Planeswalker) and the other unmodeled layouts the database does not flag as multi-face (mutate, leveler, prototype, case, augment, host): in scope or not.
  - Reject at load with a new `CardLoadError`, or skip with a log for batch callers such as `db-tests`.
  - Keep the `PLANESWALKER` and `BATTLE` type bits and check in `build_definition`, or stop mapping them in `types_line.rs`.
- Use the `grilling` and `domain-modeling` skills. A build ticket graduates once this closes.

## Resolution

The loader rejects a card when what it does face up depends on rules the engine does not model, so it would break silently. It lets a card load when the card plays correctly face up and only an optional extra is missing. Decided in auto mode, then approved.

- Reject at load: planeswalkers, battles, sagas, Class cards and the 73 Manifest, Manifest dread and Cloak producers (the 2 that also have Disguise included). The 229 Morph, Megamorph and Disguise cards load, since only the optional face-down cast is missing.
- Ixidron-type cards with no keyword load. By the rule they would be rejected, so this is a stated exception: no keyword marks them, and the oracle-text match gives 244 candidates, 147 of which only exile a card face down.
- Meld, mutate, leveler, prototype, case, augment and host layouts load. They are not named non-goals in `DESIGN.md` or backlog item 12.
- A new `CardLoadError::NonGoal { name, kind }` with a public `NonGoalKind` (Planeswalker, Battle, Saga, Class, FaceDownProducer) and a `Display`. It is checked in `build_definition` after `MultiFace` and `types_line::parse`, before `load_keywords`. The first match in that order wins. No skip-with-log, and no exemption for a card with an active script.
- Keep the `PLANESWALKER` and `BATTLE` mapping in `types_line.rs`.
- Saga and Class are found by subtype, because the type line is split on the em dash and only the left side is parsed. Producers are found by the `card_keywords` rows, since `keywords::parse` drops those names.
- Empty-string loyalty and defense stay as they are. A `parse_pt` fix would move the definition fingerprint of every card.
- The build amends backlog item 12 to producers only. Build is 150.
