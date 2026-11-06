# How the loader treats design non-goal card types

Researched 2026-11-06 on `phase5/loader-non-goal-card-types` against `../stackfish-db/stackfish.db` (620 MB SQLite, schema in `../stackfish-db/schema.sql`) and the code in `crates/carddb`, `crates/host` and `crates/engine`. Nothing in the repo was changed.

## Summary and recommendation

518 planeswalker, battle and saga cards in the database load today and reach the engine as ordinary permanents with no type behavior. `MultiFace` already stops 102 more (21 planeswalkers, 37 battles, 44 sagas). The engine does not play the 518: no loyalty counters, no defense counters, no lore counters, no chapter triggers, and no state-based action to remove them. They break silently. A deck that contains one loads, casts, resolves and sits on the battlefield doing nothing. The 38 Class cards, also a `DESIGN.md` non-goal, load the same way with no level behavior.

Face-down cards split in two. The database has no face-down marker, so both groups are found by keyword. 231 single-face cards have a face-down cast keyword (Morph, Megamorph, Disguise). 2 of them also have a producer keyword and are rejected below, so 229 load as plain face-up creatures and play correctly face up; only the face-down option is missing. 73 single-face cards make face-down permanents as their effect (Manifest, Manifest dread, Cloak). Their effect does nothing, because the engine has no producer for `face_down`. 2 cards are in both groups, so 302 in total.

The rule used here, applied only to the kinds named as non-goals in `DESIGN.md` or backlog item 12: reject a card when what it does face up depends on rules the engine does not model, because it breaks silently; allow a card when it plays correctly face up and only an optional extra (a face-down cast) is missing. Other unmodeled layouts (meld, mutate, leveler and so on) are outside this ticket and listed as open question 2.

Recommendation: reject at load, as a new `CardLoadError` variant checked in `build_definition` next to `MultiFace`, for planeswalkers, battles, sagas, Class cards and the 73 face-down producers. The 73 include Roalesk, Prime Specimen and Sleuth Instructor, which have both Disguise and Cloak. Allow the other 229 morph, megamorph and disguise cards to load, and record the missing face-down cast in the backlog. Cards that make face-down permanents without a keyword, such as Ixidron, break silently too but are not caught by the keyword check, so they load until open question 6 is answered. This departs from backlog item 12, which lists "face-down permanents" as cards to skip or reject. Item 12 needs amending if this rule is adopted, or the user can choose to reject all 302.

## Where the non-goals are written

- `docs/DESIGN.md:15-20` ("Non-goals (v1)"): "No planeswalkers, Sagas, Battles, double-faced cards, split/flip/adventure cards, Class cards in the initial card-type implementation — but the data model and layer/effect systems must leave room for them (stubbed type slots, not a wall)."
- `docs/implementation/99-backlog.md`, item 12: "Design non-goals (planeswalkers, battles, double-faced, split and flip cards, sagas, face-down permanents, team rules). If the card database holds any of these, the loader should skip or reject them explicitly before then."
- Item 9 of the same file says `face_down` has no producer.
- `crates/engine/src/types/card.rs:64-67` calls the `PLANESWALKER` and `BATTLE` bits "Reserved slot, a non-goal in `DESIGN.md`". `card.rs:217-220` says the same for `loyalty` and `defense`.

"Face-down permanents" appears in backlog item 12 but not in the DESIGN.md non-goals list.

## Counts

All counts come from the `cards` table (38,344 rows). `cards.type_line` for a multi-face card holds both faces joined by ` // `, so a `LIKE` on it catches a card whose back face has the type. "Rejected" is `has_multiple_faces = 1`. "Reaches engine" is `has_multiple_faces = 0`.

| Kind | Total in DB | Rejected by `MultiFace` | Reaches engine | Loads (probe) |
|---|---|---|---|---|
| Planeswalker | 344 | 21 (17 transform, 4 modal DFC) | 323 (322 normal + 1 meld) | 323 of 323 |
| Battle | 39 | 37 (all transform "Siege" battles) | 2 (both "Control Point" battles) | 2 of 2 |
| Saga | 237 | 44 (transform) | 193 (191 layout `saga` + 2 layout `normal`) | 193 of 193 |
| Subtotal | 620 | 102 | 518 | 518 |
| Class (layout `class`, `Enchantment — Class`) | 38 | 0 | 38 | not probed; same path as sagas |
| Face-down, by keyword (Morph, Megamorph, Disguise, Manifest, Manifest dread, Cloak) | 307 | 5 | 302 | all single-face ones |
| of which face-down cast (Morph, Megamorph, Disguise) | | | 231 (229 allowed; 2 rejected through the producer row) | |
| of which face-down producers (Manifest, Manifest dread, Cloak) | | | 73 (2 also in the row above) | |
| Face-down, by oracle text (face down, face-down, manifest, disguise, morph, cloak, turned face up) | n/a | n/a | 566 | all |

Per-keyword counts, single or multi-face: Morph 153, Megamorph 31, Disguise 47, Manifest 67 (62 single-face), Manifest dread 36 (32 single-face), Cloak 11. The keyword rows overlap, so they do not add up to 307.

Notes:

- The three planeswalker type lines that are not plain "Planeswalker" (The Aetherspark, Luxior, Ignited: `Artifact Planeswalker`; Wrenn and One: `Land Planeswalker`) are in the 323.
- Urza, Planeswalker (layout `meld`, `has_multiple_faces = 0`) loads. All 21 `meld` cards have `has_multiple_faces = 0`, so melds slip past `MultiFace` even though they are two-faced cards. That is outside this ticket's four kinds but is the same leak.
- Sagas with a creature back face (for example Fable of the Mirror-Breaker) are transform cards and are already rejected.
- No active script (`card_scripts.is_active = 1`) exists for any planeswalker, saga, battle, or multi-face card. 34 active scripts exist in total, none on these kinds. No `card_tests` row exists for any of them either.

### SQL used

```sql
-- sizes and layouts
select count(*) from cards;
select layout, count(*), sum(has_multiple_faces) from cards group by layout order by 2 desc;
select object_type, count(*) from cards group by 1;

-- planeswalkers
select object_type, layout, has_multiple_faces, count(*) from cards where type_line like '%Planeswalker%' group by 1,2,3;
select count(*) from cards c where c.has_multiple_faces=1 and exists (select 1 from card_faces f where f.oracle_id=c.oracle_id and f.type_line like '%Planeswalker%');
select name, type_line from cards where type_line like '%Planeswalker%' and has_multiple_faces=0 and type_line not like 'Legendary Planeswalker%' and type_line not like 'Planeswalker%';
select loyalty, count(*) from cards where type_line like '%Planeswalker%' and has_multiple_faces=0 group by 1 order by 2 desc limit 5;

-- battles
select object_type, layout, has_multiple_faces, count(*) from cards where type_line like '%Battle%' group by 1,2,3;
select name, type_line, defense from cards where type_line like '%Battle%' and has_multiple_faces=0;

-- sagas
select object_type, layout, has_multiple_faces, count(*) from cards where type_line like '%Saga%' group by 1,2,3;
select name, type_line from cards where type_line like '%Saga%' and layout = 'normal';

-- melds
select layout, has_multiple_faces, count(*) from cards where layout in ('meld','mutate','prototype','leveler','class','case','host','augment') group by 1,2;

-- face-down
select count(distinct ck.oracle_id),
       count(distinct case when c.has_multiple_faces=0 then ck.oracle_id end)
from card_keywords ck join keywords k on k.id = ck.keyword_id join cards c on c.oracle_id = ck.oracle_id
where k.name in ('Morph','Megamorph','Disguise','Manifest','Manifest dread','Cloak');
-- keyword-less face-down mentions: has_multiple_faces=0, oracle_text like '%face down%' or '%face-down%',
-- oracle_id not in the six-keyword set -> 244; of those, exile-face-down wording -> 147, matched by
-- oracle_text like '%exile%face down%' or '%face-down exile%' or '%exiled face down%'
-- split: same join with c.has_multiple_faces=0 and k.name in ('Morph','Megamorph','Disguise') -> 231,
-- and k.name in ('Manifest','Manifest dread','Cloak') -> 73; cards in both -> 2
select count(*) from cards where has_multiple_faces=0 and (oracle_text like '%face down%' or oracle_text like '%face-down%'
  or oracle_text like '%anifest%' or oracle_text like '%Disguise%' or oracle_text like '%Morph%' or oracle_text like '%Megamorph%'
  or oracle_text like '%Cloak%' or oracle_text like '%isguise%' or oracle_text like '%turned face up%');

-- scripts and tests on these kinds
select count(distinct c.oracle_id) from cards c join card_scripts s on s.oracle_id=c.oracle_id and s.is_active=1
 where c.type_line like '%Planeswalker%';   -- same query with '%Saga%', '%Battle%': all 0
select count(*) from card_scripts where is_active=1;                 -- 34
select count(*) from card_tests t join cards c on c.oracle_id=t.oracle_id
 where c.type_line like '%Planeswalker%' or c.type_line like '%Saga%' or c.type_line like '%Battle%' or c.has_multiple_faces=1; -- 0

-- class
select count(*), sum(has_multiple_faces) from cards where type_line like '%Class%';   -- 38, 0

-- empty loyalty and defense
select count(*), sum(loyalty=''), sum(loyalty is null), sum(defense=''), sum(defense is null) from cards;
```

Run as `sqlite3 -readonly ../stackfish-db/stackfish.db "<query>"`.

## How the loader handles each kind

Load path: `CardDb::load_by_oracle_id` (`crates/carddb/src/lib.rs:69`) calls `fetch_row` (`lib.rs:108`), then `build_definition` (`lib.rs:138`). `load_by_name` and `load_by_script_id` go through the same `build_definition`. `record_by_oracle_id` (`lib.rs:76`) calls `build_definition` first, so fingerprinting fails wherever loading fails.

`build_definition` has two refusals before it builds anything: `has_multiple_faces` returns `CardLoadError::MultiFace` (`lib.rs:139-141`), and an unknown type word returns `CardLoadError::TypeLine` (`lib.rs:154`, `types_line.rs:38-43`). `types_line::parse_card_type` maps `"Planeswalker"` and `"Battle"` to type bits (`types_line.rs:85-86`), so neither refusal fires for them. The `TypeLine` error text names Plane, Phenomenon, Scheme, Vanguard, Conspiracy, Dungeon and Class as non-goals, but not planeswalkers, battles or sagas. "Saga" is not a card type: it is a subtype, so the parser keeps it as `Subtype("Saga")` and the card gets only `ENCHANTMENT`. No code in `crates/carddb` looks at loyalty, defense or the Saga subtype beyond copying them (`lib.rs:170-171`).

Probe result: a throwaway crate in `/tmp/probe` with path dependencies on `stackfish-carddb` and `stackfish-engine` opened the real database and called `load_by_oracle_id` on named cards, then on every planeswalker, battle and saga row. The crate and its target directory are deleted. Output:

- Jace, the Mind Sculptor: `Ok`, types `PLANESWALKER`, loyalty `Fixed(3)`.
- Urza, Planeswalker (meld): `Ok`, `PLANESWALKER`, loyalty `Fixed(7)`.
- The Aetherspark: `Ok`, `ARTIFACT | PLANESWALKER`, loyalty `Fixed(4)`. Wrenn and One: `Ok`, `LAND | PLANESWALKER`, loyalty `Fixed(1)`.
- Occupation of Llanowar: `Ok`, `BATTLE`, defense `Fixed(4)`.
- The Many Deeds of Belzenlok, Night of the Flying Merfolk, Fall of the Thran: `Ok`, `ENCHANTMENT` (the Saga subtype is the only trace).
- Akroma, Angel of Fury (Morph), Ixidron, Hooded Hydra, Den Protector: `Ok`, `CREATURE`. The morph keyword is dropped: `keywords::parse` returns a `KeywordSet` and skips any name that `parse_one` does not know (`parse_one` returns `None` at `crates/carddb/src/keywords.rs:42`).
- Invasion of Tolvada // The Broken Sky: `Err` `MultiFace`.
- Bulk: all 323 planeswalkers, 2 battles and 193 sagas that are single-face loaded. Every transform and modal DFC row failed with `MultiFace`.

### Side finding: empty loyalty and defense columns

For ordinary cards (Sheoldred's Edict, Akroma), the probe showed `loyalty` and `defense` as `Some(CharacteristicDefining { display: "" })`, not `None`. The database stores an empty string, not NULL, and `parse_pt` (`lib.rs:310-319`) turns any non-integer string into `CharacteristicDefining`. It is harmless today, because `layers.rs:314-315` only reads `as_fixed()`, but a future loyalty or defense check on `definition.loyalty.is_some()` would misfire on every card. The empty string is the stored form: of 38,344 rows, 38,021 have `loyalty = ''` and 38,342 have `defense = ''`, and no row stores NULL in either column.

## Per-type findings

### Planeswalkers (323 reach the engine)

- Loads: yes, shown by the probe.
- Plays: a planeswalker spell can be cast and resolves onto the battlefield. `crates/engine/src/engine/resolve/permanent_entry.rs:26-40` sends everything that is not an instant or sorcery to the battlefield. Nothing then does anything with it:
  - No code adds `CounterKind::Loyalty` counters on entry. `CounterKind::Loyalty` exists (`types/counter.rs:7`) and the script layer can name it (`script/decode.rs:504`, `script/globals.rs:1535`), but nothing in the engine adds it automatically. Loyalty only reaches the characteristic snapshot as a number (`layers.rs:314`).
  - Loyalty abilities are not parsed from oracle text. Behavior comes only from Lua scripts (`lib.rs:1-3`), and no planeswalker has one. There is no loyalty-cost or once-per-turn sorcery-speed restriction in the engine.
  - SBA for loyalty 0 is a recorded TODO (`sba.rs:151`, CR 704.5i), and the legend rule and uniqueness rules are unrelated. The permanent never dies from loyalty.
  - Damage to planeswalkers is not modeled: `commands/zone_ops.rs:329` (TODO, CR 120.3c/d). `TargetFilter::AnyTarget` is documented as including planeswalkers and battles but only classifies the bits (`types/target.rs:65-67`).
  - Attack declaration at a planeswalker (CR 506.3) and `Equip` planeswalker (`types/ability.rs:487`, CR 702.6e) are not modeled.
- Silent breakage: yes. The player sees a permanent that has loyalty on a card face and cannot use it. The proof of "silent" is the absence of any failure path: the card loads, the cast succeeds, and no event signals the missing behavior.

### Battles (2 reach the engine)

- Loads: yes. The two reaching cards are Occupation of Llanowar and Occupation of Kulrath, both "Battle — Control Point". Per `schema.sql`, `cards.defense` holds the value and the probe showed `Fixed(4)`.
- Plays: casting follows the same permanent path. No defense counters are placed, there is no protector assignment (CR 310.8), no defeated-battle SBA (CR 704.5v), no siege flip, and no attacking-a-battle rule. A battle sits on the battlefield and is inert.
- The 37 transform Siege battles are already rejected, so the practical effect of a battle rule is 2 cards.

### Sagas (193 reach the engine)

- Loads: yes, as `ENCHANTMENT` with `Subtype("Saga")`. Their oracle text starts with `I —`, `II —` chapter lines that are not read.
- Plays: no lore counter on entry (CR 714.3a), no chapter triggers, no draw-step/main-phase lore counter addition (CR 714.2c), no sacrifice SBA once the last chapter ability has left the stack (CR 704.5s). A case-insensitive, word-bounded grep of `crates/engine/src` (`grep -rniwE 'saga|chapter|lore'`) finds nothing. A grep without word boundaries matches unrelated words such as "Colored". The saga sits on the battlefield forever and does nothing.
- Silent breakage: yes. This is the worst of the three, since the card is a real "do something over three turns" effect that never fires and never leaves.

### Face-down cards (no marker in the database; ~302 single-face keyword cards)

- The `cards` table has no face-down column. The only signals are the keywords (`keywords`, `card_keywords`) and oracle text, so the counts above are heuristic.
- Loads: yes. Morph, Megamorph, Disguise, Manifest and Cloak are not recognized by `keywords::parse` and are dropped without error. The card loads as a face-up creature with its printed characteristics.
- Plays: the card casts face up for its mana cost and works normally. What fails silently:
  - The face-down cast (CR 702.37, 702.168) is never offered, so the morph cost never matters.
  - Cards that create or interact with face-down objects (Ixidron, manifest and cloak producers, "turn face up" triggers) have no `face_down` producer. `GameObject.face_down` (`types/object.rs:52-53`) carries a TODO: "no producer sets face_down; morph, manifest, disguise and face-down exile are unmodeled (CR 708.2)". Related TODOs: `types/ability.rs:252` (face-down exile, CR 406.3), `commands/legality.rs:80` (face-down linked cards never offered, CR 601.3f), `types/game.rs:206` (a face-down source would leak through hidden-information handling, CR 708.2).
  - The host already sends `face_down` over the wire and hides it from non-controllers (`crates/host/src/view.rs:71-74`, `wire/snapshot.rs:72,89`), so the visibility side exists; only the producers are missing.
- Because no card in the database can currently put a permanent face down, a loaded face-down card does not corrupt state.
- The two groups differ under the rule in the summary. A morph, megamorph or disguise card plays correctly face up and only lacks an optional cast, so it is allowed (229 cards). A manifest, manifest dread or cloak card (73) has making a face-down permanent as its effect, so that effect does nothing and the card breaks silently, like a saga. It is rejected, including the 2 cards that also have Disguise. Ixidron and similar cards break the same way, so the rule would reject them, but they carry no keyword and the check cannot see them (open question 6).

### Class cards (38 reach the engine)

- `docs/DESIGN.md:18-19` names Class cards as a non-goal. All 38 have layout `class`, type line `Enchantment — Class` and `has_multiple_faces = 0`.
- Loads: the `TypeLine` error text names Class (`types_line.rs:41`), but that check only fires on an unknown card type. "Class" is a subtype here, so the card loads as `ENCHANTMENT`, the same path as a saga. Not probed.
- Plays: level-up abilities and class levels are not modeled, and no Class card has an active script. The enchantment enters and does nothing. Silent breakage: yes.

## Options

| Option | What changes | Effect on `CardSource` (`crates/host`) | Effect on proof-game fixtures and tests |
|---|---|---|---|
| Reject at load | New `CardLoadError` variant (name suggestion `NonGoalType`), checked in `build_definition` for `PLANESWALKER`, `BATTLE`, the `Saga` and `Class` subtypes, and the Manifest, Manifest dread and Cloak keywords. | `CardDbCardSource::load` and `fingerprint` already map every non-`NotFound` error to `card {key:?} failed to load: {e}` (`load_error`, `crates/host/src/carddb_source.rs:25-30`), and `setup.rs:156-165` turns that into a `SetupError` naming the key. So a setup request fails with the card named and nothing starts. | Hand-built proof cards (`crates/host/src/proof_cards.rs`, `crates/testutil`) never go through `CardDb`, so they are untouched. The scenario crate's own type parser (`crates/scenario/src/build.rs:20`) accepts "Planeswalker" for hand-written cards and is separate. DB-backed scenarios via `db_script_id` (`docs/scenario_format.md`, "Real card lookup") would fail for these kinds, but no active script or `card_tests` row exists for them, so no existing scenario is affected. |
| Reject at deck setup | Check in `crates/host/src/setup.rs` (or a validator in `Resolver::resolve`). | Same refusal text from the host, but `CardDb` itself still returns these cards, so `load_by_name`, `load_by_script_id` and the CLI's `db-tests` path would still produce inert permanents. | Same as above for fixtures. The leak remains for every non-host caller. |
| Allow inert | No code change. | The card loads and the game starts. The fingerprint includes loyalty and defense (`CardRecord`), so replays stay stable. | Nothing breaks, and nothing warns either. The silent failure stays. |

Reasons for rejecting at load: backlog item 12 asks for the loader to do it, `MultiFace` already sets the precedent, every caller shares one check (host, scenario `db_script_id`, CLI), and a refusal names the card so the user sees the gap. It is cheap: 629 cards out of 38,344 (518 planeswalkers, battles and sagas, 38 Class cards, 73 face-down producers), and none has a script or test.

Reasons to keep morph, megamorph and disguise cards loading: each one plays correctly face up, so under the summary's rule it is not broken, only incomplete. Rejecting them would remove 229 working creatures (231 minus the 2 that also cloak, which the producer check rejects). The loader already drops every keyword it does not know, and a missing optional cast is the same kind of gap. The cost is that this departs from backlog item 12, which names "face-down permanents" without splitting them. If the user prefers item 12 as written, reject all 302 by the same keyword check.

## Open questions

1. Should the rejection be keyed on the type bits, or should `types_line::parse_card_type` stop mapping `Planeswalker` and `Battle`? Removing the mapping would produce a `TypeLine` error with no new variant, but `DESIGN.md:17-19` and `card.rs:64-67` say the type slots stay as stubbed room to grow, and `AnyTarget` already classifies them. A separate check in `build_definition` keeps the slots.
2. Melds: 21 `meld` rows have `has_multiple_faces = 0` and load as one-faced cards. Urza, Planeswalker is already in the planeswalker count; the other 20 are not covered. Should the loader reject by layout (`meld`) as well? The same question applies to other layouts the database does not flag as multi-face (`mutate` 34, `leveler` 26, `prototype` 21, `case` 15, `augment` 14, `host` 20). These are unmodeled too, so the summary's rule would reject them if it were extended to these layouts, but none is a `DESIGN.md` or backlog non-goal, so they are outside this ticket.
3. The empty-string `loyalty` and `defense` values (side finding above): should `parse_pt` treat `""` as `None`? The empty string is the stored form for every card without the value.
4. Should the policy be reject-with-error or skip-with-log for batch callers such as `db-tests`? The ticket text says "skip or reject"; this note assumes reject, matching `MultiFace`.
5. The 5 multi-face face-down-keyword cards and the multi-face keyword counts rely on the keywords table. Whether Scryfall-derived keyword data is complete for older morph cards was not checked against the cards' oracle text beyond the counts above.
6. Cards that turn permanents face down without a Manifest, Manifest dread or Cloak keyword (Ixidron, for example) slip past a keyword check. Catching them needs an oracle-text match or a hand list. An oracle-text heuristic gives the candidate set: 244 single-face cards mention "face down" or "face-down" and have none of the six keywords. 147 of them exile a card face down. Face-down exile is also unmodeled (`types/ability.rs:252`), so these break silently too, but they make no face-down permanent, so whether backlog item 12 covers them is a separate call. The remaining 97 were not classified by hand.
7. Backlog item 12 lists "face-down permanents" as one group. Either amend it to reject the 73 producers and allow the 229 face-down-cast cards, or reject all 302 as item 12 reads today.

## What was not verified

- No planeswalker, battle, saga or face-down card was cast or played through the engine. The "plays" findings come from reading the engine and grepping for the absence of code, not from a run.
- The face-down counts are heuristic, since the database has no face-down column.
- Whether the empty-string loyalty/defense values affect any current code path other than `as_fixed()`.
- The meld, mutate, class and similar layouts were only counted, not loaded.
- The Class "plays" finding comes from reading the code, as for sagas.
