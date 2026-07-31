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

**`:SetOperation(value)`** — what the ability does. `value` is a
[`Game.Ops.*`](game-constants.md#gameops) tag or a custom function — see
[card-script-basics.md](card-script-basics.md#writing-an-operation). Required — `:Register()`
without an operation set is an error.

**`:Register()`** — finalizes and queues the ability. Requires `:SetOperation` to have been
called.

## What's not available on this builder

- **No `:SetCost`.** Triggered abilities don't have activation costs (some do have their own
  additional cost the controller can choose to pay — that's a card-specific, not-yet-modeled
  shape).
- **No `:SetTarget`.** The engine itself now genuinely pauses for a real CR 603.3d target choice
  when a triggered ability names one (`RegisteredAbility.target`, `ProcessState`/`StepStatus::
  AwaitingTriggerTargets`) — but this builder has no method to set that field, so it's only reachable
  by constructing a `RegisteredAbility` directly in Rust, not from a card script. If your triggered
  ability needs to choose something mid-resolution instead (a target, a yes/no choice), do it from
  inside the operation function — see [targeting-and-choices.md](targeting-and-choices.md).
- **No mode declaration.** Same story as `:SetTarget` — CR 603.3c's modal triggered-ability mode
  choice (`RegisteredAbility.modes`/`.mode_choice`, `Engine::submit_trigger_mode_choice`) is real in
  the engine, but this builder has no way to declare a trigger's own modes from a card script yet.

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
