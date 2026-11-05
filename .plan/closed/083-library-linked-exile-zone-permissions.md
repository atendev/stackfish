---
id: 83
title: Library and linked-exile zone permissions
type: grilling
mode: HITL
status: closed
blocked_by: [82]
claimed: false
---

## Question

How does a permission let a player play the top card of their library, or cast from among cards exiled with a source, without leaking hidden ids (CR 401.5)?

## Notes

- Origin 080, which kept zone permissions to graveyard and exile.
- Needs the 004 and 005 rules for entries that list a library card, the linked-exile list, and CR 601.3f for face-down exile.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

A permission can now open the top card of a library and cards exiled with a source. Both stay Zone permissions. `ZonePermission` gains two flags, `library_top` and `linked`. The top card is never stored; each check finds it again. Linked cards live in a new list, `GameState.exiled_with`, separate from `linked_exiles`, which only serves "until it leaves" returns. Hidden ids stay safe because `legal_actions` is built per acting Seat. A library id reaches only the player who may play it, and reaches an opponent only when the top is revealed. Face-down linked cards are not offered, since no engine code makes a card face down yet. Build is 086.

The full answer set is below.

- Representation: `ZonePermission` (`types/player.rs`) gains `library_top: bool` and `linked: bool` and `reveals_top: bool`. With `library_top` the zone is `Library` and the card matches only when it is the first card of its owner's library. With `linked` the zone is `Exile` and the card id must appear in `GameState.exiled_with: Vec<LinkedExile>` under this permission's `source` (CR 607.2a). Reusing the `LinkedExile` struct is fine, but the list is new, because `return_linked_exiles` drains `linked_exiles` (CR 610.3). `SetZone` accepts `Library` only together with `library_top`.
- Predicates: the filters from 080 apply unchanged (`cards`, `owners`, `kind`). `can_cast_from_zone` and `can_play_land_from_zone` in `commands/legality.rs` call a helper `is_top_of_library(state, card)`. "From among" is the filter. Lands from the library use CR 305.1 and the existing land limit (CR 305.2a).
- Entries: `cast_entries` and `play_land_entries` add the top card only for the acting player whose permission applies. An opponent's entry list never holds it. The host redaction layer (004) is unchanged: the id of a hidden library card is never sent to a Seat unless `reveals_top` is set, in which case `is_visible_to` on `VisibilityPolicy` is public (CR 401.5). When the card is cast, the opponent sees only the new stack id (005).
- Visibility: two behaviours. `reveals_top` true means every player sees the top card (Future Sight). False means only the permission holder may look (CR 401.5, "may look"). The top is read at check time. Once casting starts the card is on the stack (CR 601.2a), so a top change mid-cast cannot alter the cast; the new top is neither revealed nor offered until the spell is cast (CR 601.2i), and the action list is rebuilt after.
- Face-down exile: out of scope. `GameObject.face_down` has no producer (`types/object.rs`), so `can_cast_from_zone` skips any `face_down` card and none is offered. This leaks nothing. The 080 TODO on CR 601.3f stays and is reworded to "face-down linked cards are never offered".
- Linked lookup: the exile effect that carries the link pushes `LinkedExile { source, exiled }` onto `exiled_with`. A card that leaves exile is a new object (CR 400.7), so its entry is purged by `retire_object`. When the source leaves the battlefield the entries die, since the new object is unrelated (CR 607.2a). The `attach.rs` identity cut and `remap_abilities` drop them as they do `zone_permissions`.
- Lifetime: both kinds are static, tied to the source on the battlefield (CR 611.3b). Effect-granted and expiring versions are ticket 084.
- Lua: `SetZone` accepts `Library`; new builder methods `SetFromLibraryTop()`, `SetLinked()` and `SetRevealTop()`. The exile builder gains `SetLinked()` to record the link. Update `script/builders.rs`, `globals.rs`, `types.rs`, `runtime.rs`, `docs/lua/stubs/game.lua`, `docs/lua/player-modifiers.md`, `docs/LUA_API.md`, `docs/DESIGN.md` and `docs/implementation/36-zone-permissions.md`.
- TODOs: "face-down linked cards are never offered (CR 601.3f)", "revealing the top applies only to permissions, not the stand-alone 'play with top revealed' effect (CR 401.5)", and "linked records do not cover cards exiled by a replaced source copy (CR 607.2a)".
- Tests: `tests/zone_permissions.rs` gets top-card land play, top-card cast, a new top after a play, the opponent's entries lacking the id with `reveals_top` false and showing it with true, the Seat view check, a linked cast, source leaving purging entries, and a face-down card absent. Add unit tests for `is_top_of_library` and the purges. Bump `BEHAVIOUR_REV` to 8 and re-pin the replay hash in its own commit.
- Proof: Future Sight (`{2}{U}{U}{U}` enchantment) replaces one Swamp in the blue-black proof deck, since red-green cannot cast it. Linked exile has no proof card, because the only real cards need face-down exile; a scenario fixture with a test script covers it.
- Terms: `CONTEXT.md` edits "Zone permission" to add the library-top and linked cases, and adds "Linked exile (exiled with)" with the 607.2a link. No ADR.
