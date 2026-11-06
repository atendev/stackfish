# Stackfish

A deterministic Magic: The Gathering rules engine that a separate host application drives by exchanging messages.

## Language

### Host boundary

**Session**:
The host-facing wrapper around one game, which takes messages in as bytes and sends messages out as bytes. Hosts talk only to a Session, never to the engine directly.
_Avoid_: Server, connection, client

**Seat**:
One connected point of view on a Session, tied to a player or to no player at all. A Seat decides which information may be shown.
_Avoid_: Client, viewer, connection

**Spectator**:
A Seat tied to no player. The host may set its `omniscient` flag at creation, which lets it see every hand and library. A player Seat is never omniscient.
_Avoid_: Observer, viewer, watcher

**Poll**:
A request from a Seat for the events it has not received yet and its view of the open Decision. The Session queues each Seat's copy of every event, and a response carries only the requesting Seat's copy.
_Avoid_: Fetch, sync, subscribe

**Snapshot**:
A Seat's view of the current state, filtered by the same rule as its events and stamped with the last event it covers. A Seat that asks for one drops its queued events and resumes from the events after that point.
_Avoid_: Dump, state sync, resync

**Lapsed Seat**:
A Seat whose queued events passed the setup's `outbox_cap`, so the Session dropped them. It gets no events, and its responses carry the `resync` flag, until it asks for a Snapshot. The flag names what the Seat must do, not the Snapshot itself.
_Avoid_: Disconnected, stale, overflowed

**Decision**:
A point where the engine cannot continue until a specific player chooses, together with who must choose and what the choices are.
_Avoid_: Prompt, pause, request, choice

**Answer**:
A player's reply to the current Decision.
_Avoid_: Response, choice, action

**Advance**:
A host message that runs the engine while no Decision is open. It takes an optional step budget. A budget of 1 runs a single engine step. Answering a Decision also runs the engine, but always to the next Decision.
_Avoid_: Step, tick, poll

**Reply shape**:
The kind of reply a Decision asks for: pick from candidates, yes or no, order, modes, partition, payment, targets, or priority action. A Decision has one reply shape, and an Answer must match it.
_Avoid_: Type, kind, form

**Entry**:
One playable option listed in a priority, attack, block or damage assignment Decision, such as a castable card, an activatable ability, a land play or pass. An entry carries the options the player announces with it (modes, targets, X, alternative costs). The engine lists an entry without checking that the player can pay for it.
_Avoid_: Legal action, move, menu item

**CardSource**:
The interface a Session uses to turn a card key into a `CardDefinition`. The code that creates the Session passes it in, and it never travels in a message. One implementation reads the card database, another serves fixture cards.
_Avoid_: Card loader, card provider, database

**Card key**:
A card's `oracle_id` UUID, which is what a decklist and a commander slot hold. A host maps slugs or names to keys before it sends setup.
_Avoid_: Card id, slug

**Setup**:
The message that creates a Session: a player list in turn order, a decklist and optional commanders per player, a format with optional setting overrides, and a seed.
_Avoid_: Config, init, new game request

**Format**:
A named preset that fills the game settings, such as standard or commander. Setup overrides apply on top of it.
_Avoid_: Mode, ruleset

### Rules primitives

**Timing permission**:
A standing grant that lets a player cast matching spells as though they had flash. It is kept in `GameState.timing_permissions`, not on the layered `continuous_effects`, and it does not give the card the Flash keyword. `can_cast_now` is the one place that reads it. Where a card may be played or cast from is a Zone permission. A static one lasts while its source is on the battlefield, and a Timed grant lasts its `duration`.
_Avoid_: Flash grant, flash effect, timing modifier

**Zone permission**:
A standing grant that lets a player play a land or cast a spell from a zone other than hand, such as "play lands from your graveyard". It is kept in `GameState.zone_permissions` and names the zone, whose cards it covers and whether it plays lands or casts spells. A library-top permission opens only the top card of a library, which only its affected players may look at unless it reveals the top to everyone (Future Sight). A linked permission opens only the cards exiled with its source. `can_cast_from_zone` and `can_play_land_from_zone` are the places that read it. It says where, not when, so timing stays with the Timing permission and `can_cast_now`. Like a Timing permission, it is static or a Timed grant. A Cast permission may carry a Leave-stack rider. A card's own alternative cost, such as flashback, is not a Zone permission.

**Leave-stack rider**:
Where a spell goes in place of its usual zone when it leaves the stack, such as flashback's "exile it instead" (CR 702.34a) or Kess, Dissident Mage's "if a spell cast this way would be put into your graveyard, exile it instead" (CR 608.2n). It is `StackObject.leave_stack_rider`, copied at cast from the chosen alternative cost or the Zone permission the spell was cast through, so it is a fact about that one cast and outlives the permission. `StackObject::leave_destination` is the one reader.
_Avoid_: Play-from-zone effect, zone grant

**Timed grant**:
A Timing or Zone permission installed by a resolving spell or ability through `Game.Ops.GrantTimingPermission` or `GrantZonePermission`, with `UntilEndOfTurn`, `UntilYourNextTurn` or `UntilEndOfCombat` (CR 611.2a). It is independent of its source, so it survives the source leaving, and the turn drains remove it.
_Avoid_: Temporary permission, one-shot permission

**Linked exile (exiled with)**:
A record that a card was exiled by a particular source, kept in `GameState.exiled_with` (CR 607.2a). `Game.Ops.ExileLinked` writes it, and a linked Zone permission reads it. The link ends when the card leaves exile or the source leaves the battlefield, because either one becomes a new object. It is not the "exile until it leaves the battlefield" link in `linked_exiles`, which returns the card.
_Avoid_: Linked card, imprint

**Condition event**:
The plain Lua table a custom trigger condition receives for one ledger event. It carries only ids, zone and counter names, players, numbers and flags, never card characteristics, so it is safe to show to a condition any player controls. A ledger event kind that is not bridged arrives as `nil`.
_Avoid_: Event payload, trigger event

**Milled cards event**:
The one ledger record of a single mill action by one player. It lists the milled cards that ended up in a public zone, including a zone a replacement sent them to instead (CR 701.17c). Surveil never writes one, though its moves look the same.
_Avoid_: Mill event, mill trigger

## Relationships

- A **Session** runs exactly one game and has one or more **Seats**
- A **Seat** belongs to at most one player; a player may have zero or more **Seats**
- A **Seat** sees only what the visibility rule allows: its own hand, public zones, counts for everything else. A non-chooser sees a **Decision** as its kind and a count
- At any moment a **Session** has at most one open **Decision**, and it names exactly one player who must give the **Answer**

## Flagged ambiguities

- The engine's own doc comments say "the caller should prompt" a player. In host-facing language the engine opens a **Decision** and the player gives an **Answer**. "Prompt" stays a verb for what a UI does, never a noun for the engine's waiting point.
- "Action" is overloaded. The engine's `PlayerAction` is one kind of **Answer**: the one given at a priority **Decision**.
