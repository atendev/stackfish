use std::collections::HashMap;

use stackfish_engine::*;
use stackfish_engine::engine::Engine;
use stackfish_engine::types::config::{GameConfig, GameSettings, PlayerConfig};

use crate::schema::*;
use crate::build::{build_continuous_effect, card_definition_from_spec, open_card_db, parse_mana_unit, register_script_operation, register_script_target_requirement, register_target_script, register_trigger_ability};
use crate::report::remap_moved_card_ids;
use crate::run::drain;

/// Builds a `GameSettings` from a `GameSettingsSpec`, applying each present
/// field as an override over `GameSettings::default()`.
fn build_game_settings(spec: &GameSettingsSpec) -> GameSettings {
    let mut s = GameSettings::default();
    if let Some(v) = spec.shuffle_libraries { s.shuffle_libraries = v; }
    if let Some(v) = spec.starting_hand_size { s.starting_hand_size = v; }
    if let Some(v) = spec.skip_mulligan { s.skip_mulligan = v; }
    if let Some(v) = spec.free_mulligans { s.free_mulligans = v; }
    if let Some(v) = spec.max_mulligans { s.max_mulligans = Some(v); }
    if let Some(v) = spec.starting_life { s.starting_life = v; }
    if let Some(v) = spec.poison_loss_threshold { s.poison_loss_threshold = v; }
    if let Some(v) = spec.max_hand_size { s.max_hand_size = v; }
    s
}

/// Builds an `Engine` via `Engine::new_game` from a `[game]` section.
/// Also reconstructs `card_ids` so scenario steps can reference library
/// cards by their `[[cards]]` key. `Engine::new_game` assigns object ids
/// sequentially (one per library card, in player/library order starting at
/// 0), so we track those initial ids here and then remap them through the
/// pregame ledger events — `draw_opening_hand` and `return_hand_to_library`
/// both log `ZoneChange` with `last_known_info`, so `remap_moved_card_ids`
/// correctly follows each card to its post-pregame id and zone. Duplicate
/// keys in a library get the last id (same as `[[setup]]` semantics).
pub(crate) fn build_new_game_engine(file: &ScenarioFile, game: &GameSpec) -> (Engine, HashMap<String, ObjectId>) {
    let card_specs: HashMap<&str, &CardSpec> =
        file.cards.iter().map(|c| (c.key.as_str(), c)).collect();
    let card_db = open_card_db(file);

    let settings = build_game_settings(&game.settings);
    let rng_seed = game.rng_seed.or(file.seed).unwrap_or(0);

    // Track initial object ids — Engine::new_game allocates sequentially
    // starting at 0, one per library card, in the same player/card order
    // we iterate here.
    let mut card_ids: HashMap<String, ObjectId> = HashMap::new();
    let mut next_id: u64 = 0;

    let player_configs: Vec<PlayerConfig> = game.players.iter().map(|gp| {
        let library = gp.library.iter().map(|key| {
            let spec = card_specs.get(key.as_str())
                .unwrap_or_else(|| panic!("game.players library references unknown card key '{key}'"));
            card_ids.insert(key.clone(), ObjectId(next_id));
            next_id += 1;
            card_definition_from_spec(card_db.as_ref(), 0, spec)
        }).collect();
        // CR 903.6: minted after this player's library, matching
        // `Engine::new_game`'s own id-allocation order exactly (library
        // cards first, then commanders, per player) - see this block's own
        // leading comment on why `card_ids`/`next_id` have to mirror that
        // order precisely.
        let commanders = gp.commanders.iter().map(|key| {
            let spec = card_specs.get(key.as_str())
                .unwrap_or_else(|| panic!("game.players commanders references unknown card key '{key}'"));
            card_ids.insert(key.clone(), ObjectId(next_id));
            next_id += 1;
            card_definition_from_spec(card_db.as_ref(), 0, spec)
        }).collect();
        PlayerConfig { player_id: PlayerId(gp.id), library, commanders, life_override: gp.life_override }
    }).collect();

    let turn_order: Vec<PlayerId> = if game.turn_order.is_empty() {
        game.players.iter().map(|gp| PlayerId(gp.id)).collect()
    } else {
        game.turn_order.iter().map(|&id| PlayerId(id)).collect()
    };

    let mut engine = Engine::new_game(GameConfig { players: player_configs, turn_order, settings, rng_seed });
    // Drive pregame automation (shuffle/draw/mulligan) to completion before
    // any scenario step runs — `new_game` only sets up `Phase::PreGame`; the
    // actual pregame actions happen on the first `step()` call, same as they
    // would in live play.
    drain(&mut engine);
    // Remap ids through pregame zone changes (draw_opening_hand and
    // return_hand_to_library both mint new ObjectIds and log ZoneChange events).
    remap_moved_card_ids(&mut card_ids, engine.state().ledger.as_slice());
    // Same `mana_ability` -> `RegisteredAbility` workaround `build_initial_state`
    // uses (no script bridge to register these from `CardDefinition` yet — see
    // `Engine::state_mut`'s doc comment). Done after the remap above so each
    // ability's `source` is the card's current (post-pregame) id.
    for gp in &game.players {
        for key in &gp.library {
            let spec = card_specs[key.as_str()];
            if !spec.mana_ability.is_empty() {
                let units = spec.mana_ability.iter().map(|s| parse_mana_unit(s)).collect();
                let id = engine.state_mut().allocate_ability_id();
                engine.state_mut().abilities.push(RegisteredAbility {
                    modes: Vec::new(),
                    mode_choice: ModeChoice::Exactly(1),
                    id,
                    source: card_ids[key],
                    kind: AbilityKind::Activated,
                    condition: None,
                    cost: None,
                    target: None,
                    operation: ScriptHandle(0),
                    effect: Some(AbilityEffect::AddMana(units)),
                    intervening_if: None,
                });
            }
            if let Some(operation_src) = &spec.trigger_operation {
                register_trigger_ability(
                    &mut engine,
                    card_ids[key],
                    spec.trigger_condition.as_deref(),
                    operation_src,
                );
            }
            if let Some(effect_spec) = &spec.continuous_effect {
                engine.state_mut().continuous_effects.push(build_continuous_effect(card_ids[key], effect_spec));
            }
            if let Some(script) = spec.target_requirement.as_ref().and_then(|r| r.script.as_ref()) {
                register_target_script(&mut engine, card_ids[key], script);
            }
            // Same `resolved_script` reasoning as `build_initial_state` -
            // `spec.script` alone misses a `db_script_id` card's real source,
            // which only lives on the `CardDefinition` this card's own
            // object already carries post-pregame (`phase5/db-script-
            // operation-fix`).
            let resolved_script: Option<String> = match &engine
                .state()
                .object(card_ids[key])
                .unwrap_or_else(|| panic!("card_ids entry for '{key}' has no matching object"))
                .definition
                .script_ref
            {
                ScriptReference::Inline(source) if !source.is_empty() => Some(source.clone()),
                _ => None,
            };
            if spec.target_requirement.is_none() {
                if let Some(source) = &resolved_script {
                    register_script_target_requirement(&mut engine, card_ids[key], source);
                }
            }
            if let Some(source) = &resolved_script {
                register_script_operation(&mut engine, card_ids[key], source);
            }
        }
    }
    (engine, card_ids)
}

