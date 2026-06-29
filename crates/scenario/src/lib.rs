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
    /// Fake, pre-seeded floating mana — CR 605 mana abilities (tapping a
    /// land, etc) don't exist yet, so this is the only way a scenario can
    /// give a player something to pay a cost with. See
    /// docs/implementation.md's cast-spell-payment slice notes.
    #[serde(default)]
    pub mana_pool: ManaPoolSpec,
}

/// Mirrors `ManaPool`'s six fields one-to-one — kept separate (rather than
/// deriving `Deserialize` on `ManaPool` itself) so the engine's own type
/// doesn't need to depend on `serde`.
#[derive(Deserialize, Default)]
pub struct ManaPoolSpec {
    #[serde(default)]
    pub white: u32,
    #[serde(default)]
    pub blue: u32,
    #[serde(default)]
    pub black: u32,
    #[serde(default)]
    pub red: u32,
    #[serde(default)]
    pub green: u32,
    #[serde(default)]
    pub colorless: u32,
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
    /// CR 601.2f-h: answers a pending `ProcessState::AwaitingPayment` pause.
    /// Deliberately NOT routed through `to_player_action`/`submit_action` -
    /// `Engine::submit_payment` is its own entry point (see engine.rs), so
    /// `run()` intercepts this variant before `to_player_action` ever sees
    /// it. `mana` defaults to empty, which is the correct shape for paying
    /// a `{0}` cost (CR 118.5a still requires an explicit empty plan).
    SubmitPayment {
        player: u32,
        #[serde(default)]
        mana: ManaPaymentSpec,
    },
}

fn describe_target(target: &TargetSpec) -> String {
    match target {
        TargetSpec::Player(id) => format!("player {id}"),
        TargetSpec::Card(key) => format!("{key:?}"),
    }
}

impl ActionSpec {
    /// Human-readable one-liner for CLI/log output - written directly off
    /// the TOML-facing fields (card keys, raw player ids), not the engine's
    /// own ids, so it reads the same as the scenario file itself.
    pub fn describe(&self) -> String {
        match self {
            ActionSpec::PassPriority { player } => format!("pass_priority(player={player})"),
            ActionSpec::PlayLand { player, card } => {
                format!("play_land(player={player}, card={card:?})")
            }
            ActionSpec::CastSpell { player, card, targets, modes, x_value } => {
                let targets = targets.iter().map(describe_target).collect::<Vec<_>>().join(", ");
                format!(
                    "cast_spell(player={player}, card={card:?}, targets=[{targets}], modes={modes:?}, x_value={x_value:?})"
                )
            }
            ActionSpec::ActivateAbility { player, source, ability_index, targets } => {
                let targets = targets.iter().map(describe_target).collect::<Vec<_>>().join(", ");
                format!(
                    "activate_ability(player={player}, source={source:?}, ability_index={ability_index}, targets=[{targets}])"
                )
            }
            ActionSpec::DeclareAttackers { player, attackers } => {
                let attackers = attackers
                    .iter()
                    .map(|a| format!("{:?} -> {}", a.attacker, describe_target(&a.target)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("declare_attackers(player={player}, attackers=[{attackers}])")
            }
            ActionSpec::DeclareBlockers { player, blocks } => {
                let blocks = blocks
                    .iter()
                    .map(|b| format!("{:?} blocks {:?}", b.blocker, b.attacker))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("declare_blockers(player={player}, blocks=[{blocks}])")
            }
            ActionSpec::MakeChoice { player, description, selection } => {
                let selection = selection.iter().map(describe_target).collect::<Vec<_>>().join(", ");
                format!(
                    "make_choice(player={player}, description={description:?}, selection=[{selection}])"
                )
            }
            ActionSpec::SubmitPayment { player, mana } => {
                format!("submit_payment(player={player}, units={:?})", mana.units)
            }
        }
    }
}

/// What a `SubmitPayment` action pays *with* — CR 118.13a's payment choice:
/// a flat list of literal mana units actually spent (`"W"`/`"U"`/`"B"`/
/// `"R"`/`"G"` for a colored unit, `"C"` for colorless), one entry per unit,
/// covering both colored pips and whatever the payer chose to spend on the
/// generic component. Not split into separate "pip" vs "generic" fields —
/// `PaymentPlan::validate` (`cost.rs`) figures out which units satisfy which
/// requirement from the flat list itself; the scenario author just states
/// what came out of the pool, same as a real player/UI would.
#[derive(Deserialize, Default)]
pub struct ManaPaymentSpec {
    #[serde(default)]
    pub units: Vec<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssertionSpec {
    Life { player: u32, equals: i32 },
    LedgerLen { equals: usize },
    ObjectZone { card: String, equals: String },
    /// Checks one `ManaPool` field by name (`white`/`blue`/`black`/`red`/
    /// `green`/`colorless`) - the way to confirm a `SubmitPayment` action
    /// actually debited what was expected.
    ManaPool { player: u32, color: String, equals: u32 },
    Phase { equals: String },
    Step { equals: String },
    ActivePlayer { equals: u32 },
    PriorityPlayer { equals: u32 },
}

impl AssertionSpec {
    /// Human-readable one-liner, same rationale as `ActionSpec::describe`.
    pub fn describe(&self) -> String {
        match self {
            AssertionSpec::Life { player, equals } => format!("life(player={player}) == {equals}"),
            AssertionSpec::LedgerLen { equals } => format!("ledger_len == {equals}"),
            AssertionSpec::ObjectZone { card, equals } => {
                format!("object_zone(card={card:?}) == {equals:?}")
            }
            AssertionSpec::ManaPool { player, color, equals } => {
                format!("mana_pool(player={player}, color={color}) == {equals}")
            }
            AssertionSpec::Phase { equals } => format!("phase == {equals:?}"),
            AssertionSpec::Step { equals } => format!("step == {equals:?}"),
            AssertionSpec::ActivePlayer { equals } => format!("active_player == {equals}"),
            AssertionSpec::PriorityPlayer { equals } => format!("priority_player == {equals}"),
        }
    }
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

/// `CardSpec`'s simplified generic+colors cost shape, as `ManaSymbol`s.
/// `ManaPaymentSpec` (what `SubmitPayment` pays *with*) deliberately does
/// NOT share this helper - a payment is a flat list of literal units (CR
/// 118.13a's choice), not an echo of the cost's shape. See `parse_mana_unit`
/// and `build_payment_plan` for that side.
fn mana_symbols(generic_cost: u32, colors: &[String]) -> Vec<ManaSymbol> {
    let mut symbols = Vec::new();
    if generic_cost > 0 {
        symbols.push(ManaSymbol::Atom(AtomicManaSymbol::Generic(generic_cost)));
    }
    for color_letter in colors {
        symbols.push(ManaSymbol::Atom(AtomicManaSymbol::Colored(parse_color(color_letter))));
    }
    symbols
}

/// One entry from `ManaPaymentSpec.units` - `"W"`/`"U"`/`"B"`/`"R"`/`"G"`
/// reuse `parse_color`, `"C"` is colorless (the one case `parse_color`
/// itself doesn't handle, since a card's printed cost colors never include
/// colorless mana symbols the way a payment can spend colorless units).
fn parse_mana_unit(letter: &str) -> ManaUnit {
    match letter {
        "C" => ManaUnit::Colorless,
        other => ManaUnit::Colored(parse_color(other)),
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

fn parse_phase(name: &str) -> Phase {
    match name {
        "pre_game" => Phase::PreGame,
        "beginning" => Phase::Beginning,
        "main" => Phase::Main,
        "combat" => Phase::Combat,
        "ending" => Phase::Ending,
        other => panic!("unknown phase in scenario file: {other}"),
    }
}

fn parse_step(name: &str) -> Step {
    match name {
        "untap" => Step::Untap,
        "upkeep" => Step::Upkeep,
        "draw" => Step::Draw,
        "pre_combat_main" => Step::PreCombatMain,
        "beginning_of_combat" => Step::BeginningOfCombat,
        "declare_attackers" => Step::DeclareAttackers,
        "declare_blockers" => Step::DeclareBlockers,
        "combat_damage" => Step::CombatDamage,
        "end_of_combat" => Step::EndOfCombat,
        "post_combat_main" => Step::PostCombatMain,
        "end" => Step::End,
        "cleanup" => Step::Cleanup,
        other => panic!("unknown step in scenario file: {other}"),
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
            mana_pool: ManaPool {
                white: p.mana_pool.white,
                blue: p.mana_pool.blue,
                black: p.mana_pool.black,
                red: p.mana_pool.red,
                green: p.mana_pool.green,
                colorless: p.mana_pool.colorless,
            },
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

        let symbols = mana_symbols(spec.generic_cost, &spec.colors);

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
        priority_player: active_player,
        consecutive_passes: 0,
        lands_played_this_turn: HashMap::new(),
        objects,
        zones,
        stack: Vec::new(),
        continuous_effects: Vec::new(),
        replacement_effects: Vec::new(),
        abilities: Vec::new(),
        ledger: Vec::new(),
        rng: RngState { seed: file.seed, draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id,
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
        // SubmitPayment never reaches here - run() intercepts it and calls
        // Engine::submit_payment directly (see that variant's doc comment).
        ActionSpec::SubmitPayment { .. } => {
            unreachable!("SubmitPayment is handled in run(), before to_player_action")
        }
    };
    PlayerAction { player: PlayerId(player), kind }
}

/// `mana.units`, as a `PaymentPlan` - an empty `units` (the default)
/// produces the empty plan CR 118.5a requires for a `{0}` cost; anything
/// else produces the single `CostPayment::Mana` entry `PaymentPlan::validate`
/// expects (see cost.rs's doc comment on why it's exactly one entry, not
/// per-color).
fn build_payment_plan(mana: &ManaPaymentSpec) -> PaymentPlan {
    if mana.units.is_empty() {
        PaymentPlan { payments: Vec::new() }
    } else {
        let units = mana.units.iter().map(|s| parse_mana_unit(s)).collect();
        PaymentPlan { payments: vec![CostPayment::Mana(units)] }
    }
}

pub enum StepOutcome {
    Ran,
    NotImplemented(String),
    /// The engine accepted the action but a command failed mid-pipeline
    /// (`StepStatus::Error`) — distinct from a panic (`NotImplemented`):
    /// this means real code ran and rejected the action on purpose.
    Errored(String),
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

/// Steps `engine` until it pauses for the next player decision (or errors/
/// ends the game) - shared by `run_action` (after `submit_action`) and
/// `run_payment` (after `submit_payment`), since both resume the same
/// pipeline-stepping loop once their respective submission succeeds.
fn drain(engine: &mut Engine) -> StepOutcome {
    loop {
        let result = engine.step();
        match result.status {
            StepStatus::Progressed => continue,
            StepStatus::AwaitingAction { .. }
            | StepStatus::AwaitingReplacementChoice { .. }
            | StepStatus::AwaitingTriggerOrderChoice { .. }
            | StepStatus::AwaitingPayment { .. }
            | StepStatus::GameOver { .. } => {
                return StepOutcome::Ran;
            }
            StepStatus::Error(err) => return StepOutcome::Errored(format!("{err:?}")),
        }
    }
}

/// Drives one action through `engine`, catching any `todo!()` panic
/// (expected until Phase 5/7) rather than letting it abort the whole run.
fn run_action(engine: &mut Engine, action: PlayerAction) -> StepOutcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> StepOutcome {
        if let Err(err) = engine.submit_action(action) {
            return StepOutcome::Errored(format!("{err:?}"));
        }
        drain(engine)
    }));
    match result {
        Ok(outcome) => outcome,
        Err(payload) => StepOutcome::NotImplemented(panic_message(payload)),
    }
}

/// Same shape as `run_action`, but for the `SubmitPayment` action - answers
/// a `ProcessState::AwaitingPayment` pause via `Engine::submit_payment`
/// rather than `submit_action` (see `ActionSpec::SubmitPayment`'s doc
/// comment for why it can't go through `to_player_action`/`run_action`).
fn run_payment(engine: &mut Engine, plan: PaymentPlan) -> StepOutcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> StepOutcome {
        if let Err(err) = engine.submit_payment(plan) {
            return StepOutcome::Errored(format!("{err:?}"));
        }
        drain(engine)
    }));
    match result {
        Ok(outcome) => outcome,
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
        AssertionSpec::ManaPool { player, color, equals } => {
            let pid = PlayerId(*player);
            let pool = state.players.iter().find(|p| p.player_id == pid).map(|p| &p.mana_pool);
            let actual = pool.map(|pool| match color.as_str() {
                "white" => pool.white,
                "blue" => pool.blue,
                "black" => pool.black,
                "red" => pool.red,
                "green" => pool.green,
                "colorless" => pool.colorless,
                other => panic!("unknown mana_pool color in scenario file: {other}"),
            });
            if actual == Some(*equals) {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed { expected: equals.to_string(), actual: format!("{actual:?}") }
            }
        }
        AssertionSpec::Phase { equals } => {
            let expected = parse_phase(equals);
            if state.phase == expected {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed { expected: equals.clone(), actual: format!("{:?}", state.phase) }
            }
        }
        AssertionSpec::Step { equals } => {
            let expected = parse_step(equals);
            if state.step == expected {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed { expected: equals.clone(), actual: format!("{:?}", state.step) }
            }
        }
        AssertionSpec::ActivePlayer { equals } => {
            let expected = PlayerId(*equals);
            if state.active_player == expected {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.to_string(),
                    actual: format!("{:?}", state.active_player),
                }
            }
        }
        AssertionSpec::PriorityPlayer { equals } => {
            let expected = PlayerId(*equals);
            if state.priority_player == expected {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed {
                    expected: equals.to_string(),
                    actual: format!("{:?}", state.priority_player),
                }
            }
        }
    }
}

pub struct ScenarioReport {
    pub action_outcomes: Vec<StepOutcome>,
    /// `ActionSpec::describe()` for each entry in `action_outcomes`, same
    /// index - lets a caller (the CLI) say what actually ran, not just that
    /// something at index N did.
    pub action_descriptions: Vec<String>,
    pub assertion_outcomes: Vec<AssertionOutcome>,
    /// `AssertionSpec::describe()` for each entry in `assertion_outcomes`,
    /// same index.
    pub assertion_descriptions: Vec<String>,
}

impl ScenarioReport {
    /// A "hard" failure is an assertion that actually ran and produced the
    /// wrong answer — not an action that's merely unimplemented yet, which
    /// is expected and shouldn't fail a Phase 4/5 test run.
    pub fn has_hard_failures(&self) -> bool {
        self.assertion_outcomes.iter().any(|a| matches!(a, AssertionOutcome::Failed { .. }))
    }
}

/// CR 400.7: `PlayLand`/`CastSpell`/resolution all mint a *new* `ObjectId`
/// for the moved object rather than reusing the old one - so a card's entry
/// in `card_ids` (keyed by its TOML key, set once at setup time) goes stale
/// the moment that card changes zones. Without this, any scenario action or
/// assertion referencing a card by key after it's been cast/played would
/// silently resolve to the wrong (or a now-nonexistent) id. Scans the ledger
/// events `engine` just produced and follows each `ZoneChange`'s
/// `last_known_info` (the pre-move snapshot, which still carries the old
/// id) forward to its replacement.
fn remap_moved_card_ids(card_ids: &mut HashMap<String, ObjectId>, new_events: &[GameEvent]) {
    for event in new_events {
        if let GameEventKind::ZoneChange { object: new_id, last_known_info: Some(old), .. } =
            &event.kind
        {
            for tracked_id in card_ids.values_mut() {
                if *tracked_id == old.object_id {
                    *tracked_id = *new_id;
                }
            }
        }
    }
}

pub fn run(file: &ScenarioFile) -> ScenarioReport {
    let (state, mut card_ids) = build_initial_state(file);
    let mut engine = Engine::new(state);

    let mut action_outcomes = Vec::new();
    let mut action_descriptions = Vec::new();
    for action_spec in &file.actions {
        let ledger_before = engine.state().ledger.len();
        let outcome = match action_spec {
            ActionSpec::SubmitPayment { mana, .. } => {
                run_payment(&mut engine, build_payment_plan(mana))
            }
            other => run_action(&mut engine, to_player_action(other, &card_ids)),
        };
        remap_moved_card_ids(&mut card_ids, &engine.state().ledger[ledger_before..]);
        action_outcomes.push(outcome);
        action_descriptions.push(action_spec.describe());
    }

    let assertion_outcomes = file
        .assertions
        .iter()
        .map(|a| check_assertion(a, engine.state(), &card_ids))
        .collect();
    let assertion_descriptions = file.assertions.iter().map(|a| a.describe()).collect();

    ScenarioReport { action_outcomes, action_descriptions, assertion_outcomes, assertion_descriptions }
}

pub fn run_path(path: &std::path::Path) -> ScenarioReport {
    run(&load(path))
}
