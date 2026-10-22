# 23-combat-predicates

Ticket 026, branch `phase5/combat-predicates`. Ticket 024 decided that menace and must-attack are one predicate shared by the entries and the command validation. 035 made every rejection happen in `submit_action`. This slice puts the two rules in one module and teaches the entries about them.

## The predicate module

`commands/combat/predicate.rs` holds the whole-declaration rules. `declare.rs` and `entries.rs` call it, and so does `validate_pass_priority`.

- `legal_defenders` is the defender list that `attacker_entries` used to build inline.
- `required_attackers` (CR 508.1d) lists the creatures that attack each combat if able. A creature is required only when it passes `attacker_can_attack` and the active player has something to attack.
- `attack_declaration_is_legal` rejects a declaration that leaves one of them home. `validate_declare_attackers` calls it after the per-attacker loop.
- `pass_would_skip_a_required_attack` makes the active player's `PassPriority` in the declare attackers step a rejection while a required attacker is undeclared. Without it a host could skip the declaration with a pass, since combat declarations are not `ProcessState` pauses. This was not in the requirements report and is a scope addition.
- `blockers_needed` is 2 for menace and 1 otherwise. `block_declaration_is_legal` counts the direct blocks per attacker against it, before banding adds propagated blocks (CR 702.22h still bypasses the check).
- `blockable_attackers` lists each attacker with the creatures that could block it, and drops an attacker with fewer than `blockers_needed` of them. `blocker_entries` inverts that list.

## Must-attack

`KeywordSet::ATTACKS_EACH_COMBAT` (bit 23) is the requirement. It is not a CR 702 keyword, but it goes through the layer-6 fold like one, so `AddKeyword`, `RemoveKeyword` and `RemoveAllAbilities` work on it with no new layer code. Lua grants it as `"AttacksEachCombat"` (`game-constants.md` and the `game.lua` stub list it). The scenario format accepts the same name, and carddb maps the text "Attacks each combat if able". A card script can also set it in a printed keyword list through `decode.rs`.

## Entries and the wire

- `AttackerEntry` has `required: bool`. `WireAttacker` carries it as `required`. The field is additive, so `PROTOCOL_VERSION` stays 1.
- `check_attackers` refuses a reply that omits a `required` entry. This is the early filter. The engine check still runs behind it.
- A menace attacker is offered to a blocker only when the defending player has two creatures that could block it. The entry is still only a filter. Two menace attackers that share the same two possible blockers are both listed, and `block_declaration_is_legal` refuses a declaration that gives each one blocker.
- The bot declares a required attacker without a coin flip. It draws from its RNG exactly as before when nothing is required, so existing seeds give the same bytes.

## Tests

- `engine/tests/phase5_combat_predicates.rs` (13): empty, partial and full declarations with a required attacker; the cases where a creature is not required (tapped, summoning sick, defender, nobody to attack); passing priority; a granted and a removed requirement; the `required` mark on the entry; the facade refusal with the Decision left open; menace entries with one blocker, two blockers, a blocker that cannot block a flyer, and a plain attacker beside a menace one; the menace count at validation; two menace attackers sharing blockers; and a brute-force check over all 64 block assignments of a small board. Every pair the entries drop is refused in every declaration that contains it, and every listed pair is part of some accepted one.
- Replacing the menace count with 1, and the required list with an empty one, each fail 6 of the 13 tests.
- `host/src/wire/decision.rs` (1): `required` reaches the wire. `carddb/src/keywords.rs` (1): the clause sets the bit.
- Existing menace tests (`phase5_keyword_primitives.rs`, `phase5_atomicity.rs:841`) pass unchanged. `phase5_legal_actions.rs` needed `required: false` on its `AttackerEntry` literals.

## Open items

- `TODO(026-followup)`: "blocks if able" (CR 509.1c) and the maximum-requirements search. A menace attacker beside a "blocks if able" creature forces every legal blocker to block, which needs it.
- `TODO(026-followup)`: "attacks a specific player if able", costs to attack (CR 508.1g to 508.1j), and restrictions on the whole declaration ("can't attack alone"). Nothing in the engine produces them.
- The proof set has no menace or must-attack card, so the proof-game bot does not exercise either rule.
- A menace attacker that is `effect_blocked` (`BecomeBlocked`) is not special-cased.
