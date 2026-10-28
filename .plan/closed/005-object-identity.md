---
id: 005
title: How object identity crosses the boundary
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

When CR 400.7 mints a new object id on a zone change, what does a Seat receive so the UI can keep showing the same physical card moving (hand to stack to battlefield)? Is there a stable per-card id alongside the object id, and how does it interact with hidden information (a card drawn from a library must not reveal which library card it was)?

## Notes

- See `docs/implementation/02-zone-moves.md` (CR 400.7 id minting) and the lineage link described in DESIGN.md's data model section.
- Overlaps with 004 on what a hidden card's id may reveal.

## Resolution

**Ids on the wire.** Seats receive the engine's `ObjectId` directly. There are no per-Seat opaque handles and no stable per-card id. `ObjectLineage` stays inside the engine. A live Seat follows a card through the old and new ids on each move event. A Seat that joins late or reconnects gets a snapshot and needs no history.

**Counter guarantee for 004.** Every zone change mints its new id from the global counter in `GameState::allocate_object_id`, so a hidden id says nothing about deck order or shuffle results. Library ids are the exception: `new_game` mints them in deck-list order before the opening shuffle. No Seat ever receives a library id without that card's identity. B4 adds a test for this.

**Move events per Seat.** Each `ZoneChange` a Seat receives has three optional parts:

- The old id, sent if the Seat could see the object in its old zone.
- The new id, sent if the Seat can see the object in its new zone.
- The identity and `last_known_info`, sent under 004's rules.

A Seat sees objects in public zones, including face-down objects without their identity, and in its own hand. Libraries and opponents' hands show a count only. For an opponent's Seat: a draw sends nothing but the move, a cast from hand sends the new id and identity, a face-down cast sends the new id only, a card put back into the library sends nothing, a mill sends the new id and identity, and a creature dying sends all three.

**Hands.** The engine does not model hand order. A hand is a set of ids, and the owner's UI arranges it. Opponents get the count only. A `Revealed` event for a card in a hand gives opponents its identity but no id, so they cannot follow that card once it is hidden again.

**Candidates and replies.** One typed reference, `Object(ObjectId) | Player(PlayerId)`, is used in Decision candidate lists, in the target slots of priority entries, and in Pick and Target Answers. Answers repeat references rather than indices. A Target Answer is one list of references per target slot, in slot order. An Order Answer is a permutation of indices into the Decision's candidate list, because two pending triggers can be identical (`PendingTrigger` holds only `source` and `ability`). The Decision id ties the indices to one list.

**Search lists.** Library candidate lists are sorted by `ObjectId`, never in library order, because 004 hides library order from the owner too. Library ids are not reminted on shuffle. B4 adds a test that a search candidate list does not follow library order.

