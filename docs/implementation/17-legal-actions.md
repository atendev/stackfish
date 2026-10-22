# 17-legal-actions

B2 of the host protocol map. The design is in `.plan/closed/003-priority-decision-content.md` and `005`. The entry types
are in `crates/engine/src/decision/entry.rs`. The builders are in `commands/entries.rs` (priority) and
`commands/combat/entries.rs` (combat), next to the commands whose checks they mirror.

## API

- `Engine::legal_actions() -> Vec<Entry>` lists what the priority player may do: `PlayLand`, `Cast`, `Activate`, and
  `Pass` last.
- `Engine::legal_attackers()`, `legal_blockers(player)` and `pending_damage_splits()` list the combat options.
- `open_decision` calls them. `ReplyShape::Action` now holds the entries.

## Entries

- A `CastEntry` has the card, an optional `ModeRange`, `EntryTargets` (`Fixed` slots, or `PerMode` for a modal spell),
  an `x` flag, `normal` (castable for its normal cost), the valid `alternative_costs` indices and a `buyback` flag.
  `normal` is an addition to ticket 003. A flashback card in the graveyard is listed with `normal: false`.
- An `ActivateEntry` has the source, the `ability_index` (counting every ability on the source, as `find_ability` does),
  target slots, `x`, `mana_ability` and an optional `ColorChoice` for "add one mana of any color".
- A `TargetSlot` has an explicit candidate list (objects by id, then players, then abilities on the stack), `min`, and
  `max` (`None` for "N or more").
- No entry checks affordability.

## Rules the builders mirror

- Cast: not a land, in hand (or a commander in its owner's command zone), or in the `from_zone` of an alternative
  cost. Instants and flash need priority. Everything else needs the sorcery window. Every target word needs enough
  candidates. A modal spell needs enough usable modes.
- Activate: controlled by the priority player, in its function zone, Equip at sorcery speed, and a `{T}` cost needs an
  untapped source that is not summoning sick.
- Land: sorcery window and below the land play limit.
- Attackers use `attacker_can_attack` and `attack_target_is_legal`. Blockers use `block_is_legal`. These were pulled
  out of `execute_declare_attackers` and `execute_declare_blockers`, so the command and the entries share one rule.
- The whole-declaration rules (menace, "attacks each combat if able") live in `commands/combat/predicate.rs` and are
  shared the same way. An `AttackerEntry` has `required`, and a menace attacker is listed only when two creatures could
  block it. See `23-combat-predicates.md`.
- Splits use `plan_damage_splits`, pulled out of `execute_assign_combat_damage` the same way.

## Combat Decisions

Combat declarations are not `ProcessState` pauses. They happen in a priority window. `open_decision` offers them first:

1. `DeclareAttackers` for the active player while they hold priority, no combat exists and a creature can attack.
2. `DeclareBlockers` for the first defending player, in turn order from the active player, who still owes a
   declaration and has a creature that may block. An empty reply declares no blockers.
3. `AssignDamage` for the player who chooses a split in this damage step, with the splits that player chooses.
4. Otherwise `Action`.

## Answer checks

`decision/check.rs` checks an Answer against the entries before it reaches the engine: the card or ability is listed,
the cost route is offered, the mode count and X agree, every target is a candidate and the target count fits the slots.
Combat replies are checked the same way. A rejected Answer changes nothing.

## Tests

`crates/engine/tests/phase5_legal_actions.rs` (19 tests). One of them runs every entry of a mixed state through the
facade and the engine and expects no error.
