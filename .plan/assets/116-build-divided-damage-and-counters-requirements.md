# 116 Build divided damage and counters: requirements

Builds the decision in `closed/091-divide-among-targets.md`. Branch `phase5/divided-damage-counters`.

## What a successful build looks like

A player can cast "deal 3 damage divided as you choose among one, two or three targets" (built as up to three targets, see the TODOs) and announce the split with the cast. The same works for an activated ability and a triggered ability's target answer, and for "distribute N counters among one or more target creatures". The engine refuses a bad split before anything changes. At resolution each still-legal target gets its own share, an illegal target's share is lost, and the spell fizzles only when every target is illegal. A Seat sees the total on the target slot, sends `division` beside `targets`, and an old client that omits `division` still works. The random bot builds valid splits, and a proof card exercises the path in the seed loop.

## Comp rules

- CR 601.2d: the division is announced as the spell is cast, after targets. Each target must receive at least one.
- CR 602.2b applies 601.2d to activated abilities. CR 603.3d applies it to a triggered ability put on the stack.
- CR 608.2b: an illegal target gets nothing, and the other targets keep the announced amounts. The spell or ability does nothing only when all its targets are illegal.
- CR 707.10: a copy copies the division. CR 707.10c: a copy that gets new targets may change the division (TODO, below).
- CR 115.3: the same object or player can't be chosen twice for one instance of "target" (existing dedup).
- CR 122.1: counters, `CounterAdded` per target.
- The build cites a rule for "the shares land together, before state-based actions" only after reading its text (candidate: CR 608.2c or 704.3).

## Engine changes

There is no separate Decision for the split, and `ReplyShape::AssignDamage` is not reused, since that is a resolution-time split keyed attacker to blockers.

1. **Types.** `PlayerActionKind::CastSpell` and `ActivateAbility` gain `division: Vec<u32>`. `StackObject` gains `division: Vec<u32>`, index for index with `targets`, empty for undivided or a single target. Every push site copies it, and a copy of a stack object keeps it.
2. **Effects.** `AbilityEffect::DividedDamage(u32)` and `AbilityEffect::DividedCounters { kind: CounterKind, total: u32 }`, plus `AbilityEffect::divided_total() -> Option<u32>`. Both are added to every exhaustive list (`activate.rs` non-mana list, `is_modal_resolvable`, `is_synchronous`) beside `Damage` and `Counters`.
3. **Validation.** A pure `validate_division(total, n_targets, division)`:
   - `n == 0` or `n == 1`: `division` must be empty.
   - `n >= 2`: length `n`, every share at least 1, sum equals `total`. So `n <= total` falls out.
   - `[total]` for one target is a wrong length.
   A wrapper `validate_effect_division(spec, effect, targets, division, ...)` runs it for all entry points:
   - No divided word: `division` must be empty.
   - `Primitive` spec with a divided effect: `validate_division` over the whole list.
   - `Multiple` spec with `Modal { effects, words }` (two or more modes): the flat list is cut into word chunks by index. A divided mode owns exactly one word. When `division` is non-empty it has one entry per flat target. A divided chunk with 2+ targets must have shares of at least 1 summing to its total. Every other position (an undivided chunk, or a divided chunk with 0 or 1 target) must be 0. When no divided chunk has 2+ targets, `division` must be empty. Two divided modes are checked separately.
   - `Chain` and function effects never carry a division (TODO below).
   To map shares to chunks, `split_targets_by_requirement` gains an index form (`split_target_indices_by_requirement`) and the target form is built from it, so the filter-assignment path keeps working.
4. **Entry points.** `CastSpell` (after `validate_targets` in `cast.rs`), `ActivateAbility` (`activate.rs`), and the trigger target answer. `submit_trigger_targets(chosen, division)` gains the field, and `Reply::Targets` becomes `{ per_word, division }`. The madness and cascade casts (`CastAnnouncement`, `submit_madness_cast`, `submit_cascade_cast`) also carry `division`, since they are synthetic `CastSpell`s. `Game.Ops.CastSpell` passes an empty division (TODO below).
5. **Load errors.** A shared `check_divided_shape(effect, target)` beside `check_chain_words`, called from the spell declarations in `pregame.rs`, both ability builders' `:Register()` and `decode_modes` per mode. It refuses: a divided effect with a total of 0, a divided effect whose target is not one `Primitive` requirement, a requirement minimum above the total (`count` when neither `up_to` nor `at_least`, `count` for `at_least`), and `DividedCounters` with a `Player` or `AnyTarget` filter. The Lua ops also refuse a total of 0 when called, so the error names the op.
6. **Entries.** `TargetSlot` gains `divide_total: Option<u32>`, and `max` is capped at the total (an `at_least` word gets `Some(total)`). `target_slots` takes the word's effect so it can fill it. Per-mode slots use each mode's own effect. `legal_actions` lists no compositions and checks no affordability. `ReplyShape::Targets` gains `divide_totals: Vec<Option<u32>>`, one per word.
7. **Resolution.** `DividedDamage` and `DividedCounters` arms in `resolve/mod.rs`. Each still-legal target gets its share, looked up by its position in the view's `targets` and `division`, or the full total when the division is empty or the entry is 0. One `DamageDealt` or `CounterAdded` per share, in target order, all pushed into one batch before the batch goes to `ReplacementCheck`, as the plain `Damage` arm does, so every share is dealt before state-based actions run and the shares land together. In a modal stack the per-chunk shares are filtered beside `modal_chunk_legals` and `mode_view` sets the view's `division`. The fizzle rule is unchanged, since it already fires only when every chosen target is illegal.

## Lua, wire, bot, proof card

- `Game.Ops.DividedDamage(total)` and `Game.Ops.DividedCounters(kind, total)` (`kind` is a counter kind name or `Game.Counter.*` tag, as for `Counters`), used like `Damage` with `s.target_requirement()` or `:SetTarget`. No per-share values in scripts. `DealDamageTo` and `AddCountersTo` stay per-call and the docs say they do not divide.
- Docs: `docs/lua/stubs/game.lua`, `game-constants.md` (and the `Damage` row's "no divide" note), `targeting-and-choices.md` ("still isn't modeled" notes), `docs/implementation/07-targeting.md`, `DESIGN.md`. `lua_api_doc_sync` keeps passing.
- Wire: `division` on the `cast_spell` and `activate_ability` actions, the `may_cast` cast and the `targets` reply, each `#[serde(default)]` and skipped when empty where serialized. `divide_total` on `WireSlot` and on each target word, skipped when `None`. `WireStackEntry` gets `division`, skipped when empty. `PROTOCOL_VERSION` stays 1.
- Bot: for a slot with `divide_total`, pick a count up to `min(max, total)`, then a random composition with each share at least 1. One target or none sends an empty division. Modal entries build the flat form (0 outside divided chunks of 2+).
- Proof card: a "divided as you choose" damage card (Arc Lightning text, built as `DividedDamage(3)` over up to three any-targets, so zero targets is accepted) joins a proof deck, and the seed loop asserts a divided cast happened.
- Serialization: engine state is never serialized. `crates/engine/Cargo.toml` has no serde dependency and `StackObject` derives only `Clone, PartialEq, Debug` (`types/stack.rs:17`). The snapshot's `WireStackEntry` (`crates/host/src/wire/snapshot.rs`) derives `Serialize` only and is never read back. The one save-and-load format is the replay file (`crates/host/src/replay.rs`), which records each accepted reply verbatim and rebuilds the game by replaying them. So the round trip is: play a divided cast, export the replay, load it into a new Session, and check that the rebuilt game's stack entry has the same `division` and the same damage results. `WireStackEntry` also shows `division`, as a display check only.
- Replay: run the pin. If it moves, bump `BEHAVIOUR_REV` to 11 and pin the new hash.

## TODOs to remove

`commands/cast.rs` (no division), `commands/legality.rs` doc on `validate_targets`, `engine/resolve/mod.rs` (divide-as-you-choose on `Damage`), `tests/multi_target_resolution.rs:2`. The plain `Damage` arm comment says each legal target takes the full amount (CR 608.2b).

## TODOs to add

- `// TODO: an X-dependent total ("X damage divided") (CR 601.2d, 107.3)` on the divided effects. The validator never reads `x_value`.
- `// TODO: divided amounts in function and chain operations (CR 601.2d)`.
- `// TODO: a copy that chooses new targets re-announces its division (CR 707.10c)` beside the copy code.
- `// TODO: untargeted division chosen at resolution (CR 608.2d)`.
- `// TODO: Game.Ops.CastSpell announces no division, so a divided spell cast this way takes at most one target (CR 601.2d)`.
- `// TODO: "one, two or three targets" has no minimum of one, since a TargetRequirement has no lower bound beside up_to (CR 601.2c)` on the proof card.

Ticket 103's constraint goes in `docs/implementation/07-targeting.md`, not code: a redirect effect, when one is built, must keep the announced division.

## Tests

Engine (`crates/engine/tests/divided_effects.rs` plus unit tests on `validate_division`):
- `validate_division`: sum mismatch, a zero share, wrong length, one target with an empty division accepted, `[total]` for one target rejected, no targets with a division rejected.
- Cast: a division on an undivided effect is rejected, a 2+1 split resolves to 2 and 1 damage.
- Partial illegal: one target leaves before resolution, the other keeps its share with no redistribution.
- All illegal: the spell fizzles.
- Modal: a two-mode spell with one divided chunk, and two divided modes validated separately.
- Activated ability with a divided effect. Trigger answer with a division.
- Counters: `DividedCounters` distributes, and a player target is a load error.
- Load errors: total 0, minimum above total, a `Multiple` target spec.
- Entries: `divide_total` set and `max` capped.
Host: a `cast_spell` without `division` still deserializes, a division round-trips through an exported and reloaded replay file (same stack `division`, same damage), the snapshot's stack entry shows it, and the bot's seed loop casts the proof card with a division.
