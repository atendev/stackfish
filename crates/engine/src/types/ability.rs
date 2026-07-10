use crate::types::cost::CostSpec;
use crate::types::counter::CounterKind;
use crate::types::event::GameEventKind;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::mana::ManaUnit;
use crate::types::target::TargetSpec;
use crate::types::turn::Step;
use crate::types::zone::ZoneKind;

/// Per ygopro's effect lifecycle (`docs/ygo_pro_reference.md`), generalized
/// for MTG: condition/cost/target/operation are separate script callbacks
/// rather than one monolithic function, so the engine can check (e.g.)
/// condition without committing to cost/target/operation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AbilityKind {
    Triggered,
    Activated,
    Static,
}

/// Structural stand-in for an ability's effect when no script bridge exists
/// to actually run one (every `RegisteredAbility` still needs `operation` —
/// this is narrower: just enough to model CR 605's mana abilities, the one
/// concrete effect shape this engine resolves without a script callback).
/// `None` on `RegisteredAbility::effect` means "this ability's effect can
/// only be carried out via the script bridge" — not yet possible, same gap
/// as `operation` everywhere else.
#[derive(Clone, Debug)]
pub enum AbilityEffect {
    /// CR 605: adds exactly these literal units to the activator's mana
    /// pool. Reuses `ManaUnit` (see `mana.rs`) — same "literal units, not a
    /// cost shape" vocabulary `CostPayment::Mana` already uses, just added
    /// instead of spent.
    AddMana(Vec<ManaUnit>),
    /// The `Game.Ops.GainLife(n)` primitive — gains the controller exactly
    /// `n` life, no script invocation needed. Mirrors the hand-written
    /// `{ gain_life = n }` Lua operation table from the prior slice, just
    /// resolved on the Rust side instead of via `ScriptRuntime::invoke_operation`.
    GainLife(i32),
    /// The `Game.Ops.Damage(n)` primitive — deals exactly `n` damage to the
    /// resolving `StackObject`'s first chosen target (CR 608.2m). Unlike
    /// `GainLife` above, this does *not* mutate `GameState` directly —
    /// `ResolveStackTop` emits a real `GameEventKind::DamageDealt` event and
    /// lets it flow through the normal `ReplacementCheck`→`CommitEvents`
    /// pipeline, the same one combat damage already uses, so
    /// prevention/replacement effects (CR 614-616) and Lifelink/Wither/
    /// Infect/Deathtouch (`commands::apply_damage_event`) apply for free.
    /// Single fixed target only — no "divide among any number of targets"
    /// (Arc Trail-style) or "each opponent" mass-damage shape; matches
    /// `targeting-basics`'s own existing narrowing (see
    /// docs/implementation.md).
    Damage(u32),
    /// The `Game.Ops.Destroy()` primitive — destroys the resolving
    /// `StackObject`'s first chosen target (CR 701.8). Unlike `Damage`,
    /// this can't be modeled as "push a not-yet-applied event and let
    /// `CommitEvents` retroactively apply it" — every other `ZoneChange`
    /// event in this codebase describes a move that already happened, so
    /// `ResolveStackTop` performs the real CR 614.13 `WouldDie` replacement
    /// check and `sba::commit_death` inline (mirroring `resolve_deaths`'s
    /// own per-object body), the same way `Engine::submit_payment` already
    /// does for a Sacrifice cost payment. CR 702.12b: an indestructible
    /// target is untouched — checked before even proposing the
    /// replacement-checked event, not a replacement effect itself. No
    /// fields — a toggle, not a value, same shape as `SwitchPT`.
    Destroy,
    /// The `Game.Ops.Counters(kind, n)` primitive — puts `amount` counters
    /// of `kind` on the resolving `StackObject`'s first chosen target (CR
    /// 122.1a). Like `Damage`, not `Destroy`: `GameEventKind::CounterAdded`
    /// already existed as dead code (never constructed, never matched) with
    /// exactly this shape, so `ResolveStackTop` pushes it unapplied and
    /// `CommitEvents` retroactively applies it, the same pipeline `Damage`
    /// uses for `DamageDealt`. Object targets only — no player-facing
    /// counters (Energy/Experience) this slice, since real "put a counter on
    /// target creature/artifact" text vastly outnumbers those and
    /// `CounterAdded` only carries a single `ObjectId`, not a generic
    /// `Target`. No replacement/prevention modeling for counter placement
    /// either (no `EventFilter::CounterAdded` shape exists) — same gap
    /// `GainLife` already has, not fixed here.
    Counters { kind: CounterKind, amount: i32 },
    /// The `Game.Ops.DrawCards(n)` primitive — the resolving `StackObject`'s
    /// controller draws exactly `n` cards (CR 120.3/121), no target (same
    /// "always the controller" shape `GainLife` already uses, not `Damage`/
    /// `Destroy`/`Counters`'s object-target shape — "Draw a card"/"you draw
    /// two cards" vastly outnumbers "target player draws"). Cheaper than
    /// even `Counters`: a `ZoneChange` always describes a move that already
    /// happened (see `Destroy`'s own doc comment), so `ResolveStackTop`
    /// mutates state immediately via the shared `Engine::draw_one_card`
    /// helper (extracted from `draw_step`'s own body) and just logs the
    /// resulting real event — no `CommitEvents` special-case needed, same
    /// convention `draw_step` already established. Drawing from an empty
    /// library silently stops rather than looping past it - CR 704.5b's
    /// empty-library-loss SBA isn't modeled (same TODO `draw_step` already
    /// carries). No `Target::Player` "target player draws" variant, no
    /// replacement/prevention modeling for draws (no `EventFilter::
    /// DrawsCard`/"would draw" shape exists).
    DrawCards(u32),
}

/// The two condition shapes `Game.Conditions.*` covers this slice — a
/// Rust-native fast path so common trigger shapes don't need a full Lua
/// condition function written out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConditionPrimitive {
    /// "Whenever this permanent enters the battlefield" (CR 603 ETB
    /// triggers) — matches a `ZoneChange` event moving `source` to the
    /// battlefield. Mirrors the hand-written
    /// `event.object == source and event.to == "battlefield"` Lua condition
    /// from the prior slice exactly.
    EntersBattlefield,
    /// No filter — always matches. A real variant (not just `None`) so a
    /// Lua author has an explicit, correct spelling for "unconditional";
    /// see `RegisteredAbility::condition`'s doc for why both spellings
    /// exist.
    Always,
    /// "Whenever this deals damage" (CR 603.2, matched against
    /// `GameEventKind::DamageDealt`) — `source` is the one dealing the
    /// damage. Reachable today because combat damage (`AssignCombatDamage`)
    /// already routes its `DamageDealt` events through the real
    /// `CommitEvents`→`SbaLoop`→`CollectTriggers` pipeline (see the
    /// `fix-replacement-bypasses` slice) — unlike a phase/step-boundary
    /// condition, which `AdvanceTurnStep` currently can't reach at all (see
    /// this file's own TODO note below and docs/implementation.md's backlog).
    /// No combat-vs-noncombat distinction: `DamageDealt` itself doesn't
    /// carry one, so this matches any damage this object deals, by whatever
    /// means eventually produces the event.
    DealsDamage,
    /// "Whenever this is dealt damage" — the mirror image of `DealsDamage`,
    /// matching on the event's `target` instead of its `source`. Only
    /// matches a `Target::Object` target (a player being dealt damage isn't
    /// "this" for a permanent's own ability).
    IsDealtDamage,
    /// "At the beginning of your upkeep" (CR 502.1's most common trigger
    /// shape) — matches `GameEventKind::StepStarted{step: Step::Upkeep}`
    /// only when the ability's controller is also the active player.
    /// Reachable now that `AdvanceTurnStep` (engine.rs) routes its events
    /// through the real `ReplacementCheck`→`CommitEvents`→`SbaLoop`→
    /// `CollectTriggers` pipeline instead of `log_events` (see that
    /// function's own doc comment) — the concrete condition this slice adds
    /// to prove the plumbing fix actually works, not just the fix itself.
    BeginningOfYourUpkeep,
    /// "At the beginning of each upkeep" — the less common, controller-
    /// agnostic sibling of `BeginningOfYourUpkeep` (e.g. Waste Not-style
    /// effects). Matches every `Step::Upkeep` `StepStarted` regardless of
    /// whose turn it is.
    BeginningOfEachUpkeep,
    /// "When this creature dies" (CR 700.4/603.10) — matches a `ZoneChange`
    /// event moving `source` from the battlefield to a graveyard. Only ever
    /// reachable via `collect_triggers`'s CR 603.10 last-known-information
    /// fallback: by the time this is checked, `source` has already left
    /// `state.objects`/`state.abilities` (purged by `commands::
    /// retire_object` as part of the very death this condition is matching)
    /// - see that function's own doc comment. Matched against `last_known_
    /// info.object_id`, not the event's own `object` field, because a
    /// battlefield->graveyard move mints a fresh `ObjectId` for the
    /// graveyard-side object (CR 400.7) - `object` in the committed event is
    /// that *new* id, never `source`.
    Dies,
    /// "When this leaves the battlefield" (CR 603.10) — the broader sibling
    /// of `Dies`: matches any `ZoneChange` moving `source` off the
    /// battlefield, regardless of destination (graveyard, exile, hand,
    /// library, ...), not just a death. Same last-known-information
    /// matching shape as `Dies`.
    LeavesBattlefield,
    /// "Whenever you draw a card" (CR 121) — matches a `ZoneChange` event
    /// moving *any* card from library to hand, filtered to only the ability
    /// controller's own draws via `last_known_info.controller` (the pre-move
    /// object clone every `ZoneChange` carries — no live state lookup
    /// needed). Deliberately not `source`-scoped like `EntersBattlefield`:
    /// this condition doesn't care what `source` is, only who controls it
    /// and who drew. Matches by zone-shape only (`Library`→`Hand`) — there's
    /// no separate "this was a draw" flag on the event, so a hypothetical
    /// future non-draw Library→Hand effect (CR 121.5 draws a distinction
    /// real MTG cares about) would false-positive here; no such effect
    /// exists in this engine yet.
    DrawsCard,
    /// "Whenever a player draws a card" — the controller-agnostic sibling of
    /// `DrawsCard` (e.g. Underworld Dreams-style effects), same relationship
    /// `BeginningOfEachUpkeep` has to `BeginningOfYourUpkeep`.
    EachDrawsCard,
    /// "At the beginning of combat on your turn" (CR 507) — matches
    /// `GameEventKind::StepStarted{step: Step::BeginningOfCombat}` only when
    /// the ability's controller is also the active player. Same reachability
    /// story as `BeginningOfYourUpkeep`: `AdvanceTurnStep` already routes
    /// this step's `StepStarted` through the real pipeline, neither `Untap`
    /// nor `Cleanup` special-casing applies to it.
    BeginningOfYourCombat,
    /// "At the beginning of each combat" — the controller-agnostic sibling
    /// of `BeginningOfYourCombat`.
    BeginningOfEachCombat,
    /// "At the beginning of your end step" (CR 512/513) — matches
    /// `GameEventKind::StepStarted{step: Step::End}` only when the ability's
    /// controller is also the active player. Same reachability story as
    /// `BeginningOfYourUpkeep`.
    BeginningOfYourEndStep,
    /// "At the beginning of each end step" — the controller-agnostic sibling
    /// of `BeginningOfYourEndStep`.
    BeginningOfEachEndStep,
}

// TODO(backlog item 7, CR 603.10 continued): Dies/LeavesBattlefield are wired
// for the SBA-driven death path (sba::commit_death) and Sacrifice/Discard
// cost payment (commands::move_to_graveyard_for_cost, via commit_payment ->
// Engine::submit_payment) - see move_to_graveyard_for_cost's own doc comment
// for why the Discard half stays dormant. Two call sites still discard their
// departed abilities outright: submit_discard's CR 514.1 hand-size discard,
// and draw_step's library-side retirement - both would be inert to wire
// today (neither can produce a Battlefield-departure LKI). "Whenever a
// creature you control dies" (filtered by something other than "is this
// exact object") also isn't representable yet - Dies/LeavesBattlefield only
// ever match the ability's own source, not a filter over the dying object's
// characteristics.
//
// TODO(trigger system completeness, backlog item 7): once-per-turn/
// intervening-if legality (CR 603.2-603.3) is still untouched. Untap-step
// events still can't trigger anything (CR 502.4 grants no priority there, so
// there's no window to place a trigger before) and Cleanup's own
// CleanupSbaCheck flow still doesn't run collect_triggers at all (a second,
// differently-shaped fix, left for its own slice - see
// docs/implementation.md's backlog item 7a).

impl ConditionPrimitive {
    /// Zero Lua VM calls — evaluated directly against the event that's being
    /// checked. `source` is the registered ability's source object (not
    /// necessarily the event's own object); `controller` is that source's
    /// controller and `active_player` is whoever's turn it currently is —
    /// both needed for `BeginningOfYourUpkeep`'s "your" relativity, which
    /// `DealsDamage`/`IsDealtDamage`/`EntersBattlefield` never needed.
    pub fn matches(
        &self,
        source: ObjectId,
        controller: PlayerId,
        active_player: PlayerId,
        event: &GameEventKind,
    ) -> bool {
        match self {
            ConditionPrimitive::Always => true,
            ConditionPrimitive::EntersBattlefield => matches!(
                event,
                GameEventKind::ZoneChange { object, to, .. }
                    if *object == source && *to == ZoneKind::Battlefield
            ),
            ConditionPrimitive::DealsDamage => matches!(
                event,
                GameEventKind::DamageDealt { source: dealer, .. } if *dealer == source
            ),
            ConditionPrimitive::IsDealtDamage => matches!(
                event,
                GameEventKind::DamageDealt { target, .. } if *target == Target::Object(source)
            ),
            ConditionPrimitive::BeginningOfYourUpkeep => {
                controller == active_player
                    && matches!(event, GameEventKind::StepStarted { step: Step::Upkeep })
            }
            ConditionPrimitive::BeginningOfEachUpkeep => {
                matches!(event, GameEventKind::StepStarted { step: Step::Upkeep })
            }
            ConditionPrimitive::Dies => matches!(
                event,
                GameEventKind::ZoneChange { from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, last_known_info: Some(lki), .. }
                    if lki.object_id == source
            ),
            ConditionPrimitive::LeavesBattlefield => matches!(
                event,
                GameEventKind::ZoneChange { from: ZoneKind::Battlefield, last_known_info: Some(lki), .. }
                    if lki.object_id == source
            ),
            ConditionPrimitive::DrawsCard => matches!(
                event,
                GameEventKind::ZoneChange { from: ZoneKind::Library, to: ZoneKind::Hand, last_known_info: Some(lki), .. }
                    if lki.controller == controller
            ),
            ConditionPrimitive::EachDrawsCard => matches!(
                event,
                GameEventKind::ZoneChange { from: ZoneKind::Library, to: ZoneKind::Hand, .. }
            ),
            ConditionPrimitive::BeginningOfYourCombat => {
                controller == active_player
                    && matches!(event, GameEventKind::StepStarted { step: Step::BeginningOfCombat })
            }
            ConditionPrimitive::BeginningOfEachCombat => {
                matches!(event, GameEventKind::StepStarted { step: Step::BeginningOfCombat })
            }
            ConditionPrimitive::BeginningOfYourEndStep => {
                controller == active_player
                    && matches!(event, GameEventKind::StepStarted { step: Step::End })
            }
            ConditionPrimitive::BeginningOfEachEndStep => {
                matches!(event, GameEventKind::StepStarted { step: Step::End })
            }
        }
    }
}

/// A condition/operation source string resolves to one of these two shapes
/// at load time (`ScriptRuntime::load_condition`) — a recognized
/// `Game.Conditions.*` tag (`Primitive`, no further Lua calls needed ever
/// again) or a genuine custom Lua function (`Script`, invoked for real via
/// `ScriptRuntime::invoke_condition` each time, same as before this slice).
#[derive(Clone, Copy, Debug)]
pub enum ConditionSpec {
    Primitive(ConditionPrimitive),
    Script(ScriptHandle),
}

/// A card's registered ability, as set up by its script's `initial_effect`-
/// equivalent (`Game.RegisterTriggeredAbility` etc — see `script_api.rs`).
/// Lives in `GameState.abilities` as a flat list (same pattern as
/// `continuous_effects`/`replacement_effects`), not nested under
/// `GameObject`, so the engine can scan all registered abilities against an
/// event/timing window without walking every object first.
#[derive(Clone)]
pub struct RegisteredAbility {
    pub source: ObjectId,
    pub kind: AbilityKind,
    /// `None` means "no filter, always matches" — same meaning as
    /// `Some(ConditionSpec::Primitive(ConditionPrimitive::Always))`. Two
    /// spellings exist on purpose: `None` is what every non-triggered
    /// (`Activated`/`Static`) ability uses, since it never had a condition
    /// concept to begin with; `Always` is a real, explicit choice a Lua
    /// trigger author writes (`return Game.Conditions.Always`) when they
    /// mean "no filter" deliberately — loading rejects a bare Lua `nil`
    /// result as an error rather than silently treating a typo'd
    /// `Game.Conditions.Typo` as unconditional (see
    /// `ScriptRuntime::load_condition`).
    pub condition: Option<ConditionSpec>,
    /// `None` means "no cost at all" — `ActivateAbility` treats this the
    /// same as an empty `CostSpec` (mana: None, additional: empty), still
    /// routing through `AwaitingPayment` for an explicit (even if empty)
    /// acknowledgment, same CR 118.5a stance `CastSpell`'s mana cost already
    /// takes. `Some(spec)` is CR 602.2b's real cost — `spec.additional` is
    /// what `Game.CreateActivatedAbility`'s `:AddCost` accumulates for
    /// tap/sacrifice/discard/life components; `spec.mana` is set the same
    /// way from a `Game.Costs.Mana("{1}{R}")` component, parsed via
    /// `ManaCost::parse` (mana.rs) — no longer always `None`, since the
    /// ability-mana-cost-strings slice closed that gap.
    pub cost: Option<CostSpec>,
    /// `None` means "no targets required" — same "cast as though it did
    /// not require those targets" stance `CardDefinition::target_requirement`
    /// takes. `Some(TargetSpec::Primitive(_))` is what `ActivateAbility`
    /// validates non-mana-ability targets against this slice;
    /// `Some(TargetSpec::Script(_))` stays dead code (see `TargetSpec`'s own
    /// doc comment).
    pub target: Option<TargetSpec>,
    pub operation: ScriptHandle,
    /// See `AbilityEffect`'s doc comment — `Some` for an effect shape this
    /// engine can resolve without a script call at all (CR 605 mana
    /// abilities' `AddMana`, or a `Game.Ops.*` primitive like `GainLife`
    /// loaded via `ScriptRuntime::load_operation`). `None` means resolution
    /// falls back to invoking `operation` for real through the script
    /// bridge.
    pub effect: Option<AbilityEffect>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // DealsDamage/IsDealtDamage/EntersBattlefield/Always never consult
    // controller/active_player - any two PlayerIds work as filler here.
    const CONTROLLER: PlayerId = PlayerId(0);
    const ACTIVE_PLAYER: PlayerId = PlayerId(0);

    #[test]
    fn deals_damage_matches_when_source_is_the_dealer() {
        let source = ObjectId(0);
        let other = ObjectId(1);
        let event = GameEventKind::DamageDealt { source, target: Target::Object(other), amount: 3 };
        assert!(ConditionPrimitive::DealsDamage.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn deals_damage_does_not_match_a_different_dealer() {
        let source = ObjectId(0);
        let dealer = ObjectId(1);
        let event =
            GameEventKind::DamageDealt { source: dealer, target: Target::Object(source), amount: 3 };
        assert!(!ConditionPrimitive::DealsDamage.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn is_dealt_damage_matches_when_source_is_the_target() {
        let source = ObjectId(0);
        let dealer = ObjectId(1);
        let event =
            GameEventKind::DamageDealt { source: dealer, target: Target::Object(source), amount: 3 };
        assert!(ConditionPrimitive::IsDealtDamage.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn is_dealt_damage_does_not_match_a_different_target() {
        let source = ObjectId(0);
        let other = ObjectId(1);
        let event = GameEventKind::DamageDealt { source, target: Target::Object(other), amount: 3 };
        assert!(!ConditionPrimitive::IsDealtDamage.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn is_dealt_damage_does_not_match_a_player_target() {
        // A player being dealt damage isn't "this" for a permanent's own
        // ability - IsDealtDamage only ever matches a Target::Object.
        let source = ObjectId(0);
        let dealer = ObjectId(1);
        let event = GameEventKind::DamageDealt { source: dealer, target: Target::Player(PlayerId(0)), amount: 3 };
        assert!(!ConditionPrimitive::IsDealtDamage.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn deals_damage_and_is_dealt_damage_dont_match_unrelated_events() {
        let source = ObjectId(0);
        let event = GameEventKind::LifeChanged { player: PlayerId(0), delta: -1 };
        assert!(!ConditionPrimitive::DealsDamage.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
        assert!(!ConditionPrimitive::IsDealtDamage.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn beginning_of_your_upkeep_matches_when_controller_is_active_player() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let event = GameEventKind::StepStarted { step: Step::Upkeep };
        assert!(ConditionPrimitive::BeginningOfYourUpkeep.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_your_upkeep_does_not_match_someone_elses_upkeep() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted { step: Step::Upkeep };
        assert!(!ConditionPrimitive::BeginningOfYourUpkeep.matches(source, controller, active_player, &event));
    }

    #[test]
    fn beginning_of_your_upkeep_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let event = GameEventKind::StepStarted { step: Step::Draw };
        assert!(!ConditionPrimitive::BeginningOfYourUpkeep.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_each_upkeep_matches_regardless_of_whose_turn_it_is() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted { step: Step::Upkeep };
        assert!(ConditionPrimitive::BeginningOfEachUpkeep.matches(source, controller, active_player, &event));
    }

    #[test]
    fn beginning_of_each_upkeep_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let event = GameEventKind::StepStarted { step: Step::End };
        assert!(!ConditionPrimitive::BeginningOfEachUpkeep.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    // Dies/LeavesBattlefield are only ever matched against last_known_info
    // (the object itself is already gone by the time these are checked -
    // see collect_triggers's CR 603.10 fallback) - a minimal GameObject
    // snapshot stands in for that.
    fn dummy_last_known_info(object_id: ObjectId) -> crate::types::object::GameObject {
        use crate::types::card::CardDefinition;
        use crate::types::card::{CardTypeSet, KeywordSet, PTValue, ScriptReference, SupertypeSet};
        use crate::types::counter::CounterSet;
        use crate::types::ids::ObjectLineage;
        use crate::types::mana::ManaCost;
        use crate::types::object::{GameObject, VisibilityPolicy};

        GameObject {
            object_id,
            lineage: ObjectLineage { previous: None },
            definition: CardDefinition {
                definition_id: 1,
                name: "Test Creature".to_string(),
                mana_cost: ManaCost { symbols: Vec::new() },
                color_indicator: None,
                supertypes: SupertypeSet::empty(),
                card_types: CardTypeSet::CREATURE,
                subtypes: Vec::new(),
                oracle_text: String::new(),
                power: Some(PTValue::Fixed(1)),
                toughness: Some(PTValue::Fixed(1)),
                loyalty: None,
                defense: None,
                script_ref: ScriptReference::ByKey("test_creature".to_string()),
                keywords: KeywordSet::empty(),
                target_requirement: None,
                alternative_costs: Vec::new(),
                effect: None,
            },
            owner: CONTROLLER,
            controller: CONTROLLER,
            zone: ZoneKind::Battlefield,
            tapped: false,
            flipped: false,
            face_down: false,
            phased_out: false,
            damage_marked: 0,
            deathtouched: false,
            counters: CounterSet::new(),
            attachments: Vec::new(),
            attached_to: None,
            timestamp: crate::types::ids::SequenceNo(0),
            controlled_since_turn: 0,
            visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
        }
    }

    #[test]
    fn dies_matches_battlefield_to_graveyard_by_last_known_object_id() {
        let source = ObjectId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999), // the new id minted for the graveyard-side object
            from: ZoneKind::Battlefield,
            to: ZoneKind::Graveyard,
            last_known_info: Some(dummy_last_known_info(source)),
        };
        assert!(ConditionPrimitive::Dies.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn dies_does_not_match_a_different_objects_death() {
        let source = ObjectId(0);
        let other = ObjectId(1);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Battlefield,
            to: ZoneKind::Graveyard,
            last_known_info: Some(dummy_last_known_info(other)),
        };
        assert!(!ConditionPrimitive::Dies.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn dies_does_not_match_a_non_death_departure() {
        // Battlefield -> Exile (e.g. a CR 614.13 "exile instead" replacement)
        // is a LeavesBattlefield, not a Dies.
        let source = ObjectId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Battlefield,
            to: ZoneKind::Exile,
            last_known_info: Some(dummy_last_known_info(source)),
        };
        assert!(!ConditionPrimitive::Dies.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn leaves_battlefield_matches_any_destination() {
        let source = ObjectId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Battlefield,
            to: ZoneKind::Exile,
            last_known_info: Some(dummy_last_known_info(source)),
        };
        assert!(ConditionPrimitive::LeavesBattlefield.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn leaves_battlefield_does_not_match_a_move_that_never_left_the_battlefield() {
        let source = ObjectId(0);
        let event = GameEventKind::ZoneChange {
            object: source,
            from: ZoneKind::Hand,
            to: ZoneKind::Battlefield,
            last_known_info: Some(dummy_last_known_info(source)),
        };
        assert!(!ConditionPrimitive::LeavesBattlefield.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    fn drawn_card_last_known_info(drawer: PlayerId) -> crate::types::object::GameObject {
        let mut lki = dummy_last_known_info(ObjectId(42));
        lki.owner = drawer;
        lki.controller = drawer;
        lki.zone = ZoneKind::Library;
        lki
    }

    #[test]
    fn draws_card_matches_the_controllers_own_draw() {
        let source = ObjectId(0);
        let drawer = PlayerId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Library,
            to: ZoneKind::Hand,
            last_known_info: Some(drawn_card_last_known_info(drawer)),
        };
        assert!(ConditionPrimitive::DrawsCard.matches(source, drawer, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn draws_card_does_not_match_a_different_players_draw() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let drawer = PlayerId(1);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Library,
            to: ZoneKind::Hand,
            last_known_info: Some(drawn_card_last_known_info(drawer)),
        };
        assert!(!ConditionPrimitive::DrawsCard.matches(source, controller, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn draws_card_does_not_match_a_non_draw_zone_change() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Hand,
            to: ZoneKind::Graveyard,
            last_known_info: Some(drawn_card_last_known_info(controller)),
        };
        assert!(!ConditionPrimitive::DrawsCard.matches(source, controller, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn each_draws_card_matches_regardless_of_who_drew() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let drawer = PlayerId(1);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Library,
            to: ZoneKind::Hand,
            last_known_info: Some(drawn_card_last_known_info(drawer)),
        };
        assert!(ConditionPrimitive::EachDrawsCard.matches(source, controller, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn each_draws_card_does_not_match_a_non_draw_zone_change() {
        let source = ObjectId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Hand,
            to: ZoneKind::Graveyard,
            last_known_info: None,
        };
        assert!(!ConditionPrimitive::EachDrawsCard.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn beginning_of_your_combat_matches_when_controller_is_active_player() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let event = GameEventKind::StepStarted { step: Step::BeginningOfCombat };
        assert!(ConditionPrimitive::BeginningOfYourCombat.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_your_combat_does_not_match_someone_elses_combat() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted { step: Step::BeginningOfCombat };
        assert!(!ConditionPrimitive::BeginningOfYourCombat.matches(source, controller, active_player, &event));
    }

    #[test]
    fn beginning_of_your_combat_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let event = GameEventKind::StepStarted { step: Step::Upkeep };
        assert!(!ConditionPrimitive::BeginningOfYourCombat.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_each_combat_matches_regardless_of_whose_turn_it_is() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted { step: Step::BeginningOfCombat };
        assert!(ConditionPrimitive::BeginningOfEachCombat.matches(source, controller, active_player, &event));
    }

    #[test]
    fn beginning_of_each_combat_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let event = GameEventKind::StepStarted { step: Step::End };
        assert!(!ConditionPrimitive::BeginningOfEachCombat.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn beginning_of_your_end_step_matches_when_controller_is_active_player() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let event = GameEventKind::StepStarted { step: Step::End };
        assert!(ConditionPrimitive::BeginningOfYourEndStep.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_your_end_step_does_not_match_someone_elses_end_step() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted { step: Step::End };
        assert!(!ConditionPrimitive::BeginningOfYourEndStep.matches(source, controller, active_player, &event));
    }

    #[test]
    fn beginning_of_your_end_step_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let event = GameEventKind::StepStarted { step: Step::BeginningOfCombat };
        assert!(!ConditionPrimitive::BeginningOfYourEndStep.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_each_end_step_matches_regardless_of_whose_turn_it_is() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted { step: Step::End };
        assert!(ConditionPrimitive::BeginningOfEachEndStep.matches(source, controller, active_player, &event));
    }

    #[test]
    fn beginning_of_each_end_step_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let event = GameEventKind::StepStarted { step: Step::Upkeep };
        assert!(!ConditionPrimitive::BeginningOfEachEndStep.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }
}
