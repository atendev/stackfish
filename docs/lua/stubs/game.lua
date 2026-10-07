---@meta
-- EmmyLua type stub for the `Game` global injected by the Rust host (mlua, Lua 5.4).
-- Not loaded at runtime — this file exists only so the sumneko/LuaLS VSCode Lua
-- extension can offer autocomplete/hover docs while authoring card scripts.
-- Source of truth is docs/lua/*.md; regenerate by hand when those change.
-- See docs/lua/vscode-intellisense.md for setup.

-------------------------------------------------------------------------------
-- Shared primitive types
-------------------------------------------------------------------------------

---@alias ObjectId integer  -- a battlefield/stack/etc. object's id
---@alias PlayerId integer

---@class CardSummary
---@field id ObjectId
---@field card_types string[]
---@field controller PlayerId
---@field power? integer
---@field toughness? integer

---@class StateSnapshot
---@field battlefield CardSummary[]
---@field exile CardSummary[]
---@field hand_count table<PlayerId, integer>     -- hidden zone, count-only
---@field library_count table<PlayerId, integer>  -- hidden zone, count-only

---@class TargetResult
---@field kind "object"|"player"
---@field id integer

---@class ZoneChangeEvent
---@field kind "zone_change"
---@field object ObjectId
---@field from string  -- "library"|"hand"|"battlefield"|"graveyard"|"stack"|"exile"|"command"
---@field to string

-- The shared filter-table shape used across :SetAffected/:SetTarget/costs/Game.Ops.Search etc.
-- See docs/lua/targeting-and-choices.md#filter-table-shape.
---@class FilterTable
---@field controller? "you"|"opponent"|"any"
---@field card_types? string[]
---@field excluded_card_types? string[]
---@field subtypes? string[]
---@field excluded_subtypes? string[]
---@field supertypes? string[]
---@field excluded_supertypes? string[]
---@field random? boolean
---@field zone? "Hand"|"Battlefield"|"Stack"|"Graveyard"|"Library"|"Exile"|"Command"  -- default "Battlefield"
---@field mana_value_equals? integer|"X"
---@field mana_value_at_most? integer|"X"
---@field mana_value_at_least? integer|"X"

-- :SetAffected (static)/Game.Ops.DestroyAll/replacement :SetAffected all layer extra keys
-- onto the base filter shape; not every consumer accepts every key (see per-table notes
-- in docs/lua/static-abilities.md and docs/lua/game-constants.md).
---@class AffectedFilterTable : FilterTable
---@field self_only? boolean                    -- "just this effect's own source"
---@field enchanted_only? boolean                -- CR 303.4m; can't combine with other keys
---@field enchanted_player_controls? boolean     -- CR 303.4m player-side; can't combine with other keys

---@class ReplacementAffectedTable
---@field controller? "you"|"opponent"|"any"
---@field card_types? string[]
---@field self_only? boolean

---@class TargetRequirementTable : FilterTable
---@field type? "object"|"player"|"any"  -- default "object"
---@field count integer                   -- required, CR 601.2c
---@field up_to? boolean                  -- mutually exclusive with at_least
---@field at_least? boolean
---@field exclude_source? boolean         -- CR 115.4 "another target"
---@field filter? ScriptedTargetFn

---@alias ScriptedTargetFn fun(object: ObjectId, source: ObjectId, controller: PlayerId, state: StateSnapshot): boolean
---@alias PlayerPredicateFn fun(player: PlayerId, source: ObjectId, controller: PlayerId, state: StateSnapshot): boolean
---@alias ConditionFn fun(source: ObjectId, controller: PlayerId, event: ZoneChangeEvent|nil): boolean
---@alias InterveningIfFn fun(source: ObjectId, controller: PlayerId): boolean
---@alias AffectsPredicateFn fun(object: ObjectId, source: ObjectId, controller: PlayerId, state: StateSnapshot, enchanted: ObjectId|nil): boolean
---@alias EffectShapeFn fun(source: ObjectId, controller: PlayerId, state: StateSnapshot, enchanted: ObjectId|nil): EffectShapeTable

-- Only key recognized on a custom operation function's returned table today.
-- Everything else (damage/counters/draw/...) must be a `Game.Ops.*` value returned directly.
---@class OperationResultTable
---@field gain_life? integer

---@alias OperationFn fun(source: ObjectId, controller: PlayerId, state: StateSnapshot): OperationResultTable|GameOpsValue

---@class EffectShapeTable
---@field set_pt? { power: integer, toughness: integer }
---@field modify_pt? { power: integer, toughness: integer }
---@field add_keyword? string[]
---@field remove_keyword? string[]
---@field ignore_keyword? string[]
---@field add_card_type? string[]
---@field add_color? string       -- color-letter string, e.g. "WU"
---@field switch_pt? boolean
---@field remove_all_abilities? boolean
---@field double_pt? { power: boolean, toughness: boolean }

---@class BecomeCopyExceptions
---@field power? integer          -- must set with toughness
---@field toughness? integer
---@field keywords? string[]
---@field retain_name? boolean
---@field retain_color? boolean
---@field counter_kind? string    -- must set with counter_amount
---@field counter_amount? integer

---@class TokenOpts
---@field keywords? string[]
---@field name? string
---@field additional_types? string[]
---@field additional_supertypes? string[]

-- CR 601.2b: one entry of `s.alternative_costs()`'s returned array — decodes to one
-- AlternativeCost::Replacement. `costs` is the same Game.Costs.* vocabulary
-- ActivatedAbilityBuilder:AddCost accepts (Game.Costs.Mana(...) at most once per entry).
-- See docs/lua/card-script-basics.md#writing-alternative_costs.
---@class AlternativeCostEntry
---@field description string             -- required, plain text (not mechanically enforced)
---@field from_zone string                -- required, e.g. "Graveyard" — also the permission grant
---@field costs GameCostsValue[]          -- required
---@field exile_on_leaving_stack? boolean -- default false; CR 702.34a Flashback
---@field escape? boolean                 -- default false; true only for CR 702.138 Escape

-- The `s.register_abilities`/`s.target_requirement`/`s.operation`/`s.alternative_costs`/
-- `s.buyback_cost` shape every card script module returns. Not a real global — write your own
-- `local s = {}` matching this shape. See docs/lua/card-script-basics.md.
---@class CardScript
---@field register_abilities? fun(c: ObjectId)
---@field target_requirement? fun(): TargetRequirementTable
---@field operation? fun(): OperationResultTable|GameOpsValue
---@field alternative_costs? fun(): AlternativeCostEntry[]
---@field buyback_cost? fun(): string    -- a mana-cost string, e.g. "{2}" — parsed via Game.Costs.Mana's grammar

-------------------------------------------------------------------------------
-- Opaque sentinel value types — each is a bare tag or the result of a factory
-- call in the corresponding Game.* table. Treated as opaque (no fields) since
-- they're produced/consumed entirely on the Rust side.
-------------------------------------------------------------------------------

---@class GameConditionsValue
---@class GameInterveningIfValue
---@class GameOpsValue
---@class GameOpsAddManaValue : GameOpsValue  -- only AddMana/AddManaChoice/AddRestrictedMana results
---@class GameCostsValue
---@class GameLayersValue
---@class GameEffectsValue
---@class GameDurationsValue
---@class GameEventFiltersValue
---@class GameReplacementPayloadsValue
---@class GameConsumptionValue
---@class GameTokensValue

-------------------------------------------------------------------------------
-- Builders — one per Game.Create*(c)
-------------------------------------------------------------------------------

---@class TriggeredAbilityBuilder
local TriggeredAbilityBuilder = {}

--- CR 603, "did this ability's triggering event just happen." Omit ⇒ Game.Conditions.Always.
---@param value GameConditionsValue|ConditionFn
---@return TriggeredAbilityBuilder
function TriggeredAbilityBuilder:SetCondition(value) end

--- CR 603, extra board-state check evaluated when the trigger would go on the stack.
---@param value GameInterveningIfValue|InterveningIfFn
---@return TriggeredAbilityBuilder
function TriggeredAbilityBuilder:SetInterveningIf(value) end

--- CR 113.6: the ability functions from "Graveyard" or "Exile" instead of the battlefield.
--- Registers only while the source is in that zone. Other zone names are an error.
---@param zoneName "Hand"|"Graveyard"|"Exile"
---@return TriggeredAbilityBuilder
function TriggeredAbilityBuilder:SetFunctionsFrom(zoneName) end

--- What this ability does when it resolves. Required before :Register().
---@param value GameOpsValue|OperationFn
---@return TriggeredAbilityBuilder
function TriggeredAbilityBuilder:SetOperation(value) end

--- Always exactly one target. Omit ⇒ no target requirement.
---@param value FilterTable|ScriptedTargetFn
---@return TriggeredAbilityBuilder
function TriggeredAbilityBuilder:SetTarget(value) end

--- Finalizes and registers the ability. Requires :SetOperation.
function TriggeredAbilityBuilder:Register() end

---@class StaticAbilityBuilder
local StaticAbilityBuilder = {}

--- Registers only while the source is in that zone (CR 113.6); without it, only on the battlefield.
---@param zoneName "Hand"|"Graveyard"|"Exile"
---@return StaticAbilityBuilder
function StaticAbilityBuilder:SetFunctionsFrom(zoneName) end

--- CR 613 layer this effect applies in. Required before :Register().
---@param value GameLayersValue
---@return StaticAbilityBuilder
function StaticAbilityBuilder:SetLayer(value) end

--- What the effect actually does. Required before :Register().
---@param value GameEffectsValue|EffectShapeFn
---@return StaticAbilityBuilder
function StaticAbilityBuilder:SetEffect(value) end

--- Marks this as a characteristic-defining ability (CR 604.3): it functions in every zone, not just the
--- battlefield. CR 604.3a restricts it to AddSubtype (layer 4) or AddColor/SetColors (layer 5), with
--- SetAffected({ self_only = true }), no SetCondition, no SetDuration and no SetFunctionsFrom.
---@return StaticAbilityBuilder
function StaticAbilityBuilder:SetCharacteristicDefining() end

--- Which objects this effect applies to. Omit ⇒ applies to every object.
---@param value AffectedFilterTable|AffectsPredicateFn
---@return StaticAbilityBuilder
function StaticAbilityBuilder:SetAffected(value) end

--- CR 611.2c intervening-if: "this effect applies only as long as X is true."
---@param value GameInterveningIfValue|InterveningIfFn
---@return StaticAbilityBuilder
function StaticAbilityBuilder:SetCondition(value) end

--- Default Game.Durations.Permanent.
---@param value GameDurationsValue
---@return StaticAbilityBuilder
function StaticAbilityBuilder:SetDuration(value) end

--- Finalizes and registers the ability. Requires :SetLayer and :SetEffect.
function StaticAbilityBuilder:Register() end

---@class ActivatedAbilityBuilder
local ActivatedAbilityBuilder = {}

--- Appends one cost component (call once per cost). Omit entirely ⇒ no cost.
---@param value GameCostsValue
---@return ActivatedAbilityBuilder
function ActivatedAbilityBuilder:AddCost(value) end

--- CR 113.6: the ability functions from "Graveyard" or "Exile" instead of the battlefield.
--- Registers only while the source is in that zone. Other zone names are an error.
---@param zoneName "Hand"|"Graveyard"|"Exile"
---@return ActivatedAbilityBuilder
function ActivatedAbilityBuilder:SetFunctionsFrom(zoneName) end

--- What this ability does when it resolves. Required before :Register().
---@param value GameOpsValue|OperationFn
---@return ActivatedAbilityBuilder
function ActivatedAbilityBuilder:SetOperation(value) end

--- Always exactly one target when set. Optional.
---@param value FilterTable|ScriptedTargetFn|{ type: "player", filter?: PlayerPredicateFn }
---@return ActivatedAbilityBuilder
function ActivatedAbilityBuilder:SetTarget(value) end

--- Finalizes and registers the ability. Requires :SetOperation.
function ActivatedAbilityBuilder:Register() end

---@class ManaAbilityBuilder
local ManaAbilityBuilder = {}

--- Same Game.Costs.* set as activated abilities. Calling this at all (even TapSelf)
--- changes resolution timing — see docs/lua/mana-abilities.md.
---@param value GameCostsValue
---@return ManaAbilityBuilder
function ManaAbilityBuilder:AddCost(value) end

--- MUST be a Game.Ops.AddMana/AddManaChoice/AddRestrictedMana result — a custom
--- function errors at :Register(). Required before :Register().
---@param value GameOpsAddManaValue
---@return ManaAbilityBuilder
function ManaAbilityBuilder:SetOperation(value) end

--- Finalizes and registers the ability. Requires :SetOperation. No :SetTarget exists on this builder.
function ManaAbilityBuilder:Register() end

---@class ReplacementEffectBuilder
local ReplacementEffectBuilder = {}

--- Registers only while the source is in that zone (CR 113.6); without it, only on the battlefield.
---@param zoneName "Hand"|"Graveyard"|"Exile"
---@return ReplacementEffectBuilder
function ReplacementEffectBuilder:SetFunctionsFrom(zoneName) end

--- Table-only — no scripted-predicate form. Default: no filter (applies to source only via defaults below).
---@param value ReplacementAffectedTable
---@return ReplacementEffectBuilder
function ReplacementEffectBuilder:SetAffected(value) end

--- Which event this intercepts. Default Game.EventFilters.WouldDie.
---@param value GameEventFiltersValue
---@return ReplacementEffectBuilder
function ReplacementEffectBuilder:SetFilter(value) end

--- What happens instead. Default Game.ReplacementPayloads.Regenerate. Must pair with
--- :SetFilter's value per docs/lua/game-constants.md's pairing table, or errors.
---@param value GameReplacementPayloadsValue
---@return ReplacementEffectBuilder
function ReplacementEffectBuilder:SetPayload(value) end

--- Default Game.Consumption.Repeatable.
---@param value GameConsumptionValue
---@return ReplacementEffectBuilder
function ReplacementEffectBuilder:SetConsumption(value) end

--- Finalizes and registers the effect. Default builder (no fields set) = self-only
--- repeatable regeneration shield.
function ReplacementEffectBuilder:Register() end

---@class CostModifierBuilder
local CostModifierBuilder = {}

--- Registers only while the source is in that zone (CR 113.6); without it, only on the battlefield.
---@param zoneName "Hand"|"Graveyard"|"Exile"
---@return CostModifierBuilder
function CostModifierBuilder:SetFunctionsFrom(zoneName) end

--- Which spells this modifies. Required before :Register().
---@param value FilterTable
---@return CostModifierBuilder
function CostModifierBuilder:SetFilter(value) end

--- Negative reduces, positive increases; net floor is {0} generic. Required before :Register().
---@param amount integer
---@return CostModifierBuilder
function CostModifierBuilder:SetAmount(amount) end

--- Finalizes and registers the modifier. Only affects the generic-mana component of a
--- spell's normal cast cost (not alt costs like flashback).
function CostModifierBuilder:Register() end

-------------------------------------------------------------------------------
-- Game.CreateHandSizeModifier(c) — CR 402.2 "no maximum hand size" / "maximum hand size is N."
-------------------------------------------------------------------------------

---@class HandSizeModifierBuilder
local HandSizeModifierBuilder = {}

--- Registers only while the source is in that zone (CR 113.6); without it, only on the battlefield.
---@param zoneName "Hand"|"Graveyard"|"Exile"
---@return HandSizeModifierBuilder
function HandSizeModifierBuilder:SetFunctionsFrom(zoneName) end

--- Which player(s) this applies to. Required before :Register().
---@param value GameHandSizeAffectedValue
---@return HandSizeModifierBuilder
function HandSizeModifierBuilder:SetAffected(value) end

--- What the modifier does. Required before :Register().
---@param value GameHandSizeEffectValue
---@return HandSizeModifierBuilder
function HandSizeModifierBuilder:SetEffect(value) end

--- Finalizes and registers the modifier. If several apply to the same player, the one with the
--- latest timestamp wins outright (no stacking).
function HandSizeModifierBuilder:Register() end

---@class GameHandSizeAffectedValue
---@class GameHandSizeAffectedTable
---@field You GameHandSizeAffectedValue
---@field Opponent GameHandSizeAffectedValue
---@field Any GameHandSizeAffectedValue
---@field EnchantedPlayer GameHandSizeAffectedValue  -- CR 303.4m, the player this Aura is attached to
local GameHandSizeAffectedTable = {}

---@class GameHandSizeEffectValue
---@class GameHandSizeEffectsTable
---@field NoMaximum GameHandSizeEffectValue
local GameHandSizeEffectsTable = {}

---@param n integer
---@return GameHandSizeEffectValue
function GameHandSizeEffectsTable.SetTo(n) end

-------------------------------------------------------------------------------
-- Game.CreateHandRevealModifier(c) — CR 701.20a "plays with their hand revealed."
-------------------------------------------------------------------------------

---@class HandRevealModifierBuilder
local HandRevealModifierBuilder = {}

--- Registers only while the source is in that zone (CR 113.6); without it, only on the battlefield.
---@param zoneName "Hand"|"Graveyard"|"Exile"
---@return HandRevealModifierBuilder
function HandRevealModifierBuilder:SetFunctionsFrom(zoneName) end

--- Which player(s) this applies to. Required before :Register().
---@param value GameHandRevealAffectedValue
---@return HandRevealModifierBuilder
function HandRevealModifierBuilder:SetAffected(value) end

--- Finalizes and registers the modifier. NOTE: nothing enforces this yet — the engine has no
--- per-viewer visibility boundary for it to change (see docs/lua/player-modifiers.md).
function HandRevealModifierBuilder:Register() end

---@class GameHandRevealAffectedValue
---@class GameHandRevealAffectedTable
---@field You GameHandRevealAffectedValue
---@field Opponent GameHandRevealAffectedValue
---@field Any GameHandRevealAffectedValue
---@field EnchantedPlayer GameHandRevealAffectedValue
local GameHandRevealAffectedTable = {}

-------------------------------------------------------------------------------
-- Game.CreateDrawLimitModifier(c) — CR 121.2b "can't draw more than N cards each turn."
-------------------------------------------------------------------------------

---@class DrawLimitModifierBuilder
local DrawLimitModifierBuilder = {}

--- Registers only while the source is in that zone (CR 113.6); without it, only on the battlefield.
---@param zoneName "Hand"|"Graveyard"|"Exile"
---@return DrawLimitModifierBuilder
function DrawLimitModifierBuilder:SetFunctionsFrom(zoneName) end

--- Which player(s) this applies to. Required before :Register().
---@param value GameDrawLimitAffectedValue
---@return DrawLimitModifierBuilder
function DrawLimitModifierBuilder:SetAffected(value) end

--- Maximum cards drawn per turn. Required before :Register().
---@param limit integer
---@return DrawLimitModifierBuilder
function DrawLimitModifierBuilder:SetLimit(limit) end

--- Finalizes and registers the modifier. If several apply to the same player, the one with the
--- latest timestamp wins outright (no stacking).
function DrawLimitModifierBuilder:Register() end

---@class GameDrawLimitAffectedValue
---@class GameDrawLimitAffectedTable
---@field You GameDrawLimitAffectedValue
---@field Opponent GameDrawLimitAffectedValue
---@field Any GameDrawLimitAffectedValue
local GameDrawLimitAffectedTable = {}

-------------------------------------------------------------------------------
-- Game.CreateLandPlayModifier(c) — CR 305.2 "play an additional land each turn."
-------------------------------------------------------------------------------

---@class LandPlayModifierBuilder
local LandPlayModifierBuilder = {}

--- Registers only while the source is in that zone (CR 113.6); without it, only on the battlefield.
---@param zoneName "Hand"|"Graveyard"|"Exile"
---@return LandPlayModifierBuilder
function LandPlayModifierBuilder:SetFunctionsFrom(zoneName) end

--- Which player(s) this applies to. Required before :Register().
---@param value GameLandPlayAffectedValue
---@return LandPlayModifierBuilder
function LandPlayModifierBuilder:SetAffected(value) end

--- How many additional land plays this grants (usually 1). Required before :Register().
---@param additional integer
---@return LandPlayModifierBuilder
function LandPlayModifierBuilder:SetAdditional(additional) end

--- Finalizes and registers the modifier. Unlike Game.CreateDrawLimitModifier, multiple
--- registered LandPlayModifiers stack additively (CR 305.2) rather than the latest one
--- overriding the rest.
function LandPlayModifierBuilder:Register() end

---@class GameLandPlayAffectedValue
---@class GameLandPlayAffectedTable
---@field You GameLandPlayAffectedValue
---@field Opponent GameLandPlayAffectedValue
---@field Any GameLandPlayAffectedValue
local GameLandPlayAffectedTable = {}

-------------------------------------------------------------------------------
-- Game.Conditions.* — :SetCondition on a triggered ability. CR 603 trigger-event check.
-------------------------------------------------------------------------------

---@class GameConditionsTable
---@field Enters GameConditionsValue                    -- CR 603, "when this enters the battlefield"
---@field Always GameConditionsValue                     -- no filter, matches every event
---@field DealsDamage GameConditionsValue                 -- CR 603.2, "whenever this deals damage"
---@field IsDealtDamage GameConditionsValue                -- "whenever this is dealt damage" (object targets only)
---@field Upkeep GameConditionsValue                       -- CR 502.1, your upkeep
---@field EachUpkeep GameConditionsValue                   -- CR 502.1, any player's upkeep
---@field Untap GameConditionsValue                        -- CR 502.4, your untap step
---@field EachUntap GameConditionsValue                    -- any player's untap step
---@field Dies GameConditionsValue                         -- CR 700.4/603.10, "when this dies"
---@field LeavesBattlefield GameConditionsValue            -- "when this leaves the battlefield," any destination
---@field DrawsCard GameConditionsValue                     -- CR 121, "whenever you draw a card"
---@field EachDrawsCard GameConditionsValue                 -- any player draws
---@field DiscardsCard GameConditionsValue                  -- "whenever you discard a card"
---@field EachDiscardsCard GameConditionsValue              -- any player discards
---@field Discarded GameConditionsValue                     -- "when this card is discarded" (self-referential)
---@field Cast GameConditionsValue                          -- CR 601.2i, "when you cast this spell" (registers only while on the stack)
---@field Scries GameConditionsValue                        -- CR 701.22d, "whenever you scry"
---@field EachScries GameConditionsValue                    -- any player scries
---@field Surveils GameConditionsValue                      -- CR 701.25d, "whenever you surveil"
---@field EachSurveils GameConditionsValue                  -- any player surveils
---@field BeginningOfCombat GameConditionsValue             -- CR 507, beginning of combat on your turn
---@field EachBeginningOfCombat GameConditionsValue         -- any player's turn
---@field EndStep GameConditionsValue                       -- CR 512/513, your end step
---@field EachEndStep GameConditionsValue                   -- any player's end step
local GameConditionsTable = {}

--- CR 700.4/603.10, filtered death (e.g. "whenever a creature you control dies").
---@param filter FilterTable
---@return GameConditionsValue
function GameConditionsTable.DiesMatching(filter) end

--- Filtered sibling of LeavesBattlefield — any zone change off the battlefield.
---@param filter FilterTable
---@return GameConditionsValue
function GameConditionsTable.LeavesBattlefieldMatching(filter) end

--- CR 603.2h, wraps a bare tag or DiesMatching/LeavesBattlefieldMatching result — not a custom function.
---@param condition GameConditionsValue
---@return GameConditionsValue
function GameConditionsTable.OncePerTurn(condition) end

-------------------------------------------------------------------------------
-- Game.InterveningIf.* — :SetInterveningIf (triggered) or :SetCondition (static). CR 611.2c.
-------------------------------------------------------------------------------

---@class GameInterveningIfTable
local GameInterveningIfTable = {}

--- True iff controller's current life is at or below threshold.
---@param threshold integer
---@return GameInterveningIfValue
function GameInterveningIfTable.AtOrBelowLife(threshold) end

--- CR 611.2c, "as long as you control `count` or more permanents matching `filter`."
---@param filter FilterTable
---@param count integer
---@return GameInterveningIfValue
function GameInterveningIfTable.ControlsAtLeast(filter, count) end

-------------------------------------------------------------------------------
-- Game.Ops.* — :SetOperation (triggered/activated) or s.operation().
-------------------------------------------------------------------------------

---@class GameOpsTable
---@field Destroy GameOpsValue                  -- CR 701.8; indestructible unaffected
---@field Sacrifice GameOpsValue                 -- CR 701.17, target-based (vs. Game.Costs.Sacrifice)
---@field Exile GameOpsValue                     -- CR 701.18; battlefield targets only
---@field ReturnToHand GameOpsValue                -- CR 400.3; battlefield targets only, to the owner's hand
---@field Counter GameOpsValue                   -- CR 701.6a, counters a spell target (zone="Stack")
---@field Regenerate GameOpsValue                -- CR 701.19a, one-shot shield
---@field Shuffle GameOpsValue
---@field Attach GameOpsValue                    -- auto-enforces sorcery-speed
---@field ExchangeTextBoxes GameOpsValue         -- operates on ability's first two targets
local GameOpsTable = {}

--- CR 605. `letter`: W/U/B/R/G/C. `count` default 1.
---@param letter string
---@param count? integer
---@return GameOpsAddManaValue
function GameOpsTable.AddMana(letter, count) end

--- CR 605, "add `count` mana of any of these `colors`, your choice." e.g. colors="WU".
---@param count integer
---@param colors string
---@return GameOpsAddManaValue
function GameOpsTable.AddManaChoice(count, colors) end

--- CR 106.11, fixed mana spendable only toward casting a spell of `cardType`. Never usable for an activated ability's cost.
---@param letter string
---@param cardType string
---@param count? integer
---@return GameOpsAddManaValue
function GameOpsTable.AddRestrictedMana(letter, cardType, count) end

--- CR 119.
---@param amount integer
---@return GameOpsValue
function GameOpsTable.GainLife(amount) end

--- CR 608.2m. Unsigned, single fixed target; goes through the normal damage pipeline.
---@param amount integer
---@return GameOpsValue
function GameOpsTable.Damage(amount) end

--- CR 701.8/601.2c. No targets; only card_types/controller/self_only in filter (no subtypes/exclusion yet).
---@param filter { card_types?: string[], controller?: string, self_only?: boolean }
---@return GameOpsValue
function GameOpsTable.DestroyAll(filter) end

--- CR 701.21, pairs with Game.ChooseN(...) then SacrificeChosen(chosen).
---@param list TargetResult[]
---@return GameOpsValue
function GameOpsTable.SacrificeChosen(list) end

--- CR 701.9, pairs with Game.ChooseN(...) then DiscardChosen(chosen).
---@param list TargetResult[]
---@return GameOpsValue
function GameOpsTable.DiscardChosen(list) end

--- CR 701.21 + "at random," no player input. Fewer than count if fewer candidates.
---@param filter FilterTable
---@param count integer
---@return GameOpsValue
function GameOpsTable.SacrificeRandom(filter, count) end

--- CR 701.9/701.9b, "target player discards count card(s)" (Mind Rot). The targeted player chooses.
--- The target comes from s.target_requirement() (a single player target), not from an argument.
---@param count integer
---@return GameOpsValue
function GameOpsTable.Discard(count) end

--- CR 701.9b, random discard from the resolving ability's controller's hand.
---@param filter FilterTable
---@param count integer
---@return GameOpsValue
function GameOpsTable.DiscardRandom(filter, count) end

--- CR 101.4 (APNAP), active player first. `x` may be a function or a bare SacrificeRandom/DiscardRandom value.
---@param x (fun(source: ObjectId, controller: PlayerId, state: StateSnapshot): OperationResultTable|GameOpsValue)|GameOpsValue
---@return GameOpsValue
function GameOpsTable.ForEachPlayer(x) end

--- CR 122.1a. `kind` e.g. "PlusOnePlusOne"/"MinusOneMinusOne"/"Loyalty"/"Poison"/"Energy"/"Experience"/"Charge", or a custom name. Object targets only.
---@param kind string
---@param amount integer
---@return GameOpsValue
function GameOpsTable.Counters(kind, amount) end

--- "Target creature gets +X/+Y until `duration`." Every legal target gets its own independent effect.
---@param power integer
---@param toughness integer
---@param duration GameDurationsValue
---@return GameOpsValue
function GameOpsTable.Pump(power, toughness, duration) end

--- CR 108.4/613.1, Threaten-style. New controller is always whoever controls the resolving spell/ability.
---@param duration GameDurationsValue
---@return GameOpsValue
function GameOpsTable.GainControl(duration) end

---@param amount integer
---@return GameOpsValue
function GameOpsTable.DrawCards(amount) end

--- `count` default 1.
---@param spec GameTokensValue
---@param count? integer
---@return GameOpsValue
function GameOpsTable.CreateToken(spec, count) end

---@param n integer
---@return GameOpsValue
function GameOpsTable.Scry(n) end

---@param n integer
---@return GameOpsValue
function GameOpsTable.Surveil(n) end

--- destination default "hand"; shuffle default true.
---@param filter FilterTable
---@param count integer
---@param opts? { destination?: "hand"|"battlefield", tapped?: boolean, enters_attached?: boolean, shuffle?: boolean }
---@return GameOpsValue
function GameOpsTable.Search(filter, count, opts) end

---@param exceptions? BecomeCopyExceptions
---@return GameOpsValue
function GameOpsTable.BecomeCopy(exceptions) end

-------------------------------------------------------------------------------
-- Game.Costs.* — :AddCost (activated/mana abilities).
-------------------------------------------------------------------------------

---@class GameCostsTable
---@field TapSelf GameCostsValue         -- always taps the source
---@field DiscardSelf GameCostsValue     -- allows activation from Hand
---@field SacrificeSelf GameCostsValue   -- always sacrifices the source
local GameCostsTable = {}

---@param filter FilterTable
---@return GameCostsValue
function GameCostsTable.Sacrifice(filter) end

---@param filter FilterTable
---@return GameCostsValue
function GameCostsTable.Discard(filter) end

--- Taps other permanents (not the source).
---@param filter FilterTable
---@param count integer
---@return GameCostsValue
function GameCostsTable.TapPermanents(filter, count) end

---@param filter FilterTable
---@param count integer
---@return GameCostsValue
function GameCostsTable.ExileFromGraveyard(filter, count) end

---@param amount integer
---@return GameCostsValue
function GameCostsTable.Life(amount) end

--- Mana-cost string e.g. "{1}{R}". At most one per ability.
---@param cost string
---@return GameCostsValue
function GameCostsTable.Mana(cost) end

-------------------------------------------------------------------------------
-- Game.Layers.* — bare values, :SetLayer (static). CR 613.
-------------------------------------------------------------------------------

---@class GameLayersTable
---@field TextChanging GameLayersValue                -- layer 3, pairs with Effects.SetName
---@field TypeChanging GameLayersValue                -- layer 4, Effects.AddCardType
---@field ColorChanging GameLayersValue                -- layer 5, Effects.AddColor
---@field AbilityAdding GameLayersValue                -- layer 6, AddKeyword/RemoveKeyword/IgnoreKeyword/protection/landwalk/banding
---@field PowerToughnessSetting GameLayersValue        -- layer 7b, Effects.SetPT
---@field PowerToughnessModifying GameLayersValue      -- layer 7c, Effects.ModifyPT
---@field PowerToughnessSwitching GameLayersValue      -- layer 7e, Effects.SwitchPT

-------------------------------------------------------------------------------
-- Game.Effects.* — :SetEffect (static).
-------------------------------------------------------------------------------

--- Valid keyword-name strings for AddKeyword/RemoveKeyword/IgnoreKeyword. Not supported:
--- Banding (direct), Hexproof/Shroud/Ward.
---@alias KeywordName
---| "Flying" | "Reach" | "Menace" | "Vigilance" | "Haste" | "Defender" | "Flash"
---| "Deathtouch" | "Indestructible" | "Lifelink" | "Wither" | "Infect"
---| "FirstStrike" | "DoubleStrike" | "Trample" | "Fear" | "Intimidate"
---| "Shadow" | "Horsemanship" | "CantBeRegenerated"

---@class GameEffectsTable
---@field AddProtectionEverything GameEffectsValue
---@field SwitchPT GameEffectsValue
---@field RemoveAllAbilities GameEffectsValue
local GameEffectsTable = {}

---@param name string
---@return GameEffectsValue
function GameEffectsTable.SetName(name) end

---@param power integer
---@param toughness integer
---@return GameEffectsValue
function GameEffectsTable.SetPT(power, toughness) end

---@param power integer
---@param toughness integer
---@return GameEffectsValue
function GameEffectsTable.ModifyPT(power, toughness) end

---@param name KeywordName|string
---@return GameEffectsValue
function GameEffectsTable.AddKeyword(name) end

---@param name KeywordName|string
---@return GameEffectsValue
function GameEffectsTable.RemoveKeyword(name) end

---@param name KeywordName|string
---@return GameEffectsValue
function GameEffectsTable.IgnoreKeyword(name) end

---@param letter string
---@return GameEffectsValue
function GameEffectsTable.AddProtectionColor(letter) end

---@param name string
---@return GameEffectsValue
function GameEffectsTable.AddProtectionCardType(name) end

---@param name string
---@return GameEffectsValue
function GameEffectsTable.AddProtectionSubtype(name) end

---@param name string
---@return GameEffectsValue
function GameEffectsTable.AddProtectionName(name) end

---@param who "you"|"opponent"
---@return GameEffectsValue
function GameEffectsTable.AddProtectionPlayer(who) end

---@param name string
---@return GameEffectsValue
function GameEffectsTable.AddLandwalkSubtype(name) end

---@param opts table
---@return GameEffectsValue
function GameEffectsTable.AddLandwalkQuality(opts) end

---@param subtype string
---@return GameEffectsValue
function GameEffectsTable.AddBandsWithOther(subtype) end

---@param name "Basic"|"Legendary"|"Snow"|"World"
---@return GameEffectsValue
function GameEffectsTable.AddBandsWithOtherSupertype(name) end

---@param name string
---@return GameEffectsValue
function GameEffectsTable.AddCardType(name) end

--- Color-letter string, e.g. "WU".
---@param letter string
---@return GameEffectsValue
function GameEffectsTable.AddColor(letter) end

-------------------------------------------------------------------------------
-- Game.Durations.* — bare values, :SetDuration (static) or duration arg of Pump/GainControl.
-------------------------------------------------------------------------------

---@class GameDurationsTable
---@field UntilEndOfTurn GameDurationsValue
---@field UntilYourNextTurn GameDurationsValue
---@field UntilEndOfCombat GameDurationsValue
---@field Permanent GameDurationsValue   -- default

-------------------------------------------------------------------------------
-- Game.EventFilters.* — :SetFilter (replacement effects).
-------------------------------------------------------------------------------

---@class GameEventFiltersTable
---@field DamageDealt GameEventFiltersValue
---@field WouldDie GameEventFiltersValue     -- default
local GameEventFiltersTable = {}

--- Zone names: "Library"|"Hand"|"Battlefield"|"Graveyard"|"Stack"|"Exile"|"Command". Either arg may be nil.
---@param from? string
---@param to? string
---@return GameEventFiltersValue
function GameEventFiltersTable.WouldMoveZone(from, to) end

-------------------------------------------------------------------------------
-- Game.ReplacementPayloads.* — :SetPayload (replacement effects). Must pair with the
-- matching :SetFilter value — see docs/lua/game-constants.md's pairing table.
-------------------------------------------------------------------------------

---@class GameReplacementPayloadsTable
---@field PreventDamage GameReplacementPayloadsValue           -- pairs with DamageDealt
---@field RedirectDamageToSource GameReplacementPayloadsValue  -- pairs with DamageDealt
---@field Regenerate GameReplacementPayloadsValue               -- pairs with WouldDie (default)
---@field EntersTapped GameReplacementPayloadsValue             -- pairs with WouldMoveZone(_, "Battlefield")
---@field EntersWithCountersSunburst GameReplacementPayloadsValue -- pairs with WouldMoveZone(_, "Battlefield")
local GameReplacementPayloadsTable = {}

--- Pairs with WouldDie or WouldMoveZone.
---@param zone string
---@return GameReplacementPayloadsValue
function GameReplacementPayloadsTable.ReplaceZoneDestination(zone) end

---@param kind string
---@param amount integer
---@return GameReplacementPayloadsValue
function GameReplacementPayloadsTable.EntersWithCounters(kind, amount) end

---@param kind string
---@param amount integer
---@return GameReplacementPayloadsValue
function GameReplacementPayloadsTable.EntersWithCountersBloodthirst(kind, amount) end

---@param perCreature integer
---@return GameReplacementPayloadsValue
function GameReplacementPayloadsTable.EntersWithCountersDevour(perCreature) end

---@param perCard integer
---@return GameReplacementPayloadsValue
function GameReplacementPayloadsTable.EntersWithCountersAmplify(perCard) end

---@param exceptions? BecomeCopyExceptions
---@return GameReplacementPayloadsValue
function GameReplacementPayloadsTable.EntersAsCopy(exceptions) end

-------------------------------------------------------------------------------
-- Game.Consumption.* — :SetConsumption (replacement effects).
-------------------------------------------------------------------------------

---@class GameConsumptionTable
---@field OneShot GameConsumptionValue
---@field Repeatable GameConsumptionValue  -- default
local GameConsumptionTable = {}

---@param n integer
---@return GameConsumptionValue
function GameConsumptionTable.LimitedUses(n) end

-------------------------------------------------------------------------------
-- Game.Tokens.* — spec arg to Game.Ops.CreateToken.
-------------------------------------------------------------------------------

---@class GameTokensTable
---@field Clue GameTokensValue
---@field Food GameTokensValue
---@field Treasure GameTokensValue
local GameTokensTable = {}

---@param subtypes string|string[]
---@param colors string
---@param power integer
---@param toughness integer
---@param opts? TokenOpts
---@return GameTokensValue
function GameTokensTable.Creature(subtypes, colors, power, toughness, opts) end

---@param subtypes string|string[]
---@param colors string
---@param power integer
---@param toughness integer
---@param opts? TokenOpts
---@return GameTokensValue
function GameTokensTable.ArtifactCreature(subtypes, colors, power, toughness, opts) end

---@param objectId ObjectId
---@param exceptions? BecomeCopyExceptions
---@return GameTokensValue
function GameTokensTable.CopyOf(objectId, exceptions) end

-------------------------------------------------------------------------------
-- The `Game` global itself.
-------------------------------------------------------------------------------

---@class Game
---@field Conditions GameConditionsTable
---@field InterveningIf GameInterveningIfTable
---@field Ops GameOpsTable
---@field Costs GameCostsTable
---@field Layers GameLayersTable
---@field Effects GameEffectsTable
---@field Durations GameDurationsTable
---@field EventFilters GameEventFiltersTable
---@field ReplacementPayloads GameReplacementPayloadsTable
---@field Consumption GameConsumptionTable
---@field Tokens GameTokensTable
---@field LandPlayAffected GameLandPlayAffectedTable
---@field HandSizeAffected GameHandSizeAffectedTable
---@field HandSizeEffects GameHandSizeEffectsTable
---@field HandRevealAffected GameHandRevealAffectedTable
---@field DrawLimitAffected GameDrawLimitAffectedTable
local Game = {}

---@param c ObjectId
---@return TriggeredAbilityBuilder
function Game.CreateTriggeredAbility(c) end

---@param c ObjectId
---@return StaticAbilityBuilder
function Game.CreateStaticAbility(c) end

---@param c ObjectId
---@return ActivatedAbilityBuilder
function Game.CreateActivatedAbility(c) end

---@param c ObjectId
---@return ManaAbilityBuilder
function Game.CreateManaAbility(c) end

---@param c ObjectId
---@return ReplacementEffectBuilder
function Game.CreateReplacementEffect(c) end

---@param c ObjectId
---@return CostModifierBuilder
function Game.CreateCostModifier(c) end

---@param c ObjectId
---@return LandPlayModifierBuilder
function Game.CreateLandPlayModifier(c) end

---@param c ObjectId
---@return HandSizeModifierBuilder
function Game.CreateHandSizeModifier(c) end

---@param c ObjectId
---@return HandRevealModifierBuilder
function Game.CreateHandRevealModifier(c) end

---@param c ObjectId
---@return DrawLimitModifierBuilder
function Game.CreateDrawLimitModifier(c) end

--- Sugar for CreateStaticAbility/SetLayer(AbilityAdding)/SetEffect(AddKeyword(name))/SetAffected({self_only=true})/Register().
---@param source ObjectId
---@param name KeywordName|string
function Game.RegisterKeyword(source, name) end

--- Only callable inside an operation function; suspends/resumes synchronously.
---@param filter FilterTable|{ type?: "object"|"player"|"any" }
---@return TargetResult
function Game.SelectTargets(filter) end

---@return boolean
function Game.MayChoose() end

---@param filter FilterTable|ScriptedTargetFn|{ type: "player", filter: PlayerPredicateFn }
---@param n integer
---@param opts? { up_to?: boolean, at_least?: boolean }
---@return TargetResult[]
function Game.ChooseN(filter, n, opts) end
