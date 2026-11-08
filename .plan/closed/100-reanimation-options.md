---
id: 100
title: Reanimation options
type: grilling
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Which options does `Game.Ops.ReturnToBattlefield` and `ReturnAllToBattlefield` gain beyond `control` and `tapped`, and what pause lets a returned Clone, Devour, Amplify or Aura creature make its entry choices?

## Notes

- Origin: backlog item 10 and `docs/lua/not-available-yet.md` (Reanimation options).
- Gaps to rule on:
  - An extra counter on entry.
  - Entering attacking or blocking (CR 508.4).
  - "Under target opponent's control" and a "may" return.
  - Riders such as haste or "exile it at end of turn".
  - Hand and library sources.
  - A pause for entry choices that is not tied to a stack object, including the Aura host choice (CR 303.4f).
  - The all-form entering simultaneously, and Persist and Undying skipping replacement checks.
- "Riders beyond leaving the stack" decided that `exile_if_leaves = true` joins both operations, built by "Build die and leave riders". Keep it. Its leave rider does not include the end-step exile, which waits on "Delayed triggered abilities".
- This is large. Split it in the Resolution if one build slice cannot hold it.
- Use the `grilling` and `domain-modeling` skills.

## Resolution

The two ops keep their names and gain one shared options struct, `EnterOptions`. One new pause state, `AwaitingEnterChoice`, carries a returned permanent's copy, Devour, Amplify, Aura host and defender choices, with no stack object behind it. The work is six build tickets. Decided in auto mode, then approved.

"Entry" already names a Decision's listed option in `CONTEXT.md`, so the new names use "enter". `EntryControl` and `decode_entry_options` keep their names.

- Carrier: `AbilityEffect::ReturnToBattlefield`, `ReturnAllToBattlefield` and `ScriptedZoneMoveKind::Reanimate` hold one `EnterOptions` with `control`, `tapped`, `counters`, `attacking`, `keywords` and `exile_if_leaves`. A counter holds a `CounterKind`, which is `Clone` only, so `ScriptedZoneMoveKind` loses `Copy`.
- Counters: `counters = { kind, amount }`, one kind and a fixed amount (CR 122.6). They leave as `CounterAdded` through the same route as `apply_enters_battlefield_modifiers`, so doubling replacements apply. This is the existing approximation of entering with counters. Of 54 printed cards, 8 need a computed amount and 25 use a finality counter, whose replacement effect is not built. `// TODO: computed counter amounts (CR 122.6)`.
- Attacking: `attacking = true`, separate from `tapped` (CR 508.4). The creature joins `combat.attackers` and no "whenever attacks" trigger fires, because an entering creature produces a new `EnteredAttacking { object, target }` event. `AttackerDeclared` still reports declared attackers. It goes in the public list in `crates/host/src/view.rs`.
  - Unblocked needs no flag: an attacker with no `blocks` entry and outside `effect_blocked` is unblocked (CR 506.3d, 508.4d).
  - It enters and is not attacking under a non-attacking player (CR 506.3b), when it is not a creature (CR 506.3a), when it is also a battle (CR 506.3f), when the defender has gone (CR 506.3c, 508.4a), and while `state.combat` is `None`. `apply_declare_attackers` sets `state.combat` even for an empty declaration (`commands/combat/declare.rs:103`), so `None` means no declaration has happened yet.
  - Attack requirements and restrictions are ignored (CR 508.4c). The controller gets a defender Decision only with two or more legal defenders, and defenders are players, since planeswalkers and battles are design non-goals.
  - `// TODO: attacking entry before attackers are declared (CR 508.4)` and `// TODO: entering-attacking creatures joining a band (CR 702.22)`.
- Blocking and opponent control are not built. Aetherplasm is the only printed card that enters blocking. Two printed cards enter under an opponent's control (Plague Reaver and Sol'Kanar the Tainted), and Plague Reaver also needs a delayed return.
- Custom functions: a new `ScriptZoneMoveKind::Enter` carries `EnterOptions`, and `Game.Ops.PutOntoBattlefield(pick, opts)` returns it from a custom function. `PendingZoneMoves` keeps its shape, and its consumers in `resolve/misc_effects.rs` and `resolve/each_player.rs` route `Enter` to the Reanimate mover. The op accepts hand, graveyard and exile picks.
- "May" returns get no option. `Game.MayChoose` in a custom function with `PutOntoBattlefield` covers them and "if you do" text.
- Riders: `keywords = {...}` grants through a continuous effect created at entry, with an optional duration of `"permanent"` (the default) or `"end_of_turn"`. A LIKE query on `oracle_text` finds 99 graveyard-return cards with "gains haste", 16 of them "until end of turn". `exile_if_leaves` stays as "Riders beyond leaving the stack" decided. End-step exile and sacrifice wait on "Delayed triggered abilities".
- Sources: hand picks work through `PutOntoBattlefield`, because a hand card cannot be a target. The entering card's identity is public on the battlefield, so its `ZoneChange` carries it (check the `view.rs` hidden-event rule in the build). Non-search library sources are not built.
- Pause: `ProcessState::AwaitingEnterChoice { object, controller, choice, origin }`, with `EnterChoice` being Copy, Devour, Amplify, AuraHost or Defender.
  - The existing stack and search pause states stay. `submit_copy_choice`, `submit_devour_choice` and `submit_amplify_choice` share their logic through an origin that is either the `stack_id` or the move's resume data, and the chained copy rounds (CR 707.9e) branch on it. Hard-coded `ZoneKind::Stack` in that logic becomes the origin's `from`.
  - A move's resume data is the `ScriptedZoneMoveKind`, `remaining`, `then`, `trailing`, `events_so_far`, `colors_spent`, `x_value` and `from`. It works for `then` being `SbaLoop`, so `return_linked_exiles` (CR 610.3) pauses too.
  - Decision names and pick shapes stay the same for a host. AuraHost reuses the "Attach on entry" shape.
- Timing: copy, Devour and Amplify pause after the object is placed and its script has run, as the stack and search paths do, keeping the existing approximation of CR 614.12a. The Aura host (CR 303.4f) and defender are chosen before the object is placed. Triggers are collected from the ledger after resolution, so none fires during a pause. A test with a reanimated Clone that has an enters trigger pins this.
- All form: commits stay sequential, with a `batch` list set once in `continue_scripted_zone_move`. A copy candidate or Devour sacrifice excludes objects entering in the same batch (CR 603.6a, 614.13a), and linked exile returns share it (CR 610.3d). Triggers already see the final board, so candidate lists are the only visible difference.
- Persist and Undying: `ReturnSelfWithCounter` finds the graveyard object through lineage, then runs through `Reanimate` with `control = owner` and the counter option (CR 702.79a, 702.93a), so replacement checks and enter choices apply. If the lookup fails, nothing happens. The second mover and its `TODO` go away. `BEHAVIOUR_REV` moves only if the replay pin does.

Builds, in order, each blocked by the ones named:

1. "Build enter options": `EnterOptions`, decoder, `Copy` removal. Blocked by "Build die and leave riders", so `exile_if_leaves` is decoded before the struct moves it.
2. "Build enter counters and keyword grants". Blocked by 1.
3. "Build the enter-choice pause": `AwaitingEnterChoice`, the `batch` list, Aura host, the three `replacement_resume.rs` TODOs. Blocked by 1.
4. "Build entering attacking". Blocked by 3.
5. "Build PutOntoBattlefield from a custom function". Blocked by 1 and 3, so a hand pick of a Clone, Devour, Amplify or Aura creature makes its choices.
6. "Move Persist and Undying onto the Reanimate mover". Blocked by 2 and 3.

Fog: entering blocking, control by an opponent, non-search library sources, computed counter amounts, finality counters (already listed).

Terms: `CONTEXT.md` gains "Enter options" and "Enter choice".

Card counts come from `cards.oracle_text` in `../stackfish-db/stackfish.db`, `object_type = 'card'`. The base filter R is `oracle_text` LIKE any of `%graveyard%to the battlefield%`, `%graveyard%onto the battlefield%`, `%return it to the battlefield%`, `%return that card to the battlefield%`, `%put that card onto the battlefield%`, `%return this%to the battlefield%`.

- Opponent control: R and LIKE any of `%battlefield under target opponent%`, `%battlefield under that player's control%`, `%battlefield under an opponent%`, then read by hand. Reincarnation and Trove Warden, matched by a looser `%battlefield under the control of%`, return to an owner.
- Haste: R and LIKE `%gains haste%`. The until-end-of-turn count adds LIKE `%gains haste until end of turn%`.
