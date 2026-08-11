# 15-card-database-loader

New crate `crates/carddb` (`stackfish-carddb`) — the connecting point `DESIGN.md`'s "no card
database tooling" non-goal describes: reads the external card-database SQLite file and produces
real `stackfish_engine::CardDefinition`s from it. First real consumer of that non-goal — nothing
in this repo talked to a real DB before this slice; scenario TOML fixtures were the only card
source.

## Scope

Only structural/printed CR 613 layer-7 characteristics (cost, types, P/T, keywords) come from SQL.
Everything behavioral — `target_requirement`, `effect`, `alternative_costs`, `modes` — is left at
`CardDefinition`'s unset default; those are the card's own Lua script's job via the
self-registering `register_abilities(c)` module convention every hand-authored card already uses.
This loader never derives game behavior from oracle text.

## Mapping

- `cards.mana_cost` → `ManaCost::parse` (`crates/engine/src/types/mana.rs`) — already existed,
  written for exactly this Scryfall `{2}{R}`-style grammar; no new parser needed.
- `cards.type_line` → `types_line::parse` — splits on em dash; left side matched against
  `CardTypeSet`/`SupertypeSet` names (same vocabulary `stackfish_scenario`'s
  `parse_card_types`/`parse_supertypes` use, duplicated since that crate panics on an unrecognized
  name and this one must return a real `Err` instead); right side → `Vec<Subtype>` unconditionally.
  `Tribal`/`Kindred` both map to `CardTypeSet::KINDRED` (Scryfall still prints the pre-2024 name).
- `card_keywords`/`keywords` → `keywords::parse` — recognized names (~21) set `KeywordSet` bits;
  unrecognized ones (DB has Scryfall's full ~140-entry list) are silently dropped, matching
  `KeywordSet`'s own doc comment.
- `cards.power`/`toughness`/`loyalty`/`defense` → `parse_pt` — numeric string → `PTValue::Fixed`;
  anything else (`"*"`, `"1+*"`, null) → `PTValue::CharacteristicDefining { script: None }`.
- `card_scripts` (`WHERE oracle_id = ? AND is_active = 1`, highest `version`) →
  `ScriptReference::Inline(script_source)`. No active row (vanilla card) → a minimal valid stub
  module (`script::vanilla_stub`), not an empty string.
- `cards.has_multiple_faces = 1` → hard `CardLoadError::MultiFace`. `card_faces` isn't touched at
  all this slice — DFC/split/flip/adventure are an explicit `DESIGN.md` non-goal.
- A `type_line` word matching neither `CardTypeSet` nor `SupertypeSet` (Plane, Phenomenon, Scheme,
  Vanguard, Conspiracy, Dungeon, Class — all casual-format non-goals) → hard `CardLoadError::TypeLine`,
  not a skip. Decided explicitly (asked, not assumed): a bulk import needs a signal, not a silent drop.
- `CardDefinition.definition_id: u64` ← FNV-1a 64-bit hash of `oracle_id`'s UTF-8 bytes. Decided
  explicitly over SQLite `rowid` (not stable across a DB rebuild) and over
  `std::collections::hash_map::DefaultHasher` (documented as not build-stable across Rust
  versions/compilations — wrong choice for anything persisted).

## Known gap (real, not a bug)

`PTValue::CharacteristicDefining` produced by this loader always has `script: None`. The DB schema
has no column holding the bare `function(source, controller) -> integer` `ScriptRuntime::
invoke_cda` expects — only the full self-registering module lives in `card_scripts`.
`layers::effective_characteristics` already treats `script: None` as "skip rather than guess" (not
a panic — this predates this slice), so a `*`-power creature loads today with correct `display`
text but contributes no real layer-7a number. Revisit once the DB schema (or a new table) can hand
back a bare CDA function per card. Named in `parse_pt`'s own doc comment in `crates/carddb/src/lib.rs`.

## Verification

`cargo test -p stackfish-carddb` — 13 unit tests (type_line/keyword/hash/PT parsing, no DB needed)
+ 7 integration tests against an in-memory-schema fixture DB (built from the real table shapes,
written to a temp file since `CardDb::open` only takes a file path) covering: vanilla card with no
script row, scripted card with mixed recognized/unrecognized keywords, stable `definition_id`
across repeat loads, multi-face rejection, unsupported-type rejection, not-found, and the CDA gap.
`cargo build --workspace` confirmed clean alongside the pre-existing crates.

## Deviations from the plan

None — implementation matched the requirements report agreed before branching. The only two open
decisions (definition_id derivation, unsupported-type handling) were resolved by explicit
human choice before writing any code, not guessed mid-implementation.

## Follow-up: TOML scenario integration (`CardSpec.db_script_id`)

`crates/scenario` now depends on `stackfish-carddb`. `ScenarioFile` gained a top-level `card_db:
Option<String>` (path to the SQLite file); `CardSpec` gained `db_script_id: Option<String>`, which
looks the card up via the new `CardDb::load_by_script_id` (queries `cards.script_id`, the DB's
stable human-typed slug — e.g. `"lightning_bolt"` — not the printed `name` or the UUID
`oracle_id`) instead of building a `CardDefinition` from the rest of the `[[cards]]` entry's TOML
fields. Lets a scenario fixture test against the exact real card without retyping its mana
cost/types/power/toughness/keywords by hand.

- All three pre-existing `CardDefinition` construction sites (`build_initial_state`'s `[[setup]]`
  loop, `build_new_game_engine`'s library loop and commander loop) were duplicated code before this
  — extracted into one shared `card_definition_from_spec` (`crates/scenario/src/lib.rs`) that both
  the DB path and the pre-existing from-scratch TOML path go through. Preserves the two call sites'
  pre-existing (and mutually inconsistent) `definition_id` placeholder conventions exactly
  (`build_initial_state` used the real `ObjectId.0`; `build_new_game_engine` hardcoded `0`) rather
  than "fixing" that inconsistency as a drive-by of this refactor.
- `db_script_id` wins over every other characteristic field on the same `CardSpec` when set (not
  rejected if both are set — same "not a supported combination" stance this file already takes for
  `power`/`power_script`'s own overlap). `name`/`types` were widened from required to
  `#[serde(default)]` so a DB-lookup entry doesn't need to supply now-unused placeholder values.
  `script`/`mana_ability`/`trigger_condition`+`trigger_operation`/`continuous_effect` are unaffected
  either way — those build separate `RegisteredAbility`/`ContinuousEffect`/pending-script entries
  keyed off the object's id, not fields on `CardDefinition` itself, so a scenario can still attach a
  synthetic ability to a real DB-backed card.
- No active-`card_db` + `db_script_id`-set combination panics with a named message
  (`"no top-level card_db path set"`) rather than silently falling back to the from-scratch fields.

Verification: `cargo test -p stackfish-scenario --test card_db_lookup` (2 new tests: a real
DB-backed `[[setup]]` card loads correctly end to end; `db_script_id` with no `card_db` panics with
the expected message) + `cargo test --workspace` clean (every pre-existing scenario TOML fixture
unaffected — `card_db`/`db_script_id` are both purely additive/optional).

### Follow-up: relative `card_db` paths resolve against the scenario file, not the CWD

Caught before it became a real trap: `card_db` was originally handed straight to
`CardDb::open` -> `rusqlite::Connection::open`, so a relative path resolved against the test
process's *current working directory* at runtime. Under `cargo test` that's the crate root
(`crates/engine/`), not the individual scenario `.toml`'s own subdirectory under
`tests/scenarios/` — a relative `card_db` would have silently depended on which crate/directory
`cargo test` was invoked from.

Fixed by adding `ScenarioFile.base_dir: Option<PathBuf>` (`#[serde(skip)]`, populated by
`load()` from the TOML file's own parent directory — `None` for a `ScenarioFile` built directly via
`toml::from_str`, same as every existing integration test in this repo already does, so that path
keeps its pre-existing CWD-relative behavior unchanged). `open_card_db`'s new
`resolve_card_db_path` joins a relative `card_db` against `base_dir` when present; an absolute path
is always used as-is.

Verification: `relative_card_db_path_resolves_against_scenario_file_directory_not_cwd`
(`crates/scenario/tests/card_db_lookup.rs`) — writes both the scenario `.toml` and the SQLite
fixture into the same fresh temp directory with `card_db = "cards.sqlite"` (no directory
component), loads via `stackfish_scenario::load` (not `toml::from_str`, which never sets
`base_dir`), and confirms the card resolves correctly without ever touching the process's CWD.
