use std::collections::{HashMap, HashSet};

use crate::types::ability::AbilityCost;
use crate::types::ability::AbilityEffect;
use crate::types::ability::AbilityKind;
use crate::types::ability::RegisteredAbility;
use crate::types::card::{CardTypeSet, KeywordSet};
use crate::types::combat::CombatState;
use crate::types::command::{EngineCommand, PlayerActionKind};
use crate::types::cost::CostSpec;
use crate::types::counter::CounterKind;
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId, ScriptHandle, Target};
use crate::types::object::GameObject;
use crate::types::stack::{StackEntryKind, StackObject};
use crate::types::turn::{Phase, Step};
use crate::types::zone::ZoneKind;

/// Carrying out an already-validated `EngineCommand` failed partway —
/// distinct from `ActionError` (engine.rs), which rejects an action before
/// it's even accepted. Reaching a `CommandError` mid-execution should be
/// rare/exceptional once cost/target validation happens up front; this
/// exists mainly for cases validation can't fully rule out ahead of time.
#[derive(Debug)]
pub enum CommandError {
    CostNotPaid,
    InvalidTarget,
    IllegalAction(String),
}

/// Whether/how priority should move as a result of one `EngineCommand`.
/// Returned explicitly by `execute_command` rather than the command
/// mutating `state.priority_player` itself, so `engine.rs` is the one
/// place that actually writes that field — same reasoning as
/// `CommandOutcome::broke_pass_chain` not mutating `consecutive_passes`
/// directly.
pub enum PriorityHandoff {
    /// Most variants: priority stays with whoever currently has it.
    Unchanged,
    /// `PassPriority` (CR 117.3c): priority moves to the next player in
    /// `state.turn_order`. Carries the resolved target player rather than
    /// engine.rs re-deriving "next" from `turn_order`, so `execute_command`
    /// (which already has `state` to hand) stays the single place that
    /// knows the rotation rule.
    To(PlayerId),
}

/// What carrying out one `EngineCommand` produced, for `engine.rs` to act
/// on. Returned explicitly rather than left for `engine.rs` to infer from
/// `state` afterward — see `broke_pass_chain`/`priority_handoff` below.
pub struct CommandOutcome {
    /// Proposed events, not yet final — `engine.rs` runs these through
    /// `ReplacementCheck` -> `CommitEvents` to allocate ids and append to
    /// `state.ledger`. See `execute_command`'s doc comment for why this
    /// function doesn't write the ledger itself.
    pub events: Vec<GameEventKind>,
    /// `false` only for `PassPriority`; `true` for every other variant.
    /// CR 117.3c: taking any action other than passing breaks a
    /// consecutive-pass count. `engine.rs` owns `state.consecutive_passes`
    /// entirely (increment on `false`, reset on `true`) and decides "did
    /// everyone just pass" from that — this flag is the one thing it needs
    /// from execution to do so, rather than relying on every match arm
    /// here remembering to mutate `consecutive_passes` itself correctly.
    pub broke_pass_chain: bool,
    /// See `PriorityHandoff`. `engine.rs` applies this to
    /// `state.priority_player` itself.
    pub priority_handoff: PriorityHandoff,
    /// CR 601.2a puts a spell on the stack *before* its cost is determined/
    /// paid (601.2f-h) — set by `CastSpell` once it's pushed `stack_id` onto
    /// `state.stack`, so `engine.rs` can pause the rest of this pipeline run
    /// on `ProcessState::AwaitingPayment` until `Engine::submit_payment`
    /// answers, rather than this function deducting mana synchronously
    /// (which would assume the cost is always payable/free, and skip the
    /// CR 118.5a "must explicitly acknowledge a {0} cost" requirement).
    /// `None` for every other variant — nothing else here puts something on
    /// the stack pending payment.
    pub requires_payment: Option<PendingPaymentRequest>,
    /// Object ids that just became new battlefield permanents this command
    /// (CR 400.7 mints a fresh id per zone change) — `engine.rs` runs each
    /// one's card script (`ScriptRuntime::run_card_script`) right after this
    /// outcome is applied. Empty for every variant except `PlayLand`, since
    /// `execute_command` (`state: &mut GameState` only) can't reach
    /// `ScriptRuntime` itself — same "one place decides, one place performs
    /// the side effect" split `priority_handoff`/`broke_pass_chain` already
    /// use. `ResolveStackTop`'s permanent-entry branch lives directly in
    /// `engine.rs`, so it calls `run_card_script` itself without needing
    /// this field.
    pub battlefield_entries: Vec<ObjectId>,
}

/// See `CommandOutcome::requires_payment`'s doc comment for why this exists
/// as its own pause rather than a synchronous deduction.
pub struct PendingPaymentRequest {
    pub stack_id: ObjectId,
    pub payer: PlayerId,
    pub cost: CostSpec,
}

/// CR 117.1a's "sorcery speed" window — `player` may take an action that
/// requires it (playing a land, casting a sorcery, most activated
/// abilities) only when all of: they hold priority, it's their own turn
/// (CR 305.3), it's a main phase of that turn (CR 305.1), and the stack is
/// empty (CR 307.1/305.1). Pulled out as a standalone predicate rather than
/// inlined into `PlayLand` alone, since `CastSpell`'s sorcery-timing branch
/// and (most) `ActivateAbility` checks will need the exact same condition
/// once they're real.
///
/// TODO: doesn't account for effects that grant timing permission beyond
/// the base rule (e.g. "you may play an additional land," "you may cast
/// this spell as though it had flash," "you may play lands as though it
/// were a main phase") - those are all CR 113.6-style rules-text
/// permissions layered on top of this base check, and nothing here
/// consults state.continuous_effects/abilities for them yet. Revisit once
/// such an effect needs representing.
fn is_sorcery_speed(state: &GameState, player: PlayerId) -> bool {
    state.priority_player == player
        && state.active_player == player
        && state.phase == Phase::Main
        && state.stack.is_empty()
}

/// CR 302.6/602.5a's summoning sickness check: `object` is sick unless it's
/// been under its current controller's control since their most recent
/// turn began. Derived from `GameObject.controlled_since_turn` against
/// `GameState.last_active_turn_number` rather than a stored bool - see
/// those fields' doc comments for why. CR 702.10b/c: Haste bypasses this
/// check entirely.
pub(crate) fn has_summoning_sickness(state: &GameState, object: &GameObject) -> bool {
    if object.definition.keywords.contains(KeywordSet::HASTE) {
        return false;
    }
    match state.last_active_turn_number.get(&object.controller) {
        Some(&since) => object.controlled_since_turn >= since,
        // The controller has no recorded turn start at all - shouldn't
        // happen past the very first turn (PreGame seeds this), but "no
        // record" isn't evidence of long-standing control either, so treat
        // it as sick rather than silently allowing it.
        None => true,
    }
}

/// CR 613 layer 6: `id`'s live keywords (printed plus anything granted by an
/// applicable `ContinuousEffect`), via `layers::effective_characteristics`.
/// Tolerates a missing `id` (returns `KeywordSet::empty()`) rather than
/// panicking the way `effective_characteristics` itself does — several call
/// sites below already separately raise `CommandError::InvalidTarget` for a
/// missing id and just want "no keywords" as the fallback otherwise (mirrors
/// the pre-existing `Option<&GameObject>` + `is_some_and` shape this
/// replaces).
fn effective_keywords(state: &GameState, id: ObjectId) -> KeywordSet {
    if state.object(id).is_some() {
        crate::layers::effective_characteristics(state, id).keywords
    } else {
        KeywordSet::empty()
    }
}

/// CR 613 layer 7c: `id`'s live power (already netted against counters and
/// any applicable `ModifyPT`/`SetPT` continuous effect), via
/// `layers::effective_characteristics`. `None` for a missing `id` or an
/// unresolved characteristic-defining base value — same "skip rather than
/// guess" contract `effective_characteristics` itself already documents,
/// just reachable without risking its panic-on-missing-id path.
fn effective_power(state: &GameState, id: ObjectId) -> Option<i32> {
    if state.object(id).is_some() {
        crate::layers::effective_characteristics(state, id).power
    } else {
        None
    }
}

/// Looks up the `ability_index`th ability registered to `source` — any
/// kind, in registration order (the order entries were pushed onto
/// `state.abilities`). Shared by `ActivateAbility` (below) and
/// `triggers::collect_triggers`'s `PendingTrigger::ability_index`, so both
/// call sites agree on what "index" means rather than each re-deriving it.
pub(crate) fn find_ability(
    state: &GameState,
    source: ObjectId,
    ability_index: u32,
) -> Option<&RegisteredAbility> {
    state.abilities.iter().filter(|a| a.source == source).nth(ability_index as usize)
}

/// CR 400.7: abilities belong to their object and travel with it across zone
/// changes. Every zone change mints a new `ObjectId`; call this to keep
/// `state.abilities` in sync so ability lookups by source id don't go stale.
pub(crate) fn remap_abilities(state: &mut GameState, old_id: ObjectId, new_id: ObjectId) {
    for ability in state.abilities.iter_mut() {
        if ability.source == old_id {
            ability.source = new_id;
        }
    }
}

/// Removes `old_id` from `state.objects` and purges any `RegisteredAbility`
/// still pointing at it (`state.abilities.retain`). Use at a zone-change
/// site that does **not** also call `remap_abilities` for `old_id` — calling
/// both would be wrong (retire would delete what remap just carried
/// forward); calling neither leaves a dead `RegisteredAbility` sitting in
/// `state.abilities` forever (harmless per `collect_triggers`'s existing
/// "skip if source doesn't resolve" check, but pure pileup). Today that's
/// exactly the sites with no in-play "same card, new id" continuation to
/// carry forward: discard (hand→graveyard), `draw_step` (library→hand), and
/// SBA-driven death (battlefield→graveyard) — the other zone-change sites
/// (`PlayLand`, `CastSpell`, mulligan bottom/redraw, `ResolveStackTop`'s
/// permanent branch) already call `remap_abilities` instead, which is the
/// correct behavior there (e.g. a mana ability registered against a land
/// while still in hand must survive the hand→battlefield move).
pub(crate) fn retire_object(state: &mut GameState, old_id: ObjectId) {
    state.objects.remove(&old_id);
    state.abilities.retain(|a| a.source != old_id);
}

/// Executes one already-accepted `EngineCommand` against `state`, mutating
/// non-ledger state directly (zones, etc.) and returning a
/// `CommandOutcome`. Per the rationale in `command.rs`: this does not
/// decompose `command` into further sub-commands — the sub-steps of
/// carrying it out are events, not commands.
///
/// Does NOT append to `state.ledger` and does NOT allocate `EventId`s/
/// `SequenceNo`s — those, plus replacement/prevention checks (CR 614-616),
/// are engine.rs's job (`ReplacementCheck` -> `CommitEvents`) so there is
/// exactly one place that writes the ledger. Does NOT mutate
/// `state.consecutive_passes` or `state.priority_player` either — see
/// `CommandOutcome::broke_pass_chain`/`priority_handoff`.
pub fn execute_command(
    state: &mut GameState,
    command: EngineCommand,
) -> Result<CommandOutcome, CommandError> {
    match command.action {
        PlayerActionKind::PassPriority => {
            // CR 117.3c: priority moves to the next player in turn_order
            // after whoever just passed. Read-only against turn_order -
            // does not touch state.priority_player/consecutive_passes,
            // both of which engine.rs owns (see PriorityHandoff/
            // broke_pass_chain doc comments above).
            let current_idx = state
                .turn_order
                .iter()
                .position(|p| *p == command.actor)
                .ok_or_else(|| {
                    CommandError::IllegalAction("actor not in turn_order".to_string())
                })?;
            let next_idx = (current_idx + 1) % state.turn_order.len();
            return Ok(CommandOutcome {
                events: vec![GameEventKind::PriorityPassed { player: command.actor }],
                broke_pass_chain: false,
                priority_handoff: PriorityHandoff::To(state.turn_order[next_idx]),
                requires_payment: None,
                battlefield_entries: Vec::new(),
            });
        }
        PlayerActionKind::MakeChoice { .. } => {
            // Real choice application is genuinely generic - "depends
            // entirely on what asked for the choice" (see the TODO block
            // below) - no fixed event shape exists yet, so this produces
            // none. Exists as a minimal non-PassPriority stub so
            // broke_pass_chain's reset wiring (CR 117.3c) has something
            // other than PassPriority to exercise in tests; not a real
            // implementation of MakeChoice itself.
            return Ok(CommandOutcome {
                events: Vec::new(),
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                requires_payment: None,
                battlefield_entries: Vec::new(),
            });
        }
        PlayerActionKind::PlayLand { card } => {
            // CR 305.1: lands aren't spells, so this goes straight to the
            // battlefield - no stack object. Playing a land is still
            // restricted to sorcery speed (CR 305.1/305.3, via
            // is_sorcery_speed) and to once per turn (CR 305.2).
            if !is_sorcery_speed(state, command.actor) {
                return Err(CommandError::IllegalAction(
                    "a land can only be played at sorcery speed: actor's turn, a main phase, \
                     actor holding priority, and an empty stack"
                        .to_string(),
                ));
            }
            // TODO(CR 305.2): hardcoded against the literal `1` - doesn't
            // read state.continuous_effects for anything that raises a
            // player's land-plays-per-turn limit (e.g. "you may play an
            // additional land each turn"). continuous_effects has no query
            // API yet (layers.rs is still mostly todo!()) - once it does,
            // this needs "limit = 1 + sum of applicable continuous
            // effects" instead of the bare literal below.
            let already_played =
                *state.lands_played_this_turn.get(&command.actor).unwrap_or(&0);
            if already_played >= 1 {
                return Err(CommandError::IllegalAction(
                    "actor has already played their land for this turn".to_string(),
                ));
            }

            let object = state.object(card).ok_or(CommandError::InvalidTarget)?;
            // TODO: reads CardDefinition.card_types directly rather than
            // layers::effective_characteristics (CR 613 layer 4, real as of
            // the layer-engine-types-colors slice) - a continuous effect
            // granting Land type to a card still in hand would not make it
            // playable here. Deliberately not wired this slice: this
            // object is in ZoneKind::Hand, and every real consumer in this
            // codebase so far (sba.rs's creature-type check, combat's
            // power/keyword checks) targets a battlefield permanent, not a
            // hand card - flagged rather than silently wired to an
            // untested shape.
            if !object.definition.card_types.contains(CardTypeSet::LAND) {
                return Err(CommandError::IllegalAction("card is not a Land".to_string()));
            }
            if object.zone != ZoneKind::Hand || object.controller != command.actor {
                return Err(CommandError::IllegalAction(
                    "card is not in actor's hand".to_string(),
                ));
            }
            // Snapshot before the move - CR 603.10's last_known_info (see
            // GameEventKind::ZoneChange's doc comment).
            let last_known_info = object.clone();

            // CR 400.7: this zone change creates a new object - mint a fresh
            // ObjectId rather than reusing `card`'s, and link back to it via
            // ObjectLineage. `card`'s entry in state.objects is removed
            // entirely once moved; anything that still needs to know what it
            // looked like in the hand must use `last_known_info` above, not
            // a live lookup by the old id.
            let new_id = state.allocate_object_id();
            let mut new_object = last_known_info.clone();
            new_object.object_id = new_id;
            new_object.lineage = ObjectLineage { previous: Some(card) };
            new_object.zone = ZoneKind::Battlefield;
            new_object.timestamp = state.allocate_sequence_no();

            state
                .zone_mut(ZoneKind::Hand, Some(command.actor))
                .ok_or_else(|| CommandError::IllegalAction("actor has no hand zone".to_string()))?
                .remove(card);
            state.objects.remove(&card);
            state
                .zone_mut(ZoneKind::Battlefield, None)
                .ok_or_else(|| CommandError::IllegalAction("no battlefield zone".to_string()))?
                .push_bottom(new_id);
            state.objects.insert(new_id, new_object);
            *state.lands_played_this_turn.entry(command.actor).or_insert(0) += 1;
            remap_abilities(state, card, new_id);

            return Ok(CommandOutcome {
                events: vec![GameEventKind::ZoneChange {
                    object: new_id,
                    from: ZoneKind::Hand,
                    to: ZoneKind::Battlefield,
                    last_known_info: Some(last_known_info),
                }],
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                requires_payment: None,
                battlefield_entries: vec![new_id],
            });
        }
        PlayerActionKind::CastSpell { card, modes, targets, x_value } => {
            let object = state.object(card).ok_or(CommandError::InvalidTarget)?;
            if object.definition.card_types.contains(CardTypeSet::LAND) {
                return Err(CommandError::IllegalAction(
                    "lands aren't cast as spells - use PlayLand".to_string(),
                ));
            }
            if object.zone != ZoneKind::Hand || object.controller != command.actor {
                return Err(CommandError::IllegalAction(
                    "card is not in actor's hand".to_string(),
                ));
            }
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
                if state.priority_player != command.actor {
                    return Err(CommandError::IllegalAction(
                        "actor does not hold priority".to_string(),
                    ));
                }
            } else if !is_sorcery_speed(state, command.actor) {
                return Err(CommandError::IllegalAction(
                    "this spell can only be cast at sorcery speed: actor's turn, a main \
                     phase, actor holding priority, and an empty stack"
                        .to_string(),
                ));
            }

            // TODO(CR 601.2b-h): modes/targets/x_value are accepted and
            // stored on the resulting StackObject below, but none of them
            // are validated for legality yet - no mode-legality check, no
            // target-legality check (range/restriction/count per CR
            // 601.2c), no division/distribution step (CR 601.2d), and no
            // legality recheck (CR 601.2e). Also no splice (CR 601.2b/
            // 702.47) - a player revealing other cards in hand to splice
            // their text onto this spell isn't modeled at all; PlayerAction
            // has nowhere to carry "which cards I'm splicing," and
            // StackObject has nowhere to record it either.

            // CR 601.2f: total cost = the card's printed mana_cost only -
            // no additional/alternative costs, no reductions yet (see
            // CostSpec::mana_only's own doc, and PaymentPlan::validate's
            // "out of scope" list). x_value isn't folded into the generic
            // component yet either - same gap CostSpec/ManaCost flag.
            let cost = CostSpec::mana_only(object.definition.mana_cost.clone());

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
                .zone_mut(ZoneKind::Hand, Some(command.actor))
                .ok_or_else(|| CommandError::IllegalAction("actor has no hand zone".to_string()))?
                .remove(card);
            state.objects.remove(&card);
            state.objects.insert(new_id, new_object);
            remap_abilities(state, card, new_id);

            // TODO(script bridge): `operation` should be the card's
            // registered cast/resolution callback (looked up via its
            // ScriptReference) - no such lookup exists yet, same gap as
            // every other ScriptHandle placeholder in this codebase.
            // ScriptHandle(0) here is a stand-in with nothing real bound to
            // it; resolving this StackObject can't actually run spell
            // effects until the script bridge fills this in for real.
            state.stack.push(StackObject::new(
                new_id,
                StackEntryKind::Spell,
                new_id,
                command.actor,
                modes,
                targets,
                x_value,
                ScriptHandle(0),
                None,
            ));

            return Ok(CommandOutcome {
                events: vec![
                    GameEventKind::ZoneChange {
                        object: new_id,
                        from: ZoneKind::Hand,
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
                    payer: command.actor,
                    cost,
                }),
                battlefield_entries: Vec::new(),
            });
        }
        PlayerActionKind::ActivateAbility { source, ability_index, targets } => {
            // CR 602.2a/117.3c: activating an ability follows the same
            // process as casting a spell - the activator must hold priority
            // to begin with.
            if state.priority_player != command.actor {
                return Err(CommandError::IllegalAction(
                    "actor does not hold priority".to_string(),
                ));
            }

            let source_object = state.object(source).ok_or(CommandError::InvalidTarget)?;
            let controller = source_object.controller;
            if controller != command.actor {
                return Err(CommandError::IllegalAction(
                    "actor does not control the ability's source".to_string(),
                ));
            }
            // CR 602.1/112.6: activated abilities on permanents are only
            // usable while the permanent is on the battlefield (unless the
            // ability itself says otherwise — no such override exists yet).
            if source_object.zone != ZoneKind::Battlefield {
                return Err(CommandError::IllegalAction(
                    "source is not on the battlefield".to_string(),
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
            let cost = ability.cost;
            let operation = ability.operation;

            match effect {
                Some(AbilityEffect::AddMana(units)) => {
                    // CR 605.1a: a mana ability never has a target.
                    if !targets.is_empty() {
                        return Err(CommandError::IllegalAction(
                            "a mana ability cannot have targets".to_string(),
                        ));
                    }
                    let source_object = state.object(source).ok_or(CommandError::InvalidTarget)?;
                    if source_object.tapped {
                        return Err(CommandError::IllegalAction(
                            "source is already tapped".to_string(),
                        ));
                    }
                    // CR 302.6: summoning sickness only restricts a
                    // creature's {T}/{Q} abilities - non-creature permanents
                    // (lands, artifacts, ...) ignore it entirely.
                    if source_object.definition.card_types.contains(CardTypeSet::CREATURE)
                        && has_summoning_sickness(state, source_object)
                    {
                        return Err(CommandError::IllegalAction(
                            "source has summoning sickness".to_string(),
                        ));
                    }

                    state.object_mut(source).ok_or(CommandError::InvalidTarget)?.tapped = true;
                    let pool = &mut state
                        .player_mut(command.actor)
                        .ok_or_else(|| {
                            CommandError::IllegalAction("actor has no player state".to_string())
                        })?
                        .mana_pool;
                    for unit in &units {
                        pool.add_unit(*unit);
                    }

                    // CR 605.3b: a mana ability resolves immediately as part
                    // of activation - it never uses the stack.
                    return Ok(CommandOutcome {
                        events: vec![
                            GameEventKind::Tapped { object: source },
                            GameEventKind::ManaAdded { player: command.actor, units },
                        ],
                        broke_pass_chain: true,
                        priority_handoff: PriorityHandoff::Unchanged,
                        requires_payment: None,
                        battlefield_entries: Vec::new(),
                    });
                }
                // None (script-bridge-only effect) and Some(GainLife) (a
                // non-mana ActivateAbility whose effect happens to be a
                // recognized primitive) both take the same path: unlike CR
                // 605's mana abilities, a non-mana activated ability uses
                // the stack (CR 602.2) rather than resolving immediately.
                // Targets are accepted and stored on the StackObject below,
                // not validated for legality - same stance CastSpell already
                // takes (see its own TODO comment above).
                non_mana_effect @ (None | Some(AbilityEffect::GainLife(_))) => match cost {
                    Some(AbilityCost::Tap) => {
                        let source_object =
                            state.object(source).ok_or(CommandError::InvalidTarget)?;
                        if source_object.tapped {
                            return Err(CommandError::IllegalAction(
                                "source is already tapped".to_string(),
                            ));
                        }
                        // CR 302.6: same summoning-sickness restriction as
                        // the mana-ability arm above - a creature's own
                        // {T} abilities are restricted, non-creatures ignore
                        // it.
                        if source_object.definition.card_types.contains(CardTypeSet::CREATURE)
                            && has_summoning_sickness(state, source_object)
                        {
                            return Err(CommandError::IllegalAction(
                                "source has summoning sickness".to_string(),
                            ));
                        }

                        state.object_mut(source).ok_or(CommandError::InvalidTarget)?.tapped = true;

                        // CR 400.7: the stack object is a new id, distinct
                        // from `source` (which stays on the battlefield) -
                        // same convention PutTriggersApnap uses for
                        // triggered abilities.
                        let stack_id = state.allocate_object_id();
                        state.stack.push(StackObject::new(
                            stack_id,
                            StackEntryKind::ActivatedAbility,
                            source,
                            command.actor,
                            // TODO: no modes/X value on activated abilities yet.
                            Vec::new(),
                            targets,
                            None,
                            operation,
                            non_mana_effect,
                        ));

                        return Ok(CommandOutcome {
                            events: vec![
                                GameEventKind::Tapped { object: source },
                                GameEventKind::StackObjectPushed { stack_id },
                            ],
                            broke_pass_chain: true,
                            priority_handoff: PriorityHandoff::Unchanged,
                            requires_payment: None,
                            battlefield_entries: Vec::new(),
                        });
                    }
                    // No cost primitive recognized yet - same
                    // script-bridge-fallback gap as `invoke_cost` itself
                    // (still `todo!()`); no ability in this engine has ever
                    // set `cost` to anything but `Tap` or `None`.
                    None => todo!(),
                },
            }
        }
        PlayerActionKind::DeclareAttackers { attackers } => {
            // CR 508.1: a turn-based action of the active player, only
            // during the declare attackers step, and only once per combat.
            if command.actor != state.active_player {
                return Err(CommandError::IllegalAction(
                    "only the active player declares attackers".to_string(),
                ));
            }
            if state.phase != Phase::Combat || state.step != Step::DeclareAttackers {
                return Err(CommandError::IllegalAction(
                    "attackers can only be declared during the declare attackers step"
                        .to_string(),
                ));
            }
            if state.combat.is_some() {
                return Err(CommandError::IllegalAction(
                    "attackers have already been declared this combat".to_string(),
                ));
            }

            // CR 508.1: "if at any point ... unable to comply ... the
            // declaration is illegal" - validate every attacker before
            // mutating anything, so an illegal batch taps nobody.
            for (attacker, target) in &attackers {
                let object = state.object(*attacker).ok_or(CommandError::InvalidTarget)?;
                if object.controller != command.actor {
                    return Err(CommandError::IllegalAction(
                        "actor does not control a chosen attacker".to_string(),
                    ));
                }
                if !object.definition.card_types.contains(CardTypeSet::CREATURE)
                    || object.definition.card_types.contains(CardTypeSet::BATTLE)
                {
                    return Err(CommandError::IllegalAction(
                        "a chosen attacker is not a creature, or is also a battle".to_string(),
                    ));
                }
                if object.tapped {
                    return Err(CommandError::IllegalAction(
                        "a chosen attacker is already tapped".to_string(),
                    ));
                }
                // CR 508.1a/302.6/702.10b: has_summoning_sickness itself
                // grants Haste's bypass.
                if has_summoning_sickness(state, object) {
                    return Err(CommandError::IllegalAction(
                        "a chosen attacker has summoning sickness".to_string(),
                    ));
                }
                // CR 302.6b/702.3b: a creature with defender can't attack.
                if object.definition.keywords.contains(KeywordSet::DEFENDER) {
                    return Err(CommandError::IllegalAction(
                        "a chosen attacker has defender and can't attack".to_string(),
                    ));
                }
                match target {
                    Target::Player(pid) => {
                        if *pid == command.actor || !state.turn_order.contains(pid) {
                            return Err(CommandError::IllegalAction(
                                "an attacker's target must be an opponent".to_string(),
                            ));
                        }
                    }
                    Target::Object(target_id) => {
                        let target_object =
                            state.object(*target_id).ok_or(CommandError::InvalidTarget)?;
                        if target_object.controller == command.actor {
                            return Err(CommandError::IllegalAction(
                                "an attacker's target permanent must be controlled by an \
                                 opponent"
                                    .to_string(),
                            ));
                        }
                        if !target_object
                            .definition
                            .card_types
                            .intersects(CardTypeSet::PLANESWALKER | CardTypeSet::BATTLE)
                        {
                            return Err(CommandError::IllegalAction(
                                "an attacker's target permanent must be a planeswalker or \
                                 battle"
                                    .to_string(),
                            ));
                        }
                    }
                }
            }

            // CR 508.1f: tapping an attacker isn't a cost - attacking
            // simply causes it. CR 702.20a: Vigilance means attacking
            // doesn't tap it.
            let mut events = Vec::with_capacity(attackers.len() * 2);
            for (attacker, target) in &attackers {
                let object = state.object_mut(*attacker).expect("validated above");
                if !object.definition.keywords.contains(KeywordSet::VIGILANCE) {
                    object.tapped = true;
                    events.push(GameEventKind::Tapped { object: *attacker });
                }
                events
                    .push(GameEventKind::AttackerDeclared { attacker: *attacker, target: *target });
            }
            state.combat =
                Some(CombatState { attackers, blocks: Vec::new(), blockers_declared: false });

            return Ok(CommandOutcome {
                events,
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                requires_payment: None,
                battlefield_entries: Vec::new(),
            });
        }
        PlayerActionKind::DeclareBlockers { blocks } => {
            // CR 509.1: a turn-based action of the defending player. This
            // engine only models 2-player combat (CR 506.2's nonactive
            // player is the defending player) - multiplayer's "which
            // defending player" ambiguity (CR 508.5a) isn't representable
            // yet.
            let defending_player = state
                .turn_order
                .iter()
                .copied()
                .find(|p| *p != state.active_player)
                .ok_or_else(|| {
                    CommandError::IllegalAction("no defending player exists".to_string())
                })?;
            if command.actor != defending_player {
                return Err(CommandError::IllegalAction(
                    "only the defending player declares blockers".to_string(),
                ));
            }
            if state.phase != Phase::Combat || state.step != Step::DeclareBlockers {
                return Err(CommandError::IllegalAction(
                    "blockers can only be declared during the declare blockers step".to_string(),
                ));
            }
            let combat = state.combat.as_ref().ok_or_else(|| {
                CommandError::IllegalAction("no attackers have been declared this combat".to_string())
            })?;
            if combat.blockers_declared {
                return Err(CommandError::IllegalAction(
                    "blockers have already been declared this combat".to_string(),
                ));
            }

            // CR 509.1a: "the defending player chooses one creature for it
            // to block" - a blocker can only appear once across this
            // declaration.
            let mut seen_blockers = Vec::with_capacity(blocks.len());
            for (blocker, attacker) in &blocks {
                if seen_blockers.contains(blocker) {
                    return Err(CommandError::IllegalAction(
                        "the same blocker can't be assigned to block twice".to_string(),
                    ));
                }
                seen_blockers.push(*blocker);

                let blocker_object = state.object(*blocker).ok_or(CommandError::InvalidTarget)?;
                if blocker_object.controller != command.actor {
                    return Err(CommandError::IllegalAction(
                        "actor does not control a chosen blocker".to_string(),
                    ));
                }
                if !blocker_object.definition.card_types.contains(CardTypeSet::CREATURE)
                    || blocker_object.definition.card_types.contains(CardTypeSet::BATTLE)
                {
                    return Err(CommandError::IllegalAction(
                        "a chosen blocker is not a creature, or is also a battle".to_string(),
                    ));
                }
                if blocker_object.tapped {
                    return Err(CommandError::IllegalAction(
                        "a chosen blocker is already tapped".to_string(),
                    ));
                }

                let (_, attacking) = combat
                    .attackers
                    .iter()
                    .find(|(id, _)| id == attacker)
                    .ok_or_else(|| {
                        CommandError::IllegalAction(
                            "a chosen attacker isn't actually attacking this combat".to_string(),
                        )
                    })?;
                let attacking_this_defender = match attacking {
                    Target::Player(pid) => *pid == command.actor,
                    Target::Object(target_id) => state
                        .object(*target_id)
                        .is_some_and(|o| o.controller == command.actor),
                };
                if !attacking_this_defender {
                    return Err(CommandError::IllegalAction(
                        "a chosen attacker isn't attacking this defending player, their \
                         planeswalker, or their battle"
                            .to_string(),
                    ));
                }

                // CR 702.9b: an attacker with flying can only be blocked by
                // a creature with flying and/or reach. Reads through
                // effective_keywords (CR 613 layer 6) - a continuous effect
                // granting/removing Flying/Reach now actually changes
                // blocking legality, not just the printed KeywordSet.
                let attacker_object = state.object(*attacker).ok_or(CommandError::InvalidTarget)?;
                let attacker_keywords = effective_keywords(state, attacker_object.object_id);
                let blocker_keywords = effective_keywords(state, blocker_object.object_id);
                if attacker_keywords.contains(KeywordSet::FLYING)
                    && !blocker_keywords.intersects(KeywordSet::FLYING | KeywordSet::REACH)
                {
                    return Err(CommandError::IllegalAction(
                        "an attacker with flying can only be blocked by a creature with \
                         flying or reach"
                            .to_string(),
                    ));
                }
            }

            // CR 702.111b: an attacker with menace can't be blocked except
            // by two or more creatures - a whole-batch check, since it's
            // about how many blockers ended up assigned to that attacker,
            // not any single pairing. Same effective_keywords treatment as
            // Flying/Reach above.
            let mut blocker_counts: HashMap<ObjectId, u32> = HashMap::new();
            for (_, attacker) in &blocks {
                *blocker_counts.entry(*attacker).or_insert(0) += 1;
            }
            for (attacker, count) in &blocker_counts {
                let attacker_object = state.object(*attacker).ok_or(CommandError::InvalidTarget)?;
                let attacker_keywords = effective_keywords(state, attacker_object.object_id);
                if attacker_keywords.contains(KeywordSet::MENACE) && *count < 2 {
                    return Err(CommandError::IllegalAction(
                        "an attacker with menace can't be blocked except by two or more \
                         creatures"
                            .to_string(),
                    ));
                }
            }

            let mut events = Vec::with_capacity(blocks.len());
            for (blocker, attacker) in &blocks {
                events.push(GameEventKind::BlockerDeclared {
                    blocker: *blocker,
                    attacker: *attacker,
                });
            }
            let combat = state.combat.as_mut().expect("checked Some above");
            combat.blocks = blocks;
            combat.blockers_declared = true;

            return Ok(CommandOutcome {
                events,
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                requires_payment: None,
                battlefield_entries: Vec::new(),
            });
        }
        PlayerActionKind::AssignCombatDamage { assignments } => {
            // CR 510.1/509.1's turn-based-action shape, same as
            // DeclareAttackers/DeclareBlockers: active player only, only
            // during the combat damage step.
            if command.actor != state.active_player {
                return Err(CommandError::IllegalAction(
                    "only the active player assigns combat damage".to_string(),
                ));
            }
            if state.phase != Phase::Combat || state.step != Step::CombatDamage {
                return Err(CommandError::IllegalAction(
                    "combat damage can only be assigned during the combat damage step"
                        .to_string(),
                ));
            }
            let combat = state.combat.as_ref().ok_or_else(|| {
                CommandError::IllegalAction("no attackers have been declared this combat".to_string())
            })?;

            // CR 510.1c: a real split only exists for an attacker with 2+
            // blockers - every other attacker (0 or 1 blockers) has exactly
            // one legal place for its damage to go, computed below rather
            // than chosen.
            let mut blockers_by_attacker: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
            for (blocker, attacker) in &combat.blocks {
                blockers_by_attacker.entry(*attacker).or_default().push(*blocker);
            }
            let multi_blocked: HashSet<ObjectId> = blockers_by_attacker
                .iter()
                .filter(|(_, blockers)| blockers.len() >= 2)
                .map(|(attacker, _)| *attacker)
                .collect();

            // CR 510.1e atomicity: `assignments` must cover exactly the
            // multi-blocked set - no extra (nothing to split for a 0/1-
            // blocker attacker), no missing (the engine can't guess a
            // split).
            let submitted: HashSet<ObjectId> = assignments.iter().map(|(attacker, _)| *attacker).collect();
            if submitted != multi_blocked {
                return Err(CommandError::IllegalAction(
                    "assignments must cover exactly the attackers with two or more blockers"
                        .to_string(),
                ));
            }

            for (attacker, splits) in &assignments {
                if state.object(*attacker).is_none() {
                    return Err(CommandError::InvalidTarget);
                }
                // CR 510.1a: 0-or-less power assigns no damage. Reads through
                // effective_power (CR 613 layer 7c) so this validation stays
                // consistent with the actual damage-event amounts computed
                // below, which use the same helper.
                let power = effective_power(state, *attacker).map(|p| p.max(0) as u32).unwrap_or(0);
                let blockers = &blockers_by_attacker[attacker];

                let mut seen = Vec::with_capacity(splits.len());
                let mut sum: u32 = 0;
                for (blocker, amount) in splits {
                    if !blockers.contains(blocker) {
                        return Err(CommandError::IllegalAction(
                            "a split entry's blocker isn't actually blocking that attacker"
                                .to_string(),
                        ));
                    }
                    if seen.contains(blocker) {
                        return Err(CommandError::IllegalAction(
                            "a split can't assign the same blocker twice".to_string(),
                        ));
                    }
                    seen.push(*blocker);
                    sum += amount;
                }
                if seen.len() != blockers.len() {
                    return Err(CommandError::IllegalAction(
                        "a split must assign an amount (possibly zero) to every blocker"
                            .to_string(),
                    ));
                }
                if sum != power {
                    return Err(CommandError::IllegalAction(
                        "a split's amounts must sum to exactly the attacker's power".to_string(),
                    ));
                }
            }

            let assignment_map: HashMap<ObjectId, &Vec<(ObjectId, u32)>> =
                assignments.iter().map(|(attacker, splits)| (*attacker, splits)).collect();

            // CR 510.1b/c/d + 510.2: assign and deal in the same pass - no
            // priority window between them.
            let mut events = Vec::new();
            for (attacker, target) in &combat.attackers {
                if state.object(*attacker).is_none() {
                    return Err(CommandError::InvalidTarget);
                }
                // CDA power (PTValue::CharacteristicDefining) can't be
                // resolved without the script bridge - skip rather than
                // guess, same stance sba.rs's toughness check already takes.
                // effective_power (CR 613 layer 7c) folds in any applicable
                // ModifyPT/SetPT continuous effect on top of that.
                let Some(power) = effective_power(state, *attacker) else {
                    continue;
                };
                let power = power.max(0) as u32;
                if power == 0 {
                    continue;
                }

                match blockers_by_attacker.get(attacker) {
                    None => {
                        // 510.1b: unblocked - all damage to the original target.
                        events.push(GameEventKind::DamageDealt {
                            source: *attacker,
                            target: *target,
                            amount: power,
                        });
                    }
                    Some(blockers) if blockers.len() == 1 => {
                        // 510.1c: exactly one blocker - no choice, all
                        // damage there.
                        events.push(GameEventKind::DamageDealt {
                            source: *attacker,
                            target: Target::Object(blockers[0]),
                            amount: power,
                        });
                    }
                    Some(_) => {
                        // 510.1c: 2+ blockers - the validated split above.
                        for (blocker, amount) in assignment_map[attacker] {
                            if *amount > 0 {
                                events.push(GameEventKind::DamageDealt {
                                    source: *attacker,
                                    target: Target::Object(*blocker),
                                    amount: *amount,
                                });
                            }
                        }
                    }
                }
            }
            for (blocker, attacker) in &combat.blocks {
                if state.object(*blocker).is_none() {
                    return Err(CommandError::InvalidTarget);
                }
                let Some(power) = effective_power(state, *blocker) else {
                    continue;
                };
                let power = power.max(0) as u32;
                if power == 0 {
                    continue;
                }
                // 510.1d: a blocker deals all its damage to the one
                // attacker it blocks - never ambiguous under this engine's
                // one-blocker-one-attacker block model.
                events.push(GameEventKind::DamageDealt {
                    source: *blocker,
                    target: Target::Object(*attacker),
                    amount: power,
                });
            }

            // CR 510.2: deal it - mark damage on creatures, convert to life
            // loss on players, no replacement/prevention check yet
            // (CR 614-616, replacement.rs is still mostly todo!()).
            for event in &events {
                let GameEventKind::DamageDealt { source, target, amount } = event else {
                    unreachable!("only DamageDealt events are pushed above");
                };
                // CR 702.15/702.80/702.90: Lifelink/Wither/Infect all modify
                // how this same damage event plays out, keyed off the
                // source's effective keywords (CR 613 layer 6 - a granted
                // Lifelink/Deathtouch/etc now applies here too, not just the
                // printed KeywordSet) - looked up once, up front, so the
                // borrow ends before the target mutations below (which may
                // need a mutable borrow of the same GameState).
                let source_keywords = effective_keywords(state, *source);
                let has_lifelink = source_keywords.contains(KeywordSet::LIFELINK);
                let replaces_with_counters =
                    source_keywords.intersects(KeywordSet::WITHER | KeywordSet::INFECT);
                let is_infect = source_keywords.contains(KeywordSet::INFECT);
                let has_deathtouch = source_keywords.contains(KeywordSet::DEATHTOUCH);
                let source_controller = state.object(*source).map(|o| o.controller);

                match target {
                    Target::Player(pid) => {
                        // CR 702.90b: Infect damage to a player becomes
                        // poison counters, not life loss.
                        if is_infect {
                            if let Some(player) = state.player_mut(*pid) {
                                player.counters.add(CounterKind::Poison, *amount as i32);
                            }
                        } else if let Some(player) = state.player_mut(*pid) {
                            player.lose_life(*amount as i32);
                        }
                    }
                    Target::Object(id) => {
                        // TODO(planeswalker/battle damage, CR 120.3c/d):
                        // damage to those marks loyalty/defense loss, not
                        // damage_marked - GameObject has no loyalty/defense-
                        // damage path wired yet, so only creature targets
                        // are functional this slice.
                        if let Some(object) = state.object_mut(*id) {
                            if object.definition.card_types.contains(CardTypeSet::CREATURE) {
                                // CR 702.80a/702.90c: Wither/Infect damage
                                // to a creature becomes -1/-1 counters
                                // instead of marked damage.
                                if replaces_with_counters {
                                    object.counters.add(CounterKind::MinusOneMinusOne, *amount as i32);
                                } else {
                                    object.damage_marked += amount;
                                    if has_deathtouch && *amount > 0 {
                                        object.deathtouched = true;
                                    }
                                }
                            }
                        }
                    }
                }

                // CR 702.15b: Lifelink's life gain happens "in addition to
                // any other results that damage causes" - applies to damage
                // dealt to a player or an object alike.
                if has_lifelink && *amount > 0 {
                    if let Some(controller) = source_controller {
                        if let Some(player) = state.player_mut(controller) {
                            player.lose_life(-(*amount as i32));
                        }
                    }
                }
            }

            return Ok(CommandOutcome {
                events,
                broke_pass_chain: true,
                priority_handoff: PriorityHandoff::Unchanged,
                requires_payment: None,
                battlefield_entries: Vec::new(),
            });
        }
        _ => {}
    }
    // TODO: match command.action (PlayerActionKind) and handle each variant:
    //   CastSpell is implemented above.
    //   ActivateAbility (mana-ability case) is implemented above; non-mana
    //     abilities fall through to todo!() pending the script bridge.
    //   PassPriority/PlayLand are implemented above (the match arms before
    //     this comment block) - everything else here is still unimplemented.
    //   DeclareAttackers/DeclareBlockers are implemented above (declarations
    //     only - no combat damage resolution, no triggered-ability
    //     collection beyond the AttackerDeclared/BlockerDeclared events
    //     themselves, no costs-to-attack/block, no restrictions check).
    //   SubmitPayment is NOT handled here, deliberately - see
    //     Engine::submit_payment (engine.rs) and CommandOutcome::
    //     requires_payment's doc comment above. Payment answers
    //     ProcessState::AwaitingPayment, a mid-pipeline pause, the same way
    //     submit_replacement_choice/submit_trigger_order answer their own
    //     pauses - never goes through submit_action/execute_command at all.
    //   MakeChoice: generic - depends entirely on what asked for the
    //     choice. broke_pass_chain = true, priority_handoff = Unchanged.
    // Every branch ends in returning
    // Ok(CommandOutcome { events, broke_pass_chain, priority_handoff }).
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::counter::CounterSet;
    use crate::types::game::RngState;
    use crate::types::mana::ManaPool;
    use crate::types::player::PlayerState;

    fn minimal_state() -> GameState {
        let p0 = PlayerId(0);
        GameState {
            players: vec![PlayerState {
                player_id: p0,
                life: 20,
                mana_pool: ManaPool { white: 0, blue: 0, black: 0, red: 0, green: 0, colorless: 0 },
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                mulligans_taken: 0,
                hand_kept: false,
            }],
            turn_order: vec![p0],
            active_player: p0,
            turn_number: 1,
            phase: Phase::PreGame,
            step: Step::Untap,
            priority_player: p0,
            consecutive_passes: 0,
            lands_played_this_turn: HashMap::new(),
            combat: None,
            last_active_turn_number: HashMap::new(),
            objects: HashMap::new(),
            zones: Vec::new(),
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState { seed: 1, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 0,
            pending_step_skips: Vec::new(),
        }
    }

    fn dummy_ability(source: ObjectId) -> RegisteredAbility {
        RegisteredAbility {
            source,
            kind: AbilityKind::Triggered,
            condition: None,
            cost: None,
            target: None,
            operation: ScriptHandle(0),
            effect: Some(AbilityEffect::GainLife(1)),
        }
    }

    #[test]
    fn retire_object_purges_only_the_retired_source() {
        let mut state = minimal_state();
        state.abilities.push(dummy_ability(ObjectId(1)));
        state.abilities.push(dummy_ability(ObjectId(2)));

        retire_object(&mut state, ObjectId(1));

        assert_eq!(state.abilities.len(), 1, "only ObjectId(1)'s ability should be purged");
        assert_eq!(state.abilities[0].source, ObjectId(2), "ObjectId(2)'s ability must survive untouched");
    }
}
