//! The `ActivateAbility` command body. See `DESIGN.md` for the rest.

use crate::script::ScriptRuntime;
use crate::types::ability::AbilityEffect;
use crate::types::ability::AbilityKind;
use crate::layers::effective_characteristics;
use crate::types::card::{CardTypeSet, SupertypeSet};
use crate::types::cost::{AdditionalCost, CostSpec};
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::mana::{Color, ManaUnit};
use crate::types::stack::{StackEntryKind, StackObject};
use crate::types::target::TargetSpec;
use crate::types::ids::ScriptHandle;
use crate::types::zone::ZoneKind;

use super::attach::find_ability;
use super::legality::{has_summoning_sickness, is_sorcery_speed, validate_targets};
use super::{CommandOutcome, Internal, PaymentResolution, PendingPaymentRequest, PriorityHandoff, Rejected, Validated};

/// What `apply_activate_ability` needs, all settled by `validate_activate_ability`.
#[derive(Debug)]
pub(crate) enum ActivatePlan {
    /// CR 605.3b: a mana ability with no cost resolves at once.
    FreeMana { actor: PlayerId, source: ObjectId, units: Vec<ManaUnit> },
    /// A mana ability with a real cost still pauses for payment (CR 605.3b).
    PaidMana { actor: PlayerId, source: ObjectId, cost: CostSpec, units: Vec<ManaUnit> },
    /// CR 602.2: any other activated ability goes on the stack.
    Stack {
        actor: PlayerId,
        source: ObjectId,
        source_owner: PlayerId,
        targets: Vec<Target>,
        target_requirement: Option<TargetSpec>,
        x_value: Option<u32>,
        operation: ScriptHandle,
        effect: Option<AbilityEffect>,
        cost: CostSpec,
    },
}

pub(super) fn validate_activate_ability(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    source: ObjectId,
    ability_index: u32,
    targets: Vec<Target>,
    chosen_colors: Vec<Color>,
    x_value: Option<u32>,
) -> Result<Validated, Rejected> {
    // CR 602.2a/117.3c
    // Activating follows the casting process, so the activator must hold priority.
    if state.priority_player != actor {
        return Err(Rejected::IllegalAction(
            "actor does not hold priority".to_string(),
        ));
    }

    let source_object = state.object(source).ok_or(Rejected::InvalidTarget)?;
    // CR 800.4a
    // The ability's owner is its source's owner, not its (possibly `GainControl`'d)
    // controller. Captured now because the borrow ends here.
    let source_owner = source_object.owner;
    // CR 602.2a
    // Only the source's current controller may activate, so this reads
    // layers::effective_controller and not the raw field.
    let controller = crate::layers::effective_controller(state, script_runtime, source);
    if controller != actor {
        return Err(Rejected::IllegalAction(
            "actor does not control the ability's source".to_string(),
        ));
    }
    let ability = find_ability(state, source, ability_index).ok_or_else(|| {
        Rejected::IllegalAction("no such ability on source".to_string())
    })?;
    if ability.kind != AbilityKind::Activated {
        return Err(Rejected::IllegalAction(
            "ability is not an activated ability".to_string(),
        ));
    }
    let effect = ability.effect.clone();
    let cost = ability.cost.clone();
    let operation = ability.operation;
    let target_requirement = ability.target.clone();

    // CR 602.1/112.6
    // Abilities on permanents work only on the battlefield, except Cycling
    // (CR 702.29a), which works from the hand. Matched on the cost shape
    // (DiscardSelf) so RegisteredAbility needs no zone field, as with the Equip
    // check below.
    let requires_hand = cost
        .as_ref()
        .is_some_and(|c| c.additional.iter().any(|a| matches!(a, AdditionalCost::DiscardSelf)));
    // CR 113.6: a script-declared `:SetFunctionsFrom(zone)` ability overrides
    // both (`GameState::ability_function_zones`).
    let declared_zone = state.ability_function_zones.get(&ability.id).copied();
    let required_zone = declared_zone
        .unwrap_or(if requires_hand { ZoneKind::Hand } else { ZoneKind::Battlefield });
    if source_object.zone != required_zone {
        return Err(Rejected::IllegalAction(match required_zone {
            ZoneKind::Hand => "source is not in hand".to_string(),
            ZoneKind::Battlefield => "source is not on the battlefield".to_string(),
            other => format!("source is not in the {other:?} zone"),
        }));
    }

    // CR 702.6a
    // Equip is sorcery speed. Matching on the effect shape avoids a per-ability
    // timing field that only this effect needs (see AbilityEffect::Attach).
    if matches!(effect, Some(AbilityEffect::Attach)) && !is_sorcery_speed(state, actor) {
        return Err(Rejected::IllegalAction(
            "this ability can only be activated as a sorcery".to_string(),
        ));
    }

    // CR 602.2b
    // `chosen_colors` only means something with `AddManaChoice`. Rejecting it
    // elsewhere keeps a stray value from being dropped silently.
    if !chosen_colors.is_empty() && !matches!(effect, Some(AbilityEffect::AddManaChoice { .. })) {
        return Err(Rejected::IllegalAction(
            "chosen_colors given but this ability has no mana-color choice to make".to_string(),
        ));
    }

    // CR 107.3a/107.3k/602.2b
    // An announced x_value and an {X} in this ability's own cost must both be
    // present or both absent. Checked up front, as CastSpell does (cast.rs). CR
    // 107.3k limits this to the ability's own cost.
    let cost_has_x = cost.as_ref().and_then(|c| c.mana.as_ref()).is_some_and(|m| m.has_x());
    match (cost_has_x, x_value) {
        (true, None) => {
            return Err(Rejected::IllegalAction(
                "this ability's cost includes {X} - a value must be announced".to_string(),
            ));
        }
        (false, Some(_)) => {
            return Err(Rejected::IllegalAction(
                "an X value was announced but this ability's cost has no {X}".to_string(),
            ));
        }
        _ => {}
    }

    match effect {
        Some(effect @ (AbilityEffect::AddMana(_) | AbilityEffect::AddManaChoice { .. })) => {
            // CR 605.1a
            if !targets.is_empty() {
                return Err(Rejected::IllegalAction(
                    "a mana ability cannot have targets".to_string(),
                ));
            }

            let units: Vec<ManaUnit> = match effect {
                AbilityEffect::AddMana(units) => units,
                AbilityEffect::AddManaChoice { count, choices } => {
                    if chosen_colors.len() != count as usize {
                        return Err(Rejected::IllegalAction(format!(
                            "expected {count} chosen color(s), got {}",
                            chosen_colors.len()
                        )));
                    }
                    let mut units = Vec::with_capacity(chosen_colors.len());
                    for color in &chosen_colors {
                        if !choices.contains_color(*color) {
                            return Err(Rejected::IllegalAction(format!(
                                "chosen color {color:?} is not among this ability's allowed colors"
                            )));
                        }
                        units.push(ManaUnit::Colored(*color));
                    }
                    units
                }
                _ => unreachable!("matched only AddMana/AddManaChoice above"),
            };
            // CR 106.3/107.4h
            // Mana from a snow source is snow mana, which can pay {S}.
            // TODO: a triggered mana ability's mana is not tagged, because triggered mana abilities are not built
            let units = if effective_characteristics(state, script_runtime, source)
                .supertypes
                .contains(SupertypeSet::SNOW)
            {
                units.into_iter().map(ManaUnit::from_snow_source).collect()
            } else {
                units
            };

            // A mana ability with no `Game.Costs.*` component (`cost: None`, the only shape
            // the `mana_ability` TOML makes) taps and adds mana with no payment pause. One
            // with a real cost (`:AddCost`, a Treasure-style sacrifice, or
            // `Game.Costs.TapSelf`) still skips the stack (CR 605.3b) but pays first
            // (CR 602.2f-h, 118.5a). See `PaymentResolution::ManaAbility`.
            let Some(mut cost) = cost.filter(|c| c.mana.is_some() || !c.additional.is_empty()) else {
                let source_object = state.object(source).ok_or(Rejected::InvalidTarget)?;
                if source_object.tapped {
                    return Err(Rejected::IllegalAction(
                        "source is already tapped".to_string(),
                    ));
                }
                // CR 302.6
                // Summoning sickness restricts only a creature's {T}/{Q} abilities.
                if source_object.definition.card_types.contains(CardTypeSet::CREATURE)
                    && has_summoning_sickness(state, script_runtime, source_object)
                {
                    return Err(Rejected::IllegalAction(
                        "source has summoning sickness".to_string(),
                    ));
                }
                if state.player(actor).is_none() {
                    return Err(Rejected::IllegalAction("actor has no player state".to_string()));
                }
                return Ok(Validated::ActivateAbility(Box::new(ActivatePlan::FreeMana { actor, source, units })));
            };
            // CR 107.3a
            // Substitute the announced X before the cost reaches PaymentPlan. `cost_has_x`
            // guarantees x_value is `Some` when there is an {X}.
            if let Some(x) = x_value {
                cost.mana = cost.mana.map(|m| m.resolve_x(x));
            }

            // CR 602.5a/302.6
            // The same TapSelf pre-check as the non-mana branch. The tap commits only when
            // Engine::submit_payment does.
            if cost.additional.iter().any(|c| matches!(c, AdditionalCost::TapSelf)) {
                let source_object =
                    state.object(source).ok_or(Rejected::InvalidTarget)?;
                if source_object.tapped {
                    return Err(Rejected::IllegalAction(
                        "source is already tapped".to_string(),
                    ));
                }
                if source_object.definition.card_types.contains(CardTypeSet::CREATURE)
                    && has_summoning_sickness(state, script_runtime, source_object)
                {
                    return Err(Rejected::IllegalAction(
                        "source has summoning sickness".to_string(),
                    ));
                }
            }

            Ok(Validated::ActivateAbility(Box::new(ActivatePlan::PaidMana { actor, source, cost, units })))
        }
        // Every effect here goes on the stack: unlike a mana ability (CR 605), a
        // non-mana activated ability does not resolve immediately (CR 602.2). Targets
        // are validated below with validate_targets (CR 602.2b/601.2c-e) before the
        // stack push. For Equip this is where CR 702.6a's target is enforced.
        non_mana_effect @ (None
        | Some(AbilityEffect::GainLife(_))
        | Some(AbilityEffect::Damage(_))
        | Some(AbilityEffect::Destroy)
        | Some(AbilityEffect::BecomeBlocked)
        // CR 701.8/601.2c
        // "Destroy all" has no target requirement to validate, like Shuffle below.
        | Some(AbilityEffect::DestroyAll(_))
        | Some(AbilityEffect::Sacrifice)
        // CR 701.21a: untargeted self form, goes on the stack like Sacrifice.
        | Some(AbilityEffect::SacrificeSelf)
        // CR 701.17a/119.3: Mill and LoseLife are ordinary stack effects too.
        | Some(AbilityEffect::Mill(_))
        | Some(AbilityEffect::LoseLife(_))
        | Some(AbilityEffect::Exile)
        | Some(AbilityEffect::ExileUntilSourceLeaves)
        | Some(AbilityEffect::ReturnToHand)
        | Some(AbilityEffect::ReturnSelfToHand)
        | Some(AbilityEffect::ReturnAllToHand { .. })
        | Some(AbilityEffect::ExileSelf)
        | Some(AbilityEffect::ExileAll { .. })
        // CR 110.2a/400.7: reanimation goes on the stack like any other zone move.
        | Some(AbilityEffect::ReturnToBattlefield { .. })
        | Some(AbilityEffect::ReturnAllToBattlefield { .. })
        // CR 701.26: tap/untap goes on the stack like any other
        // non-mana effect (Icy Manipulator's own ability does).
        | Some(AbilityEffect::Tap)
        | Some(AbilityEffect::Untap)
        | Some(AbilityEffect::TapSelf)
        | Some(AbilityEffect::UntapSelf)
        | Some(AbilityEffect::TapAll { .. })
        | Some(AbilityEffect::UntapAll { .. })
        // CR 701.6a
        // No card grants Counter as an activated ability's effect (a Stifle-style
        // counter is a different targeting shape; see AbilityEffect::Counter). Listed
        // so every variant is named, as with ForEachPlayer, SacrificeRandom and
        // DiscardRandom below.
        | Some(AbilityEffect::Counter)
        // CR 118.12/118.12a
        // Listed for exhaustiveness only: no card grants an "unless paid" consequence
        // as an activated ability's effect.
        | Some(AbilityEffect::UnlessPaid { .. })
        // Listed for exhaustiveness only: no card grants these three as an activated
        // ability's effect.
        | Some(AbilityEffect::ForEachPlayer(_))
        | Some(AbilityEffect::SacrificeRandom { .. })
        | Some(AbilityEffect::DiscardRandom { .. })
        // CR 701.9/701.9b
        // Listed for exhaustiveness only: no card grants a targeted discard as an
        // activated ability's effect.
        | Some(AbilityEffect::Discard { .. })
        | Some(AbilityEffect::Counters { .. })
        | Some(AbilityEffect::Pump { .. })
        | Some(AbilityEffect::GainControl(_))
        | Some(AbilityEffect::Regenerate)
        | Some(AbilityEffect::DrawCards(_))
        | Some(AbilityEffect::CreateToken { .. })
        | Some(AbilityEffect::Shuffle)
        | Some(AbilityEffect::Scry(_))
        | Some(AbilityEffect::Surveil(_))
        | Some(AbilityEffect::Search { .. })
        | Some(AbilityEffect::BecomeCopy(_))
        | Some(AbilityEffect::Attach)
        | Some(AbilityEffect::ExchangeTextBoxes)
        // CR 701.14: fight and the one-sided bite are ordinary stack effects.
        | Some(AbilityEffect::Fight)
        | Some(AbilityEffect::Bite)
        // CR 608.2c: several effects in one ability, run in order.
        | Some(AbilityEffect::Chain { .. })
        // CR 700.2b
        // Listed for exhaustiveness only, because RegisteredAbility has no modes field.
        | Some(AbilityEffect::Modal { .. })
        // Script comes only from a plain spell's CardDefinition.effect, which CastSpell
        // unpacks before the push, so an ability never has this shape. Listed for
        // exhaustiveness only.
        | Some(AbilityEffect::Script(_))
        // CR 702.79a/702.93a
        // Meaningful only on a Dies-triggered ability, and no card grants Persist or
        // Undying as an activated ability's effect. Listed for exhaustiveness only.
        | Some(AbilityEffect::ReturnSelfWithCounter(_))
        // CR 702.35a
        // Meaningful only on a MadnessExiled-triggered ability. Listed for
        // exhaustiveness only.
        | Some(AbilityEffect::MadnessCastOrDiscard)
        // CR 702.85a
        // Meaningful only on a Cast-triggered ability. Listed for exhaustiveness only.
        | Some(AbilityEffect::Cascade)) => {
            validate_targets(
                state,
                script_runtime,
                target_requirement.as_ref(),
                &targets,
                actor,
                source,
                // CR 202.3/107.3a
                // This activation's announced X, which a `mana_value` filter's X bound
                // resolves against, as in CastSpell.
                x_value,
            )?;

            // `None` means no cost: the same empty CostSpec RegisteredAbility::cost
            // describes.
            let mut cost = cost.unwrap_or(CostSpec {
                mana: None,
                additional: Vec::new(),
                alternative: Vec::new(),
            });
            // CR 107.3a
            // Substitute the announced X before the cost reaches PaymentPlan. `cost_has_x`
            // guarantees x_value is `Some` when there is an {X}.
            if let Some(x) = x_value {
                cost.mana = cost.mana.map(|m| m.resolve_x(x));
            }

            // CR 602.5a/302.6
            // An already-tapped source cannot pay a TapSelf cost, and summoning sickness
            // restricts a creature's own {T} ability. Both are checked against the declared
            // cost before anything is pushed, so an illegal activation never happens. The
            // tap itself commits in Engine::submit_payment (CR 602.2f-h).
            if cost.additional.iter().any(|c| matches!(c, AdditionalCost::TapSelf)) {
                let source_object =
                    state.object(source).ok_or(Rejected::InvalidTarget)?;
                if source_object.tapped {
                    return Err(Rejected::IllegalAction(
                        "source is already tapped".to_string(),
                    ));
                }
                if source_object.definition.card_types.contains(CardTypeSet::CREATURE)
                    && has_summoning_sickness(state, script_runtime, source_object)
                {
                    return Err(Rejected::IllegalAction(
                        "source has summoning sickness".to_string(),
                    ));
                }
            }

            Ok(Validated::ActivateAbility(Box::new(ActivatePlan::Stack {
                actor,
                source,
                source_owner,
                targets,
                target_requirement,
                x_value,
                operation,
                effect: non_mana_effect,
                cost,
            })))
        }
    }
}

pub(super) fn apply_activate_ability(state: &mut GameState, plan: ActivatePlan) -> Result<CommandOutcome, Internal> {
    match plan {
        ActivatePlan::FreeMana { actor, source, units } => {
            state
                .object_mut(source)
                .ok_or_else(|| Internal::new("activate: the validated mana source is gone"))?
                .tapped = true;
            let pool = &mut state
                .player_mut(actor)
                .ok_or_else(|| Internal::new("activate: the validated player state is gone"))?
                .mana_pool;
            for unit in &units {
                pool.add_unit(*unit);
            }

            // CR 605.3b
            // A mana ability resolves as part of activation and never uses the stack.
            Ok(CommandOutcome {
                events: vec![GameEventKind::Tapped { object: source }, GameEventKind::ManaAdded { player: actor, units }],
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                requires_payment: None,
                battlefield_entries: Vec::new(),
            })
        }
        ActivatePlan::PaidMana { actor, source, cost, units } => {
            // CR 605.1a/605.3b
            // No StackObject is pushed for a mana ability. `stack_id` is only a handle that
            // identifies this payment pause; Engine::unwind_payment tolerates one that
            // matches nothing on `state.stack`.
            let stack_id = state.allocate_object_id();

            Ok(CommandOutcome {
                events: Vec::new(),
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                requires_payment: Some(PendingPaymentRequest {
                    stack_id,
                    payer: actor,
                    cost,
                    source,
                    on_paid: PaymentResolution::ManaAbility { controller: actor, units },
                }),
                battlefield_entries: Vec::new(),
            })
        }
        ActivatePlan::Stack { actor, source, source_owner, targets, target_requirement, x_value, operation, effect, cost } => {
            let stack_id = state.allocate_object_id();
            state.stack.push(StackObject::new(
                stack_id,
                StackEntryKind::ActivatedAbility,
                source,
                source_owner,
                actor,
                // TODO: no modes on activated abilities, because RegisteredAbility has no modes field (CR 700.2b)
                Vec::new(),
                targets,
                target_requirement,
                x_value,
                false,
                // CR 702.27a
                // Buyback is spell-only.
                false,
                // CR 702.138b
                // Escape is spell-only.
                false,
                operation,
                effect,
                // CR 603.4
                // An intervening-if clause belongs to triggered abilities only.
                None,
                // CR 700.2e
                // Only a modal spell can defer its mode choice.
                None,
                // An activation always comes from a live object, so there is no departed source.
                None,
            ));

            Ok(CommandOutcome {
                events: vec![GameEventKind::StackObjectPushed { stack_id }],
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                // CR 602.2f-h
                // The same AwaitingPayment pause as CastSpell, even for a bare {T} cost
                // (CR 118.5a), through the empty-CostSpec default above.
                requires_payment: Some(PendingPaymentRequest {
                    stack_id,
                    payer: actor,
                    cost,
                    source,
                    on_paid: PaymentResolution::StackObject,
                }),
                battlefield_entries: Vec::new(),
            })
        }
    }
}
