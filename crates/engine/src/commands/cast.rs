//! `execute_command`'s `CastSpell` arm body, split out of the original
//! monolithic `commands.rs` - the single largest arm (mode/target/X-value/
//! alternative-cost handling, stack-object construction).

use std::collections::HashSet;

use crate::script::ScriptRuntime;
use crate::types::ability::AbilityEffect;
use crate::types::card::{CardTypeSet, KeywordSet, ModeChoice};
use crate::types::cost::{self, AdditionalCost, AlternativeCost, CostSpec};
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId, ScriptHandle, Target};
use crate::types::stack::{PendingModeChoice, StackEntryKind, StackObject};
use crate::types::target::{TargetFilter, TargetRequirement, TargetSpec};
use crate::types::zone::ZoneKind;

use super::attach::{non_eliminated_players, remap_abilities};
use super::legality::{is_sorcery_speed, is_modal_resolvable, validate_targets};
use super::{CommandError, CommandOutcome, PaymentResolution, PendingPaymentRequest, PriorityHandoff};

pub(super) fn execute_cast_spell(
    state: &mut GameState,
    script_runtime: &ScriptRuntime,
    actor: PlayerId,
    card: ObjectId,
    modes: Vec<u32>,
    targets: Vec<Target>,
    x_value: Option<u32>,
    alternative_cost: Option<usize>,
) -> Result<CommandOutcome, CommandError> {
    let object = state.object(card).ok_or(CommandError::InvalidTarget)?;
    if object.definition.card_types.contains(CardTypeSet::LAND) {
        return Err(CommandError::IllegalAction(
            "lands aren't cast as spells - use PlayLand".to_string(),
        ));
    }
    // CR 601.2b: an announced alternative cost (flashback, etc)
    // replaces the mana cost entirely and, for the ones this slice
    // supports, is what grants permission to cast from a zone other
    // than the hand in the first place - so the zone permission and
    // the alternative-cost choice are resolved together, not as two
    // independent checks.
    let alternative: Option<&AlternativeCost> = match alternative_cost {
        Some(idx) => Some(
            object
                .definition
                .alternative_costs
                .get(idx)
                .ok_or(CommandError::IllegalAction(
                    "no such alternative cost on this card".to_string(),
                ))?,
        ),
        None => None,
    };
    // CR 903.8: a commander may be cast from the command zone by its
    // owner (not just controller - matching the rule text) at the
    // normal mana cost, no alternative cost announced. Checked
    // before the general zone-permission rule below rather than
    // folded into `required_zone`, since this path cares about
    // ownership, not control, and only applies with no
    // `alternative_cost` at all.
    let cast_from_command_zone = alternative.is_none()
        && object.is_commander
        && object.zone == ZoneKind::Command
        && object.owner == actor;
    // TODO: doesn't require casting from the graveyard specifically
    // for flashback's own zone permission to have been granted at
    // all (e.g. nothing here checks the card actually has a
    // Flashback keyword vs. an ad-hoc alternative_costs entry) -
    // simplest correct rule for now: the chosen alternative's
    // from_zone must match where the card actually is, full stop.
    // Without an alternative_cost declared, the card must be in
    // hand, same as before this slice.
    let required_zone = match alternative {
        Some(AlternativeCost::Replacement { from_zone, .. }) => *from_zone,
        None => ZoneKind::Hand,
    };
    if !cast_from_command_zone
        && (object.zone != required_zone || object.controller != actor)
    {
        return Err(CommandError::IllegalAction(
            "card is not in the zone required to cast it this way".to_string(),
        ));
    }
    let from_zone = object.zone;
    // CR 117.1a/602/702.8b: instants, and anything with flash, may
    // be cast any time the actor holds priority; everything else
    // needs the full sorcery-speed window (is_sorcery_speed already
    // covers "holds priority" as one of its four conditions, so
    // this branch only adds the priority check on its own rather
    // than duplicating it).
    // TODO: effects granting flash (e.g. "you may cast this as
    // though it had flash") aren't represented on
    // continuous_effects yet - same class of gap is_sorcery_speed
    // flags for PlayLand. Only the card's own printed Flash keyword
    // is consulted here.
    let castable_at_instant_speed = object.definition.card_types.contains(CardTypeSet::INSTANT)
        || object.definition.keywords.contains(KeywordSet::FLASH);
    if castable_at_instant_speed {
        if state.priority_player != actor {
            return Err(CommandError::IllegalAction(
                "actor does not hold priority".to_string(),
            ));
        }
    } else if !is_sorcery_speed(state, actor) {
        return Err(CommandError::IllegalAction(
            "this spell can only be cast at sorcery speed: actor's turn, a main \
             phase, actor holding priority, and an empty stack"
                .to_string(),
        ));
    }

    // CR 700.2a/d/h: a modal card requires the caster to announce
    // their mode choice(s) here, before targets are chosen (601.2b
    // comes before 601.2c). A non-modal card (definition.modes
    // empty) is untouched: modes must be empty, and the card's own
    // singular target_requirement/effect are used exactly as before
    // this field existed. `mode_choice` names how many of `modes`
    // must be chosen (`ModeChoice::Exactly(1)` is the pre-existing
    // "Choose one —" behavior); its `*Repeatable` variants are CR
    // 700.2d's reminder-text-gated "you may choose the same mode
    // more than once" exception - a repeated index is rejected
    // unless `repeatable` is set. `mode_additional_costs` collects
    // every chosen mode's own `SpellMode::additional_cost` (CR
    // 700.2h) - appended to the spell's overall `CostSpec.additional`
    // once `cost` itself is computed below, so it's paid together
    // with the mana cost through the same already-generic
    // `PaymentPlan` machinery an activated ability's own additional
    // costs already use. A repeated mode with its own additional
    // cost pays that cost once per instance (CR 700.2d "treated as
    // if that mode appeared that many times in sequence" + 700.2h
    // "if more than one such mode is chosen, all additional costs
    // must be paid") - falls out for free below since
    // `mode_additional_costs` is collected per chosen instance, not
    // deduped by mode index.
    let (target_requirement, modal_effect, mode_additional_costs, pending_mode_choice): (
        Option<TargetSpec>,
        Option<AbilityEffect>,
        Vec<AdditionalCost>,
        Option<PendingModeChoice>,
    ) = if object.definition.modes.is_empty() {
            if !modes.is_empty() {
                return Err(CommandError::IllegalAction(
                    "this card isn't modal - no modes to choose".to_string(),
                ));
            }
            (object.definition.target_requirement.clone(), None, Vec::new(), None)
        } else if let ModeChoice::ExactlyChosenByOpponent(_) | ModeChoice::UpToChosenByOpponent(_) =
            object.definition.mode_choice
        {
            // CR 700.2e: the other player chooses, not the caster -
            // see `ModeChoice`'s own doc comment for the full
            // narrowing this variant carries. The choice itself is
            // deferred to a resolution-time pause rather than
            // supplied here.
            if !modes.is_empty() {
                return Err(CommandError::IllegalAction(
                    "this card's mode is chosen by another player, not the caster - submit \
                     no modes when casting it"
                        .to_string(),
                ));
            }
            if !targets.is_empty() {
                return Err(CommandError::IllegalAction(
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
                return Err(CommandError::IllegalAction(
                    "a chosen-by-opponent modal card's modes must have no target \
                     requirement and no additional cost"
                        .to_string(),
                ));
            }
            let others: Vec<PlayerId> =
                non_eliminated_players(state).into_iter().filter(|&p| p != actor).collect();
            let [chooser] = others.as_slice() else {
                return Err(CommandError::IllegalAction(
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
        } else {
            let (min, max, repeatable) = match object.definition.mode_choice {
                ModeChoice::Exactly(n) => (n, n, false),
                // CR 700.2a: "choose a number of those options" -
                // never zero, even for an UpTo spell.
                ModeChoice::UpTo(n) => (1, n, false),
                ModeChoice::ExactlyRepeatable(n) => (n, n, true),
                ModeChoice::UpToRepeatable(n) => (1, n, true),
                ModeChoice::ExactlyChosenByOpponent(_) | ModeChoice::UpToChosenByOpponent(_) => {
                    unreachable!("handled above")
                }
            };
            let chosen_count = modes.len() as u32;
            if chosen_count < min || chosen_count > max {
                return Err(CommandError::IllegalAction(format!(
                    "this modal spell requires choosing between {min} and {max} mode(s), got {chosen_count}"
                )));
            }
            if !repeatable {
                let mut seen_modes = HashSet::new();
                for &m in &modes {
                    if !seen_modes.insert(m) {
                        return Err(CommandError::IllegalAction(
                            "the same mode can't be chosen more than once".to_string(),
                        ));
                    }
                }
            }
            // CR 700.2's own convention (per Wizards' rulings on
            // e.g. Cryptic Command): chosen modes' effects happen in
            // the order printed on the card, not the order the
            // player listed them in `modes`.
            let mut chosen_indices = modes.clone();
            chosen_indices.sort_unstable();
            let mut chosen_modes = Vec::with_capacity(chosen_indices.len());
            for &idx in &chosen_indices {
                let mode = object.definition.modes.get(idx as usize).ok_or_else(|| {
                    CommandError::IllegalAction("no such mode on this card".to_string())
                })?;
                chosen_modes.push(mode);
            }
            // CR 700.2h: every chosen mode's own additional cost (if
            // any), collected regardless of whether one or several
            // modes were chosen - "if more than one such mode is
            // chosen, all additional costs must be paid."
            let mode_additional_costs: Vec<AdditionalCost> =
                chosen_modes.iter().filter_map(|m| m.additional_cost.clone()).collect();
            if let [only] = chosen_modes.as_slice() {
                // Single mode chosen - unchanged pre-multi-mode
                // shape, no TargetSpec::Multiple/AbilityEffect::Modal
                // wrapping.
                (only.target_requirement.clone(), only.effect.clone(), mode_additional_costs, None)
            } else {
                // 2+ modes chosen: combine into one TargetSpec::
                // Multiple (one TargetRequirement per chosen mode, a
                // synthetic zero-count placeholder for a mode with
                // no target_requirement of its own, CR 700.2c) and
                // one AbilityEffect::Modal (one slot per chosen
                // mode, same order) - ResolveStackTop zips the two
                // back together at resolution time via
                // split_targets_by_requirement, the same helper
                // multi-target-word spells already use. A mode
                // whose own target_requirement is itself
                // TargetSpec::Multiple (a mode that independently
                // needs several differently-filtered targets) can't
                // be combined this way - rejected outright rather
                // than silently flattened, a named narrow gap.
                let mut reqs = Vec::with_capacity(chosen_modes.len());
                for mode in &chosen_modes {
                    match &mode.target_requirement {
                        None => reqs.push(TargetRequirement {
                            filter: TargetFilter::AnyTarget,
                            count: 0,
                            up_to: false,
                            at_least: false,
                            exclude_source: false,
                        }),
                        Some(TargetSpec::Primitive(req)) => reqs.push(req.clone()),
                        Some(TargetSpec::Multiple(_)) => {
                            return Err(CommandError::IllegalAction(
                                "a mode with its own multi-target requirement can't be \
                                 combined with other chosen modes"
                                    .to_string(),
                            ));
                        }
                    }
                }
                // Always TargetSpec::Multiple, even if every chosen
                // mode's requirement is the synthetic zero-count
                // placeholder - ResolveStackTop's Modal handling
                // always re-derives per-mode target chunks from
                // this shape, so it must never collapse to `None`
                // the way a single chosen mode's bare `None` can.
                for mode in &chosen_modes {
                    if let Some(effect) = &mode.effect {
                        if !is_modal_resolvable(effect) {
                            return Err(CommandError::IllegalAction(
                                "this mode's effect can't be combined with other chosen \
                                 modes yet - only GainLife/Damage/Counters/Pump/\
                                 GainControl/Shuffle are supported inside a multi-mode cast"
                                    .to_string(),
                            ));
                        }
                    }
                }
                let target_requirement = Some(TargetSpec::Multiple(reqs));
                let effects: Vec<Option<AbilityEffect>> =
                    chosen_modes.iter().map(|m| m.effect.clone()).collect();
                (target_requirement, Some(AbilityEffect::Modal(effects)), mode_additional_costs, None)
            }
        };

    // CR 601.2c/601.2e: target count/filter/dedup validated below via
    // validate_targets, rejecting the whole cast before anything is
    // mutated if illegal. TODO(CR 601.2d): no division/distribution
    // step for effects that split among targets. TODO: no splice
    // (CR 601.2b/702.47) - a player revealing other cards in hand to
    // splice their text onto this spell isn't modeled at all;
    // PlayerAction has nowhere to carry "which cards I'm splicing,"
    // and StackObject has nowhere to record it either.
    validate_targets(
        state,
        script_runtime,
        target_requirement.as_ref(),
        &targets,
        actor,
        card,
    )?;

    // CR 601.2f: total cost = the card's printed mana_cost, unless
    // an alternative cost was announced (CR 601.2b), in which case
    // it replaces the mana cost entirely - never mixed with it.
    // `cost.additional` is populated below only from chosen modes'
    // own CR 700.2h costs (`mode_additional_costs`, computed above)
    // - a non-modal card's own top-level additional cost has no
    // representation on `CardDefinition` at all yet (only
    // `alternative_costs` exists), a separate, still-open gap.
    //
    // CR 107.3a/601.2b: an {X} cost and an announced x_value must
    // agree - both present or both absent - and once agreed, X is
    // substituted for a generic amount before PaymentPlan::validate
    // ever sees the cost (see ManaCost::resolve_x's own doc comment
    // for why that's enough on its own, no changes needed in
    // cost.rs). TODO: X on an activated ability's own cost isn't
    // supported at all yet - ActivateAbility has no x_value field
    // to announce one with, a real struct gap, not just this check
    // missing. TODO: X combined with an alternative cost (e.g.
    // flashback on an X spell) isn't handled - x_value is simply
    // unchecked in that branch below.
    let (mut cost, exile_on_leaving_stack) = match alternative {
        Some(AlternativeCost::Replacement { cost, exile_on_leaving_stack, .. }) => {
            (cost.clone(), *exile_on_leaving_stack)
        }
        None => {
            let has_x = object.definition.mana_cost.has_x();
            let resolved_mana_cost = match (has_x, x_value) {
                (true, Some(x)) => object.definition.mana_cost.resolve_x(x),
                (false, None) => object.definition.mana_cost.clone(),
                (true, None) => {
                    return Err(CommandError::IllegalAction(
                        "this spell's cost includes {X} - a value must be announced"
                            .to_string(),
                    ));
                }
                (false, Some(_)) => {
                    return Err(CommandError::IllegalAction(
                        "an X value was announced but this spell's cost has no {X}"
                            .to_string(),
                    ));
                }
            };
            // CR 601.2f/118.9: static cost-modification effects
            // (Goblin Warchief, Thalia, ...) apply after X is
            // resolved, before the cost is locked in for payment -
            // see CostModifier's own doc comment (cost.rs) for why
            // this only touches the plain mana-cost path (never an
            // alternative cost, this slice).
            let resolved_mana_cost =
                cost::apply_cost_modifiers(&resolved_mana_cost, &state.cost_modifiers, object);
            // CR 903.8/702.124d: commander tax - {2} generic per
            // prior time this player has cast *this* commander (by
            // name, not ObjectId - see PlayerState::commander_casts'
            // own doc comment for why) from the command zone this
            // game. Only the 903.8 path taxes; a normal hand cast or
            // an announced alternative cost never does (`alternative`
            // being `None` is this whole match arm's own condition,
            // and `cast_from_command_zone` narrows it further).
            let resolved_mana_cost = if cast_from_command_zone {
                let prior_casts = state
                    .player(actor)
                    .and_then(|p| p.commander_casts.get(&object.definition.name))
                    .copied()
                    .unwrap_or(0);
                // A first cast (prior_casts == 0, delta == 0) must
                // be a true no-op - apply_cost_delta(0) isn't one,
                // it always drops an explicit Generic(0) symbol
                // (see cost::apply_cost_modifiers's own doc comment
                // for the same bug shape, caught by phase5/cost-
                // modifier-order). An {X} commander with X == 0,
                // cast from the command zone for the first time,
                // would otherwise lose that explicit zero marker.
                if prior_casts == 0 {
                    resolved_mana_cost
                } else {
                    resolved_mana_cost.apply_cost_delta(2 * prior_casts as i32)
                }
            } else {
                resolved_mana_cost
            };
            (CostSpec::mana_only(resolved_mana_cost), false)
        }
    };
    // CR 700.2h: every chosen mode's own additional cost (collected
    // above, regardless of whether an alternative or normal mana
    // cost is being paid - CR 700.2h doesn't exclude either) is
    // paid together with the rest of this cost, through the same
    // already-generic PaymentPlan machinery.
    cost.additional.extend(mode_additional_costs);

    // Snapshot before the move - CR 603.10's last_known_info (see
    // GameEventKind::ZoneChange's doc comment).
    let last_known_info = object.clone();

    // CR 400.7/601.2a: casting moves the card to the stack, which is
    // a zone change that mints a new ObjectId (same convention
    // PlayLand uses for hand->battlefield) - see ObjectLineage.
    let new_id = state.allocate_object_id();
    let mut new_object = last_known_info.clone();
    new_object.object_id = new_id;
    new_object.lineage = ObjectLineage { previous: Some(card) };
    new_object.zone = ZoneKind::Stack;
    new_object.timestamp = state.allocate_sequence_no();

    state
        .zone_mut(from_zone, Some(actor))
        .ok_or_else(|| CommandError::IllegalAction("actor has no such zone".to_string()))?
        .remove(card);
    state.objects.remove(&card);
    state.objects.insert(new_id, new_object);
    remap_abilities(state, card, new_id);

    // CR 903.8: this cast counts toward the *next* commander tax
    // once it actually commits (the zone move above, not merely
    // attempted) - matches where the tax was computed above (this
    // player's own count, at the point of casting). TODO: if this
    // cast is later abandoned via `Engine::unwind_payment` (CR
    // 601.6), this increment isn't rolled back - a real gap, same
    // family as `unwind_payment`'s own pre-existing bug of returning
    // any unwound spell to hand regardless of its real origin zone
    // (so an unwound commander cast already loses its command-zone
    // recastability independent of this). Flagged, not fixed here.
    if cast_from_command_zone {
        if let Some(p) = state.player_mut(actor) {
            *p.commander_casts.entry(last_known_info.definition.name.clone()).or_insert(0) += 1;
        }
    }

    // CR 608.2m: a real, Rust-native fast-path effect (e.g. "Shock
    // deals 2 damage to any target") if the card's own
    // CardDefinition carries one - see CardDefinition::effect's own
    // doc comment for why a permanent spell never has one. For a
    // modal card, the chosen mode's own effect (resolved above,
    // before targets) is used instead of the card's singular field.
    let effect = if last_known_info.definition.modes.is_empty() {
        last_known_info.definition.effect.clone()
    } else {
        modal_effect
    };

    // A script-declared custom-function operation (`s.operation()`
    // returning a Lua function rather than a `Game.Ops.*` tag) is
    // packed onto `CardDefinition.effect` as `AbilityEffect::
    // Script(handle)` - see that variant's own doc comment for why
    // `CardDefinition` has no separate `operation: ScriptHandle`
    // field to carry it natively. Unpacked here into StackObject's
    // real two-field shape (`operation`/`effect`) so
    // `ResolveStackTop` never has to know the difference from a
    // triggered/activated ability's own script fallback - `None` on
    // `effect` is what makes it invoke `operation` for real. Every
    // other shape (a `Game.Ops.*` primitive, or no script-declared
    // operation at all) keeps the pre-existing `ScriptHandle(0)`
    // placeholder - "nothing real bound to it," only `effect`'s
    // Rust-native fast path can resolve those.
    let (operation, effect) = match effect {
        Some(AbilityEffect::Script(handle)) => (handle, None),
        other => (ScriptHandle(0), other),
    };

    state.stack.push(StackObject::new(
        new_id,
        StackEntryKind::Spell,
        new_id,
        // CR 800.4a: a spell's owner is its card's own owner, not
        // necessarily whoever is casting it.
        last_known_info.owner,
        actor,
        modes,
        targets,
        target_requirement,
        x_value,
        exile_on_leaving_stack,
        operation,
        effect,
        // CR 603.4's "if" clause is triggered-ability-only text - a
        // spell never carries one.
        None,
        pending_mode_choice,
    ));

    return Ok(CommandOutcome {
        events: vec![
            GameEventKind::ZoneChange {
                object: new_id,
                from: from_zone,
                to: ZoneKind::Stack,
                last_known_info: Some(last_known_info),
            },
            GameEventKind::StackObjectPushed { stack_id: new_id },
        ],
        // CR 601.2i: the controller already held priority before
        // casting (checked above), so they keep it - it doesn't move.
        broke_pass_chain: true,
        priority_handoff: PriorityHandoff::Unchanged,
        // CR 601.2f-h: cost isn't paid yet just by being on the
        // stack - engine.rs pauses on AwaitingPayment until
        // Engine::submit_payment answers, even for a {0} cost (CR
        // 118.5a still requires an explicit empty acknowledgment).
        requires_payment: Some(PendingPaymentRequest {
            stack_id: new_id,
            payer: actor,
            cost,
            // Placeholder - a spell's CostSpec never contains
            // AdditionalCost::TapSelf, so this is never consulted.
            source: new_id,
            on_paid: PaymentResolution::StackObject,
        }),
        battlefield_entries: Vec::new(),
    });
}
