---
id: 051
title: Audit card scripts for engine helper candidates
type: research
mode: AFK
status: closed
blocked_by: []
claimed: false
---

## Requirements report

### Successful implementation

Inspect every active card script available to this checkout and record:

- repeated multi-line Lua patterns;
- repeated filter-table shapes;
- script boilerplate that an engine primitive or a small shared Lua helper could replace;
- candidates that should remain card-specific because they encode card text rather than API noise.

The result must name the affected cards, count each repeated pattern, and state whether a
future helper belongs in the engine API, a shared Lua module, or neither.

### Comprehensive Rules requirements

- Mana-ability helpers must preserve CR 605 timing and activation behavior.
- Target helpers must preserve CR 601.2c target announcement and the existing engine legality
  checks.
- Spell operation helpers must preserve CR 608.2 resolution and target handling.
- `Balance` must preserve the player-by-player choice and sacrifice/discard behavior required by
  CR 101.4, CR 701.8, and CR 701.21.
- A helper must not move rules decisions from Rust into an unchecked Lua convenience function.

### TODOs

No source TODO is required. This ticket is an inventory for ticket 052. The implementation
tickets should add TODOs only where a selected canonical helper or API change is actually
implemented.

### Tests to add or update

- Load every active database script through the card database loader and fail on script errors.
- Confirm each basic-land script registers exactly one mana ability with its printed output.
- Confirm repeated targeted-spell shapes still reject an illegal target and resolve the legal one.
- Run the existing Balance scenario or add one that checks the minimum-land, minimum-creature,
  and minimum-hand choices for multiple players.
- Keep a regression test for the custom `Balance` operation's public-state-only access.

## Question

Which active card scripts contain repeated workarounds or boilerplate that should become an
engine helper or shared Lua helper?

## Notes

The checkout has no `cards/` directory. The active scripts were read from the sibling
`../stackfish-db/stackfish.db`, whose `card_scripts` table contains 27 rows, 25 active rows,
and two inactive rows. Script names below are the card names in that database.

## Resolution

### Inventory

The 25 active scripts divide into these groups:

| Pattern | Count | Cards |
|---|---:|---|
| `register_abilities` creates one mana ability and registers it | 12 | Forest, Island, Mountain, Plains, Swamp, Tundra, Wastes, and five snow-covered basics |
| `operation` plus one `target_requirement` | 7 | Counterspell, Death Ward, Lightning Bolt, Shock, Spell Blast, Unsummon, Sovereign's Bite |
| `operation` without a target | 3 | Armageddon, Quick Study, Balance |
| Empty module returning `s` | 2 | Benalish Hero, Roc of Kher Ridges |
| Static ability with an enchanted-only filter | 1 | Animate Wall |

The basic-land group is the only broad boilerplate candidate. Eleven scripts use the same
four-line builder and differ only in the mana output. Tundra uses `AddManaChoice(1, "WU")`
instead, and Wastes uses `AddMana("C")`; both are still instances of the same printed-mana
ability but need a choice-aware representation. A future helper could be useful only if it
takes a declarative mana-production value and still creates the normal `ManaAbility`. It should
not encode card names or bypass the builder.

Target tables repeat only seven times and are short. Five use `count = 1`, two use
`zone = "Battlefield"` with a creature filter, and two use `zone = "Stack"`. This is not
enough repetition to justify a script helper. The tables are card text data and should stay
visible unless ticket 052 selects a canonical target schema.

`Balance` defines `type_has` to inspect public `card_types`, counts lands and creatures by
controller, computes minimum counts, calls `ChooseN` three times, and merges the discard result
into the sacrifice result. The counting is specific to Balance's text and should remain in the
card script. The result-table merge is API friction, but replacing it needs a deliberate
multi-operation result shape decision in ticket 052 rather than a local helper.

`Sovereign's Bite` uses `Game.Ops.Chain` for two effects. This is the intended primitive, not a
workaround. The single-use `Game.Ops.ForEachPlayer`, `Game.Ops.DestroyAll`, and
`Game.Ops.AddManaChoice` calls are also direct matches for their card text.

### Candidates

1. **Possible engine/API helper:** a declarative mana-ability constructor for one fixed mana
   symbol or a set of choices. It should be considered only after ticket 052 settles the
   canonical `AddMana` and `AddManaChoice` forms.
2. **No helper:** target requirement tables. Their repetition is low, and hiding them would make
   card text harder to audit.
3. **No helper:** `Balance`'s public-state counting and choices. It is card-specific rules logic.
4. **API decision, not a local helper:** allow a chain or structured result to combine
   `SacrificeChosen` and `DiscardChosen` without manually copying `discard_chosen`.

### Missing-source limitation

The database contains only 25 active scripts, not the complete card-script repository referenced
by the cleanup map. This audit cannot establish whether a larger script set relies on ignored
table keys, repeated filter declarations, or local workarounds. Ticket 053 must not migrate or
strictly validate an absent `cards/scripts/` tree. It needs the complete script source or an
explicitly limited migration scope.
