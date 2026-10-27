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
