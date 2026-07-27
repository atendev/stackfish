//! `resolve_ability_effect`'s `CreateToken` arm (CR 111), plus the two free
//! functions that only it calls — `build_token_definition`/
//! `register_token_abilities`, moved here from `engine/mod.rs` since nothing
//! else uses them.

use std::ops::ControlFlow;

use crate::replacement;
use crate::types::ability::{AbilityEffect, AbilityKind, NamedToken, RegisteredAbility, TokenSpec};
use crate::types::card::{CardDefinition, CardTypeSet, KeywordSet, ModeChoice, PTValue, ScriptReference, Subtype, SupertypeSet};
use crate::types::cost::{AdditionalCost, CostSpec, FilterController, FilterSpec};
use crate::types::counter::CounterSet;
use crate::types::event::GameEventKind;
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, ObjectLineage, PlayerId, ScriptHandle};
use crate::types::mana::{ColorSet, ManaCost};
use crate::types::object::{GameObject, VisibilityPolicy};
use crate::types::stack::StackObject;
use crate::types::zone::ZoneKind;

use super::super::{Engine, StepStatus};

impl Engine {
    /// CR 111: create one or more token permanents on the battlefield under
    /// the controller's control. Named tokens (Clue, Food) get their
    /// activated abilities registered here. Tokens have empty scripts so
    /// `run_card_script` is deliberately skipped - except `TokenSpec::CopyOf`
    /// (CR 707.10f), whose "definition" is a real snapshot of another
    /// object's copiable values (see `TokenSpec::CopyOf`'s own doc comment),
    /// abilities and all, so its self-registration does need to run. TODO:
    /// CR 111.7 cessation (tokens leaving the battlefield should cease to
    /// exist) is not enforced — blocked on a zone-watch SBA hook (future
    /// slice).
    ///
    /// `apply_enters_battlefield_modifiers` is real now too, the 4th of the
    /// four real to-Battlefield producers (`PlayLand`, the resolving-spell
    /// path, `Search`) - closes a real, previously-silent gap:
    /// EntersTapped/EntersWithCounters/Bloodthirst/Sunburst all already
    /// worked for every other permanent-entry shape but never applied to a
    /// token, even one externally granted "tokens you control enter
    /// tapped"-style modifier (matched by controller, not `self_only`, so a
    /// plain `Named`/`Creature` token with no script of its own can still be
    /// affected) or a `CopyOf` token whose freshly re-run `run_card_script`
    /// self-registered one. Devour/Amplify stay unwired here on purpose -
    /// both need a genuine mid-ETB player pause this function structurally
    /// can't raise (it only ever mutates state/proposes events
    /// synchronously) - same narrow "no real producer reaches this yet"
    /// posture `pending_devour_choice`'s own doc comment already takes for
    /// `Search`. `colors_spent` is always `ColorSet::empty()` - CR 702.44b,
    /// a token creation is never a cast, same convention `PlayLand`/`Search`
    /// already use for Sunburst.
    pub(super) fn resolve_create_token_effect(
        &mut self,
        top: &StackObject,
        spec: &TokenSpec,
        count: u32,
        mut events: Vec<GameEventKind>,
    ) -> ControlFlow<StepStatus, Vec<GameEventKind>> {
        let controller = top.controller;
        let copy_snapshot = match spec {
            TokenSpec::CopyOf(source_id, exceptions) => self.state.object(*source_id).map(|obj| {
                let mut snapshot = obj.copiable_override.clone().unwrap_or_else(|| obj.definition.clone());
                // CR 707.9c/d: no `original` here - a
                // freshly created token has no prior
                // "own identity" to retain a name/
                // color from, unlike BecomeCopy/
                // submit_copy_choice's own already-
                // existing object. retain_name/
                // retain_color are a deliberate,
                // tested no-op for this producer.
                exceptions.apply(&mut snapshot, None);
                snapshot
            }),
            _ => None,
        };
        for _ in 0..count {
            let token_def = match (&copy_snapshot, spec) {
                (Some(def), _) => def.clone(),
                (None, TokenSpec::CopyOf(_, _)) => {
                    // Source no longer resolves - skip rather
                    // than guess, same stance every other
                    // unresolvable reference in this engine
                    // already takes.
                    continue;
                }
                (None, _) => build_token_definition(spec),
            };
            let token_id = self.state.allocate_object_id();
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
                timestamp: ts,
                controlled_since_turn: self.state.turn_number,
                is_token: true,
                is_commander: false,
                visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
            };
            if let Some(zone) = self.state.zone_mut(ZoneKind::Battlefield, None) {
                zone.push_bottom(token_id);
                self.state.objects.insert(token_id, token_obj);
            }
            register_token_abilities(&mut self.state, token_id, spec, controller);
            if matches!(spec, TokenSpec::CopyOf(_, _)) {
                // TODO: a script error here is silently dropped,
                // same gap every other run_card_script call site
                // in this file already flags.
                let _ = self.script_runtime.run_card_script(&mut self.state, &token_def, token_id);
            }
            // Checked after self-registration above
            // so a `CopyOf` token's freshly re-run
            // script is already in `state.
            // replacement_effects` by this point,
            // same ordering `PlayLand`/`Search`/the
            // resolving-spell path all already use.
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
        }
        ControlFlow::Continue(events)
    }
}

/// Builds a `CardDefinition` for a token from its spec. Tokens have no mana
/// cost, no script, and no alternative costs; their color comes from the
/// color indicator field rather than a mana cost symbol.
///
/// Never called with `TokenSpec::CopyOf` — the `CreateToken` resolution site
/// builds that variant's `CardDefinition` from the copied object's own
/// current copiable values instead (see `TokenSpec::CopyOf`'s own doc
/// comment), before this function would ever be reached.
fn build_token_definition(spec: &TokenSpec) -> CardDefinition {
    match spec {
        TokenSpec::CopyOf(_, _) => unreachable!(
            "TokenSpec::CopyOf is built from the copied object's snapshot, not build_token_definition"
        ),
        TokenSpec::Named(NamedToken::Clue) => CardDefinition {
            definition_id: 0,
            name: "Clue".to_string(),
            mana_cost: ManaCost { symbols: Vec::new() },
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
            effect: None,
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
        },
        TokenSpec::Named(NamedToken::Food) => CardDefinition {
            definition_id: 0,
            name: "Food".to_string(),
            mana_cost: ManaCost { symbols: Vec::new() },
            color_indicator: None,
            supertypes: SupertypeSet::empty(),
            card_types: CardTypeSet::ARTIFACT,
            subtypes: vec![Subtype("Food".to_string())],
            oracle_text: "{2}, Sacrifice this artifact: You gain 3 life.".to_string(),
            power: None,
            toughness: None,
            loyalty: None,
            defense: None,
            script_ref: ScriptReference::Inline(String::new()),
            keywords: KeywordSet::empty(),
            target_requirement: None,
            alternative_costs: Vec::new(),
            effect: None,
            modes: Vec::new(),
            mode_choice: ModeChoice::Exactly(1),
        },
        TokenSpec::Creature(spec) => {
            let name = spec.name.clone().unwrap_or_else(|| spec.subtypes.join(" "));
            let color_indicator = if spec.colors.is_empty() { None } else { Some(spec.colors) };
            CardDefinition {
                definition_id: 0,
                name,
                mana_cost: ManaCost { symbols: Vec::new() },
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
                effect: None,
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
            }
        }
    }
}

/// Pushes any activated abilities that a named token carries into
/// `state.abilities`. Creature tokens have no script-side abilities to
/// register here (keywords live on the definition; richer effects are a
/// future slice). Named tokens (Clue, Food) get their sacrifice ability
/// directly as a Rust-native `RegisteredAbility`.
fn register_token_abilities(state: &mut GameState, token_id: ObjectId, spec: &TokenSpec, _controller: PlayerId) {
    let artifact_sacrifice_filter = FilterSpec {
        card_types: Some(CardTypeSet::ARTIFACT),
        supertypes: None,
        subtypes: None,
        controller: Some(FilterController::You),
        random: false,
        extra: None,
    };
    match spec {
        TokenSpec::Named(NamedToken::Clue) => {
            let id = state.allocate_ability_id();
            state.abilities.push(RegisteredAbility {
                id,
                source: token_id,
                kind: AbilityKind::Activated,
                condition: None,
                // TODO: "sacrifice THIS artifact" -- the filter doesn't enforce
                // identity, only type+controller. Same pre-existing gap as every
                // other sacrifice-cost site. Blocked on backlog item 9.
                cost: Some(CostSpec {
                    mana: ManaCost::parse("{2}").ok(),
                    additional: vec![AdditionalCost::Sacrifice { filter: artifact_sacrifice_filter }],
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
                id,
                source: token_id,
                kind: AbilityKind::Activated,
                condition: None,
                cost: Some(CostSpec {
                    mana: ManaCost::parse("{2}").ok(),
                    additional: vec![AdditionalCost::Sacrifice { filter: artifact_sacrifice_filter }],
                    alternative: Vec::new(),
                }),
                target: None,
                operation: ScriptHandle(0),
                effect: Some(AbilityEffect::GainLife(3)),
                intervening_if: None,
            });
        }
        TokenSpec::Creature(_) => {}
        // A copy token's abilities come from run_card_script against its
        // copied CardDefinition (see the CreateToken resolution site), not
        // from this hand-built-Rust-native path.
        TokenSpec::CopyOf(_, _) => {}
    }
}
