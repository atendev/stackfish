---
id: 101
title: Ward with non-mana costs, player hexproof and hexproof from
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

How do targeting protections grow past permanents with plain keywords? Ward takes only a mana cost, hexproof and shroud work only on permanents, and "hexproof from [quality]" does not exist.

## Notes

- Origin: backlog item 10 and `docs/lua/not-available-yet.md` (Ward, Hexproof and Shroud).
- Gaps to rule on:
  - Ward with life, discard or sacrifice costs, and a cost that depends on X.
  - Granting Ward to another permanent through a continuous effect.
  - Hexproof on a player.
  - "Hexproof from [quality]".
  - `IgnoreKeyword("Hexproof")`, which is accepted but unread.
  - Team rules stay a design non-goal.
- Read `docs/implementation/07-targeting.md` and `14-protection-attach.md`. The Ward payer queue TODO is in `types/ability.rs:402`.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

Ward takes structured costs and can be granted by a continuous effect. Players get hexproof and shroud through a new player static, and "hexproof from" is a layer-6 list. The work is four build tickets. Decided in auto mode, then approved.

- Cost shape: `Game.RegisterWard(source, cost)` and `Game.Ops.CounterUnlessPaid(cost)` share one decoder. `cost` is a mana string or a list of `Game.Costs.*` values, decoded into `CostSpec`. No Lua callback, as "Script-backed costs" decided. Of the Ward cards, 19 pay life, 13 discard, 8 sacrifice and 3 pay mana and life.
- Allowed variants: mana, `LifePayment`, `Discard { filter }` and `Sacrifice { filter }`. The loader rejects every other variant with an error naming it. The `*Self` variants read a `source`, and `submit_unless_paid_payment` only has `stack_id` there as a placeholder. Its comment changes to say the allowed set never reads `source`. Collect evidence, blight and similar wait on "Cost shapes past the current set".
- Payment: `payment` in `decision/open.rs` already lists `eligible` per additional cost, so the payer picks from that list. The `Unless paid` route must accept a plan with additional payments. `AdditionalCost::accepts` already reads filters relative to the payer, and one sacrifice ward test pins it.
- Computed amounts: "Pay life equal to its power" (2 cards) and `ward {X}` (Minthara) are not built, because CR 702.21b needs an amount evaluated at resolution. The loader rejects them. `// TODO: ward amounts computed at resolution (CR 702.21b)`.
- Granted Ward: a layer-6 `EffectPrimitive` holds the cost and collects into `EffectiveCharacteristics`. On `BecameTarget`, `collect_triggers` emits one `PendingTrigger` per instance. `source` is the warded permanent and `controller` is its effective controller (CR 702.21a). `ability` is one registered template that carries only the effect, target declaration and cost. Each instance triggers separately (CR 113.2c, 603.2c), and printed and granted instances add up. `RemoveAllAbilities` clears a granted instance (CR 613.1f). The granting permanent does not register its own trigger, because an Aura can have a different controller from the permanent it enchants.
- Printed Ward: `Game.RegisterWard` keeps its signature and becomes a self-only static on the same path, so Ward stops under `RemoveAllAbilities`. A test pins that. The "(CR 702.21b)" citation on instances in `script/globals.rs` is wrong and is corrected.
- Player hexproof and shroud: a new player-static builder in the shape of `HandSizeModifierBuilder`, with `Game.Affected.You`, `Opponent`, `Any` and `EnchantedPlayer`. `target_is_legal_impl` enforces it in the `TargetFilter::Player` and `PlayerScript` arms and the any-target player branch. `attachment_target_is_legal` skips it, because an Aura permanent does not target (CR 115.1b). Hexproof stops only spells and abilities of other players (CR 702.11c). Shroud stops the player's own too (CR 702.18a). "You and permanents you control have hexproof" (4 cards) is two registrations, with no combined `Affected` value.
- Hexproof from: a layer-6 `AddHexproofFrom(quality)` list with color, card type, monocolored and multicolored, checked against the source's effective characteristics (CR 702.11d). `target_blocked_by_hexproof_or_shroud` takes `source` as well as `caster`. It does not set `KeywordSet::HEXPROOF`, or plain hexproof would block every opponent. "Each color" expands to the five colors at decode (CR 702.11g), and "A and from B" is two entries (CR 702.11f). `RemoveKeyword(HEXPROOF)` and `RemoveAllAbilities` clear the list (CR 702.11e). "That color" and "activated and triggered abilities" (Volatile Stormdrake) wait with TODOs.
- `IgnoreKeyword`: "Hexproof" and "Shroud" become load errors. Detection Tower, Glaring Spotlight and Kaya bypass hexproof for one player's spells, which an object-level flag cannot say. No active script passes either name. `// TODO: a player-relative "as though it didn't have hexproof" permission (CR 702.11e)` replaces the `ignored_keywords` TODO in `commands/legality.rs`.
- Looking for hexproof: CR 702.11e says an effect that looks for hexproof finds "hexproof from". Nothing in Lua or `FilterSpec` reads keywords today, so this is a TODO that cites the rule.
- Team rules stay a non-goal and `// TODO: no team model` stays.

Each TODO above `target_blocked_by_hexproof_or_shroud` closes in one build: "hexproof from" in "Build hexproof from", which leaves a narrower TODO, and players in "Build player hexproof and shroud".

Builds, each blocked by the ones named:

1. "Build ward cost shapes".
2. "Build granted ward", which also moves printed Ward. Blocked by 1.
3. "Build player hexproof and shroud".
4. "Build hexproof from", which carries the `IgnoreKeyword` rejection.
