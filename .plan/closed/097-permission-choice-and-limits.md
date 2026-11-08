---
id: 97
title: Player choice between matching permissions and once-per-turn limits
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

When several zone permissions allow the same cast, who picks which one applies (CR 601.2), and where does a once-per-turn limit on a permission live?

## Notes

- Origin: the Zone permissions entry in `99-backlog.md`, the TODO at `commands/legality.rs:82`, and 088's Resolution. Today a rider-less permission wins, then list order, because permissions have no id. Kess, Dissident Mage's once-per-turn limit is not represented.
- Decide whether permissions get ids, whether the choice is a Decision before or inside the cast announcement, how `legal_actions` lists the options, and how a per-turn use count is stored and reset.
- Read `docs/implementation/36-zone-permissions.md` and `38-zone-permission-riders.md`.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

Decided in an auto-mode grilling run. A subagent stood in for the user over two rounds and took every recommendation, with two additions noted below. The comprehensive rules name no procedure for picking between permissions (CR 601.2 lists the casting steps only), and the card database has no rulings, so the case comes from oracle text: Muldrotha, the Gravetide prints "If a card has multiple permanent types, choose one as you play it". The build is "Build permission choice and once-per-turn limits".

**The player chooses.** Which permission applies changes the rider and which limit is spent, so the engine does not pick. The rider-less-then-list-order rule goes away for a player's cast.

**The choice is an action field.** `CastSpell` and `PlayLand` gain `permission: Option<usize>`, as 091 did for division. No Decision opens for it. A Payment Decision accepts only a plan, a refusal or a mana ability, so the choice cannot go there.

**No permission ids.** The Answer is an index into `permissions`, a list on the cast entry and the land entry. An option is a rider plus a use key, where a use key is `(source, group)` and an unlimited row has none. The options are in row order, and rows collapse only when both parts match. Two unlimited rider-less rows from different sources are therefore one option, and that option's listed `source` is the first row's and is only a label. Two copies of Lurrus of the Dream-Den are two options because their use keys differ. An index therefore fixes the rider applied and the use spent. Each option lists a source and any group so a UI can tell limited options apart (an addition from the grilling subagent). No state changes between a Decision and its Answer (001, 024), so the index cannot go stale, and the options are computed before targets, so targeting cannot shrink them. No new id kind enters the fingerprint (029).

**When the field is required.** `None` with zero or one option. Required with two or more. An index that is out of range, or sent with fewer than two options, is rejected and changes nothing (024). There is no hidden default, since no rule or card states one. A lone option is still listed, so a UI has a label. `permission` together with `alternative_cost` is rejected: `AlternativeCost` has one variant, bound to its `from_zone`, so such a cast uses no permission and spends no use. A flashback card under a permission still lists both ways.

**Land plays get the same field.** Serra Paragon beside Crucible of Worlds is a real choice between a limited and an unlimited row. Dryad Arbor under Muldrotha has two options too.

**The limit.** `ZonePermission` gains `once_per_turn: Option<String>` (a group name) and `your_turns_only: bool`. Uses live in `GameState.zone_permission_uses: BTreeSet<ZonePermissionUse>`, where a use is `(source, group)`. A group lets two rows share one use (Serra Paragon, Kagha and The Eighth Doctor across a land row and a cast row) and lets one card keep separate uses (Muldrotha has one row per permanent type, and picking an option is the printed choice; Hedonist's Trove limits its cast row and not its land row). A new source object has a fresh limit (CR 400.7). The store is a set with no count, since no permission says "twice".

The store clears on every turn change, beside `once_per_turn_triggered`. It holds nothing that outlives its turn, so "once each turn" and "once during each of your turns" need no separate reset. The turn restriction is the separate flag because Festival of Embers and Valgavoth have it with no limit and Johann, Apprentice Sorcerer has a limit with no restriction. It compares `state.active_player` with the row's `controller`, whom the card's "your" refers to.

A row whose use is spent, or whose turn restriction fails, is not a match. It leaves the option list, `cast_zone_source`, `can_play_land_from_zone` and `legal_actions`. The check sits outside `covers`, so `library_top_visible_to` does not change.

**When a use counts.** When payment completes, at the one `SpellCast` log in `engine/payment.rs`. `apply_cast_spell` always pauses for payment, even for `{0}` (CR 118.5a), so there is one completion point. The chosen use key travels on `StackObject` beside `leave_stack_rider`. Only mana abilities run during a payment pause, so nothing can spend the limit twice, and a cast abandoned under CR 601.6 records nothing and needs no reversal. Countering the spell refunds nothing. A land play records at apply. The next Decision reflects the spent use.

**Internal casts.** A cast made from inside a resolving effect (`resolve_pending_cast`, cascade, madness) sends `None`, is never rejected for a missing field and spends no use, because the printed effect is what allows that cast. It takes a rider-less option, then row order, which keeps today's result (the tightening from the grilling subagent). A TODO names the missing player choice (CR 601.2).

**Timed grants refuse a limit.** No card found needs one, and a key of `(source, group)` would make two grants from one source share a use. The builder refuses `:SetOncePerTurn` on a Timed grant, with a TODO.

**Lua.** `:SetOncePerTurn(group)` takes a required string, `Register` refuses an empty one, and `:SetYourTurnsOnly()` takes nothing. No default group fits both Serra Paragon and Coram, the Undertaker. Same source and same group share one use.

**Wire and scenarios.** `permission` on both actions with `#[serde(default)]`. `permissions` on the entries as `[{ source, group?, rider? }]`, omitted when empty. Scenario steps gain `permission = n`, under the same rule. `PROTOCOL_VERSION` stays 1 (090).

**Bot and proof coverage.** The bot picks uniformly from `permissions` with its seeded RNG when there are two or more. Two fixture proof cards with overlapping instant and sorcery cast rows, one with a rider and one limited, reach the two-option branch. The seed loop asserts a two-option entry opens and that the option disappears once its use is spent, as 074 did.

**Card counts.** SQL over `cards.oracle_text`: 21 cards say "once during each of your turns", and 4 of those are from-hand or pay-life alternative costs, not Zone permissions (Zaffai, Vision, Demon of Fate's Design, One with the Multiverse). 5 say "once each turn". No permission says "twice". A narrow search found no Timed grant with a use limit.

**Follow-ups** go to Not yet specified in the map: once-per-turn on alternative-cost grants, a limit on Timing permissions, a limit on Timed grants, an alternative cost used through a permission (evoke through Muldrotha), and permissions with extra costs or riders on the permanent (Kotis, Festival of Embers, Edgar, Serra Paragon's exile clause).
