---
id: 94
title: Reveal event for a library top and the stand-alone reveal effect
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

How does an event-only client learn that a library top became revealed or stopped being the top? And does the stand-alone "play with the top card of your library revealed" effect (CR 401.5) get built?

## Notes

- Origin: `map.md` Not yet specified (found by 086) and the Zone permissions TODO in `99-backlog.md`. A revealed top shows in snapshots and Decisions today, but no event announces it.
- Decide the event name and payload, who produces it, how it interacts with the Seat visibility rule in `crates/host/src/view.rs` (ticket 004), and whether `reveals_top` on a permission and the stand-alone effect share one representation.
- Read `docs/implementation/36-zone-permissions.md` and `31-snapshot-fold-events.md`.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

Decided in an auto-mode grilling run. A subagent stood in for the user over four rounds, took every recommendation and added the conditions below. A haiku auditor approved the question report in its second round. The build is "Build library top views and the shown-top event".

**Representation.** "Who sees a library top" is a new row, `LibraryTopView`, in a new `GameState` list beside the permission rows. `reveals_top` leaves `ZonePermission`, which then only grants playing or casting. Future Sight registers one reveal view plus its two permissions, as its two sentences say. A filtered cast permission (Garruk's Horde) no longer hides a top card that fails the filter, because the reveal has no filter (CR 401.5).

**No implicit look.** A library-top permission does not let its affected players see the top. Only a view does. A "you may look at the top card any time" card (Vizier of the Menagerie, Bolas's Citadel) registers a look view. The chooser's own Decision still carries the identity of a hidden card it is offered. This revises 083's "only the permission holder may look". The build greps scripts, fixtures and the docs for a play-from-top card that would lose its look.

**Row shape.** `LibraryTopView { source, controller, owners: FilterController, audience: TopAudience, duration: EffectDuration }` in `types/player.rs`. `TopAudience` is `Everyone` (reveal) or `Players(FilterController)` (look, relative to `controller`). There is no card filter. `owners` is `You` for Future Sight and Courser of Kruphix, and `Any` for Lantern of Insight. `EnchantedPlayer` is refused for both. A static row is dropped with its source by `retire_object` and `remap_abilities` (CR 611.3b). A Timed row from a resolving effect expires through the `engine/turn.rs` drains, and the durations permissions reject stay rejected. When several rows cover one library, `Everyone` wins and look rows union their players. The audience resolves relative to `controller` at sync time.

**Shown top.** `GameState` stores each library's shown top: the top `ObjectId` and its resolved audience, or nothing. A `pub(crate)` sync function beside `library_top_visible_to` computes it from the rows, stores it, and returns one `LibraryTopShown` per changed library in `PlayerId` order. The rows are the input and the stored value is the output. Only the engine writes it, and every reader (snapshot, opaque Decision, chooser view, event filter) reads the stored value with no live fallback. The engine calls the sync at the start and end of every `Engine::step` and after every accepted `Engine::answer`, and logs what it returns. It skips the call while `pending_payment` is set, so a new top is not shown until the spell is cast (CR 401.5, 601.2i). Every way out of pending payment (paid, cannot pay, rolled back) is followed by a sync. A sync with no change writes nothing. The start-of-step call covers a state built with rows in place, so `Engine::new` and `run_battlefield_card_scripts` still write no event (093). A rejected Answer does not sync. The stored field joins the loop fingerprint with ids relabeled. The mid-payment leak is inferred from the code (`engine/action.rs:203` sets the pending payment after the card moves), so the build writes a failing test for it first.

**Event.** `GameEventKind::LibraryTopShown { owner: PlayerId, card: Option<RevealedCard>, audience: ShownAudience }`, where `ShownAudience` is `Everyone` or `Players(Vec<PlayerId>)` sorted. `card: None` means nothing is shown any more (an empty library, or no row covers it). It fires only when the stored (top id, audience) pair changes, so a draw under Future Sight writes one and a draw with no row writes none. This reverses 086's "a revealed top is snapshot-only, with no reveal event". `Revealed` stays the momentary CR 701.20a reveal, since it cannot say "no longer shown" or name a look audience. A host-made event was ruled out because it has no ledger ids.

**Seat filter.** One `Observed` arm in `crates/host/src/view.rs`. A Seat in the new audience, and every omniscient Seat, gets the card's id and identity. A Seat that was in that library's previous audience and is not in the new one gets the event with `card: null`. Every other Seat gets nothing, so a scry reorder under a look view tells an opponent nothing (CR 701.22a). The `Observer` remembers each library's last audience from the events it has seen. `Everyone` includes non-omniscient spectators, and a `Players` audience never does. Move events keep 005's rule: a card cast or played from a shown top still shows an opponent only its new id, and the next top's `LibraryTopShown` follows in the same batch, so a folding client still matches the snapshot.

**CR 401.6.** `// TODO: new object when a revealed top is revealed again (CR 401.6)` at the sync. The id stays the same across a gap.

**Lua.** `Game.CreateLibraryTopView(c)` returns a builder with `:SetFunctionsFrom`, `:SetOwner(Game.Affected.*)` (default `You`), exactly one of `:SetReveal()` or `:SetLook(Game.Affected.*)`, and `:Register()`, which refuses neither, both, or `EnchantedPlayer`. `Game.Ops.GrantLibraryTopView(builder, duration)` is the Timed form, with the durations `GrantZonePermission` accepts. `:SetRevealTop()` and its `Register` refusal are removed. The Future Sight fixture, `docs/lua/stubs/game.lua`, `docs/LUA_API.md` and `docs/lua/player-modifiers.md` change to match.

**Lua bridge.** `LibraryTopShown` bridges as nil, since no printed trigger reads it. `event_to_lua` still has a wildcard today. Whichever of this build and "Bridge condition events into Lua" lands second adds the explicit nil arm and leaves no wildcard.

**Tests.** Engine unit tests for the sync: the merge rule, `owners` relative to `controller`, an empty library, `PlayerId` order, no event when nothing changed, a dropped row clearing the top. Engine integration: a draw under a reveal view, a look view showing only its players, the CR 401.5 freeze written failing first (the stored top still names the card on the stack, no library card shows, no event, the new top after payment), the other payment exits, a bounced source writing `card: None`, a Timed view expiring at cleanup. Lua: the builder round trip, the setters, the refusals, `:SetRevealTop` gone. Host `view.rs`: the stored value in the snapshot and opaque Decision, the Seat filter cases, a scry reorder giving an outside Seat no event, and a cast from a shown top showing an opponent only the new id. Host fold: `seat_visibility.rs` folds each library's shown top, including a Seat leaving a look audience and a spectator under `Everyone`. Scenario: `crates/engine/tests/scenarios/permissions/lantern_reveal.toml`, an `Any`-owner reveal with no permission. Bump `BEHAVIOUR_REV` to 9 if the replay hash moves, and re-pin in its own commit. No new proof card.

**Docs.** Rewrite "Seeing the top" and the "Events are unchanged" bullet in `docs/implementation/36-zone-permissions.md`, add an implementation note for top views, add one line to `DESIGN.md`, and update the Lua pages above. `CONTEXT.md` has **Library top view** and **Shown top**.
