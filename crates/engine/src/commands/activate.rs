//! `execute_command`'s `ActivateAbility` arm body, split out of the
//! original monolithic `commands.rs`.

use crate::script::ScriptRuntime;
use crate::types::ability::AbilityEffect;
use crate::types::ability::AbilityKind;
use crate::types::card::CardTypeSet;
use crate::types::cost::{AdditionalCost, CostSpec};
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, Target};
use crate::types::mana::{Color, ManaUnit};
use crate::types::stack::{StackEntryKind, StackObject};
use crate::types::zone::ZoneKind;

use super::attach::find_ability;
use super::legality::{has_summoning_sickness, is_sorcery_speed, validate_targets};
use super::{CommandError, CommandOutcome, PaymentResolution, PendingPaymentRequest, PriorityHandoff};

pub(super) fn execute_activate_ability(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    source: ObjectId,
    ability_index: u32,
    targets: Vec<Target>,
    chosen_colors: Vec<Color>,
    x_value: Option<u32>,
) -> Result<CommandOutcome, CommandError> {
    // CR 602.2a/117.3c: activating an ability follows the same
    // process as casting a spell - the activator must hold priority
    // to begin with.
    if state.priority_player != actor {
        return Err(CommandError::IllegalAction(
            "actor does not hold priority".to_string(),
        ));
    }

    let source_object = state.object(source).ok_or(CommandError::InvalidTarget)?;
    // CR 800.4a: an ability's owner is its source permanent's owner,
    // not its (possibly `GainControl`'d) controller - captured now,
    // this borrow doesn't survive past this statement.
    let source_owner = source_object.owner;
    // CR 602.2a: only the source's *current* controller may activate
    // it - layers::effective_controller, not the raw field, so a
    // GainControl'd permanent's abilities follow its new controller.
    let controller = crate::layers::effective_controller(state, script_runtime, source);
    if controller != actor {
        return Err(CommandError::IllegalAction(
            "actor does not control the ability's source".to_string(),
        ));
    }
    let ability = find_ability(state, source, ability_index).ok_or_else(|| {
        CommandError::IllegalAction("no such ability on source".to_string())
    })?;
    if ability.kind != AbilityKind::Activated {
        return Err(CommandError::IllegalAction(
            "ability is not an activated ability".to_string(),
        ));
    }
    let effect = ability.effect.clone();
    let cost = ability.cost.clone();
    let operation = ability.operation;
    let target_requirement = ability.target.clone();

    // CR 602.1/112.6: activated abilities on permanents are only usable
    // while the permanent is on the battlefield, "unless the ability itself
    // says otherwise" - CR 702.29a Cycling is exactly that override, so its
    // own required zone is Hand instead. Matched on the ability's own cost
    // shape (does it include DiscardSelf?) rather than a new RegisteredAbility
    // field, same "match on shape, don't grow the struct" precedent the
    // Equip sorcery-speed check just below already set.
    let requires_hand = cost
        .as_ref()
        .is_some_and(|c| c.additional.iter().any(|a| matches!(a, AdditionalCost::DiscardSelf)));
    let required_zone = if requires_hand { ZoneKind::Hand } else { ZoneKind::Battlefield };
    if source_object.zone != required_zone {
        return Err(CommandError::IllegalAction(if requires_hand {
            "source is not in hand".to_string()
        } else {
            "source is not on the battlefield".to_string()
        }));
    }

    // CR 702.6a: "Activate only as a sorcery" - Equip's own timing
    // restriction, enforced here by matching on the effect shape
    // rather than a new RegisteredAbility field (see AbilityEffect::
    // Attach's own doc comment for why - a generic per-ability
    // timing-restriction field isn't worth the ~94-call-site
    // blast radius for a restriction only this one effect needs
    // today). `is_sorcery_speed` was written anticipating exactly
    // this call site (see its own doc comment) but had no real
    // caller until now.
    if matches!(effect, Some(AbilityEffect::Attach)) && !is_sorcery_speed(state, actor) {
        return Err(CommandError::IllegalAction(
            "this ability can only be activated as a sorcery".to_string(),
        ));
    }

    // CR 602.2b: `chosen_colors` is only meaningful alongside
    // `AddManaChoice` - reject rather than silently ignore it
    // against any other ability (a plain `AddMana`'s units are
    // fixed, and no non-mana ability has a color choice at all), so
    // a caller passing it by mistake gets a loud error instead of a
    // quietly-dropped value.
    if !chosen_colors.is_empty() && !matches!(effect, Some(AbilityEffect::AddManaChoice { .. })) {
        return Err(CommandError::IllegalAction(
            "chosen_colors given but this ability has no mana-color choice to make".to_string(),
        ));
    }

    // CR 107.3a/107.3k/602.2b: an announced x_value and an {X} in this
    // ability's own activation cost must agree - both present or both
    // absent - checked once, up front, before either match arm below
    // touches `cost` at all. Mirrors CastSpell's own has_x/x_value check
    // (cast.rs) exactly; CR 107.3k is why this only ever looks at this
    // ability's own cost, never anything else already on the stack.
    let cost_has_x = cost.as_ref().and_then(|c| c.mana.as_ref()).is_some_and(|m| m.has_x());
    match (cost_has_x, x_value) {
        (true, None) => {
            return Err(CommandError::IllegalAction(
                "this ability's cost includes {X} - a value must be announced".to_string(),
            ));
        }
        (false, Some(_)) => {
            return Err(CommandError::IllegalAction(
                "an X value was announced but this ability's cost has no {X}".to_string(),
            ));
        }
        _ => {}
    }

    match effect {
        Some(effect @ (AbilityEffect::AddMana(_) | AbilityEffect::AddManaChoice { .. })) => {
            // CR 605.1a: a mana ability never has a target.
            if !targets.is_empty() {
                return Err(CommandError::IllegalAction(
                    "a mana ability cannot have targets".to_string(),
                ));
            }

            let units: Vec<ManaUnit> = match effect {
                AbilityEffect::AddMana(units) => units,
                AbilityEffect::AddManaChoice { count, choices } => {
                    if chosen_colors.len() != count as usize {
                        return Err(CommandError::IllegalAction(format!(
                            "expected {count} chosen color(s), got {}",
                            chosen_colors.len()
                        )));
                    }
                    let mut units = Vec::with_capacity(chosen_colors.len());
                    for color in &chosen_colors {
                        if !choices.contains_color(*color) {
                            return Err(CommandError::IllegalAction(format!(
                                "chosen color {color:?} is not among this ability's allowed colors"
                            )));
                        }
                        units.push(ManaUnit::Colored(*color));
                    }
                    units
                }
                _ => unreachable!("matched only AddMana/AddManaChoice above"),
            };

            // A mana ability built with no `Game.Costs.*` component at
            // all (`cost: None` - the only shape `mana_ability` TOML
            // can ever produce) keeps its original hardcoded-tap,
            // no-payment-pause behavior unchanged below. A mana
            // ability with a *real* cost (`Game.CreateManaAbility`'s
            // own `:AddCost`, e.g. a Treasure/Lotus-Petal-style
            // sacrifice, or an explicit `Game.Costs.TapSelf`) still
            // never reaches the stack (CR 605.3b), but does need a
            // real payment step first, same as any other ability (CR
            // 602.2f-h/118.5a) - see `PaymentResolution::ManaAbility`.
            let has_real_cost = cost
                .as_ref()
                .is_some_and(|c| c.mana.is_some() || !c.additional.is_empty());

            if !has_real_cost {
                let source_object = state.object(source).ok_or(CommandError::InvalidTarget)?;
                if source_object.tapped {
                    return Err(CommandError::IllegalAction(
                        "source is already tapped".to_string(),
                    ));
                }
                // CR 302.6: summoning sickness only restricts a
                // creature's {T}/{Q} abilities - non-creature
                // permanents (lands, artifacts, ...) ignore it
                // entirely.
                if source_object.definition.card_types.contains(CardTypeSet::CREATURE)
                    && has_summoning_sickness(state, script_runtime, source_object)
                {
                    return Err(CommandError::IllegalAction(
                        "source has summoning sickness".to_string(),
                    ));
                }

                state.object_mut(source).ok_or(CommandError::InvalidTarget)?.tapped = true;
                let pool = &mut state
                    .player_mut(actor)
                    .ok_or_else(|| {
                        CommandError::IllegalAction("actor has no player state".to_string())
                    })?
                    .mana_pool;
                for unit in &units {
                    pool.add_unit(*unit);
                }

                // CR 605.3b: a mana ability resolves immediately as
                // part of activation - it never uses the stack.
                return Ok(CommandOutcome {
                    events: vec![
                        GameEventKind::Tapped { object: source },
                        GameEventKind::ManaAdded { player: actor, units },
                    ],
                    broke_pass_chain: true,
                    priority_handoff: PriorityHandoff::Unchanged,
                    requires_payment: None,
                    battlefield_entries: Vec::new(),
                });
            }

            let mut cost = cost.expect("has_real_cost already confirmed Some");
            // CR 107.3a: substitute the announced X into this mana
            // ability's own cost before it's ever handed to PaymentPlan -
            // `cost_has_x` above already guarantees `x_value` is `Some`
            // whenever this cost actually has an `{X}` to resolve.
            if let Some(x) = x_value {
                cost.mana = cost.mana.map(|m| m.resolve_x(x));
            }

            // CR 602.5a/302.6: same TapSelf pre-check the non-mana
            // branch below takes, when this mana ability's own cost
            // happens to include one - the actual tap still only
            // commits once Engine::submit_payment does, same as any
            // other AdditionalCost::TapSelf.
            if cost.additional.iter().any(|c| matches!(c, AdditionalCost::TapSelf)) {
                let source_object =
                    state.object(source).ok_or(CommandError::InvalidTarget)?;
                if source_object.tapped {
                    return Err(CommandError::IllegalAction(
                        "source is already tapped".to_string(),
                    ));
                }
                if source_object.definition.card_types.contains(CardTypeSet::CREATURE)
                    && has_summoning_sickness(state, script_runtime, source_object)
                {
                    return Err(CommandError::IllegalAction(
                        "source has summoning sickness".to_string(),
                    ));
                }
            }

            // No StackObject is ever pushed for a mana ability (CR
            // 605.1a/605.3b) - `stack_id` here is only an opaque
            // handle for this payment pause to be identified by
            // (`Engine::unwind_payment` already tolerates a `stack_id`
            // matching nothing on `state.stack`), never a real
            // resolvable stack entry.
            let stack_id = state.allocate_object_id();

            return Ok(CommandOutcome {
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
            });
        }
        // None (script-bridge-only effect) and Some(GainLife)/
        // Some(Damage)/Some(Destroy)/Some(Counters)/Some(Pump)/
        // Some(GainControl)/Some(Regenerate)/Some(DrawCards)/
        // Some(Shuffle)/Some(Scry)/Some(Surveil)/Some(Search)/
        // Some(BecomeCopy)/Some(Attach) (a non-mana ActivateAbility
        // whose effect happens to be a recognized primitive) all
        // take the same path: unlike CR 605's mana abilities, a
        // non-mana activated ability uses the stack (CR 602.2)
        // rather than resolving immediately. Targets are validated
        // below (CR 602.2b/601.2c-e) via the same validate_targets
        // CastSpell uses, before any state mutation (stack push)
        // happens - for Attach (Equip), this is where CR 702.6a's
        // "target creature you control" is enforced, same as any
        // other targeted ability.
        non_mana_effect @ (None
        | Some(AbilityEffect::GainLife(_))
        | Some(AbilityEffect::Damage(_))
        | Some(AbilityEffect::Destroy)
        | Some(AbilityEffect::BecomeBlocked)
        // CR 701.8/601.2c: no different from any other untargeted
        // effect shape at this level - "destroy all X" needs no
        // targets validated (target_requirement is None for it),
        // same as Shuffle just below.
        | Some(AbilityEffect::DestroyAll(_))
        | Some(AbilityEffect::Sacrifice)
        | Some(AbilityEffect::Exile)
        // CR 701.6a: no real card grants Counter as an activated ability's
        // own effect (Stifle-style "counter target activated/triggered
        // ability" is a different, unbuilt targeting shape - see
        // AbilityEffect::Counter's own doc comment), but it's still a
        // recognized primitive that goes on the stack the same way as any
        // other here. Listed for exhaustiveness, same reason as
        // ForEachPlayer/SacrificeRandom/DiscardRandom below.
        | Some(AbilityEffect::Counter)
        // CR 118.12/118.12a: same exhaustiveness-only listing as Counter
        // just above - no real card grants an "unless paid" consequence
        // as an activated ability's own effect either.
        | Some(AbilityEffect::UnlessPaid { .. })
        // `phase5/mass-choice-effects`: no real card grants any of these
        // three as an activated ability's own effect yet, but they're
        // recognized primitives that go on the stack the same way as any
        // other here - listed for exhaustiveness, same reason as
        // ReturnSelfWithCounter/MadnessCastOrDiscard/Cascade below.
        | Some(AbilityEffect::ForEachPlayer(_))
        | Some(AbilityEffect::SacrificeRandom { .. })
        | Some(AbilityEffect::DiscardRandom { .. })
        // CR 701.9/701.9b: same exhaustiveness-only listing as Counter/
        // UnlessPaid above - no real card grants a targeted discard as an
        // activated ability's own effect either.
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
        // CR 700.2b (a modal activated/triggered ability) is still
        // unmodeled - RegisteredAbility has no modes field, so this
        // variant can't actually originate from an ability today.
        // Listed for exhaustiveness only, same "every variant named,
        // no wildcard" convention this disjunction already follows.
        | Some(AbilityEffect::Modal(_))
        // AbilityEffect::Script only ever comes from a plain spell's
        // CardDefinition.effect (CastSpell unpacks it before push -
        // see that variant's own doc comment) - an ability's effect
        // can't be this shape. Listed for exhaustiveness only, same
        // reason as Modal just above.
        | Some(AbilityEffect::Script(_))
        // CR 702.79a/702.93a: only ever meaningful on a Dies-triggered
        // ability (see this variant's own doc comment) - no real card
        // grants Persist/Undying as an activated ability's own effect, but
        // it's still a recognized primitive that goes on the stack the
        // same way as any other here. Listed for exhaustiveness, same
        // reason as Modal/Script above.
        | Some(AbilityEffect::ReturnSelfWithCounter(_))
        // CR 702.35a: only ever meaningful on a MadnessExiled-triggered
        // ability (see this variant's own doc comment) - no real card
        // grants Madness's own "cast or discard" as an activated ability's
        // effect, but it's still a recognized primitive that goes on the
        // stack the same way as any other here. Listed for exhaustiveness,
        // same reason as ReturnSelfWithCounter above.
        | Some(AbilityEffect::MadnessCastOrDiscard)
        // CR 702.85a: only ever meaningful on a Cast-triggered ability (see
        // this variant's own doc comment) - no real card grants Cascade as
        // an activated ability's own effect either. Listed for
        // exhaustiveness, same reason as MadnessCastOrDiscard above.
        | Some(AbilityEffect::Cascade)) => {
            validate_targets(
                state,
                script_runtime,
                target_requirement.as_ref(),
                &targets,
                actor,
                source,
                // CR 202.3/107.3a: this activation's own announced X, if
                // any - a `mana_value` filter's own `X` bound (e.g. "target
                // permanent with mana value X") resolves against exactly
                // this, same as CastSpell's own call below.
                x_value,
            )?;

            // `None` means "no cost at all" - same empty-CostSpec
            // treatment RegisteredAbility::cost's own doc comment
            // describes.
            let mut cost = cost.unwrap_or(CostSpec {
                mana: None,
                additional: Vec::new(),
                alternative: Vec::new(),
            });
            // CR 107.3a: substitute the announced X into this ability's own
            // cost before it's ever handed to PaymentPlan - `cost_has_x`
            // above already guarantees `x_value` is `Some` whenever this
            // cost actually has an `{X}` to resolve.
            if let Some(x) = x_value {
                cost.mana = cost.mana.map(|m| m.resolve_x(x));
            }

            // CR 602.5a/302.6: a TapSelf cost can't be paid by an
            // already-tapped source, and a creature's own {T}
            // ability is restricted by summoning sickness - both
            // checked against the declared cost up front, before
            // anything is pushed (an illegal activation never
            // happens, same stance validate_targets already takes).
            // The actual tap (like every other additional-cost
            // mutation) happens once Engine::submit_payment commits
            // a real PaymentPlan - CR 602.2f-h treats paying the
            // cost as its own step, separate from announcing the
            // activation.
            if cost.additional.iter().any(|c| matches!(c, AdditionalCost::TapSelf)) {
                let source_object =
                    state.object(source).ok_or(CommandError::InvalidTarget)?;
                if source_object.tapped {
                    return Err(CommandError::IllegalAction(
                        "source is already tapped".to_string(),
                    ));
                }
                if source_object.definition.card_types.contains(CardTypeSet::CREATURE)
                    && has_summoning_sickness(state, script_runtime, source_object)
                {
                    return Err(CommandError::IllegalAction(
                        "source has summoning sickness".to_string(),
                    ));
                }
            }

            // CR 400.7: the stack object is a new id, distinct
            // from `source` (which stays on the battlefield) -
            // same convention PutTriggersApnap uses for
            // triggered abilities.
            let stack_id = state.allocate_object_id();
            state.stack.push(StackObject::new(
                stack_id,
                StackEntryKind::ActivatedAbility,
                source,
                source_owner,
                actor,
                // TODO: no modes on activated abilities yet (CR 700.2b -
                // RegisteredAbility has no modes field at all).
                Vec::new(),
                targets,
                target_requirement,
                x_value,
                false,
                // CR 702.27a: buyback is a spell-only concept - an
                // activated ability never carries it.
                false,
                // CR 702.138b: "escaped" is a spell-cast-shaped concept - an
                // activated ability never carries it either.
                false,
                operation,
                non_mana_effect,
                // CR 603.4's "if" clause is triggered-ability-only
                // text - an activated ability never carries one.
                None,
                // CR 700.2e: only a modal spell can defer its mode
                // choice this way - an activated ability never does.
                None,
                // An activated ability never has a departed source - it's
                // always activated from a live object.
                None,
            ));

            return Ok(CommandOutcome {
                events: vec![GameEventKind::StackObjectPushed { stack_id }],
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                // CR 602.2f-h: same AwaitingPayment pause CastSpell
                // uses, even for a bare {T} cost (CR 118.5a's
                // "explicit empty acknowledgment" stance applies
                // here too via the empty-CostSpec default above).
                requires_payment: Some(PendingPaymentRequest {
                    stack_id,
                    payer: actor,
                    cost,
                    source,
                    on_paid: PaymentResolution::StackObject,
                }),
                battlefield_entries: Vec::new(),
            });
        }
    }
}
