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

## Relationships

- A **Session** runs exactly one game and has one or more **Seats**
- A **Seat** belongs to at most one player; a player may have zero or more **Seats**
- A **Seat** sees only what the visibility rule allows: its own hand, public zones, counts for everything else. A non-chooser sees a **Decision** as its kind and a count
- At any moment a **Session** has at most one open **Decision**, and it names exactly one player who must give the **Answer**

## Flagged ambiguities

- The engine's own doc comments say "the caller should prompt" a player. In host-facing language the engine opens a **Decision** and the player gives an **Answer**. "Prompt" stays a verb for what a UI does, never a noun for the engine's waiting point.
- "Action" is overloaded. The engine's `PlayerAction` is one kind of **Answer**: the one given at a priority **Decision**.
