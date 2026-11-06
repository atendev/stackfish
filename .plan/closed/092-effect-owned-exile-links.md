---
id: 92
title: Effect-owned exile links for impulse draw from a spell
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Who owns an exile link when a spell exiles a card and lets its controller play it this turn? `ExileLinked` records a link only under a battlefield source and the link ends when the source moves, so a Timed grant from a spell matches nothing.

## Notes

- Origin: `map.md` Not yet specified (found by 087), the TODO in `docs/implementation/99-backlog.md` under Zone permissions (CR 607.2a), and 087's Resolution.
- Decide whether the link belongs to the resolving effect or the stack object, when it ends (end of the permission's duration, not the source leaving), and whether `GameState.exiled_with` changes shape.
- Decide whether `ExileLinked` gets an untargeted or library form at the same time. Today it has none.
- Read `docs/implementation/36-zone-permissions.md` and `37-expiring-permissions.md` first.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

Decided in an auto-mode grilling run. A subagent stood in for the user. It took every recommendation except two: the `ForEachPlayer` wrap is deferred, and there is no wire change, because host snapshots do not serialize `ZonePermission`. The build is "Build effect-owned exile links and a library-top ExileLinked".

**Owner.** The Timed grant row owns the link, not a source object. A spell's "exile it; you may play it this turn" finds the exiled card through CR 400.7j: one effect, not two linked abilities. CR 607.2a stays the model for "cards exiled with [this]" on a permanent. The spell goes to its graveyard as a new object (CR 400.7), so nothing is keyed by its id and no purge needs an exemption.

**State.** `ZonePermission.linked: bool` becomes `linked: ExileLink { None, Source, Cards(Vec<ObjectId>) }`. `Source` reads `exiled_with` under the row's source as today. `Cards` matches only its ids. `exiled_with` (CR 607.2a) and `linked_exiles` (CR 610.3) keep their shape. A third record, `GameState.exiled_this_resolution: Vec<LinkedExile>` with `source = stack_id`, lives for one resolution. Every `ExileLinked` commit that lands in exile pushes to it, whatever zone its source is in; a card a replacement sends elsewhere is not pushed. `exiled_with` is still written only when the source is on the battlefield.

**Clearing the scratch list.** Cleared when a stack object starts resolving, beside the `modal_continuation` reset in `resolve/mod.rs`, and at the pipeline commit of a `StackObjectResolved` or `Fizzled` that `modal_resume` does not skip (`event_pipeline.rs`). Not at the push sites, because a chain pushes an intermediate `StackObjectResolved`. A reader also checks `source == stack_id`. A test asserts the list is empty whenever a player gets priority, so it never needs to join the loop fingerprint.

**Lua.** A new zone-permission builder setter, `:SetExiledByThisEffect()`, makes a Timed grant copy this resolution's ids into `Cards` when it resolves. `:Register` refuses it, because a static has no resolution, and it is refused with `:SetLinked` or outside `"Exile"`. `:SetLinked()` keeps its CR 607.2a meaning for statics and for a permanent's Timed grant over earlier exiles. A `Cards` row ignores `owners`, since the ids already name the cards and the `You` default would block an opponent's card. It still applies `cards` and `affected`.

**Ending.** The row ends at its duration through `expire_permissions`. `retire_object` and `remap_abilities` prune an id from `Cards` when its card moves, as they do for `exiled_with`. A row whose list empties stays until its duration and matches nothing.

**Library form.** `Game.Ops.ExileTopLinked(n)` is a new async `AbilityEffect::ExileTopLinked(u32)`. It picks the library with `affected_player`, like `Mill`: a player target if the stack object has one, else the controller. It takes `top_of_library_ids(player, n)`, so a short library exiles what it has and an empty one exiles nothing (no draw, no CR 704.5b). It exiles face up through `ScriptedZoneMoveKind::ExileLinked`. It stays out of `is_synchronous`. TODO: `ExileTopLinked` inside `ForEachPlayer`. The `ForEachPlayer` rejection message lists it. The untargeted battlefield TODO on `ExileLinked` stays, and face-down exile keeps its TODO (CR 601.3f).

**Wire and replay.** Host snapshots and the wire do not carry `ZonePermission`, so `PROTOCOL_VERSION` is unchanged. The loop fingerprint is the Debug string of the state and picks up `Cards` with no code change. No proof card, so `BEHAVIOUR_REV` stays 8 unless the seed 9 pin moves.

**TODOs.** The TODO on `ZonePermission::linked` about a spell's impulse draw is resolved and removed, along with its backlog line. New: "until the end of your next turn" has no `EffectDuration` (Light Up the Stage), and `ExileTopLinked` inside `ForEachPlayer`.

**Tests.** Act on Impulse as a scenario fixture (exile the top three, play a land and cast a spell from among them this turn, both gone after cleanup). Integration tests: `Chain(ExileTopLinked, GrantZonePermission)` with `:SetExiledByThisEffect`, a replacement redirecting one card so only the others are covered, a player target on an opponent's library with the controller allowed to play the card, a card that leaves exile and comes back not being covered, a short and an empty library, the `:Register` refusal, the `Cards` prune in both purges, the scratch list empty at priority, and a permanent's `:SetLinked` grant still reading `exiled_with`.

**Docs.** `docs/lua/player-modifiers.md`, `docs/lua/stubs/game.lua`, `docs/LUA_API.md`, `docs/lua/index.md`, `36-zone-permissions.md`, `37-expiring-permissions.md` and `DESIGN.md`. Keep `lua_api_doc_sync` passing.
