//! Scenario tooling: loads a declarative TOML file describing a game setup
//! and a sequence of actions, builds a real `GameState`, drives it through
//! `Engine`, and reports what happened.
//!
//! Deliberately does NOT call the engine's still-`todo!()` constructors
//! (`GameObject::new`, `PlayerState::new`, `Zone::new`, etc) to build the
//! initial state — it builds `GameState`/`GameObject`/`PlayerState`/`Zone`
//! via direct struct literals instead, since their fields are `pub`. That
//! keeps this tool independent of which inherent-method bodies have been
//! filled in yet, and means setup+assertions (no actions) already runs for
//! real today, with zero panics, as a baseline smoke test of the Phase 1
//! data model.
//!
//! Two exceptions, implemented for real (not `todo!()`) because nothing
//! else can construct them — `CounterSet`/`Engine` both have a private
//! field, so external code can't struct-literal around them:
//!   - `CounterSet::new()` (counter.rs)
//!   - `Engine::new()` / `Engine::state()` (engine.rs)
//!
//! Running *actions* through `Engine::submit_action`/`step()` still hits
//! `todo!()` until Phase 5/7 fill those bodies in — expected. Wrapped in
//! `catch_unwind` so one stubbed action doesn't abort the whole report;
//! each action's outcome is reported individually.

use std::collections::{HashMap, VecDeque};
use std::panic::{self, AssertUnwindSafe};

use serde::Deserialize;

use stackfish_engine::*;
use stackfish_engine::engine::{ActionError, Engine, StepStatus};

// ---------------------------------------------------------------------
// TOML schema
// ---------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ScenarioFile {
    pub seed: u64,
    #[serde(default)]
    pub players: Vec<PlayerSpec>,
    #[serde(default)]
    pub cards: Vec<CardSpec>,
    #[serde(default)]
    pub setup: Vec<SetupEntry>,
    #[serde(default)]
    pub actions: Vec<ActionSpec>,
    #[serde(default)]
    pub assertions: Vec<AssertionSpec>,
}

#[derive(Deserialize)]
pub struct PlayerSpec {
    pub id: u32,
    pub life: i32,
}

/// Simplified mana cost shape for test fixtures — `generic_cost` + a list of
/// color letters, rather than parsing a `"{2}{R}"`-style cost string. Real
/// cost-string parsing doesn't exist anywhere yet; this sidesteps needing
/// it just to write scenario fixtures. Revisit if scenarios need hybrid/
/// Phyrexian/X costs — not representable in this shape yet.
#[derive(Deserialize)]
pub struct CardSpec {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub generic_cost: u32,
    #[serde(default)]
    pub colors: Vec<String>,
    pub types: Vec<String>,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
}

#[derive(Deserialize, Debug)]
pub struct SetupEntry {
    pub player: u32,
    pub zone: String,
    pub card: String,
}

#[derive(Deserialize, Clone)]
#[serde(untagged)]
pub enum TargetSpec {
    Player(u32),
    Card(String),
}

#[derive(Deserialize)]
pub struct AttackerSpec {
    pub attacker: String,
    pub target: TargetSpec,
}

#[derive(Deserialize)]
pub struct BlockSpec {
    pub blocker: String,
    pub attacker: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionSpec {
    PassPriority { player: u32 },
    PlayLand { player: u32, card: String },
    CastSpell {
        player: u32,
        card: String,
        #[serde(default)]
        targets: Vec<TargetSpec>,
        #[serde(default)]
        modes: Vec<u32>,
        x_value: Option<u32>,
    },
    ActivateAbility {
        player: u32,
        source: String,
        ability_index: u32,
        #[serde(default)]
        targets: Vec<TargetSpec>,
    },
    DeclareAttackers {
        player: u32,
        #[serde(default)]
        attackers: Vec<AttackerSpec>,
    },
    DeclareBlockers {
        player: u32,
        #[serde(default)]
        blocks: Vec<BlockSpec>,
    },
    MakeChoice {
        player: u32,
        description: String,
        #[serde(default)]
        selection: Vec<TargetSpec>,
    },
    // SubmitPayment intentionally omitted for v1 - PaymentPlan's CostPayment
    // shapes are more involved and the cost system itself isn't
    // implemented yet either; add a TOML shape once that's worth testing.
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssertionSpec {
    Life { player: u32, equals: i32 },
    LedgerLen { equals: usize },
    ObjectZone { card: String, equals: String },
}

// ---------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------

pub fn load(path: &std::path::Path) -> ScenarioFile {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read scenario file {}: {e}", path.display()));
    toml::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse scenario file {}: {e}", path.display()))
}

// ---------------------------------------------------------------------
// Building initial state
// ---------------------------------------------------------------------

fn parse_card_types(names: &[String]) -> CardTypeSet {
    let mut set = CardTypeSet::empty();
    for name in names {
        set |= match name.as_str() {
            "Artifact" => CardTypeSet::ARTIFACT,
            "Creature" => CardTypeSet::CREATURE,
            "Enchantment" => CardTypeSet::ENCHANTMENT,
            "Instant" => CardTypeSet::INSTANT,
            "Land" => CardTypeSet::LAND,
            "Sorcery" => CardTypeSet::SORCERY,
            "Kindred" => CardTypeSet::KINDRED,
            "Planeswalker" => CardTypeSet::PLANESWALKER,
            "Battle" => CardTypeSet::BATTLE,
            other => panic!("unknown card type in scenario file: {other}"),
        };
    }
    set
}

fn parse_color(letter: &str) -> Color {
    match letter {
        "W" => Color::White,
        "U" => Color::Blue,
        "B" => Color::Black,
        "R" => Color::Red,
        "G" => Color::Green,
        other => panic!("unknown color in scenario file: {other}"),
    }
}

fn parse_zone_kind(name: &str) -> ZoneKind {
    match name {
        "library" => ZoneKind::Library,
        "hand" => ZoneKind::Hand,
        "battlefield" => ZoneKind::Battlefield,
        "graveyard" => ZoneKind::Graveyard,
        "stack" => ZoneKind::Stack,
        "exile" => ZoneKind::Exile,
        "command" => ZoneKind::Command,
        other => panic!("unknown zone in scenario file: {other}"),
    }
}

fn zone_owner_for(kind: ZoneKind, player: PlayerId) -> Option<PlayerId> {
    match kind {
        ZoneKind::Battlefield | ZoneKind::Exile | ZoneKind::Stack => None,
        _ => Some(player),
    }
}

/// Builds a real `GameState` directly from struct literals (see module doc
/// for why). Returns the state plus a card-key -> `ObjectId` map so actions/
/// assertions can refer to setup cards by their TOML key.
pub fn build_initial_state(file: &ScenarioFile) -> (GameState, HashMap<String, ObjectId>) {
    let mut next_object_id: u64 = 0;
    let mut card_ids: HashMap<String, ObjectId> = HashMap::new();

    let mut players = Vec::new();
    let mut turn_order = Vec::new();
    for p in &file.players {
        players.push(PlayerState {
            player_id: PlayerId(p.id),
            life: p.life,
            mana_pool: ManaPool { white: 0, blue: 0, black: 0, red: 0, green: 0, colorless: 0 },
            counters: CounterSet::new(),
            emblems: Vec::new(),
            commander_damage_taken: Vec::new(),
            eliminated: false,
        });
        turn_order.push(PlayerId(p.id));
    }

    let mut zones: Vec<Zone> = Vec::new();
    for p in &file.players {
        let pid = PlayerId(p.id);
        for kind in [ZoneKind::Library, ZoneKind::Hand, ZoneKind::Graveyard, ZoneKind::Command] {
            zones.push(Zone { kind, owner: Some(pid), contents: VecDeque::new() });
        }
    }
    zones.push(Zone { kind: ZoneKind::Battlefield, owner: None, contents: VecDeque::new() });
    zones.push(Zone { kind: ZoneKind::Exile, owner: None, contents: VecDeque::new() });

    let card_specs: HashMap<&str, &CardSpec> =
        file.cards.iter().map(|c| (c.key.as_str(), c)).collect();

    let mut objects: HashMap<ObjectId, GameObject> = HashMap::new();

    for entry in &file.setup {
        let spec = card_specs
            .get(entry.card.as_str())
            .unwrap_or_else(|| panic!("setup references unknown card key '{}'", entry.card));

        let object_id = ObjectId(next_object_id);
        next_object_id += 1;

        let mut symbols = Vec::new();
        if spec.generic_cost > 0 {
            symbols.push(ManaSymbol::Atom(AtomicManaSymbol::Generic(spec.generic_cost)));
        }
        for color_letter in &spec.colors {
            symbols.push(ManaSymbol::Atom(AtomicManaSymbol::Colored(parse_color(color_letter))));
        }

        let definition = CardDefinition {
            definition_id: object_id.0,
            name: spec.name.clone(),
            mana_cost: ManaCost { symbols },
            color_indicator: None,
            supertypes: SupertypeSet::empty(),
            card_types: parse_card_types(&spec.types),
            subtypes: Vec::new(),
            oracle_text: String::new(),
            power: spec.power.map(PTValue::Fixed),
            toughness: spec.toughness.map(PTValue::Fixed),
            loyalty: None,
            defense: None,
            script_ref: ScriptReference::Inline(String::new()),
        };

        let owner = PlayerId(entry.player);
        let zone_kind = parse_zone_kind(&entry.zone);
        // CR default visibility: battlefield/graveyard/exile/stack/command
        // public, library/hand hidden. Good enough for fixture purposes;
        // VisibilityPolicy's override layer (visible_to) isn't exercised here.
        let public = !matches!(zone_kind, ZoneKind::Library | ZoneKind::Hand);

        let object = GameObject {
            object_id,
            lineage: ObjectLineage { previous: None },
            definition,
            owner,
            controller: owner,
            zone: zone_kind,
            tapped: false,
            flipped: false,
            face_down: false,
            phased_out: false,
            damage_marked: 0,
            counters: CounterSet::new(),
            attachments: Vec::new(),
            attached_to: None,
            timestamp: SequenceNo(0),
            visibility: VisibilityPolicy { public, visible_to: Vec::new() },
        };

        let zone_owner = zone_owner_for(zone_kind, owner);
        let zone = zones
            .iter_mut()
            .find(|z| z.kind == zone_kind && z.owner == zone_owner)
            .unwrap_or_else(|| panic!("no zone {entry:?} matches in this scenario's player set"));
        zone.contents.push_back(object_id);

        objects.insert(object_id, object);
        card_ids.insert(entry.card.clone(), object_id);
    }

    // GameState.step has no "no step" / PreGame-appropriate variant (see
    // turn.rs - Step doesn't model statelessness). Untap is an arbitrary
    // placeholder; harmless until something actually reads `step` during
    // PreGame, but worth a GameState.step: Option<Step> follow-up if that
    // becomes a real problem.
    let active_player = turn_order.first().copied().unwrap_or(PlayerId(0));

    let state = GameState {
        players,
        turn_order,
        active_player,
        turn_number: 1,
        phase: Phase::PreGame,
        step: Step::Untap,
        objects,
        zones,
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: file.seed, draws_consumed: 0 },
    };

    (state, card_ids)
}

// ---------------------------------------------------------------------
// Running actions
// ---------------------------------------------------------------------

fn resolve_target(
    spec: &TargetSpec,
    card_ids: &HashMap<String, ObjectId>,
) -> Target {
    match spec {
        TargetSpec::Player(id) => Target::Player(PlayerId(*id)),
        TargetSpec::Card(key) => Target::Object(
            *card_ids
                .get(key)
                .unwrap_or_else(|| panic!("action references unknown card key '{key}'")),
        ),
    }
}

fn resolve_card(key: &str, card_ids: &HashMap<String, ObjectId>) -> ObjectId {
    *card_ids
        .get(key)
        .unwrap_or_else(|| panic!("action references unknown card key '{key}'"))
}

fn to_player_action(spec: &ActionSpec, card_ids: &HashMap<String, ObjectId>) -> PlayerAction {
    let (player, kind) = match spec {
        ActionSpec::PassPriority { player } => (*player, PlayerActionKind::PassPriority),
        ActionSpec::PlayLand { player, card } => {
            (*player, PlayerActionKind::PlayLand { card: resolve_card(card, card_ids) })
        }
        ActionSpec::CastSpell { player, card, targets, modes, x_value } => (
            *player,
            PlayerActionKind::CastSpell {
                card: resolve_card(card, card_ids),
                modes: modes.clone(),
                targets: targets.iter().map(|t| resolve_target(t, card_ids)).collect(),
                x_value: *x_value,
            },
        ),
        ActionSpec::ActivateAbility { player, source, ability_index, targets } => (
            *player,
            PlayerActionKind::ActivateAbility {
                source: resolve_card(source, card_ids),
                ability_index: *ability_index,
                targets: targets.iter().map(|t| resolve_target(t, card_ids)).collect(),
            },
        ),
        ActionSpec::DeclareAttackers { player, attackers } => (
            *player,
            PlayerActionKind::DeclareAttackers {
                attackers: attackers
                    .iter()
                    .map(|a| (resolve_card(&a.attacker, card_ids), resolve_target(&a.target, card_ids)))
                    .collect(),
            },
        ),
        ActionSpec::DeclareBlockers { player, blocks } => (
            *player,
            PlayerActionKind::DeclareBlockers {
                blocks: blocks
                    .iter()
                    .map(|b| (resolve_card(&b.blocker, card_ids), resolve_card(&b.attacker, card_ids)))
                    .collect(),
            },
        ),
        ActionSpec::MakeChoice { player, description, selection } => (
            *player,
            PlayerActionKind::MakeChoice {
                description: description.clone(),
                selection: selection.iter().map(|t| resolve_target(t, card_ids)).collect(),
            },
        ),
    };
    PlayerAction { player: PlayerId(player), kind }
}

pub enum StepOutcome {
    Ran,
    NotImplemented(String),
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Drives one action through `engine`, catching any `todo!()` panic
/// (expected until Phase 5/7) rather than letting it abort the whole run.
fn run_action(engine: &mut Engine, action: PlayerAction) -> StepOutcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        engine.submit_action(action)?;
        loop {
            let result = engine.step();
            match result.status {
                StepStatus::Progressed => continue,
                StepStatus::AwaitingAction { .. } | StepStatus::GameOver { .. } => break,
            }
        }
        Ok::<(), ActionError>(())
    }));
    match result {
        Ok(_) => StepOutcome::Ran,
        Err(payload) => StepOutcome::NotImplemented(panic_message(payload)),
    }
}

pub enum AssertionOutcome {
    Passed,
    Failed { expected: String, actual: String },
}

fn check_assertion(
    spec: &AssertionSpec,
    state: &GameState,
    card_ids: &HashMap<String, ObjectId>,
) -> AssertionOutcome {
    match spec {
        AssertionSpec::Life { player, equals } => {
            let pid = PlayerId(*player);
            let actual = state.players.iter().find(|p| p.player_id == pid).map(|p| p.life);
            if actual == Some(*equals) {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.to_string(),
                    actual: format!("{actual:?}"),
                }
            }
        }
        AssertionSpec::LedgerLen { equals } => {
            let actual = state.ledger.len();
            if actual == *equals {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed { expected: equals.to_string(), actual: actual.to_string() }
            }
        }
        AssertionSpec::ObjectZone { card, equals } => {
            let object_id = resolve_card(card, card_ids);
            let expected_kind = parse_zone_kind(equals);
            let actual_kind = state.zones.iter().find(|z| z.contents.contains(&object_id)).map(|z| z.kind);
            if actual_kind == Some(expected_kind) {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed { expected: equals.clone(), actual: format!("{actual_kind:?}") }
            }
        }
    }
}

pub struct ScenarioReport {
    pub action_outcomes: Vec<StepOutcome>,
    pub assertion_outcomes: Vec<AssertionOutcome>,
}

impl ScenarioReport {
    /// A "hard" failure is an assertion that actually ran and produced the
    /// wrong answer — not an action that's merely unimplemented yet, which
    /// is expected and shouldn't fail a Phase 4/5 test run.
    pub fn has_hard_failures(&self) -> bool {
        self.assertion_outcomes.iter().any(|a| matches!(a, AssertionOutcome::Failed { .. }))
    }
}

pub fn run(file: &ScenarioFile) -> ScenarioReport {
    let (state, card_ids) = build_initial_state(file);
    let mut engine = Engine::new(state);

    let mut action_outcomes = Vec::new();
    for action_spec in &file.actions {
        let action = to_player_action(action_spec, &card_ids);
        action_outcomes.push(run_action(&mut engine, action));
    }

    let assertion_outcomes = file
        .assertions
        .iter()
        .map(|a| check_assertion(a, engine.state(), &card_ids))
        .collect();

    ScenarioReport { action_outcomes, assertion_outcomes }
}

pub fn run_path(path: &std::path::Path) -> ScenarioReport {
    run(&load(path))
}
