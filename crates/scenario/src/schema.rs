use serde::Deserialize;
use stackfish_engine::ObjectId;

#[derive(Deserialize)]
pub struct ScenarioFile {
    #[serde(default)]
    pub seed: Option<u64>,
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
    /// Actions and assertions in one ordered list. Assertion entries use `assert_`-prefixed
    /// type names. When non-empty, `actions` and `assertions` are ignored.
    #[serde(default)]
    pub steps: Vec<ScenarioStep>,
    /// When present, the scenario starts through `Engine::new_game` with real pregame
    /// automation, and `players` and `setup` are ignored.
    #[serde(default)]
    pub game: Option<GameSpec>,
    /// Path to the card-database SQLite file that a `CardSpec.db_script_id` reads from.
    /// A relative path resolves against `base_dir`; setting `db_script_id` without it panics.
    #[serde(default)]
    pub card_db: Option<String>,
    /// The scenario file's directory, set by `load()` and never by TOML. It lets `card_db`
    /// resolve against the file and not the process's working directory.
    #[serde(skip)]
    pub base_dir: Option<std::path::PathBuf>,
}

/// The `[game]` section, which starts the scenario through `Engine::new_game`.
#[derive(Deserialize)]
pub struct GameSpec {
    pub players: Vec<GamePlayerSpec>,
    /// Player ids in turn order. Defaults to the order of `players`.
    #[serde(default)]
    pub turn_order: Vec<u32>,
    /// Defaults to the top-level `seed`.
    pub rng_seed: Option<u64>,
    #[serde(default)]
    pub settings: GameSettingsSpec,
}

#[derive(Deserialize)]
pub struct GamePlayerSpec {
    pub id: u32,
    /// Card keys from `[[cards]]` in library order, top to bottom. A key may repeat.
    #[serde(default)]
    pub library: Vec<String>,
    /// Commander card keys, placed in the command zone. Several are legal for Partner. CR 903.6
    #[serde(default)]
    pub commanders: Vec<String>,
    /// Overrides `settings.starting_life` for this player.
    pub life_override: Option<u32>,
}

/// Every field is optional and falls back to `GameSettings::default()`.
#[derive(Deserialize, Default)]
pub struct GameSettingsSpec {
    pub shuffle_libraries: Option<bool>,
    pub starting_hand_size: Option<u8>,
    pub skip_mulligan: Option<bool>,
    pub free_mulligans: Option<u32>,
    /// Absent means unlimited.
    pub max_mulligans: Option<u32>,
    pub starting_life: Option<u32>,
    pub poison_loss_threshold: Option<u32>,
    pub max_hand_size: Option<u32>,
}

#[derive(Deserialize)]
pub struct PlayerSpec {
    pub id: u32,
    pub life: i32,
    /// Floating mana the player starts with, for scenarios that pay without mana abilities.
    #[serde(default)]
    pub mana_pool: ManaPoolSpec,
}

// Separate from `ManaPool` so the engine type needs no serde.
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

/// A card fixture. The cost is `generic_cost` plus color letters plus an optional bare
/// `{X}`, so hybrid and Phyrexian costs cannot be written here.
#[derive(Deserialize)]
pub struct CardSpec {
    pub key: String,
    /// Required unless `db_script_id` is set.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub generic_cost: u32,
    #[serde(default)]
    pub colors: Vec<String>,
    /// Adds a bare `{X}` to the cost, so `{X}{R}` is `has_x = true, colors = ["R"]`. Casting
    /// then needs an `x_value` on the `cast_spell` action. CR 107.3
    #[serde(default)]
    pub has_x: bool,
    /// Required unless `db_script_id` is set.
    #[serde(default)]
    pub types: Vec<String>,
    /// A `cards.script_id` slug (such as `"lightning_bolt"`) looked up in the `card_db` file.
    ///
    /// When set, every other characteristic field (`name`, costs, `types`, `supertypes`,
    /// `subtypes`, power and toughness, `keywords`, `target_requirement`) is ignored, not
    /// rejected. `script`, `mana_ability`, the trigger fields and `continuous_effect` still
    /// apply, so a scenario can attach a synthetic ability to a real card.
    #[serde(default)]
    pub db_script_id: Option<String>,
    /// Printed supertypes such as `["Legendary"]`. Not the same as
    /// `TargetRequirementSpec.supertypes`, which restricts targets. CR 205.4a
    #[serde(default)]
    pub supertypes: Vec<String>,
    /// Printed subtypes such as `["Human", "Wizard"]`. Unlike `types` and `supertypes`, an
    /// unrecognized name is accepted because the subtype space is open. CR 205.4b
    #[serde(default)]
    pub subtypes: Vec<String>,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    /// Lua source of a characteristic-defining power, `function(source, controller) -> integer`.
    /// When set, `power` is ignored. CR 604.3
    #[serde(default)]
    pub power_script: Option<String>,
    /// Same as `power_script`, for toughness.
    #[serde(default)]
    pub toughness_script: Option<String>,
    /// Mana units one activation adds, as ability index 0, for example `["G"]` for `{T}: Add {G}`.
    /// A card gets one mana ability this way. CR 605
    #[serde(default)]
    pub mana_ability: Vec<String>,
    /// Printed keywords by name, such as `["Flying", "Deathtouch"]`. CR 702
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Lua source of the one triggered ability's condition. Absent means it always triggers.
    #[serde(default)]
    pub trigger_condition: Option<String>,
    /// Lua source of the triggered ability's operation. It is what registers the ability, and
    /// a card gets one this way.
    #[serde(default)]
    pub trigger_operation: Option<String>,
    /// A full self-registering card script (`docs/lua/card-script-basics.md`). It runs through
    /// the engine pipeline when the card reaches the battlefield. Combining it with
    /// `trigger_condition` or `trigger_operation` is not rejected and not supported.
    #[serde(default)]
    pub script: Option<String>,
    /// One static continuous effect sourced from the card, built in Rust because Lua has no
    /// binding for it. CR 613
    #[serde(default)]
    pub continuous_effect: Option<ContinuousEffectSpec>,
    /// Casting needs exactly `count` targets matching the filter. CR 601.2c
    #[serde(default)]
    pub target_requirement: Option<TargetRequirementSpec>,
    /// One entry per mode of a modal spell. Empty means not modal. CR 700.2
    #[serde(default)]
    pub modes: Vec<SpellModeSpec>,
    /// How many of `modes` must be chosen. Ignored when `modes` is empty. CR 700.2d
    #[serde(default = "default_mode_count")]
    pub mode_count: u32,
    /// Choose up to `mode_count` instead of exactly. CR 700.2d
    #[serde(default)]
    pub mode_up_to: bool,
    /// The same mode may be chosen more than once. CR 700.2d
    #[serde(default)]
    pub mode_repeatable: bool,
    /// A pawprint budget in place of a count. When set, `mode_count`, `mode_up_to` and
    /// `mode_repeatable` are ignored. CR 700.2i
    #[serde(default)]
    pub mode_pawprint_budget: Option<u32>,
}

fn default_mode_count() -> u32 {
    1
}

/// One mode of `CardSpec.modes`. TOML cannot give a mode an effect, so a mode built here
/// resolves with `effect: None`.
#[derive(Deserialize)]
pub struct SpellModeSpec {
    /// Absent means the mode needs no targets.
    #[serde(default)]
    pub target_requirement: Option<TargetRequirementSpec>,
    /// The mode's pawprint weight, used with `CardSpec.mode_pawprint_budget`. CR 700.2i
    #[serde(default)]
    pub pawprint_weight: u32,
}

/// `CardSpec.continuous_effect`. `layer` picks the effect: `"type_changing"` (uses `types`),
/// `"color_changing"` (`colors`), `"ability_adding"` (`keywords`), and
/// `"power_toughness_setting"` or `"power_toughness_modifying"` (`power` and `toughness`).
/// The duration is permanent with timestamp 0.
#[derive(Deserialize)]
pub struct ContinuousEffectSpec {
    pub layer: String,
    #[serde(default)]
    pub power: i32,
    #[serde(default)]
    pub toughness: i32,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Card types added by a `type_changing` effect. CR 613.1
    #[serde(default)]
    pub types: Vec<String>,
    /// Colors added by a `color_changing` effect, using the `CardSpec.colors` letters. CR 613.1
    #[serde(default)]
    pub colors: Vec<String>,
    #[serde(default)]
    pub filter_controller: Option<String>,
    #[serde(default)]
    pub filter_card_types: Vec<String>,
    /// Matches any listed subtype. CR 205.4b
    #[serde(default)]
    pub filter_subtypes: Vec<String>,
    #[serde(default)]
    pub filter_excluded_subtypes: Vec<String>,
    /// Matches any listed supertype. CR 205.4a
    #[serde(default)]
    pub filter_supertypes: Vec<String>,
    #[serde(default)]
    pub filter_excluded_supertypes: Vec<String>,
    #[serde(default)]
    pub filter_self_only: bool,
    /// With `condition_count`, the effect applies only while its controller controls at least
    /// that many permanents of any of these types. CR 611.2c
    #[serde(default)]
    pub condition_card_types: Vec<String>,
    #[serde(default)]
    pub condition_count: Option<u32>,
}

/// `CardSpec.target_requirement`. `filter` picks the `TargetFilter`: `"object"` (uses
/// `card_types`, `subtypes`, `controller` and `zone`, defaulting to the battlefield),
/// `"player"`, `"any"`, `"ability"` or `"script"`. `count` means exact, "up to" or "at
/// least" depending on the mutually exclusive `up_to` and `at_least`. CR 115
#[derive(Deserialize)]
pub struct TargetRequirementSpec {
    pub filter: String,
    #[serde(default)]
    pub card_types: Vec<String>,
    #[serde(default)]
    pub excluded_card_types: Vec<String>,
    /// Matches any listed subtype. CR 205.4b
    #[serde(default)]
    pub subtypes: Vec<String>,
    #[serde(default)]
    pub excluded_subtypes: Vec<String>,
    /// CR 205.4a
    #[serde(default)]
    pub supertypes: Vec<String>,
    #[serde(default)]
    pub excluded_supertypes: Vec<String>,
    #[serde(default)]
    pub controller: Option<String>,
    #[serde(default)]
    pub zone: Option<String>,
    pub count: u32,
    #[serde(default)]
    pub up_to: bool,
    /// CR 601.2c
    #[serde(default)]
    pub at_least: bool,
    #[serde(default)]
    pub exclude_source: bool,
    /// Lua source `function(object, source, controller, state) -> bool`, used when
    /// `filter = "script"`. It loads after construction, once an engine exists.
    #[serde(default)]
    pub script: Option<String>,
}

// The `Pending*` structs record scripts that `build_initial_state` finds before any engine
// exists. `run()` loads them into the engine afterward.

/// A triggered ability to register, found while building the state.
pub struct PendingTriggerScript {
    pub source: ObjectId,
    pub condition: Option<String>,
    pub operation: String,
}

/// A `filter = "script"` target predicate whose placeholder handle `run()` patches.
pub struct PendingTargetScript {
    pub object: ObjectId,
    pub script: String,
}

/// A card whose own script declares its `target_requirement`. Collected only when the card
/// has no TOML `target_requirement` and resolved to a real script, from `CardSpec.script` or
/// a `db_script_id` lookup.
pub struct PendingScriptTargetRequirement {
    pub object: ObjectId,
    pub source: String,
}

/// A card whose script may declare its resolution effect through `operation()`. Every card
/// with a real script gets one, since `CardSpec` has no TOML field for an effect.
pub struct PendingScriptOperation {
    pub object: ObjectId,
    pub source: String,
}

#[derive(Deserialize, Debug)]
pub struct SetupEntry {
    pub player: u32,
    pub zone: String,
    pub card: String,
    /// Turn the object came under its controller's control. The default 0 means never
    /// summoning sick; set the current turn to make a sick creature. CR 302.6
    #[serde(default)]
    pub controlled_since_turn: u32,
    /// Pre-seeded damage, for state-based action scenarios. CR 704.5g
    #[serde(default)]
    pub damage_marked: u32,
    /// Marks the instance as its player's commander. Not checked against the zone. CR 903.3
    #[serde(default)]
    pub is_commander: bool,
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

/// One attacker's damage split among its blockers. Only attackers blocked by two or more
/// need an entry. CR 510.1c
#[derive(Deserialize)]
pub struct DamageAssignmentSpec {
    pub attacker: String,
    pub splits: Vec<DamageSplitSpec>,
}

#[derive(Deserialize)]
pub struct DamageSplitSpec {
    pub blocker: String,
    pub amount: u32,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionSpec {
    PassPriority {
        player: u32,
    },
    PlayLand {
        player: u32,
        card: String,
    },
    CastSpell {
        player: u32,
        card: String,
        #[serde(default)]
        targets: Vec<TargetSpec>,
        #[serde(default)]
        modes: Vec<u32>,
        x_value: Option<u32>,
        /// Pays automatically if the cast pauses for payment, so no `submit_payment` step is
        /// needed.
        #[serde(default)]
        auto_pay: bool,
    },
    ActivateAbility {
        player: u32,
        source: String,
        ability_index: u32,
        #[serde(default)]
        targets: Vec<TargetSpec>,
        /// One WUBRG letter per mana chosen, for an "any color" ability. CR 605
        #[serde(default)]
        chosen_colors: Vec<String>,
        /// X for an ability whose cost has `{X}`. CR 107.3a
        x_value: Option<u32>,
        /// Same shortcut as `CastSpell::auto_pay`.
        #[serde(default)]
        auto_pay: bool,
    },
    /// Shortcut for activating ability index 0 with no targets: tapping a land for its mana.
    TapForMana {
        player: u32,
        source: String,
    },
    /// Passes priority for whoever holds it until `state.phase` matches `phase`.
    AdvanceToPhase {
        phase: String,
    },
    /// Same as `AdvanceToPhase`, for `state.step`.
    AdvanceToStep {
        step: String,
    },
    DeclareAttackers {
        player: u32,
        #[serde(default)]
        attackers: Vec<AttackerSpec>,
    },
    /// Groups the declared attackers into bands, each inner list one band of card keys.
    /// CR 702.22c
    DeclareBands {
        player: u32,
        #[serde(default)]
        bands: Vec<Vec<String>>,
    },
    DeclareBlockers {
        player: u32,
        #[serde(default)]
        blocks: Vec<BlockSpec>,
    },
    AssignCombatDamage {
        player: u32,
        #[serde(default)]
        assignments: Vec<DamageAssignmentSpec>,
    },
    MakeChoice {
        player: u32,
        description: String,
        #[serde(default)]
        selection: Vec<TargetSpec>,
    },
    /// Answers a pending payment. `run()` handles it before `to_player_action`, since payment
    /// has its own engine entry point. An empty `mana` pays a `{0}` cost. CR 118.5a
    SubmitPayment {
        player: u32,
        #[serde(default)]
        mana: ManaPaymentSpec,
    },
    /// Abandons a pending payment through `Engine::unwind_payment`. CR 601.6
    UnwindPayment {
        player: u32,
    },
    /// Answers a pending discard with the card keys to discard. CR 514.1
    SubmitDiscard {
        player: u32,
        #[serde(default)]
        cards: Vec<String>,
    },
    /// Keeps the player's hand during pregame. CR 103.5
    KeepHand {
        player: u32,
    },
    /// Returns the hand and draws a fresh one. CR 103.5
    TakeMulligan {
        player: u32,
    },
    /// Bottoms `first_n` cards of the hand in zone order; 0 is valid only when none are owed.
    /// CR 103.5
    BottomCards {
        player: u32,
        #[serde(default)]
        first_n: usize,
    },
    /// Answers a script's mid-resolution target request from `Game.SelectTargets` or
    /// `Game.ChooseN`. `player` is not validated against the request.
    SubmitScriptChoice {
        player: u32,
        target: TargetSpec,
    },
}

fn describe_target(target: &TargetSpec) -> String {
    match target {
        TargetSpec::Player(id) => format!("player {id}"),
        TargetSpec::Card(key) => format!("{key:?}"),
    }
}

impl ActionSpec {
    /// A one-line description in TOML terms (card keys and player ids) for CLI output.
    pub fn describe(&self) -> String {
        match self {
            ActionSpec::PassPriority { player } => format!("pass_priority(player={player})"),
            ActionSpec::PlayLand { player, card } => {
                format!("play_land(player={player}, card={card:?})")
            }
            ActionSpec::CastSpell {
                player,
                card,
                targets,
                modes,
                x_value,
                auto_pay,
            } => {
                let targets = targets
                    .iter()
                    .map(describe_target)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "cast_spell(player={player}, card={card:?}, targets=[{targets}], modes={modes:?}, x_value={x_value:?}, auto_pay={auto_pay})"
                )
            }
            ActionSpec::ActivateAbility {
                player,
                source,
                ability_index,
                targets,
                chosen_colors,
                x_value,
                auto_pay,
            } => {
                let targets = targets
                    .iter()
                    .map(describe_target)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "activate_ability(player={player}, source={source:?}, ability_index={ability_index}, targets=[{targets}], chosen_colors={chosen_colors:?}, x_value={x_value:?}, auto_pay={auto_pay})"
                )
            }
            ActionSpec::TapForMana { player, source } => {
                format!("tap_for_mana(player={player}, source={source:?})")
            }
            ActionSpec::AdvanceToPhase { phase } => format!("advance_to_phase(phase={phase:?})"),
            ActionSpec::AdvanceToStep { step } => format!("advance_to_step(step={step:?})"),
            ActionSpec::DeclareAttackers { player, attackers } => {
                let attackers = attackers
                    .iter()
                    .map(|a| format!("{:?} -> {}", a.attacker, describe_target(&a.target)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("declare_attackers(player={player}, attackers=[{attackers}])")
            }
            ActionSpec::DeclareBands { player, bands } => {
                let bands = bands
                    .iter()
                    .map(|band| {
                        format!(
                            "[{}]",
                            band.iter()
                                .map(|c| format!("{c:?}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("declare_bands(player={player}, bands=[{bands}])")
            }
            ActionSpec::DeclareBlockers { player, blocks } => {
                let blocks = blocks
                    .iter()
                    .map(|b| format!("{:?} blocks {:?}", b.blocker, b.attacker))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("declare_blockers(player={player}, blocks=[{blocks}])")
            }
            ActionSpec::AssignCombatDamage {
                player,
                assignments,
            } => {
                let assignments = assignments
                    .iter()
                    .map(|a| {
                        let splits = a
                            .splits
                            .iter()
                            .map(|s| format!("{:?} <- {}", s.blocker, s.amount))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{:?} -> [{splits}]", a.attacker)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("assign_combat_damage(player={player}, assignments=[{assignments}])")
            }
            ActionSpec::MakeChoice {
                player,
                description,
                selection,
            } => {
                let selection = selection
                    .iter()
                    .map(describe_target)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "make_choice(player={player}, description={description:?}, selection=[{selection}])"
                )
            }
            ActionSpec::SubmitPayment { player, mana } => {
                format!("submit_payment(player={player}, units={:?})", mana.units)
            }
            ActionSpec::UnwindPayment { player } => format!("unwind_payment(player={player})"),
            ActionSpec::SubmitDiscard { player, cards } => {
                format!("submit_discard(player={player}, cards={cards:?})")
            }
            ActionSpec::SubmitScriptChoice { player, target } => {
                format!(
                    "submit_script_choice(player={player}, target={})",
                    describe_target(target)
                )
            }
            ActionSpec::KeepHand { player } => format!("keep_hand(player={player})"),
            ActionSpec::TakeMulligan { player } => format!("take_mulligan(player={player})"),
            ActionSpec::BottomCards { player, first_n } => {
                format!("bottom_cards(player={player}, first_n={first_n})")
            }
        }
    }
}

/// The mana a `SubmitPayment` action spends: one literal unit per entry (`"W"`, `"U"`, `"B"`,
/// `"R"`, `"G"` or `"C"`), covering colored pips and generic mana alike. `PaymentPlan::validate`
/// works out which unit pays which part. CR 118.13a
#[derive(Deserialize, Default)]
pub struct ManaPaymentSpec {
    #[serde(default)]
    pub units: Vec<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssertionSpec {
    Life {
        player: u32,
        equals: i32,
    },
    LedgerLen {
        equals: usize,
    },
    ObjectZone {
        card: String,
        equals: String,
    },
    /// One pool field by name: `white`, `blue`, `black`, `red`, `green` or `colorless`.
    ManaPool {
        player: u32,
        color: String,
        equals: u32,
    },
    Tapped {
        card: String,
        equals: bool,
    },
    Eliminated {
        player: u32,
        equals: bool,
    },
    Phase {
        equals: String,
    },
    Step {
        equals: String,
    },
    ActivePlayer {
        equals: u32,
    },
    PriorityPlayer {
        equals: u32,
    },
    HandSize {
        player: u32,
        equals: usize,
    },
    /// A counter count by kind; see `parse_counter_kind` for the names.
    ObjectCounters {
        card: String,
        kind: String,
        equals: i32,
    },
    PlayerCounters {
        player: u32,
        kind: String,
        equals: i32,
    },
    /// Combat damage a player took from one commander. CR 903.10a
    CommanderDamage {
        player: u32,
        source_card: String,
        equals: u32,
    },
    /// The full membership of `card`'s band by key, order-independent and including `card`.
    /// `[]` asserts `card` is in no band. CR 702.22c
    BandMembers {
        card: String,
        equals: Vec<String>,
    },
}

/// `AssertionSpec` with `assert_`-prefixed type names, for `[[steps]]` where actions and
/// assertions share one array.
#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum AssertionStepSpec {
    #[serde(rename = "assert_life")]
    Life { player: u32, equals: i32 },
    #[serde(rename = "assert_ledger_len")]
    LedgerLen { equals: usize },
    #[serde(rename = "assert_object_zone")]
    ObjectZone { card: String, equals: String },
    #[serde(rename = "assert_mana_pool")]
    ManaPool {
        player: u32,
        color: String,
        equals: u32,
    },
    #[serde(rename = "assert_tapped")]
    Tapped { card: String, equals: bool },
    #[serde(rename = "assert_eliminated")]
    Eliminated { player: u32, equals: bool },
    #[serde(rename = "assert_phase")]
    Phase { equals: String },
    #[serde(rename = "assert_step")]
    Step { equals: String },
    #[serde(rename = "assert_active_player")]
    ActivePlayer { equals: u32 },
    #[serde(rename = "assert_priority_player")]
    PriorityPlayer { equals: u32 },
    #[serde(rename = "assert_hand_size")]
    HandSize { player: u32, equals: usize },
    #[serde(rename = "assert_object_counters")]
    ObjectCounters {
        card: String,
        kind: String,
        equals: i32,
    },
    #[serde(rename = "assert_player_counters")]
    PlayerCounters {
        player: u32,
        kind: String,
        equals: i32,
    },
    #[serde(rename = "assert_commander_damage")]
    CommanderDamage {
        player: u32,
        source_card: String,
        equals: u32,
    },
    #[serde(rename = "assert_band_members")]
    BandMembers { card: String, equals: Vec<String> },
}

impl AssertionStepSpec {
    pub(crate) fn to_assertion_spec(&self) -> AssertionSpec {
        match self {
            Self::Life { player, equals } => AssertionSpec::Life {
                player: *player,
                equals: *equals,
            },
            Self::LedgerLen { equals } => AssertionSpec::LedgerLen { equals: *equals },
            Self::ObjectZone { card, equals } => AssertionSpec::ObjectZone {
                card: card.clone(),
                equals: equals.clone(),
            },
            Self::ManaPool {
                player,
                color,
                equals,
            } => AssertionSpec::ManaPool {
                player: *player,
                color: color.clone(),
                equals: *equals,
            },
            Self::Tapped { card, equals } => AssertionSpec::Tapped {
                card: card.clone(),
                equals: *equals,
            },
            Self::Eliminated { player, equals } => AssertionSpec::Eliminated {
                player: *player,
                equals: *equals,
            },
            Self::Phase { equals } => AssertionSpec::Phase {
                equals: equals.clone(),
            },
            Self::Step { equals } => AssertionSpec::Step {
                equals: equals.clone(),
            },
            Self::ActivePlayer { equals } => AssertionSpec::ActivePlayer { equals: *equals },
            Self::PriorityPlayer { equals } => AssertionSpec::PriorityPlayer { equals: *equals },
            Self::HandSize { player, equals } => AssertionSpec::HandSize {
                player: *player,
                equals: *equals,
            },
            Self::ObjectCounters { card, kind, equals } => AssertionSpec::ObjectCounters {
                card: card.clone(),
                kind: kind.clone(),
                equals: *equals,
            },
            Self::PlayerCounters {
                player,
                kind,
                equals,
            } => AssertionSpec::PlayerCounters {
                player: *player,
                kind: kind.clone(),
                equals: *equals,
            },
            Self::CommanderDamage {
                player,
                source_card,
                equals,
            } => AssertionSpec::CommanderDamage {
                player: *player,
                source_card: source_card.clone(),
                equals: *equals,
            },
            Self::BandMembers { card, equals } => AssertionSpec::BandMembers {
                card: card.clone(),
                equals: equals.clone(),
            },
        }
    }
}

/// One entry of `[[steps]]`: an assertion if its `type` starts with `assert_`, an action
/// otherwise. The `Deserialize` impl is hand-written because an untagged enum would report
/// only "data did not match any variant", with no field or step.
pub enum ScenarioStep {
    Action {
        spec: ActionSpec,
        /// Inverts the step: the engine rejecting the action passes, and accepting it fails.
        /// It checks that an action is illegal without a dedicated assertion.
        expect_error: bool,
    },
    Assertion(AssertionStepSpec),
}

impl<'de> serde::Deserialize<'de> for ScenarioStep {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let value = toml::Value::deserialize(deserializer)?;
        let step_type = value
            .get("type")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| D::Error::custom("scenario step is missing its `type` field"))?
            .to_string();

        if step_type.starts_with("assert_") {
            AssertionStepSpec::deserialize(value)
                .map(ScenarioStep::Assertion)
                .map_err(|e| D::Error::custom(format!("step `{step_type}`: {e}")))
        } else {
            let spec = ActionSpec::deserialize(value.clone())
                .map_err(|e| D::Error::custom(format!("step `{step_type}`: {e}")))?;
            let expect_error = value
                .get("expect_error")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
            Ok(ScenarioStep::Action { spec, expect_error })
        }
    }
}

impl AssertionSpec {
    /// A one-line description in TOML terms, like `ActionSpec::describe`.
    pub fn describe(&self) -> String {
        match self {
            AssertionSpec::Life { player, equals } => format!("life(player={player}) == {equals}"),
            AssertionSpec::LedgerLen { equals } => format!("ledger_len == {equals}"),
            AssertionSpec::ObjectZone { card, equals } => {
                format!("object_zone(card={card:?}) == {equals:?}")
            }
            AssertionSpec::ManaPool {
                player,
                color,
                equals,
            } => {
                format!("mana_pool(player={player}, color={color}) == {equals}")
            }
            AssertionSpec::Tapped { card, equals } => format!("tapped(card={card:?}) == {equals}"),
            AssertionSpec::Eliminated { player, equals } => {
                format!("eliminated(player={player}) == {equals}")
            }
            AssertionSpec::Phase { equals } => format!("phase == {equals:?}"),
            AssertionSpec::Step { equals } => format!("step == {equals:?}"),
            AssertionSpec::ActivePlayer { equals } => format!("active_player == {equals}"),
            AssertionSpec::PriorityPlayer { equals } => format!("priority_player == {equals}"),
            AssertionSpec::HandSize { player, equals } => {
                format!("hand_size(player={player}) == {equals}")
            }
            AssertionSpec::ObjectCounters { card, kind, equals } => {
                format!("object_counters(card={card:?}, kind={kind:?}) == {equals}")
            }
            AssertionSpec::PlayerCounters {
                player,
                kind,
                equals,
            } => {
                format!("player_counters(player={player}, kind={kind:?}) == {equals}")
            }
            AssertionSpec::CommanderDamage {
                player,
                source_card,
                equals,
            } => {
                format!(
                    "commander_damage(player={player}, source_card={source_card:?}) == {equals}"
                )
            }
            AssertionSpec::BandMembers { card, equals } => {
                format!("band_members(card={card:?}) == {equals:?}")
            }
        }
    }
}
