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
  an optional `ModeRange`, target slots (`EntryTargets`, per mode for a modal ability), `x`, `mana_ability` and an
  optional `ColorChoice` for "add one mana of any color". A modal ability is listed when some selection of modes has
  its targets, as a modal spell is. An opponent-chosen or pawprint activated ability is not listed, since
  `ActivateAbility` refuses both.
- A `TargetSlot` has an explicit candidate list (objects by id, then players, then abilities on the stack), `min`, and
  `max` (`None` for "N or more"), and `x_dependent`.
- A word whose `mana_value` filter mentions X (`ManaValueBound::X`) is `x_dependent`. The entry is listed before the
  caster announces X (CR 601.2b comes before 601.2c), so such a word lists every candidate legal for some X, by
  dropping the mana value restriction (`legal_target_candidates_for_x`). The facade's `check_targets` checks only the
  superset. The engine rejects a pair of X and target that does not match when the Answer reaches it, and the Decision
  stays open. The wire slot carries `x_dependent` only when true. It ignores whether the caster can pay that X, like
  every entry. The bot does not yet pick X and targets together (TODO in `bot.rs`).
- No entry checks affordability.

## Rules the builders mirror

- Cast: not a land, in hand (or a commander in its owner's command zone), or in the `from_zone` of an alternative
  cost. Instants, flash and spells a timing permission covers need priority (`can_cast_now`). Everything else needs the sorcery window. Every target word needs enough
  candidates. A modal spell needs enough usable modes.
- Activate: controlled by the priority player, in its function zone, Equip at sorcery speed, and a `{T}` cost needs an
  untapped source that is not summoning sick.
- Land: sorcery window and below the land play limit.
- Attackers use `attacker_can_attack` and `attack_target_is_legal`. Blockers use `block_is_legal`. These were pulled
  out of `execute_declare_attackers` and `execute_declare_blockers`, so the command and the entries share one rule.
- The whole-declaration rules (menace, "attacks each combat if able") live in `commands/combat/predicate.rs` and are
  shared the same way. An `AttackerEntry` has `required`, and a menace attacker is listed only when two creatures could
  block it. `legal_block_minimums(&blockers)` lists the listed attackers that need more than one blocker, so the
  block Decision carries `min_blockers` (`{ attacker, count }`). See `23-combat-predicates.md`.
- Splits use `plan_damage_splits`, pulled out of `execute_assign_combat_damage` the same way.

## Combat Decisions

Combat declarations are not `ProcessState` pauses. They happen in a priority window. `open_decision` offers them first:

1. `DeclareAttackers` for the active player while they hold priority, no combat exists and a creature can attack.
2. `DeclareBands` for the active player while they hold priority in the declare attackers step, bands are undeclared
   this combat and `legal_bands()` is non-empty. An attacker is listed when it forms a legal two-member band, which
   finds every member of every legal band. Attack triggers may already be on the stack (TODO, CR 508.1e). A second
   declaration is refused (CR 702.22e), so `CombatState::bands_declared` closes the Decision even for an empty reply.
3. `DeclareBlockers` for the first defending player, in turn order from the active player, who still owes a
   declaration and has a creature that may block. An empty reply declares no blockers.
4. `AssignDamage` for the player who chooses a split in this damage step, with the splits that player chooses.
5. Otherwise `Action`.

## Answer checks

`decision/check.rs` checks an Answer against the entries before it reaches the engine: the card or ability is listed,
the cost route is offered, the mode count and X agree, every target is a candidate and the target count fits the slots.
Combat replies are checked the same way. A band reply names only listed attackers, each once, and the engine checks
what makes a band legal. A Payment Decision's `Action` reply must activate one of its listed mana abilities. A rejected Answer changes nothing.

## Tests

`crates/engine/tests/legal_actions.rs`. One of them runs every entry of a mixed state through the
facade and the engine and expects no error.
