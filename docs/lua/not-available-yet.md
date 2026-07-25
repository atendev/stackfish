# Not available yet

[← index](index.md)

Real, named gaps — don't spend time trying these, they aren't silently working under a different
name.

- **Custom operation functions only support `gain_life`.** A card whose operation is written as a
  custom function (rather than a `Game.Ops.*` tag) can currently only express "gain N life" via
  its returned table. Every other effect shape (`damage`, `counters`, `draw_cards`, ...) is only
  reachable through the `Game.Ops.*` tag form for now. See
  [card-script-basics.md](card-script-basics.md#writing-an-operation).
- **"Spend this mana only to..." restricted mana** (e.g. Chrome Mox) isn't modeled — see
  [mana-abilities.md](mana-abilities.md).
- **No lookup-by-key script resolution.** A card's script always has to be the literal Lua source,
  not a key into an external registry.
- **No script-backed costs.** `Game.Costs.*` covers every structured cost shape without needing a
  custom function; there's no way to write an arbitrary custom-function cost.
- **Triggered abilities can't have a target yet**, even though the underlying data model has a
  slot for one. See [triggered-abilities.md](triggered-abilities.md#whats-not-available-on-this-builder).
- **No target-filter builder for a spell's own base targeting** (only an *activated ability*'s
  `:SetTarget` exists) — a spell's own inherent target requirement isn't something a card script
  sets up via this API yet.
- **{X} costs, alternative costs (flashback-style), and hybrid/Phyrexian/snow mana payment** are
  pre-existing gaps in cost-paying generally, not specific to anything in this API.
- **Only zone changes are visible to a condition.** A condition wanting to check a damage event,
  life change, or tap/untap event will always see `nil` for `event` — see
  [card-script-basics.md](card-script-basics.md#writing-a-condition).
- **Modal spells ("choose one —") aren't driven by a card script at all.** The mode is chosen
  before the spell goes on the stack, as part of casting it — a fundamentally different mechanism
  from the mid-resolution `Game.SelectTargets`/`Game.MayChoose` asks documented in
  [targeting-and-choices.md](targeting-and-choices.md). A card with modes is declared through its
  card definition directly, outside this scripting API.
- **`Game.Costs.Other`** (an unstructured, free-text additional cost) doesn't exist — every cost
  has to be one of the structured shapes in [game-constants.md](game-constants.md#gamecosts).
- **Devour/Amplify quality restrictions.** "Devour [quality] N" (sacrificing only permanents of a
  named quality, not just any creature) and a single creature having multiple independent Amplify
  instances aren't modeled — see [game-constants.md](game-constants.md#gamereplacementpayloads).
- **Damage redirection only supports "to this permanent."** Redirecting to some other chosen
  target isn't available — see [replacement-effects.md](replacement-effects.md).
- **Protection from an attached Aura/Equipment.** CR 702.16b — attaching works, but doesn't grant
  protection on its own yet.
- **Enchanting/equipping a player** isn't modeled — attachment only ever targets a permanent.
