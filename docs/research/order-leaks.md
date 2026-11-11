# Remaining order leaks

Audited 2026-11-10 on `phase5/audit-order-leaks` against the code at master `318633d` in `crates/*/src`, the inline scenario scripts and `../stackfish-db/scripts`. Every file and line cited below was read in that code. No code was changed. The `pairs` probes ran in scratch crates outside the repo.

## Summary

No hash container leaks its order. Every hash container in non-test code is either read by key only or, for the loop fingerprint, sorted before comparison. None of them is consumed by `into_iter`, `extend` or `from_iter`.

Lua `pairs` leaks into one place: the text of a `ScriptError` event. Lua `pairs` order over string keys differs between processes (probe A) and can differ between two engine runtimes in one process (probe B). No engine input, the game seed included, reaches it. Three key checks walk a table with `pairs` and stop at the first bad key, so a table with two or more bad keys gets a different error message on each run. At game time that message travels in a `ScriptError` event, which is in the ledger and goes to every Seat. A buggy script can therefore give different response bytes for the same seed. Game state, Decision candidate lists and Answers are unchanged, because any bad key fails the script either way, and a replay plays out the same.

The clippy rules from ticket 075 also miss the hash set algebra methods. No current site uses them on a hash set.

Recommendation: one small build ticket, "Fix Lua key-error order and lint hash set algebra", with fixes 1 and 2 below. Fix 3 is a scripting rule for `docs/lua`.

## Method

1. Listed every `HashMap` and `HashSet` in `crates/*/src` (non-test modules). `crates/carddb`, `crates/cli` and `crates/godot` have none.
2. Followed every use of each container and checked for `into_iter`, `extend`, `from_iter`/`collect` out of the container, set algebra, `Debug` formatting and serde. Every field container was also grepped by name across `crates/*/src`, so a container passed to code in another file was followed there too. A `collect` cannot take a hash container's order without first calling `iter`, `keys`, `values` or `drain`, which the lint refuses (`clippy.toml:2-13`), or `into_iter`, which is in the grep below. The three `.drain(..)` calls in `crates/*/src` (`script/runtime.rs:207`, `:263`, `:277`) take a range, which only `Vec` and `VecDeque` accept, and their receivers are `Vec` registry queues. Bare `IntoIterator::into_iter(..)` and `::from_iter(..)` calls have no hits in `crates/*/src`. As a cross-check, grepped the 21 engine source files that hold a hash container, plus all of `crates/host/src` and `crates/scenario/src`, for `into_iter()`, `.extend(`, `from_iter`, `into_keys`, `into_values`, `extract_if`, `.union(`, `.difference(`, `.intersection(` and `symmetric_difference`. Each of the 75 hits has a `Vec`, an array, a `VecDeque`, an `Option`, a JSON array or a `BTreeSet` as its receiver or argument, or builds the `objects` `BTreeMap` in a test fixture. None is a hash container.
3. Listed every Rust walk over a Lua table (`Table::pairs`) and every Lua `pairs` or `next` in the engine's Lua prelude, the inline test scripts, the scenario TOML files and the 408 scripts in `../stackfish-db/scripts`.
4. Ran the `pairs` probe.

## Rust hash containers

Columns: whether the container's order can reach state (S), a Decision candidate list (D), the ledger (L) or the replay (R).

| Site | Type | How it is read | S | D | L | R | Fix |
|---|---|---|---|---|---|---|---|
| `GameState.lands_played_this_turn` (`types/game.rs:54`) | `HashMap<PlayerId, u32>` | `get`, `entry`, `clear` | no | no | no | no | none |
| `GameState.last_active_turn_number` (`types/game.rs:63`) | `HashMap<PlayerId, u32>` | `get`, `insert` | no | no | no | no | none |
| `GameState.once_per_turn_triggered` (`types/game.rs:118`) | `HashSet<AbilityId>` | `contains`, `insert`, `clear` | no | no | no | no | none |
| `GameState.commander_return_offered` (`types/game.rs:122`) | `HashSet<ObjectId>` | `contains`, `insert` | no | no | no | no | none |
| `GameState.linked_ability_generation` (`types/game.rs:133`) | `HashMap<AbilityId, u32>` | `get`, `insert`, keyed purge | no | no | no | no | none |
| `GameState.script_ability_ids` (`types/game.rs:140`) | `HashSet<AbilityId>` | `insert`, keyed purge | no | no | no | no | none |
| `GameState.ability_function_zones` (`types/game.rs:145`) | `HashMap<AbilityId, ZoneKind>` | `get`, `insert`, keyed purge | no | no | no | no | none |
| `GameState.off_battlefield_scripts_run` (`types/game.rs:150`) | `HashSet<ObjectId>` | `contains`, `insert`, `remove` | no | no | no | no | none |
| `PlayerState.commander_casts` (`types/player.rs:63`) | `HashMap<String, u32>` | `get`, `entry` | no | no | no | no | none |
| `CombatState.pending_damage_splits` (`types/combat.rs:40`) | `HashMap<ObjectId, Vec<_>>` | `contains_key`, `insert`, `clone`, index by attacker or blocker id | no | no | no | no | none |
| `assignment_map` (`commands/combat/damage.rs:500`) | `HashMap<ObjectId, Vec<_>>` | `insert`, `len`, index by id | no | no | no | no | none |
| `seen` in `activate_entries` (`commands/entries.rs:268`) | `HashMap<ObjectId, u32>` | `entry` per ability, walked in `abilities` order | no | no | no | no | none |
| `loaded` (`engine/pregame.rs:222`) | `HashMap<String, _>` | `get`, `insert` cache | no | no | no | no | none |
| `zones` (`script/runtime.rs:206`) | `HashMap<usize, ZoneKind>` | built from a `Vec` drain, read by index | no | no | no | no | none |
| `ScriptRegistry.scripts`, `suspended_threads` (`script/types.rs:146`, `:206`) | `HashMap<ScriptHandle, RegistryKey>` | `get`, `insert`, `len` | no | no | no | no | none |
| `seen` dedup sets (`decision/check.rs`, `commands/cast.rs`, `commands/legality.rs:918`, `engine/choice_answers.rs`, `engine/library_choices.rs`, `engine/triggers_pipeline.rs`) | `HashSet<_>` | `insert` as a duplicate test while walking a `Vec` | no | no | no | no | none |
| `listed`, `given`, `recipients` (`decision/check.rs:304-316`) | `HashSet<ObjectId>` | equality and `contains` | no | no | no | no | none |
| `FixtureCardSource.cards` (`host/card_source.rs:33`) | `HashMap<String, CardDefinition>` | `get`, `insert` | no | no | no | no | none |
| `loaded` (`host/setup.rs:152`) | `HashMap<String, CardDefinition>` | `get`, `insert` cache | no | no | no | no | none |
| `retired` (`host/view.rs:320`) | `HashMap<ObjectId, Place>` | `get`, `insert` | no | no | no | no | none |
| `Bot` `pools`, `life`, `costs`, `events` (`host/bot.rs:99-122`) | `HashMap` | `get`, `entry`, `insert`; `events` is extended from a JSON array into its `Vec` value | no | no | no | no | none |
| `index_by_source` (`scenario/auto_pay.rs:115`) | `HashMap<ObjectId, u32>` | `entry` per ability | no | no | no | no | none |
| `card_specs` (`scenario/build.rs:528`, `build_new_game.rs:52`) | `HashMap<&str, &CardSpec>` | `get`, index | no | no | no | no | none |
| `existing_keys` (`scenario/inline_setup.rs:25`) | `HashSet<String>` | `insert` as a duplicate test | no | no | no | no | none |

Notes on the sites that come closest:

- The loop fingerprint (`engine/loop_watch.rs:125-135`) formats every hash field of `GameState` with `{:?}`, which walks in hash order. `sort_hash_collections` sorts the entries of every `{...}` collection after the ids are relabeled by rank, so two equal states give equal text. The relabel ranks are taken from a sorted, deduplicated list, so they do not depend on print order either. No leak.
- `host/replay.rs:98-106` uses `symmetric_difference` and `intersection`, but on two `BTreeSet`s. No leak.
- The engine has no serde, and no host mirror type holds a hash container, so no hash order reaches the wire through serialization.
- Collecting into a hash container (`from_iter` with a hash target) cannot leak. Every `from_iter`/`collect` site above builds a hash container from an ordered source and never walks it.

## Lua `pairs`

### Probes

The ticket asks for one script walked twice under different seeds. The seed that decides `pairs` order is the Lua state's string-hash seed. Lua 5.4 makes it from the clock and from addresses each time a state is created (`luai_makeseed` in `lstate.c`). The game seed never reaches it: `ScriptRuntime::new()` takes no argument, and `sandboxed_lua` (`script/runtime.rs:48-60`) builds the state with `LuaOptions::default()`. So, from the code, the game seed does not reach the Lua state. No probe varied the game seed. These probes vary the Lua string-hash seed instead, by creating new Lua states. That seed is independent of the game seed.

Probe A is a scratch crate on `mlua 0.9.9` with the engine's features (`lua54`, `vendored`) and the same `StdLib` set as `sandboxed_lua`. It walks a ten-key string table with `pairs` in two Lua states per process, in five processes one second apart. Both walks in a process matched, and every process gave a different order:

```
control,at_least,alpha,up_to,delta,charlie,foxtrot,bravo,echo,tapped
control,at_least,charlie,delta,up_to,bravo,echo,foxtrot,tapped,alpha
foxtrot,control,echo,alpha,tapped,up_to,bravo,at_least,delta,charlie
bravo,foxtrot,alpha,tapped,control,up_to,echo,delta,charlie,at_least
at_least,delta,foxtrot,control,charlie,bravo,alpha,tapped,echo,up_to
```

Probe B is a scratch crate that depends on `stackfish-engine` by path, so it goes through the real sandbox and a real leak site. It creates two `ScriptRuntime`s per process and calls `build_target_requirement` on a script whose requirement table is `{ count = 1, zone_typo = 1, kind_typo = 2, alpha_typo = 3, max_typo = 4 }`. Over five processes the ten errors named:

```
max_typo   zone_typo
max_typo   alpha_typo
kind_typo  max_typo
zone_typo  alpha_typo
max_typo   max_typo
```

Each row is one process. Probe A's two states per process happened to match, but probe B's rows 1 to 4 show two runtimes in one process disagreeing, so same-process stability cannot be relied on. A replay starts a new game through `Session::from_replay` (`host/session.rs:172`) and `Replay::start`, and every new game builds its own `ScriptRuntime::new()` (`engine/pregame.rs:205`), so a replay gets a fresh order whether or not it runs in the recording's process. The existing TODO at `script/runtime.rs:46` describes this and stays.

Integer keys are hashed with no seed (`hashint` in `ltable.c`), so a table keyed by player id or object id has the same `pairs` order on every run, given the same inserts. This is from reading the Lua source and was not probed.

### Sites

| Site | Keys | When it runs | Effect of order | S | D | L | R | Fix |
|---|---|---|---|---|---|---|---|---|
| `reject_unknown_keys` (`script/decode.rs:308`), 10 callers, one of them `decode_object_filter_keys` (`:386`), which probe B reaches through `build_target_requirement`, `decode_target_spec_table` (`:1285`), `decode_target_requirement_table` (`:1230`) and `decode_target_filter_value` (`:1252`) | strings | load and game time | names the first unknown key it meets; probe B shows the name changing | no | no | yes, in `ScriptError.message` at game time | no | fix 1 |
| `decode_entry_options` (`script/globals.rs:1631`), from `Game.Ops.ReturnToBattlefield` and `ReturnAllToBattlefield` | strings | when the op is built, at load or inside a resolving function | names the first bad key or value it meets | no | no | yes, as above | no | fix 1 |
| `Game.ChooseN` opts check (Lua prelude, `script/globals.rs:1382`) | strings | resolution | names the first unknown key it meets | no | no | yes, as above | no | fix 1 |
| `reject_uncalled_op` (`script/decode.rs:86`) | strings | load | finds the one `Game.Ops` name bound to a function; no two names share a function | no | no | no | no | none |
| `decode_target_spec_table` (`script/decode.rs:1285`) and `decode_target_spec_value` (`:1351`) | any | load | counts keys only | no | no | no | no | none |
| Balance (`../stackfish-db/scripts/m00003639.lua:30`, and the inline copy in `crates/engine/tests/scenarios/scripting/mass_choice_balance.toml:56`) | player ids | resolution | takes three minimums, which ignore order | no | no | no | no | none |

No other card script, inline test script or proof card calls `pairs` or `next`.

"Ledger: yes" in the table means only the text of a `ScriptError` event, and only for a script with two or more bad keys in one table. The game takes the same path either way, because any bad key fails the script. `view.rs:460` shows `ScriptError` to every Seat, so the response bytes differ between runs. Replay still plays out the same, because it replays Answers and not event text.

## Lint gaps

The 075 lint (`clippy.toml` plus `iter_over_hash_type`) refuses `for` loops and the listed walking methods. It does not list:

- `HashSet::union`, `difference`, `intersection`, `symmetric_difference`: they return iterators in hash order. No current site uses them on a hash set.
- `HashMap::extract_if` and `HashSet::extract_if`: not used. (`drain` and `retain` on both types are already refused, at `clippy.toml:9-10` and `:12-13`.)
- An explicit `.into_iter()` on a hash container, or passing one to `extend` or `Vec::from_iter`. These go through the `IntoIterator` trait rather than an inherent hash method. As far as the clippy docs describe it, a `disallowed-methods` entry names a method path, so banning `IntoIterator::into_iter` would ban it on every type. This was not tested with clippy. No current site does this, so review has to catch new ones unless a test proves a narrower entry works.

The set algebra and `extract_if` methods are inherent methods on `HashSet` and `HashMap`, the same kind of path as the `HashSet::iter` entry at `clippy.toml:11`, which ticket 078 built and which clippy enforces today. `extract_if` is stable on the pinned toolchain (`rust-toolchain.toml:2`, channel 1.98.0).

## Fixes

1. Make the three key checks order-free: collect every bad key, sort, and name them all (or the smallest) in one error. That covers `reject_unknown_keys`, `decode_entry_options` and the `Game.ChooseN` prelude, which can check against a sorted key list in Lua. A test runs a script with two bad keys in a fresh `ScriptRuntime` several times and asserts one message.
2. Add `HashSet::union`, `difference`, `intersection`, `symmetric_difference` and both `extract_if` paths to `disallowed-methods` in `clippy.toml`.
3. Add a rule to the Lua scripting docs: a script that needs order walks with `ipairs` or sorts its keys first, and never lets `pairs` order over string keys decide an outcome. The Lua stub needs no change.

Replacing `pairs` and `next` in the sandbox with a sorted walk was considered and is not recommended. It would cost every script a sort, and fix 3 covers the risk.

## Not covered

- `tostring` of a table or function prints an address that changes between runs. That is an identity leak and not an order leak. No script uses it.
- Test-only code (`#[cfg(test)]` modules and `crates/*/tests`) was not audited, because it does not reach a Seat or a replay.
