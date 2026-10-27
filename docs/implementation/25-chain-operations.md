# 25-chain-operations

## Slice: chain-operations (branch `phase5/chain-operations`)

- Scope: CR 608.2c. An ability or spell held one `Game.Ops.*` value, and a second `:SetOperation` overwrote the first.
  `Game.Ops.Chain(a, b, ...)` now builds `AbilityEffect::Chain(Vec<AbilityEffect>)`, which runs its entries in order.
- No new resolver. `Chain` runs through the modal loop (`engine/resolve/modal.rs`) with every entry chosen. The loop
  already flushes pending events before a pausing entry, holds the rest in `Engine::modal_continuation`, and resumes after
  CR 616.1 ordering choices, searches, scries and token attach choices.
- What differs from `Modal`: entries share the ability's one target requirement and one legal-target set. `ModalContinuation`
  gained `shared_legal: Option<Vec<Target>>`; `Some` marks a chain. `resolve_chain_effect` passes empty `words`, and
  `resolve_modal_from` builds each entry's view as the whole stack object with only `effect` and `targets` swapped, so
  `affected_player` and `Fight` read the ability's own requirement. The per-word chunks pass through unchanged.
- Validation is at load time in `Game.Ops.Chain`, using `commands::is_modal_resolvable` (now `pub(crate)`) so a chain can't
  hold an entry the loop would silently skip. Load errors: no arguments, a nested `Chain`, `ForEachPlayer`, `AddMana*`
  and the other kinds `is_modal_resolvable` refuses, a custom function, an uncalled constructor (the existing
  `reject_uncalled_op`, now `pub(crate)`, names it), and a non-operation value.
- `commands/activate.rs` lists `Chain` with the other non-mana activated-ability effects. A chain is not a valid `Modal`
  entry (`is_modal_resolvable` returns false for it).
- Verify: `phase5_chain_operations.rs` (20): 8 decode and load-error tests; destroy then draw on one target; untargeted
  chain; a player target read by two entries; order (`Untap, Tap` against `Tap, Untap`); damage committed before a later
  mill; a CR 616.1 pause in the first entry; a scry pause; an indestructible target leaving the next entry to run
  (CR 609.3); a gone target fizzling the whole chain (CR 608.2b); an activated ability; a scripted spell `operation()`;
  a real ETB trigger (cast the creature, resolve the spell, resolve the trigger) drawing and then losing life. Full workspace suite green, no new warnings.
- Open:
  - Separate targets per entry. `modal-multi-word-modes` already splits target words per mode for spells, so a chain with
    a `Multiple` spec could assign words to entries.
  - A chain inside `ForEachPlayer`, a custom function as an entry (it would need the entry's own `ScriptHandle` on the
    view), and a chain as a mode of a modal spell.
  - Scenario TOML has no field for a chain.

## Slice: chain-per-entry-targets (branch `phase5/chain-per-entry-targets`)

- Scope: CR 601.2c/608.2c. `Game.Ops.Chain` gave every entry the whole target set. `Game.Ops.ChainEach({ op, words }, ...)`
  gives entry i the `words` target words it owns, so "Draw 2 cards. Target opponent draws 2 cards." is one ability.
- `AbilityEffect::Chain` changed from `Chain(Vec<_>)` to `Chain { entries, words: Option<Vec<usize>> }`. `None` is the
  shared form from the first slice. `Some` reuses the modal loop's `words` convention unchanged: `resolve_chain_effect`
  passes `words` and the per-word chunks to `resolve_modal_from`, and `mode_view` builds each entry's view from its word
  range. Two small changes made that work for a chain: `mode_view` gained a `Primitive` arm (a lone word belongs to the
  entry whose range is `0..1`), and `resolve_chain_effect` builds the single chunk itself because `modal_chunk_legals` is
  only derived for a `Multiple`.
- `:SetTarget` on activated and triggered abilities takes an array of filters (`decode_target_spec_value`), one count-1
  word each. A one-entry array is a `Primitive`, like a bare table. The builders now store `Option<TargetSpec>` instead
  of `Option<TargetFilter>` and no longer wrap it at `Register`. This also lets a two-word fight be declared on an
  ability, which is untested.
- The operation and the target requirement are declared separately, so the word counts can't be checked in the decoder.
  `check_chain_words` (`script/decode.rs`) runs in both builders' `Register` and in `pregame::load_script_declarations`
  for a spell's `s.operation()` with `s.target_requirement()`. A mismatch fails the registration with the owned and
  declared counts, and drops the spell's effect and logs a script error.
- `DrawCards` now takes a player target. `resolve_draw_cards_effect` draws for `affected_player` when the target spec
  has a player word (`Player`, `PlayerScript` or `AnyTarget`), and for the controller otherwise. The first version used
  `affected_player` for any spec, which stopped "destroy target creature, then draw" from drawing (a creature spec has
  no player target). Two shared-chain tests caught it. `Mill` and `LoseLife` keep the stricter rule.
- Verify: `phase5_chain_per_entry_targets.rs` (17), 3 unit tests for `check_chain_words` in `decode.rs`, the 20
  shared-chain tests unchanged except for the new variant shape. Full workspace suite green, no new warnings. The tests
  cover "Draw 2 cards. Target opponent draws 2 cards." (hand sizes and draw order), a plain "target player draws",
  a creature word plus a player word plus a zero-word entry, partial and total fizzle, a CR 616.1 pause in the first
  entry, an activated ability with a two-word `:SetTarget`, and an ETB trigger that pauses for both words.
- Open:
  - A word shared by two entries ("destroy target creature, then its controller gains 3 life").
  - The spell-load word check (`pregame`) has no integration test. Only the function itself is unit tested.
  - `up_to` and `at_least` words in a `ChainEach` are not retested, and `:SetTarget` arrays can't declare them.
  - A two-word `Fight` on an ability is untested.
  - Same exclusions as `Chain`: `ForEachPlayer`, custom functions as entries, a chain as a modal mode, scenario TOML.
