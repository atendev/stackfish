---
id: 162
title: Activation limits per turn
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

How does an activated ability declare "Activate only once each turn" and its combined forms, and where are spent uses recorded?

## Notes

- Origin: "Activate only as a sorcery on any ability". 108 printed cards say "once each turn", plus "as a sorcery and only once each turn" (8), "during your turn and only once each turn" (5), "during your upkeep and only once each turn" (3), "once" (16) and "as a sorcery and only once" (4).
- The limit is a CR 602.5b restriction that stays with the object across a control change. A new object is a new ability (CR 400.7).
- Weigh a field on `ActivationRestriction` and spent uses keyed by ability, like `GameState.zone_permission_uses` from 097, cleared each turn. Also weigh "Activate only once" with no turn reset.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

A use limit becomes a third part of the ability's activation restriction. Spent uses live in two `GameState` stores keyed by object and ability, and a use counts when the activation begins. Decided in auto mode. The build is "Build activation limits".

The card counts come from oracle text in `../stackfish-db/stackfish.db`. Decision 127's SQL found 108 "once each turn". A broader count, names whose text has both "once each turn" and "Activate", finds 170, which includes triggered "do this only once each turn" and granting shapes. "Activate no more than twice each turn" appears on 7 cards (Pit Imp, Vampire Bats) and "three times each turn" on 3 (Sewer Rats, Soul Kiss, Manaforge Cinder). "Activate only once." appears on 16.

- **Field.** `ActivationRestriction` gains `limit: Option<ActivationLimit>`, where `ActivationLimit { uses: u8, period: LimitPeriod }` and `LimitPeriod` is `Turn` or `ObjectLifetime`. One shape covers once, twice and three times each turn, and "Activate only once". A bool would miss the counts. A scripted counting condition would hide the limit from `can_activate_now`.
- **Key.** A spent use is keyed by `(ObjectId, AbilityId)`, with no controller, so a control change keeps it spent (CR 602.5b). A new object gets a fresh limit on both paths (CR 400.7). Script-registered abilities are dropped by `remap_abilities` (`commands/attach.rs:48`) and re-registered with a new `AbilityId` (`script/runtime.rs:244`). Abilities kept by that function's `ability.source = new_id` branch keep their `AbilityId`, and only the `ObjectId` part separates them. The `AbilityId` part lets one object have two limited abilities.
- **Stores.** `GameState.activations_this_turn: BTreeMap<(ObjectId, AbilityId), u8>` clears at the single turn-change site in `engine/turn.rs`, beside `once_per_turn_triggered`, with a `// CR 602.5b` citation. Extra turns pass through the same site. `GameState.activated_once: BTreeSet<(ObjectId, AbilityId)>` is never cleared or pruned. Stale keys never match, and it grows by at most one entry per "Activate only once" activation, like `commander_return_offered`. Counts stay out of `RegisteredAbility`, because they are game state and a remap would carry them. `AbilityId` gains `PartialOrd, Ord`. Both fields go into the `GameState` destructure in `engine/loop_watch.rs`.
- **When a use counts.** When that activation begins. For a stack ability this is the push in `apply_activate_ability`. For a costed mana ability it is the apply of that same activation, and for `ActivatePlan::FreeMana` it is the plan apply. It is never the outer payment's completion. This differs from 097, because a limited mana ability with a cost (Manaforge Cinder) can be activated again during its own payment pause. `unwind_payment` refunds only the use of the activation whose payment unwinds, by its own key. A mana ability completed inside that payment stays spent. A countered ability stays spent.
- **Combined forms.** `sorcery_speed`, `condition` and `limit` compose, with no combined variants. "As a sorcery and only once each turn" sets two fields. "During your turn" or "during your upkeep" plus a limit sets `condition` and `limit`. "Once each turn" counts on every player's turn. `can_activate_now` checks the limit last, so existing error strings stay stable. A spent ability leaves `activate_entries` through the same helper, and so it also leaves `mana_ability_entries`.
- **Lua.** On `ActivatedAbilityBuilder`, `:SetActivationsPerTurn(n)` sets `{ uses: n, period: Turn }` and `:SetActivateOnlyOnce()` sets `{ uses: 1, period: ObjectLifetime }`. n = 0 is refused, and so is a second limit call of either kind. Update `docs/lua/index.md`, `docs/LUA_API.md`, `docs/lua/activated-abilities.md` and `docs/lua/stubs/game.lua`.
- **Wire and replay.** No protocol change. `BEHAVIOUR_REV` does not move, because no existing script or fixture sets a limit. The build auditor confirms that.
- **Copies.** A copy of an activated ability is not activated (CR 707.10), so it spends no use. A permanent that enters as a copy is a new object and has a fresh limit (CR 400.7). A permanent that becomes a copy without changing zones stays the same object. The engine re-registers its abilities with new `AbilityId`s, so any limit restarts. The rules text here does not settle whether that is right, so a TODO citing CR 707.2 and 602.5b marks it, and docs and comments do not claim it is correct.
- **Out of scope, each with a TODO.** Loyalty abilities, one per permanent each turn (CR 606.3), including Urza, Planeswalker's "twice rather than only once". Exhaust. Mairsil, the Pretender's granted limits.
- **Tests.** A once-each-turn ability is refused after one use and is legal again next turn, including on the opponent's turn. A twice-each-turn ability exercises a count above one. "Activate only once" is not reset by the turn. A control change keeps the use spent. A zone change resets it. An unwound payment refunds the use. A mana ability completed inside an unwound outer payment stays spent. A costed limited mana ability cannot exceed its limit through a nested activation. One fixture proof card carries a once-each-turn ability.

New ticket: 169 Build activation limits, blocked by 160 and 161, so the shared struct lands in order.
