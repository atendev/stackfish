//! The `CastSpell` command body. See `DESIGN.md` for the rest.

use std::collections::HashSet;

use crate::script::ScriptRuntime;
use crate::types::ability::AbilityEffect;
use crate::types::card::{CardTypeSet, KeywordSet, ModeChoice};
use crate::types::cost::{self, AdditionalCost, AlternativeCost, CostSpec};
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId, ScriptHandle, Target};
use crate::types::mana::ManaCost;
use crate::types::stack::{PendingModeChoice, StackEntryKind, StackObject};
use crate::types::target::TargetSpec;
use crate::types::zone::ZoneKind;

use super::attach::{non_eliminated_players, remap_abilities};
use super::legality::{combine_chosen_modes, is_sorcery_speed, validate_targets};
use super::{CommandOutcome, Internal, PaymentResolution, PendingPaymentRequest, PriorityHandoff, Rejected, Validated};

/// What `apply_cast_spell` needs, all settled by `validate_cast_spell`.
#[derive(Debug)]
pub(crate) struct CastPlan {
    actor: PlayerId,
    card: ObjectId,
    modes: Vec<u32>,
    targets: Vec<Target>,
    x_value: Option<u32>,
    pay_buyback: bool,
    from_zone: ZoneKind,
    from_owner: Option<PlayerId>,
    cost: CostSpec,
    exile_on_leaving_stack: bool,
    escaped: bool,
    cast_from_command_zone: bool,
    modal_effect: Option<AbilityEffect>,
    target_requirement: Option<TargetSpec>,
    pending_mode_choice: Option<PendingModeChoice>,
}

pub(super) fn validate_cast_spell(
    state: &GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    card: ObjectId,
    modes: Vec<u32>,
    targets: Vec<Target>,
    x_value: Option<u32>,
    alternative_cost: Option<usize>,
    pay_buyback: bool,
) -> Result<Validated, Rejected> {
    // CR 601.3/702.35b
    // A one-shot timing waiver set by `Engine::submit_madness_cast` just before this
    // call. The resolving madness trigger's "you may cast it" is the effect that
    // allows casting outside the card's normal timing (see
    // GameState::pending_timing_bypass for why this is a side channel).
    // `apply_cast_spell` clears both grants so they cannot leak into a later cast of
    // the same card.
    let timing_bypassed = state.pending_timing_bypass == Some(card);
    // CR 702.85a
    // A one-shot "cast this object for {0} from Exile" grant set by
    // `Engine::submit_cascade_cast` just before this call. It cannot be a printed
    // `alternative_costs` entry because the permission comes from Cascade's effect
    // (see GameState::pending_free_cast).
    let free_cast = state.pending_free_cast == Some(card);
    let free_cast_alternative = AlternativeCost::Replacement {
        description: "cast without paying its mana cost".to_string(),
        cost: CostSpec::mana_only(ManaCost { symbols: Vec::new() }),
        from_zone: ZoneKind::Exile,
        exile_on_leaving_stack: false,
        escape: false,
    };
    let object = state.object(card).ok_or(Rejected::InvalidTarget)?;
    if object.definition.card_types.contains(CardTypeSet::LAND) {
        return Err(Rejected::IllegalAction(
            "lands aren't cast as spells - use PlayLand".to_string(),
        ));
    }
    // CR 702.27a
    // Buyback is announced up front with alternative_cost and x_value. A card
    // without one is rejected before any mutation.
    if pay_buyback && object.definition.buyback_cost.is_none() {
        return Err(Rejected::IllegalAction(
            "this spell has no buyback cost to pay".to_string(),
        ));
    }
    // CR 601.2b
    // An announced alternative cost (flashback, etc) replaces the mana cost and, for
    // the supported ones, grants permission to cast from a zone other than the hand.
    // So the zone permission and the cost choice resolve together.
    let alternative: Option<&AlternativeCost> = if free_cast {
        Some(&free_cast_alternative)
    } else {
        match alternative_cost {
            Some(idx) => Some(
                object
                    .definition
                    .alternative_costs
                    .get(idx)
                    .ok_or(Rejected::IllegalAction(
                        "no such alternative cost on this card".to_string(),
                    ))?,
            ),
            None => None,
        }
    };
    // CR 903.8
    // A commander may be cast from the command zone by its owner (not controller)
    // at its normal cost, with no alternative cost announced. Checked before the
    // general zone rule because it depends on ownership and not on required_zone.
    let cast_from_command_zone = alternative.is_none()
        && object.is_commander
        && object.zone == ZoneKind::Command
        && object.owner == actor;
    // TODO: nothing checks that the chosen alternative cost really grants flashback's graveyard permission (a Flashback keyword versus an ad-hoc alternative_costs entry)
    // The alternative's from_zone must match where the card is. Without an
    // alternative cost the card must be in hand.
    let required_zone = match alternative {
        Some(AlternativeCost::Replacement { from_zone, .. }) => *from_zone,
        None => ZoneKind::Hand,
    };
    if !cast_from_command_zone
        && (object.zone != required_zone || object.controller != actor)
    {
        return Err(Rejected::IllegalAction(
            "card is not in the zone required to cast it this way".to_string(),
        ));
    }
    let from_zone = object.zone;
    // CR 117.1a/602/702.8b
    // Instants and flash spells may be cast any time the actor holds priority.
    // Everything else needs the sorcery window; is_sorcery_speed already requires
    // priority, so this branch adds only the priority check.
    // TODO: effects that grant flash ("as though it had flash") are not on continuous_effects, so only the card's own Flash keyword counts
    let castable_at_instant_speed = object.definition.card_types.contains(CardTypeSet::INSTANT)
        || object.definition.keywords.contains(KeywordSet::FLASH);
    if !timing_bypassed {
        if castable_at_instant_speed {
            if state.priority_player != actor {
                return Err(Rejected::IllegalAction(
                    "actor does not hold priority".to_string(),
                ));
            }
        } else if !is_sorcery_speed(state, actor) {
            return Err(Rejected::IllegalAction(
                "this spell can only be cast at sorcery speed: actor's turn, a main \
                 phase, actor holding priority, and an empty stack"
                    .to_string(),
            ));
        }
    }

    // CR 700.2a/d/h
    // A modal card announces its modes here, before targets (CR 601.2b comes before
    // 601.2c). A non-modal card must announce none. `mode_choice` says how many
    // modes to choose, and a repeated index is rejected unless `repeatable` is set
    // (CR 700.2d). `mode_additional_costs` collects each chosen instance's
    // `SpellMode::additional_cost` (CR 700.2h). It is added to `CostSpec.additional`
    // once `cost` is computed, so it pays through the same PaymentPlan as an
    // activated ability's additional costs. A repeated mode pays its cost once per
    // instance because the list is per instance and not deduped by index.
    let (target_requirement, modal_effect, mode_additional_costs, pending_mode_choice): (
        Option<TargetSpec>,
        Option<AbilityEffect>,
        Vec<AdditionalCost>,
        Option<PendingModeChoice>,
    ) = if object.definition.modes.is_empty() {
            if !modes.is_empty() {
                return Err(Rejected::IllegalAction(
                    "this card isn't modal - no modes to choose".to_string(),
                ));
            }
            (object.definition.target_requirement.clone(), None, Vec::new(), None)
        } else if let ModeChoice::ExactlyChosenByOpponent(_) | ModeChoice::UpToChosenByOpponent(_) =
            object.definition.mode_choice
        {
            // CR 700.2e
            // The other player chooses, not the caster (see `ModeChoice`). The choice is
            // deferred to a pause at resolution.
            if !modes.is_empty() {
                return Err(Rejected::IllegalAction(
                    "this card's mode is chosen by another player, not the caster - submit \
                     no modes when casting it"
                        .to_string(),
                ));
            }
            if !targets.is_empty() {
                return Err(Rejected::IllegalAction(
                    "this card's mode (and therefore its targets) isn't chosen yet"
                        .to_string(),
                ));
            }
            if object
                .definition
                .modes
                .iter()
                .any(|m| m.target_requirement.is_some() || m.additional_cost.is_some())
            {
                return Err(Rejected::IllegalAction(
                    "a chosen-by-opponent modal card's modes must have no target \
                     requirement and no additional cost"
                        .to_string(),
                ));
            }
            let others: Vec<PlayerId> =
                non_eliminated_players(state).into_iter().filter(|&p| p != actor).collect();
            let [chooser] = others.as_slice() else {
                return Err(Rejected::IllegalAction(
                    "a chosen-by-opponent modal card needs exactly one other player in the \
                     game - CR 700.2e's 3+ player 'controller decides who chooses' \
                     sub-clause isn't supported yet"
                        .to_string(),
                ));
            };
            let (min, max) = match object.definition.mode_choice {
                ModeChoice::ExactlyChosenByOpponent(n) => (n, n),
                ModeChoice::UpToChosenByOpponent(n) => (1, n),
                _ => unreachable!("matched above"),
            };
            (
                None,
                None,
                Vec::new(),
                Some(PendingModeChoice { chooser: *chooser, min, max }),
            )
        } else if let ModeChoice::PawprintBudget(budget) = object.definition.mode_choice {
            // CR 700.2i
            // Limited by total pawprint weight and not mode count: zero modes is legal, and
            // there are no repeats (no card combines a pawprint budget with the repeat
            // exception of CR 700.2d).
            let mut seen_modes = HashSet::new();
            let mut total_weight: u32 = 0;
            for &m in &modes {
                if !seen_modes.insert(m) {
                    return Err(Rejected::IllegalAction(
                        "the same mode can't be chosen more than once".to_string(),
                    ));
                }
                let mode = object.definition.modes.get(m as usize).ok_or_else(|| {
                    Rejected::IllegalAction("no such mode on this card".to_string())
                })?;
                total_weight += mode.pawprint_weight;
            }
            if total_weight > budget {
                return Err(Rejected::IllegalAction(format!(
                    "this modal spell allows choosing modes worth up to {budget} pawprint(s), got {total_weight}"
                )));
            }
            let mut chosen_indices = modes.clone();
            chosen_indices.sort_unstable();
            let mode_additional_costs: Vec<AdditionalCost> = chosen_indices
                .iter()
                .filter_map(|&idx| object.definition.modes.get(idx as usize).and_then(|m| m.additional_cost.clone()))
                .collect();
            let (target_requirement, modal_effect) =
                combine_chosen_modes(&object.definition.modes, &chosen_indices).map_err(Rejected::IllegalAction)?;
            (target_requirement, modal_effect, mode_additional_costs, None)
        } else {
            // CR 700.2a
            // Never zero, even for an UpTo spell (ModeChoice::bounds already floors it).
            let (min, max, repeatable) = object.definition.mode_choice.bounds();
            let chosen_count = modes.len() as u32;
            if chosen_count < min || chosen_count > max {
                return Err(Rejected::IllegalAction(format!(
                    "this modal spell requires choosing between {min} and {max} mode(s), got {chosen_count}"
                )));
            }
            if !repeatable {
                let mut seen_modes = HashSet::new();
                for &m in &modes {
                    if !seen_modes.insert(m) {
                        return Err(Rejected::IllegalAction(
                            "the same mode can't be chosen more than once".to_string(),
                        ));
                    }
                }
            }
            // CR 700.2
            // Chosen modes' effects happen in printed order, not the order the player
            // listed them.
            let mut chosen_indices = modes.clone();
            chosen_indices.sort_unstable();
            let mut chosen_modes = Vec::with_capacity(chosen_indices.len());
            for &idx in &chosen_indices {
                let mode = object.definition.modes.get(idx as usize).ok_or_else(|| {
                    Rejected::IllegalAction("no such mode on this card".to_string())
                })?;
                chosen_modes.push(mode);
            }
            // CR 700.2h
            // Every chosen mode's additional cost, whether one or several modes were chosen.
            let mode_additional_costs: Vec<AdditionalCost> =
                chosen_modes.iter().filter_map(|m| m.additional_cost.clone()).collect();
            // CR 700.2c/d
            // A single mode passes through bare. Two or more combine into one
            // TargetSpec::Multiple and AbilityEffect::Modal pair, which ResolveStackTop
            // splits again with split_targets_by_requirement. A modal triggered ability
            // shares this through commands::legality::combine_chosen_modes (CR 603.3c); see
            // its doc comment for the combinations it cannot make.
            let (target_requirement, modal_effect) =
                combine_chosen_modes(&object.definition.modes, &chosen_indices).map_err(Rejected::IllegalAction)?;
            (target_requirement, modal_effect, mode_additional_costs, None)
        };

    // CR 601.2c/601.2e
    // Validated below, so an illegal cast is rejected before anything mutates.
    // TODO: no division of an effect among targets (CR 601.2d)
    // TODO: no splice, since PlayerAction and StackObject cannot carry which cards are being spliced (CR 601.2b/702.47)
    validate_targets(
        state,
        script_runtime,
        target_requirement.as_ref(),
        &targets,
        actor,
        card,
        // CR 202.3/107.3a
        // This cast's announced X, which a `mana_value` target filter's X bound
        // resolves against.
        x_value,
    )?;

    // CR 601.2f
    // Total cost is the printed mana cost, or the announced alternative cost in its
    // place (CR 601.2b), never both. `cost.additional` is filled only from chosen
    // modes' costs (CR 700.2h).
    // TODO: CardDefinition has no top-level additional cost, so a non-modal card cannot have one
    //
    // CR 107.3a/601.2b
    // An {X} cost and an announced x_value must both be present or both absent. X
    // is then substituted before PaymentPlan::validate sees the cost (see
    // ManaCost::resolve_x). Activated abilities do the same (activate.rs
    // `cost_has_x`).
    // TODO: X combined with an alternative cost (flashback on an X spell) is not handled, and x_value goes unchecked in that branch
    let (mut cost, exile_on_leaving_stack, escaped) = match alternative {
        Some(AlternativeCost::Replacement { cost, exile_on_leaving_stack, escape, .. }) => {
            (cost.clone(), *exile_on_leaving_stack, *escape)
        }
        None => {
            let has_x = object.definition.mana_cost.has_x();
            let resolved_mana_cost = match (has_x, x_value) {
                (true, Some(x)) => object.definition.mana_cost.resolve_x(x),
                (false, None) => object.definition.mana_cost.clone(),
                (true, None) => {
                    return Err(Rejected::IllegalAction(
                        "this spell's cost includes {X} - a value must be announced"
                            .to_string(),
                    ));
                }
                (false, Some(_)) => {
                    return Err(Rejected::IllegalAction(
                        "an X value was announced but this spell's cost has no {X}"
                            .to_string(),
                    ));
                }
            };
            // CR 601.2f/118.9
            // Static cost modifiers (Goblin Warchief, Thalia) apply after X is resolved and
            // before the cost is locked. They touch only the plain mana-cost path, never an
            // alternative cost (see CostModifier in cost.rs).
            let resolved_mana_cost =
                cost::apply_cost_modifiers(&resolved_mana_cost, &state.cost_modifiers, object);
            // CR 903.8/702.124d
            // Commander tax is {2} generic per prior cast of this commander from the
            // command zone this game, counted by name and not ObjectId (see
            // PlayerState::commander_casts). Only the command-zone path taxes.
            let resolved_mana_cost = if cast_from_command_zone {
                let prior_casts = state
                    .player(actor)
                    .and_then(|p| p.commander_casts.get(&object.definition.name))
                    .copied()
                    .unwrap_or(0);
                // A first cast (prior_casts == 0) must add nothing. apply_cost_delta(0) is not
                // a no-op because it always drops an explicit Generic(0) symbol (see
                // cost::apply_cost_modifiers), which would erase an {X} commander's explicit
                // zero when X == 0.
                if prior_casts == 0 {
                    resolved_mana_cost
                } else {
                    resolved_mana_cost.apply_cost_delta(2 * prior_casts as i32)
                }
            } else {
                resolved_mana_cost
            };
            (CostSpec::mana_only(resolved_mana_cost), false, false)
        }
    };
    // CR 700.2h
    // Every chosen mode's additional cost is paid with the rest of the cost,
    // whether an alternative or the normal mana cost applies.
    cost.additional.extend(mode_additional_costs);
    // CR 702.27a
    // Buyback's cost is paid in addition to whatever cost applies (pay_buyback
    // implies buyback_cost.is_some(), checked above). It is concatenated onto the
    // mana symbols so PaymentPlan::validate enforces one combined total.
    if pay_buyback {
        let buyback_cost = object.definition.buyback_cost.clone().ok_or_else(|| {
            Rejected::IllegalAction("this spell has no buyback cost to pay".to_string())
        })?;
        let mut mana = cost.mana.unwrap_or(ManaCost { symbols: Vec::new() });
        mana.symbols.extend(buyback_cost.symbols);
        cost.mana = Some(mana);
    }

    // CR 400.7
    // Exile, like Battlefield, is a shared zone and not per-player, using the same
    // owner derivation as `commands::commit_zone_move`. Looked up before any id is
    // allocated or the card is removed, so a missing zone rejects without a partial
    // move.
    let from_owner = if from_zone == ZoneKind::Exile { None } else { Some(actor) };
    if state.zone(from_zone, from_owner).is_none() {
        return Err(Rejected::IllegalAction("actor has no such zone".to_string()));
    }

    Ok(Validated::CastSpell(Box::new(CastPlan {
        actor,
        card,
        modes,
        targets,
        x_value,
        pay_buyback,
        from_zone,
        from_owner,
        cost,
        exile_on_leaving_stack,
        escaped,
        cast_from_command_zone,
        modal_effect,
        target_requirement,
        pending_mode_choice,
    })))
}

pub(super) fn apply_cast_spell(state: &mut GameState, plan: CastPlan) -> Result<CommandOutcome, Internal> {
    let CastPlan {
        actor,
        card,
        modes,
        targets,
        x_value,
        pay_buyback,
        from_zone,
        from_owner,
        cost,
        exile_on_leaving_stack,
        escaped,
        cast_from_command_zone,
        modal_effect,
        target_requirement,
        pending_mode_choice,
    } = plan;
    // CR 601.3/702.85a: the one-shot grants are spent by this cast, or by none.
    state.pending_timing_bypass.take();
    state.pending_free_cast.take();
    let object = state.object(card).ok_or_else(|| Internal::new("cast: the validated card is gone"))?;
    // CR 603.10
    // Snapshot before the move, for last_known_info.
    let last_known_info = object.clone();

    // CR 400.7/601.2a
    // Casting moves the card to the stack, a zone change that mints a new ObjectId
    // (see ObjectLineage), as PlayLand does.
    let new_id = state.allocate_object_id();
    let mut new_object = last_known_info.clone();
    new_object.object_id = new_id;
    new_object.lineage = ObjectLineage { previous: Some(card) };
    new_object.zone = ZoneKind::Stack;
    new_object.timestamp = state.allocate_sequence_no();

    state
        .zone_mut(from_zone, from_owner)
        .ok_or_else(|| Internal::new("cast: the validated source zone is gone"))?
        .remove(card);
    state.objects.remove(&card);
    state.objects.insert(new_id, new_object);
    remap_abilities(state, card, new_id);

    // CR 903.8
    // This cast counts toward the next commander tax once the zone move above
    // commits.
    // TODO: a cast abandoned through Engine::unwind_payment (CR 601.6) keeps this increment, and unwind_payment returns any unwound spell to hand whatever its origin zone, so an unwound commander cast already loses its command-zone recastability
    if cast_from_command_zone {
        if let Some(p) = state.player_mut(actor) {
            *p.commander_casts.entry(last_known_info.definition.name.clone()).or_insert(0) += 1;
        }
    }

    // CR 608.2m
    // The card's Rust-native fast-path effect (Shock's damage, say), if it has one;
    // a permanent spell never does (see CardDefinition::effect). A modal card uses
    // the chosen mode's effect, resolved above, instead of its singular field.
    let effect = if last_known_info.definition.modes.is_empty() {
        last_known_info.definition.effect.clone()
    } else {
        modal_effect
    };

    // A script custom-function operation (`s.operation()` returning a Lua function
    // and not a `Game.Ops.*` tag) arrives packed as `AbilityEffect::Script(handle)`
    // on `CardDefinition.effect` (see that variant for why). It is unpacked into
    // StackObject's `operation`/`effect` pair so `ResolveStackTop` treats it like a
    // triggered or activated ability's script fallback: `None` on `effect` makes it
    // run `operation`. Every other shape keeps the `ScriptHandle(0)` placeholder,
    // and only the Rust-native `effect` can resolve those.
    let (operation, effect) = match effect {
        Some(AbilityEffect::Script(handle)) => (handle, None),
        other => (ScriptHandle(0), other),
    };

    state.stack.push(StackObject::new(
        new_id,
        StackEntryKind::Spell,
        new_id,
        // CR 800.4a
        // A spell's owner is its card's owner, not necessarily the caster.
        last_known_info.owner,
        actor,
        modes,
        targets,
        target_requirement,
        x_value,
        exile_on_leaving_stack,
        // CR 702.27a
        // Baked in so the resolution-side destination check
        // (engine/resolve/permanent_entry.rs) need not re-read CardDefinition.
        pay_buyback,
        // CR 702.138b
        // Resolved above from the chosen alternative cost's `escape` flag.
        escaped,
        operation,
        effect,
        // CR 603.4
        // An intervening-if clause belongs to triggered abilities only.
        None,
        pending_mode_choice,
        // A spell never carries a departed-source LKI; only a triggered ability pushed
        // off a Dies-style departed source does.
        None,
    ));

    Ok(CommandOutcome {
        events: vec![
            GameEventKind::ZoneChange {
                object: new_id,
                from: from_zone,
                to: ZoneKind::Stack,
                last_known_info: Some(last_known_info),
            },
            GameEventKind::StackObjectPushed { stack_id: new_id },
        ],
        // CR 601.2i
        // The caster held priority before casting (checked above) and keeps it.
        broke_pass_chain: true,
        priority_handoff: PriorityHandoff::Unchanged,
        // CR 601.2f-h
        // Being on the stack does not pay the cost. The engine pauses on
        // AwaitingPayment until Engine::submit_payment answers, even for a {0} cost
        // (CR 118.5a).
        requires_payment: Some(PendingPaymentRequest {
            stack_id: new_id,
            payer: actor,
            cost,
            // Placeholder: a spell's CostSpec never has AdditionalCost::TapSelf, so this is
            // never read.
            source: new_id,
            on_paid: PaymentResolution::StackObject,
        }),
        battlefield_entries: Vec::new(),
    })
}
