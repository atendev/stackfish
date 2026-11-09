---
id: 103
title: Damage redirected to another target
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

How does a replacement effect redirect damage to a target other than "this permanent"? Redirection supports only the source permanent today.

## Notes

- Origin: backlog item 10 and `docs/lua/not-available-yet.md` (Damage redirection).
- Decide who chooses the new recipient and when (CR 614.9, 615), whether a redirect to a player or planeswalker-like recipient is in scope given the design non-goals, and how a chain of redirects ends.
- Read `docs/implementation/08-replacements.md` and `docs/lua/replacement-effects.md`.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

A static redirect stays a `ReplacementEffect` row and gains recipients other than its own permanent. A redirect made by a resolved spell or ability is a row in a new `GameState` list of redirect shields. The work is four build tickets. Decided in auto mode, then approved.

- Carrier: `ReplacementEffect` rows have no duration, die with their source (`retire_object`), stop matching once the source object is gone (`event_target_matches`) and are found by `source`, so two shields from one permanent collide. A shield row has its own id and a new CR 616.1 candidate kind, as a Battlefield rider does. It also stores its creating object, read only to build the wire candidate. Nothing reads that for matching or lifetime. The shield list has its own copy of the CR 614.9 validity check.
- Recipients: a `RedirectRecipient` on the static payload and the shield row. `Fixed(Target)` covers a chosen target, "this creature" and "you", all fixed at creation. The attached creature, the affected object's controller, the damage source's controller and the damage source itself are read when damage would be dealt. The replacement loop resolves the recipient before `apply`, so `apply` keeps its signature. A recipient that cannot be resolved means the effect does nothing for that event, and a static `matches` returns false.
- Who chooses, and when: a chosen recipient is a normal target picked on cast or activation (CR 601.2c, 602.2b) and checked at resolution (CR 608.2b). After that only CR 614.9 applies. No recipient is chosen when the damage would be dealt. "Choose a creature" with no targeting is a resolution pick (Gideon's Sacrifice).
- Scope: players and creatures. `redirect_target_still_valid` keeps its planeswalker and battle arms, which match CR 614.9, with no new work. Its `!p.eliminated` check covers a destination player who left the game. The "from" half of that rule gets a test, or a note if no event can reach it.
- "To you" and "to your permanents": `Game.EventFilters.DamageDealtToPlayer` and `DamageDealtToPermanent` join `DamageDealt`, which still means either. `controller = "you"` matches a player and that player's permanents (`event_target_matches`), so the documented Pariah script also redirects damage dealt to the controller's creatures. The Pariah doc and script are corrected. "Other permanents you control" stays approximate, because a redirect to itself changes nothing.
- What a shield watches: one `DamageWatch` on the row holds the recipient (a fixed object, a fixed player, "you and permanents you control", "creatures you control"), the damage source (any, a chosen object, a spell type, attacking, unblocked) and combat only. Source properties are read at damage time (CR 609.7b). "You" is the player fixed at creation. "Creatures you control" is checked when damage would be dealt, so a creature arriving later is covered. CR 611.2c does not fix that set, because a shield changes neither characteristics nor control.
- Lifetime: `UntilEndOfTurn` only, since every printed non-Saga redirect says "this turn". Rows drain in `begin_cleanup_step` (CR 514.2). A row ends when a watched object leaves (CR 400.7) and never with its source (CR 611.2a). The list joins the `loop_watch.rs` fingerprint (CR 104.4b).
- Counting: `All`, `NextEvent` and `Budget(n)`. A budget smaller than the damage splits the event into the redirected part and a residual to the first recipient, at the `ReplacementCheck` stage. A shield is consumed only by a real replacement, and zero damage never matches (CR 120.8, 614.7a). `ReplacementEffect::matches` gains that amount check.
- Chains: each effect applies at most once per event (CR 614.5, 616.1f), with shield rows keyed by id. The candidate list is finite, so A to B and B to A ends. Candidates are re-evaluated after each rewrite (CR 616.2) and the affected player is recomputed (CR 616.1). A residual runs the same loop and inherits `excluded`, so `resolve_replacement_check_state` needs a per-event `excluded`.
- A source of your choice (CR 609.7a): a new `Game.ChooseDamageSource()` at resolution, with its own candidates of permanents, spells on the stack and face-up command zone objects. `Game.SelectTargets` runs target legality, which would wrongly refuse some sources.
- The damage itself: source, amount and combat flag stay the same (CR 614.9), so lifelink, deathtouch and infect read the original source. Protection on the new recipient is re-evaluated in the loop.
- Lua: `Game.ReplacementPayloads.RedirectDamageTo(recipient)` with a `Game.DamageRecipients.*` set, keeping `RedirectDamageToSource`. `Game.Ops.RedirectDamage(opts)` is an ordinary `AbilityEffect`, with the `engine/resolve/modal.rs` arm `Regenerate` has, and takes the watched slot, recipient, count and optional source. `docs/lua/stubs/game.lua`, `game-constants.md` and `replacement-effects.md` are updated.
- Wire: one additive `WireCandidate` arm `{ id, source }`, with `PROTOCOL_VERSION` unchanged. Shields are not listed in the snapshot, and that gets a TODO.

Two readings, not rules:

- A shield with no valid recipient is not used up. This is CR 609.7b applied to a failed CR 614.9 check. A "that source's controller" shield whose source is gone from `state.objects` also does nothing for that event.
- When a `NextEvent` or `Budget` shield could apply to several simultaneous damage events, the shielded player or the watched permanent's controller chooses. This copies CR 615.7, which is written for prevention. CR 616.1 does not fit, because it orders several effects on one event. Queue order can pick the wrong event when the sources differ, such as deathtouch on one blocker of a double block.

TODOs for the builds:

- `// TODO: the shielded player chooses among simultaneous damage events (CR 615.7)`, closed by "Build the simultaneous redirect choice".
- `// TODO: a permanent spell chosen as a source covers the permanent it becomes (CR 609.7a)`.
- `// TODO: an object referred to by a waiting effect as a chosen source (CR 609.7a)`.
- `// TODO: last-known information for a damage source that has left`.
- `// TODO: a recipient chosen by an opponent`, for Nova Pentacle, and `// TODO: a redirect to the owner`, for Personal Incarnation.
- `// TODO: static redirects with a source quality or an untapped condition`, for Martyrs of Korlis, Veteran Bodyguard, Weathered Bodyguards and Harsh Judgment.
- `// TODO: a fallback label for a shield whose creating object has left the game`.

Printed card counts from `stackfish.db` oracle text, hand classified and approximate: 12 static redirects, 13 "all damage this turn", 12 "the next time" and 19 "the next N damage". Twelve of the effect-made cards say "a source of your choice". Ascent of the Worthy is a Saga, so it is a non-goal.

Not this ticket: Carom, Captain's Maneuver, Kor Chant, Kor Dirge and Razia wait on "another target", because two target slots can pick the same object today. Damage doublers, excess damage (CR 120.4a), resolved prevention shields (CR 615.7) and Whippoorwill's "can't be dealt instead" are "Not yet specified" lines. Flaming Gambit is a choice during resolution, not a redirection.

Terms: `CONTEXT.md` adds "Redirect shield".

Builds, each blocked by the ones named:

1. "Build static redirect recipients".
2. "Build redirect shields". Blocked by 1.
3. "Build a source of your choice". Blocked by 2.
4. "Build the simultaneous redirect choice". Blocked by 2.
