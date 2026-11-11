---
id: 170
title: Build ability text, copy kind and hidden-source identity on stack entries
type: task
mode: AFK
status: closed
blocked_by: []
claimed: false
---

## Question

Build the three stack-entry fields asked for by the view's engine brief (`stackfish-view/.plan/maps/card-view/assets/24-engine-brief-stack-entries.md`): ability text and index, the kind a copy copies, and the source's identity when the Seat cannot see the source id.

## Notes

### What success looks like

- A `stack` entry in a Seat snapshot for an activated or triggered ability carries `ability_text` when the script gave the ability text, and `ability_index` when the ability could be found on its source at push time. A spell entry carries neither.
- A `Copy` entry carries `copy_of`: `spell`, `activated_ability` or `triggered_ability`. A copy of a copy reports the innermost kind. Other entries leave it out.
- An ability entry whose `source` is withheld from a Seat carries `source_identity`, the source's `WireIdentity`, unless that source was face down. Entries whose source the Seat can see are unchanged.
- No event, Decision or rule outcome changes. `BEHAVIOUR_REV` stays, and the replay pin does not move.

### Engine design

- `RegisteredAbility` gains `text: Option<String>`. Lua sets it with `:SetText(text)` on `Game.CreateTriggeredAbility` and `Game.CreateActivatedAbility` builders. An empty string or a second call is a load error. Mana abilities get no setter, since they never use the stack (CR 405.6c).
- `StackObject` gains `ability_index: Option<u32>` and `ability_text: Option<String>`, captured at push, because CR 113.7a makes the ability independent of its source. `StackObject::new` keeps its arguments and sets both to `None`. The two push sites fill them in:
  - `ActivatePlan::Stack` carries the activated `ability_index` and the ability's `text`.
  - `push_trigger_stack_object` copies the text from the found ability (live or departed) and computes the index as the ability's position among `state.abilities` with that source, the same order `find_ability` uses. A departed source gives `None`.
- `triggers::find_departed_lki` becomes `pub` so the host can reach a retired source's last known information.
- `RegisteredAbility` has 178 struct literals and `StackObject` has 19, mostly in tests. Each literal gets `text: None` or `ability_index: None, ability_text: None` by a mechanical edit. No `Default` is added, so a new field still forces every literal to choose.
- A trigger's `ability_index` counts every ability of the source in `state.abilities` order, static and mana abilities included, the same as `find_ability`.
- Doc comments: `RegisteredAbility::text`, `StackObject::ability_index`, `StackObject::ability_text`, the four `WireStackEntry` fields and the now-public `find_departed_lki` each get a two-line doc comment.

### Host design

- `WireStackEntry` gains `ability_index`, `ability_text`, `copy_of` and `source_identity`, each skipped when absent. The brief asks for `ability_text: String`. Sending it as optional keeps "no text" distinct from empty text, and the view already degrades without it.
- `copy_of` reuses `WireStackKind` and flattens nested copies.
- `source_identity` is built in `snapshot_for` only when `visible(entry.source)` is false: from the live object, else `entry.source_lki`, else `find_departed_lki`. A face-down object gives nothing. It is on the entry only, so the identity goes when the entry leaves the stack, as CR 701.20a's reveal does.
- `ability_text` and `ability_index` go to every Seat, whatever the source's state. The stack is a public zone (CR 400.2) and the text is the stack object's own characteristic (CR 405.4). Turning the source face down after the push does not change the ability (CR 113.7a), and a face-down permanent has no printed abilities to put there (CR 708.2a).

### Comp rules

- CR 405.4 and 602.2a: an ability on the stack has the text of the ability that created it.
- CR 113.7 and 113.7a: the source is the object whose ability it is, and the ability exists on its own after it is put on the stack, so text and index are captured at push.
- CR 602.2a and 701.20a: an activated ability from a hidden zone reveals its card, and the card stays revealed until the ability leaves the stack.
- CR 113.7a and 405.4: a triggered ability is public on the stack apart from its source, which is the basis for naming a hidden source of a triggered ability.
- CR 400.2: the stack is a public zone.
- CR 707.10 and 707.10b: a copy has the copied ability's characteristics and source.
- CR 708.2a: a face-down permanent has no text, so it cannot put a printed ability on the stack.
- CR 708.5: only a face-down object's controller may look at it.

### TODOs to mark

- `// TODO: no spell or ability copy is ever put on the stack, so a copy's text and index are never set (CR 707.10)` beside the `copy_of` mapping.
- `// TODO: abilities the engine makes without a script, such as tokens' and scenario abilities, have no text (CR 405.4)` at `RegisteredAbility::text`.
- `// TODO: a hidden source whose zone change kept no last known information has no identity (CR 602.2a)` in `snapshot_for`.
- `// TODO: delayed triggered abilities have no text (CR 603.7)` only if a delayed-trigger push site exists outside `push_trigger_stack_object`.

### Docs

- `docs/lua/stubs/game.lua`: `TriggeredAbilityBuilder:SetText` and `ActivatedAbilityBuilder:SetText`.
- `docs/lua/triggered-abilities.md`, `docs/lua/activated-abilities.md`, `docs/LUA_API.md`, `docs/lua/index.md` where they list builder methods.
- `docs/implementation/18-host-session.md`: the snapshot `stack` fields.

### Tests

- Engine, `crates/engine/tests/`: a scripted activated ability with `:SetText` puts a stack object with that text and its index. A scripted trigger does the same. An ability without `:SetText` has `None` text. The trigger's index matches its position when the source has two abilities.
- Engine: `:SetText("")` and a second `:SetText` are load errors.
- Host, `view.rs` unit tests: a `Copy { of: Copy { of: TriggeredAbility } }` entry serializes `copy_of: "triggered_ability"`, and a `Copy { of: Spell }` serializes `copy_of: "spell"`. A spell entry has no `copy_of`, `ability_text` or `ability_index`.
- Host: an opponent's snapshot of an ability whose source is in the activator's hand has no `source` and has `source_identity` with the card's name. The activator's snapshot has `source` and no `source_identity`. A face-down hidden source gives no `source_identity`.

## Resolution

Built as the report says. `:SetText` on the triggered and activated builders fills `RegisteredAbility::text`, and both push sites copy the text and the ability's index onto `StackObject`. A Seat's snapshot sends `ability_text`, `ability_index`, `copy_of` and, for a hidden non-face-down source, `source_identity`. `ability_text` is optional on the wire so missing text stays distinct from empty text. The activation looks its text up at apply time, because carrying it in `ActivatePlan::Stack` tripped clippy's large-variant lint. No card script sets text yet, so real cards send none until `stackfish-db` scripts call `:SetText`.
