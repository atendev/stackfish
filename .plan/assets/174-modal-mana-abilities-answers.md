# 174 Modal mana abilities: answers

## Q1. Should the engine build modal mana abilities now?

Chosen: B. Keep the load error and record the shape as out of scope.

No printed card needs it, so a build would add engine paths, wire fields and tests that nothing exercises. CR 605.1a and 605.3b would apply to such an ability, but no card has one. The load error at `crates/engine/src/script/decode.rs:1526` stops a script from writing the shape by accident, and the existing TODOs stay.

Evidence, against `/home/aten/Documents/Github/stackfish-db/stackfish.db` (38344 cards), read-only:
- The questions report's first query matches only `}: Choose one`, `}: Choose two` and `: Choose one or` and finds 14 cards. A broader line scan finds 30 modal activated abilities on 30 cards and none with a mode that adds mana. The scan takes every card whose text has a bullet, keeps each line with a `}` before `: Choose one|two|three|any|up to|both` that does not start with When, Whenever or At, and checks the bullets directly under that line for the word "Add". The script is in this file's appendix.
- The report's second query (`oracle_text LIKE '%•%Add %'`) lists every bulleted mode that mentions adding mana. Item Crate is the only one under an activated ability. That ability is not modal. It creates one token at random from the listed names, and the Banana token's own "{T}, Sacrifice this token: Add {R} or {G}" is a plain mana ability where the player picks the color. Chandra, Flameshaper's +2 is a loyalty ability, which CR 605.1a excludes.
- Cards that say "Choose a color. Add one mana of that color" have no bulleted modes, so CR 700.2 does not make them modal.

## Q2. Where is the mode announced for an ability that does not use the stack?

Chosen: A. On the existing `ActivateAbility.modes` field, in the same action.

CR 700.2a puts the mode choice inside activation, CR 602.2b applies CR 601.2b to it, and CR 605.3b leaves no later point for a Decision, since the ability resolves at once. The non-mana path already works this way (`crates/engine/src/commands/activate.rs:107-113` takes modes in the activate action before targets). The mana branch (`activate.rs:215`) is the only place that would need to pick the chosen mode's effect. Design only, nothing is built.

## Q3. Which modal abilities count as mana abilities?

Chosen: B. Judge each activation by its chosen modes.

CR 700.2c says a targeted mode that was not chosen leaves the ability as though it had no target, so CR 605.1a is met for an activation whose chosen modes have no target and could add mana, and CR 605.3b applies. CR 605.5a sends an activation that chose a targeted mode through the stack. CR 115.6 does not block this: it defines "targeted" by chosen targets, and 700.2c removes the unchosen mode's targets. Listing during payment (CR 605.3a) stays limited to the modes that have no target and add mana, so every activation made then is a mana ability. Note the engine now fixes `mana_ability` from the effect type at load time (`entries.rs:322`), so B means classification moves to activation time. That belongs to the build ticket if a card ever appears.

## Q4. How does a Payment Decision list a modal mana ability?

Chosen: A, as a recorded design only.

The mana listing gets the same `modes: Option<ModeRange>` as `ActivateEntry`, restricted to the Q3 modes. Today `mana_ability_entries` filters `Entry::Activate` with `mana_ability` set, and `entries.rs:346-347` sets `modes` to `None` for mana abilities, so the field exists on the type already and only the value changes. Nothing is built and the proof deck gains no card. B would contradict CR 605.3a, which lets a player activate any mana ability during payment.

## Q5. The hole where `AddMana` resolving from the stack does nothing

Chosen: A, with one amendment: the build ticket must split the triggers.

Confirmed in code. `decode_operation_value` (`script/decode.rs:99-141`) accepts any `OperationTag`, and the triggered builder's `:SetOperation` (`script/builders.rs:349-355`) calls it with no mana check. The mode decoder refuses `AddMana*` only for `ModeOwner::Activated` (`decode.rs:1526-1537`), so a spell or trigger operation or mode loads cleanly. `AbilityEffect::packed` keeps the primitive (`types/ability.rs:700`), the stack object carries `Some(AddMana)`, and the dispatch in `engine/resolve/mod.rs:508-511` sends it to `resolve_misc_or_script_effect`. That function returns `ControlFlow::Continue(events)` for `AddMana` and `AddManaChoice` (`misc_effects.rs:190-192`) with no pool change. `is_modal_resolvable` (`commands/legality.rs:1135`) is an allow-list that leaves out `AddMana` and `AddManaChoice`, so such a mode is solo-only. Its doc comment and the TODO at line 1198 say so.

CR 605.5b says a spell is never a mana ability and resolves like any spell, so rituals must add mana on resolution. CR 605.5a says an ability with a target, or a triggered ability that does not meet CR 605.1b, follows the normal stack rules. The amendment: CR 605.1b and 605.4a make a triggered ability a mana ability when it triggers from a mana ability's activation or from mana being added, has no target and could add mana. Such triggers (Wild Growth style) resolve immediately and never use the stack. How many of the roughly 125 trigger cards are triggered mana abilities is unmeasured, and the build ticket must measure it before it sets its scope. The engine does not build them (`docs/implementation/26-special-mana-payment.md:39`, TODO at `activate.rs:247`). So the roughly 125 trigger cards the report counts include some that need the CR 605.1b path, not stack resolution. The build ticket should resolve `AddMana` for spells and ordinary triggers from the stack, and list the triggered mana abilities as a separate follow-up.

B alone would turn working-looking cards into missing ones, and C puts engine work in a grilling ticket, which the workflow does not allow. A also needs a decision on how `AddManaChoice` announces its color on a spell or trigger, since `chosen_colors` exists only on `ActivateAbility` (`commands/activate.rs:184-186`).

## Appendix: Q1 scan

Run with `python3 -I scan.py /home/aten/Documents/Github/stackfish-db/stackfish.db`.

```python
import re, sqlite3, sys
db = sqlite3.connect(sys.argv[1])
rows = db.execute("SELECT name, oracle_text, type_line FROM cards WHERE oracle_text LIKE '%•%'").fetchall()
act = re.compile(r'^[^"]*\}[^"]*:\s*Choose (one|two|three|any|up to|both)', re.I)
cards_modal, abil_modal, abil_mana = set(), 0, []
for name, text, _ in rows:
    lines = text.split('\n')
    for i, line in enumerate(lines):
        if act.search(line) and not re.match(r'^(When|Whenever|At)\b', line):
            abil_modal += 1; cards_modal.add(name)
            bullets = []
            for nxt in lines[i+1:]:
                if nxt.startswith('•'): bullets.append(nxt)
                else: break
            if any(re.search(r'\bAdd\b', b) for b in bullets): abil_mana.append(name)
print('modal activated abilities', abil_modal, 'cards', len(cards_modal), 'with an Add mode', abil_mana)
```

Output: `modal activated abilities 30 cards 30 with an Add mode []`.
