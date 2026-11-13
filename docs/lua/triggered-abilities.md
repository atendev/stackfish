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
cares about just happen? `value` is a [`Game.Conditions.*`](game-constants.md#gameconditions) event
tag (`Enters`, `Dies`, `Upkeep`, ...), a `Game.Conditions.OncePerTurn(...)` wrapper, or a custom
function — see [card-script-basics.md](card-script-basics.md#writing-a-condition) for the
custom-function signature. Omitting `:SetCondition` entirely is the only way to write an
unconditional trigger. `:SetCondition(nil)` and an unknown tag are load errors, and so is a state tag
such as `AtOrBelowLife`, which belongs to `:SetInterveningIf`.

**`:SetInterveningIf(value)`** — CR 603.4's separate "if" clause, for abilities phrased
"When/Whenever/At X, **if Y**, Z." Independent of `:SetCondition` — set both if your ability has
both a trigger event and a board-state gate. `value` is a state tag from
[`Game.Conditions.*`](game-constants.md#state-tags) (`AtOrBelowLife`, `ControlsAtLeast`, `Escaped`,
`HadNoCounters`) or a custom function. An event tag is a load error. The function form is:

```lua
function(source, controller) -> bool
```

Called with no event — it's a pure board-state check, evaluated both when the trigger event
happens and again right before the ability would resolve (an ability whose intervening-if has
since gone false simply doesn't resolve).

**`:SetText(text)`** — CR 405.4, the ability's rules text, such as `"Whenever you draw a card, you gain 1 life."`. A Seat
sees it with the ability on the stack. Optional. Empty text or a second call is a script error.

**`:SetFunctionsFrom(zoneName)`** — CR 113.6, for an ability that functions from a zone other than
the battlefield, e.g. "{2}{B}: Return this card from your graveyard to your hand." `zoneName` is
`"Hand"`, `"Graveyard"` or `"Exile"` (anything else is a script error; Stack, Library and Command have no
registration point yet). Hand abilities written without it, like Cycling, are still recognized by their
`DiscardSelf` cost. The ability
exists only while its source is in that zone: it registers when the card enters it, is dropped when
the card leaves, and can only be activated/triggers from there. Abilities that don't call this keep
the default — they never register for a card in the graveyard or exile, so an ETB trigger in the same
script stays dormant there. Statics, replacement effects and modifiers accept it too (see their own pages).

**`:SetOperation(value)`** — what the ability does. `value` is a
[`Game.Ops.*`](game-constants.md#gameops) tag or a custom function — see
[card-script-basics.md](card-script-basics.md#writing-an-operation). `:Register()` needs exactly
one of `:SetOperation` or `:SetModes`.

**`:SetTarget(value)`** — CR 603.3d: "target [something]" in the ability's own text. `value` is
either a flat option-bag table (the same shape [`Game.SelectTargets`](targeting-and-choices.md)
accepts, e.g. `{type="object", card_types={"Creature"}, controller="opponent"}`) or a custom
scripted-legality function `function(object, source, controller, state) -> bool`. A table takes
`s.target_requirement()`'s count keys (`count`, `up_to`, `at_least`, `exclude_source`), with `count`
defaulting to 1. A bare function is exactly one target. An array of those values is
CR 601.2c's multi-word form ("target creature and target land"), one word per entry in order, and a
one-entry array is the same single word as a bare table. Pair a multi-word `:SetTarget` with
[`Game.Ops.ChainEach`](game-constants.md#gameops) so each operation reads its own word. A table that
mixes array entries with other keys is a load error. Omitting `:SetTarget` means no target requirement at all (`target: None`).
When set, the real engine pause this implies (`ProcessState`/`StepStatus::AwaitingTriggerTargets`,
`Engine::submit_trigger_targets`) happens automatically the moment the ability would go on the
stack — nothing else needs to be written to make that happen.

**`:SetModes(choice, modes)`** — CR 603.3c, a modal trigger. See
[modal triggered abilities](#modal-triggered-abilities) below.

**`:Register()`** — finalizes and queues the ability. Requires exactly one of `:SetOperation` or
`:SetModes`.

## Modal triggered abilities

"When this enters, choose one —" uses `:SetModes` in place of `:SetOperation`:

```lua
local e = Game.CreateTriggeredAbility(c)
e:SetCondition(Game.Conditions.Enters)
e:SetModes(Game.ModeChoice.Exactly(1), {
    { operation = Game.Ops.GainLife(2) },
    { operation = Game.Ops.DrawCards(1) },
})
e:Register()
```

`choice` is a [`Game.ModeChoice.*`](game-constants.md#gamemodechoice) value, and each mode is a
`{ target = ..., operation = ... }` table. `target` takes the same table or array as
`s.target_requirement()`, where `count` is required on every table, or a scripted-legality function. The
controller chooses the modes as the trigger goes on the stack, then its targets (CR 603.3c). With
`ExactlyChosenByOpponent` or `UpToChosenByOpponent` the other player chooses, and no mode may have
a target. `:SetTarget` beside `:SetModes` is an error, since each mode owns its targets. A mode
can't have a `cost` or `pawprint`, and `PawprintBudget` is refused: those belong to a spell's
`s.modes()` (see [card-script-basics.md](card-script-basics.md#writing-modes)). A function
operation, a chain and a `ForEachPlayer` are solo-only modes, as on a spell.

## What's not available on this builder

- **No `:SetCost`.** Triggered abilities don't have activation costs (some do have their own
  additional cost the controller can choose to pay — that's a card-specific, not-yet-modeled
  shape).

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
