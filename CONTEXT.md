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
A standing grant that lets a player cast matching spells as though they had flash, or activate equip abilities any time they could cast an instant. It is kept in `GameState.timing_permissions`, not on the layered `continuous_effects`, and it does not give the card the Flash keyword. `can_cast_now` is the one place that reads it for a cast. Where a card may be played or cast from is a Zone permission. A static one lasts while its source is on the battlefield and may hold only while a condition is true, and a card can hold one for itself in hand ("you may cast this spell as though it had flash if..."). A Timed grant lasts its `duration` and has no condition.
_Avoid_: Flash grant, flash effect, timing modifier

**Zone permission**:
A standing grant that lets a player play a land or cast a spell from a zone other than hand, such as "play lands from your graveyard". It is kept in `GameState.zone_permissions` and names the zone, whose cards it covers and whether it plays lands or casts spells. A library-top permission opens only the top card of a library. Who may see that card is a Library top view, not the permission. A linked permission opens only the cards exiled with its source. `can_cast_from_zone` and `can_play_land_from_zone` are the places that read it. It says where, not when, so timing stays with the Timing permission and `can_cast_now`. Like a Timing permission, it is static or a Timed grant. A Cast permission may carry a Leave-stack rider. A card's own alternative cost, such as flashback, is not a Zone permission.

**Permission option**:
One way that Zone permissions let a player cast a card or play a land: the Leave-stack rider it applies and the use it spends, if any. Permissions with the same rider and the same use to spend are one option, labelled with the first matching permission's source. A player names an option in the action when an Entry lists two or more. A card's own alternative cost is not a Permission option.
_Avoid_: Permission choice, grant

**Use group**:
The name that makes a Zone permission once per turn. Permissions from one source with the same Use group share one use each turn, so "play a land or cast a spell" spends one use whichever the player does. A new source object starts with an unspent limit (CR 400.7), and every Use group resets when the turn changes. A cast counts as a use when its payment completes.
_Avoid_: Limit key, counter, usage

**Library top view**:
A standing effect that opens the top card of matching libraries to an audience: everyone for "play with the top card of your library revealed", or some players for "you may look at the top card of your library any time" (CR 401.5). It has no card filter. Like a Zone permission, it is static or a Timed grant.
_Avoid_: Reveal permission, top reveal, reveals_top

**Shown top**:
The card a Library top view currently opens in one library, with its audience. The engine stores it and updates it only between actions, never while a spell or ability waits for payment (CR 401.5). Every Seat view reads the stored value.
_Avoid_: Revealed top, visible top

**Leave-stack rider**:
Where a spell goes in place of its usual zone when it leaves the stack, such as flashback's "exile it instead" (CR 702.34a) or Kess, Dissident Mage's "if a spell cast this way would be put into your graveyard, exile it instead" (CR 608.2n). It is `StackObject.leave_stack_rider`, copied at cast from the chosen alternative cost or the Zone permission the spell was cast through, so it is a fact about that one cast and outlives the permission. `StackObject::leave_destination` is the one reader.
_Avoid_: Play-from-zone effect, zone grant

**Battlefield rider**:
What happens in place of one named permanent's move off the battlefield, for a set duration, such as "if that creature would die this turn, exile it instead" or "if it would leave the battlefield, exile it instead of putting it anywhere else". It competes with other replacement effects on that move (CR 616.1) and ends when the permanent moves (CR 400.7), so it can outlast whatever created it. A Leave-stack rider is a separate thing that applies to a spell.
_Avoid_: Leave-stack rider, exile clause

**Redirect shield**:
A redirection effect made by a resolved spell or ability, such as "the next 1 damage that would be dealt to this creature this turn is dealt to target creature instead" (CR 614.9). It lasts for the turn and outlives whatever created it (CR 611.2a). A redirect written on a permanent, such as Pariah's, is a static replacement effect and not a shield.
_Avoid_: Redirection row, damage rider

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

**Mode**:
One option of a modal spell or ability (CR 700.2). It holds its own targets and operation, and a spell's mode can also carry its own cost and pawprint weight. The modes are chosen when the spell is cast, the ability is activated or the triggered ability is put on the stack, and only a chosen mode's targets are chosen.
_Avoid_: Option, choice, charm

**Solo-only mode**:
A mode that can be chosen on its own but not together with any other mode of the same spell or ability. No rule forbids choosing it with other modes, so this is a limit of the engine, not of the game, and a Decision says which modes it applies to.
_Avoid_: Exclusive mode, single mode


**Enter options**:
What a reanimation effect says about how a card comes onto the battlefield: who controls it, whether it is tapped or attacking, what counters it has and what keywords it gains. They come from the effect, not from the card.
_Avoid_: Entry options, return flags

**Enter choice**:
A choice a player makes as a permanent comes onto the battlefield with no spell behind it, such as what a returned Clone copies or what a returned Aura enchants (CR 614.12a, 303.4f). It is one open Decision, answered like the same choice on a resolving spell.
_Avoid_: Entry choice, entry pause

**Untap attempt**:
The moment a tapped permanent would become untapped, from the untap step or from an effect (CR 701.26b). A stun counter replaces the attempt with removing that counter, so the permanent stays tapped (CR 122.1d). A "doesn't untap" effect stops the attempt from happening at all, so no stun counter is removed. A permanent that is already untapped, or phased out, makes no attempt.
_Avoid_: Untap event, stun check

## Relationships

- A **Session** runs exactly one game and has one or more **Seats**
- A **Seat** belongs to at most one player; a player may have zero or more **Seats**
- A **Seat** sees only what the visibility rule allows: its own hand, public zones, counts for everything else. A non-chooser sees a **Decision** as its kind and a count
- At any moment a **Session** has at most one open **Decision**, and it names exactly one player who must give the **Answer**

## Flagged ambiguities

- The engine's own doc comments say "the caller should prompt" a player. In host-facing language the engine opens a **Decision** and the player gives an **Answer**. "Prompt" stays a verb for what a UI does, never a noun for the engine's waiting point.
- "Action" is overloaded. The engine's `PlayerAction` is one kind of **Answer**: the one given at a priority **Decision**.
- "Card key" in `docs/scenario_format.md` means the `key` string of a scenario's `[[cards]]` entry, a fixture name local to one file. It is not a **Card key**, which is an `oracle_id`. New scenario docs say "fixture name".
