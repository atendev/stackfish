//! The `CreateToken` arm (CR 111) and the token-definition builders only it uses.
//! See `DESIGN.md`.

use std::ops::ControlFlow;

use crate::commands;
use crate::replacement;
use crate::types::ability::{AbilityEffect, AbilityKind, NamedToken, RegisteredAbility, TokenSpec};
use crate::types::card::{
    CardDefinition, CardTypeSet, KeywordSet, ModeChoice, PTValue, ScriptReference, Subtype,
    SupertypeSet,
};
use crate::types::cost::{AdditionalCost, CostSpec};
use crate::types::counter::CounterSet;
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId, ScriptHandle, Target};
use crate::types::mana::{ColorSet, ManaCost};
use crate::types::object::{GameObject, VisibilityPolicy};
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::{Engine, ProcessState, StepStatus};

impl Engine {
    /// CR 111: creates token permanents under the controller's control. Named tokens
    /// register their abilities. Scripts are skipped except for `TokenSpec::CopyOf`
    /// (CR 707.10f), whose snapshot carries abilities. Enters-the-battlefield
    /// modifiers apply, with no colors spent since a token is never cast
    /// (CR 702.44b). Pre-attached entry (CR 303.4f) is handled in `continue_create_token`.
    // TODO: devour pause for a `CopyOf` token; `continue_create_token` could reuse its attach-pause shape (CR 702.82a)
    // TODO: amplify pause for a `CopyOf` token (CR 702.38a)
    pub(super) fn resolve_create_token_effect(
        &mut self,
        top: &StackObject,
        spec: &TokenSpec,
        count: u32,
        events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let controller = top.controller;
        let copy_snapshot = match spec {
            TokenSpec::CopyOf(source_id, exceptions) => self.state.object(*source_id).map(|obj| {
                let mut snapshot = obj
                    .copiable_override
                    .clone()
                    .unwrap_or_else(|| obj.definition.clone());
                // CR 707.9c: a new token has no prior identity to retain a name or color from.
                exceptions.apply(&mut snapshot, None);
                snapshot
            }),
            _ => None,
        };
        self.continue_create_token(
            spec,
            &copy_snapshot,
            count,
            controller,
            top.stack_id,
            events,
        )
    }

    /// The resumable body of the token loop, called fresh (`remaining == count`) and
    /// from `submit_token_attach_choice` with the tokens still owed. The attach scan
    /// runs before the `GameObject` is built, using the real id, so a zero-candidate
    /// Aura token is simply never created (CR 303.4g).
    pub(in crate::engine) fn continue_create_token(
        &mut self,
        spec: &TokenSpec,
        copy_snapshot: &Option<CardDefinition>,
        mut remaining: u32,
        controller: PlayerId,
        stack_id: ObjectId,
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        while remaining > 0 {
            remaining -= 1;
            let token_def = match (copy_snapshot, spec) {
                (Some(def), _) => def.clone(),
                (None, TokenSpec::CopyOf(_, _)) => {
                    // The source no longer resolves; skip rather than guess.
                    continue;
                }
                (None, _) => build_token_definition(spec),
            };
            let token_id = self.state.allocate_object_id();
            // CR 303.4f: scanned before the token exists so it can't be its own candidate.
            let attach_candidates = commands::legal_entry_attach_candidates(
                &self.state,
                &self.script_runtime,
                &token_def,
                controller,
                token_id,
            );
            if let Some(candidates) = &attach_candidates {
                // CR 303.4g: an Aura token with no legal host isn't created. Equipment
                // and Fortification still enter unattached (CR 301.5e).
                if candidates.is_empty() && commands::is_aura(&token_def) {
                    continue;
                }
            }
            let ts = self.state.allocate_sequence_no();
            let token_obj = GameObject {
                object_id: token_id,
                lineage: ObjectLineage { previous: None },
                definition: token_def.clone(),
                copiable_override: None,
                owner: controller,
                controller,
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
                attached_to_player: None,
                timestamp: ts,
                controlled_since_turn: self.state.turn_number,
                is_token: true,
                is_commander: false,
                visibility: VisibilityPolicy {
                    public: true,
                    visible_to: Vec::new(),
                },
                copy_generation: 0,
                entered_via_escape: false,
            };
            if let Some(zone) = self.state.zone_mut(ZoneKind::Battlefield, None) {
                zone.push_bottom(token_id);
                self.state.objects.insert(token_id, token_obj);
            }
            register_token_abilities(&mut self.state, token_id, spec, controller);
            if matches!(spec, TokenSpec::CopyOf(_, _)) {
                let result =
                    self.script_runtime
                        .run_card_script(&mut self.state, &token_def, token_id);
                self.record_script_error(token_id, &token_def.script_ref, result);
            }
            // Checked after self-registration so a `CopyOf` token's script effects are in place.
            let counter_events = replacement::apply_enters_battlefield_modifiers(
                &mut self.state,
                &self.script_runtime,
                token_id,
                ZoneKind::Stack,
                ColorSet::empty(),
            );
            events.push(GameEventKind::ZoneChange {
                object: token_id,
                from: ZoneKind::Stack,
                to: ZoneKind::Battlefield,
                last_known_info: None,
            });
            events.extend(counter_events);

            if let Some(candidates) = attach_candidates {
                match candidates.len() {
                    // CR 301.5e: no legal host, so it enters unattached (an Aura was skipped above).
                    0 => {}
                    1 => match candidates[0] {
                        Target::Object(target_id) => {
                            commands::attach_permanent(&mut self.state, token_id, target_id);
                            events.push(GameEventKind::Attached {
                                object: token_id,
                                attached_to: target_id,
                            });
                        }
                        Target::Player(player) => {
                            commands::attach_permanent_to_player(&mut self.state, token_id, player);
                            events.push(GameEventKind::AttachedToPlayer {
                                object: token_id,
                                player,
                            });
                        }
                        // Candidates are objects or players only.
                        Target::Ability(_) => unreachable!("attach candidates are never abilities"),
                    },
                    _ => {
                        // CR 303.4f: with 2+ candidates the controller chooses; the token is
                        // already on the battlefield, unattached.
                        self.process_state = ProcessState::AwaitingTokenAttachChoice {
                            attaching: token_id,
                            controller,
                            candidates,
                            stack_id,
                            events_so_far: events,
                            remaining_count: remaining,
                            spec: spec.clone(),
                            copy_snapshot: copy_snapshot.clone(),
                        };
                        return ControlFlow::Break(StepStatus::Progressed);
                    }
                }
            }
        }
        ControlFlow::Continue(events)
    }
}

/// Builds a token's `CardDefinition` from its spec: no mana cost or script, with color
/// from the indicator. Not called for `CopyOf`, which snapshots the copied object.
fn build_token_definition(spec: &TokenSpec) -> CardDefinition {
    match spec {
        TokenSpec::CopyOf(_, _) => unreachable!(
            "TokenSpec::CopyOf is built from the copied object's snapshot, not build_token_definition"
        ),
        TokenSpec::Named(NamedToken::Clue) => CardDefinition {
            definition_id: 0,
            name: "Clue".to_string(),
            mana_cost: ManaCost {
                symbols: Vec::new(),
            },
            color_indicator: None,
            supertypes: SupertypeSet::empty(),
            card_types: CardTypeSet::ARTIFACT,
            subtypes: vec![Subtype("Clue".to_string())],
            oracle_text: "{2}, Sacrifice this artifact: Draw a card.".to_string(),
            power: None,
            toughness: None,
            loyalty: None,
            defense: None,
            script_ref: ScriptReference::Inline(String::new()),
            keywords: KeywordSet::empty(),
            target_requirement: None,
            alternative_costs: Vec::new(),
            buyback_cost: None,
            effect: None,
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
        },
        TokenSpec::Named(NamedToken::Food) => CardDefinition {
            definition_id: 0,
            name: "Food".to_string(),
            mana_cost: ManaCost {
                symbols: Vec::new(),
            },
            color_indicator: None,
            supertypes: SupertypeSet::empty(),
            card_types: CardTypeSet::ARTIFACT,
            subtypes: vec![Subtype("Food".to_string())],
            oracle_text: "{2}, {T}, Sacrifice this artifact: You gain 3 life.".to_string(),
            power: None,
            toughness: None,
            loyalty: None,
            defense: None,
            script_ref: ScriptReference::Inline(String::new()),
            keywords: KeywordSet::empty(),
            target_requirement: None,
            alternative_costs: Vec::new(),
            buyback_cost: None,
            effect: None,
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
        },
        TokenSpec::Named(NamedToken::Treasure) => CardDefinition {
            definition_id: 0,
            name: "Treasure".to_string(),
            mana_cost: ManaCost {
                symbols: Vec::new(),
            },
            color_indicator: None,
            supertypes: SupertypeSet::empty(),
            card_types: CardTypeSet::ARTIFACT,
            subtypes: vec![Subtype("Treasure".to_string())],
            oracle_text: "{T}, Sacrifice this artifact: Add one mana of any color.".to_string(),
            power: None,
            toughness: None,
            loyalty: None,
            defense: None,
            script_ref: ScriptReference::Inline(String::new()),
            keywords: KeywordSet::empty(),
            target_requirement: None,
            alternative_costs: Vec::new(),
            buyback_cost: None,
            effect: None,
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
        },
        TokenSpec::Creature(spec) => {
            let name = spec.name.clone().unwrap_or_else(|| spec.subtypes.join(" "));
            let color_indicator = if spec.colors.is_empty() {
                None
            } else {
                Some(spec.colors)
            };
            CardDefinition {
                definition_id: 0,
                name,
                mana_cost: ManaCost {
                    symbols: Vec::new(),
                },
                color_indicator,
                supertypes: spec.additional_supertypes,
                card_types: CardTypeSet::CREATURE | spec.additional_types,
                subtypes: spec.subtypes.iter().map(|s| Subtype(s.clone())).collect(),
                oracle_text: String::new(),
                power: Some(PTValue::Fixed(spec.power)),
                toughness: Some(PTValue::Fixed(spec.toughness)),
                loyalty: None,
                defense: None,
                script_ref: ScriptReference::Inline(String::new()),
                keywords: spec.keywords,
                target_requirement: None,
                alternative_costs: Vec::new(),
                buyback_cost: None,
                effect: None,
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
            }
        }
    }
}

/// Registers a named token's activated abilities (Clue, Food) as native
/// `RegisteredAbility`s. Creature tokens have none to register.
fn register_token_abilities(
    state: &mut GameState,
    token_id: ObjectId,
    spec: &TokenSpec,
    _controller: PlayerId,
) {
    match spec {
        TokenSpec::Named(NamedToken::Clue) => {
            let id = state.allocate_ability_id();
            state.abilities.push(RegisteredAbility {
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
                id,
                source: token_id,
                kind: AbilityKind::Activated,
                condition: None,
                cost: Some(CostSpec {
                    mana: ManaCost::parse("{2}").ok(),
                    additional: vec![AdditionalCost::SacrificeSelf],
                    alternative: Vec::new(),
                }),
                target: None,
                operation: ScriptHandle(0),
                effect: Some(AbilityEffect::DrawCards(1)),
                intervening_if: None,
            });
        }
        TokenSpec::Named(NamedToken::Food) => {
            let id = state.allocate_ability_id();
            state.abilities.push(RegisteredAbility {
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
                id,
                source: token_id,
                kind: AbilityKind::Activated,
                condition: None,
                // CR 111.10b
                cost: Some(CostSpec {
                    mana: ManaCost::parse("{2}").ok(),
                    additional: vec![AdditionalCost::TapSelf, AdditionalCost::SacrificeSelf],
                    alternative: Vec::new(),
                }),
                target: None,
                operation: ScriptHandle(0),
                effect: Some(AbilityEffect::GainLife(3)),
                intervening_if: None,
            });
        }
        TokenSpec::Named(NamedToken::Treasure) => {
            let id = state.allocate_ability_id();
            state.abilities.push(RegisteredAbility {
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
                id,
                source: token_id,
                kind: AbilityKind::Activated,
                condition: None,
                // CR 111.10a
                cost: Some(CostSpec {
                    mana: None,
                    additional: vec![AdditionalCost::TapSelf, AdditionalCost::SacrificeSelf],
                    alternative: Vec::new(),
                }),
                target: None,
                operation: ScriptHandle(0),
                effect: Some(AbilityEffect::AddManaChoice {
                    count: 1,
                    choices: ColorSet::all(),
                }),
                intervening_if: None,
            });
        }
        TokenSpec::Creature(_) => {}
        // A copy token's abilities come from `run_card_script`, not this path.
        TokenSpec::CopyOf(_, _) => {}
    }
}
