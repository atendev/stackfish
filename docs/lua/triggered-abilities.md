# Triggered abilities

[← index](index.md)

CR 603 — "When/Whenever/At X, do Y." Built with `Game.CreateTriggeredAbility(c)`.

```lua
local e = Game.CreateTriggeredAbility(c)
e:SetCondition(Game.Conditions.Enters)
e:SetOperation(Game.Ops.GainLife(1))
e:Register()
```

See [card-script-basics.md](card-script-basics.md) for the surrounding `register_abilities(c)`
structure and the condition/operation concept in general.

## Methods

**`:SetCondition(value)`** — CR 603's own trigger-event condition: did the thing this ability
cares about just happen? `value` is a [`Game.Conditions.*`](game-constants.md#gameconditions) tag,
a `Game.Conditions.OncePerTurn(...)` wrapper, or a custom function — see
[card-script-basics.md](card-script-basics.md#writing-a-condition) for the custom-function
signature. Omitting `:SetCondition` entirely means unconditional (equivalent to
`Game.Conditions.Always`).

**`:SetInterveningIf(value)`** — CR 603.4's separate "if" clause, for abilities phrased
"When/Whenever/At X, **if Y**, Z." Independent of `:SetCondition` — set both if your ability has
both a trigger event and a board-state gate. `value` is a
[`Game.InterveningIf.*`](game-constants.md#gameinterveningif) tag or a custom function:

```lua
function(source, controller) -> bool
```

Called with no event — it's a pure board-state check, evaluated both when the trigger event
happens and again right before the ability would resolve (an ability whose intervening-if has
since gone false simply doesn't resolve).

**`:SetFunctionsFrom(zoneName)`** — CR 113.6, for an ability that functions from a zone other than
the battlefield, e.g. "{2}{B}: Return this card from your graveyard to your hand." `zoneName` is
`"Graveyard"` or `"Exile"` (anything else is a script error — hand abilities like Cycling are
recognized by their `DiscardSelf` cost, and no other zone has a registration point yet). The ability
exists only while its source is in that zone: it registers when the card enters it, is dropped when
the card leaves, and can only be activated/triggers from there. Abilities that don't call this keep
the default — they never register for a card in the graveyard or exile, so an ETB trigger in the same
script stays dormant there. Statics, replacement effects and modifiers can't be declared this way yet.

**`:SetOperation(value)`** — what the ability does. `value` is a
[`Game.Ops.*`](game-constants.md#gameops) tag or a custom function — see
[card-script-basics.md](card-script-basics.md#writing-an-operation). Required — `:Register()`
without an operation set is an error.

**`:SetTarget(value)`** — CR 603.3d: "target [something]" in the ability's own text. `value` is
either a flat option-bag table (the same shape [`Game.SelectTargets`](targeting-and-choices.md)
accepts, e.g. `{type="object", card_types={"Creature"}, controller="opponent"}`) or a custom
scripted-legality function `function(object, source, controller, state) -> bool`. Always a single,
exact-count target (`count: 1, up_to: false`) — no "up to N" and no CR 601.2c multi-word form
("target creature and target land") from this builder; construct a `RegisteredAbility` directly in
Rust for either of those. Omitting `:SetTarget` means no target requirement at all (`target: None`).
When set, the real engine pause this implies (`ProcessState`/`StepStatus::AwaitingTriggerTargets`,
`Engine::submit_trigger_targets`) happens automatically the moment the ability would go on the
stack — nothing else needs to be written to make that happen.

**`:Register()`** — finalizes and queues the ability. Requires `:SetOperation` to have been
called.

## What's not available on this builder

- **No `:SetCost`.** Triggered abilities don't have activation costs (some do have their own
  additional cost the controller can choose to pay — that's a card-specific, not-yet-modeled
  shape).
- **No mode declaration.** CR 603.3c's modal triggered-ability mode choice
  (`RegisteredAbility.modes`/`.mode_choice`, `Engine::submit_trigger_mode_choice`) is real in the
  engine, but this builder has no way to declare a trigger's own modes from a card script yet — modal
  *spells* don't have a script-facing way to declare modes either, so this isn't a trigger-specific
  gap so much as "modes in general aren't part of this scripting API yet."

## Registering more than one triggered ability on a card

Call `Game.CreateTriggeredAbility(c)` again — each `:Register()` is independent:

```lua
function s.register_abilities(c)
    local etb = Game.CreateTriggeredAbility(c)
    etb:SetCondition(Game.Conditions.Enters)
    etb:SetOperation(Game.Ops.GainLife(1))
    etb:Register()

    local dies = Game.CreateTriggeredAbility(c)
    dies:SetCondition(Game.Conditions.Dies)
    dies:SetOperation(Game.Ops.DrawCards(1))
    dies:Register()
end
```
