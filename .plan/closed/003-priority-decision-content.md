---
id: 003
title: What a priority Decision offers
type: grilling
mode: HITL
status: closed
blocked_by: [002]
claimed: false
---

## Question

At a priority Decision, does the engine enumerate every legal action (castable cards, activatable abilities, modes, targets, X values, alternative costs), or offer a coarse menu ("cast", "activate", "pass") and validate the detailed Answer when it arrives? How are payments and target choices split between the Decision and later Decisions?

## Notes

- There is no `Engine::legal_actions()` yet (see the doc comment above `ProcessState` in `process_state.rs`).
- `PlayerActionKind::CastSpell` bundles modes, targets, X and alternative cost into one action (`crates/engine/src/types/command.rs`).
- The random bot in the destination needs enough enumeration to always find a legal Answer.

## Resolution

**Playable entries.** A priority Decision lists one entry for each action the player may take: each castable card, each activatable ability, each land play, and pass. The Answer is still a full `PlayerAction`, and the engine validates it when it arrives. The engine does not list every concrete combination of modes, targets and costs.

**What an entry holds.** A spell or ability entry carries the fields the player announces at cast time:

- The mode range (`min` and `max`).
- The target slots, each with an explicit candidate list and a `min` and `max`. When the chosen modes change the slots, the entry lists the slots per mode.
- `x: true` when the cost has an `{X}`. There is no maximum.
- The valid alternative-cost indices.
- Whether buyback may be paid.
- For an ability, `mana_ability: true` when it is a mana ability, so a UI can fold those entries away.

**Split between Decisions.** Modes, targets, X, alternative cost and buyback stay in the priority Answer, as `CastSpell` and `ActivateAbility` work today. Payment stays a separate Payment Decision afterward. No new pause is needed.

**No affordability check.** An entry is listed when timing, zone, restrictions and target existence allow it. The engine does not check whether mana can cover the cost. The upper layer decides what to hide or skip. Because of this, the Payment Decision accepts a second Answer, "cannot pay", which calls `Engine::unwind_payment` (CR 601.6) and returns the card to hand. This amends the Payment shape from ticket 002. A bot that picks an entry it cannot pay therefore always has a legal way out.

**Mana abilities.** Listed as ordinary activate entries and flagged. A player can still float mana at priority.

**Combat.** The declare attackers, declare blockers and damage assignment Decisions list their options the same way:

- Eligible attackers, each with its legal defenders.
- Eligible blockers, each with the attackers it can block.
- The damage recipients for each attacker or blocker.

The engine still validates the whole declaration on Answer, because requirements such as "must attack" depend on the combination.

**legal_actions().** The engine builds the entry list from priority state. `Engine::legal_actions()` is the function that does this. A concrete expansion into single `PlayerAction`s, if the bot needs one, is host-side work on top of the entries. The bot picks its own X.

Handed to other tickets:

- 005: the identifier type for the candidate lists in entries.
- 004: whether a Seat sees another player's entries. Entries belong to the player who holds priority.
- 007: how entries and the "cannot pay" Answer are encoded on the wire.

