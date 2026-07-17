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
use stackfish_engine::engine::{ActionError, DiscardError, Engine, StepStatus};
use stackfish_engine::types::config::{GameConfig, GameSettings, PlayerConfig};

// ---------------------------------------------------------------------
// TOML schema
// ---------------------------------------------------------------------

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
    /// Unified ordered sequence of actions and assertions — use instead of
    /// `actions`/`assertions` when you need assertions interleaved with actions.
    /// Each action entry uses the same type names as `[[actions]]`; each
    /// assertion entry uses `assert_`-prefixed names (e.g. `assert_life`,
    /// `assert_phase`) so assertions are visually distinct in the file.
    /// If non-empty, `actions` and `assertions` are ignored.
    #[serde(default)]
    pub steps: Vec<ScenarioStep>,
    /// If present, the scenario uses `Engine::new_game(GameConfig)` instead of
    /// the manual `build_initial_state` path. Enables real pregame automation
    /// (shuffle, draw, mulligan) with full `GameSettings` control. The
    /// `[[players]]` / `[[setup]]` sections are ignored when `[game]` is set.
    #[serde(default)]
    pub game: Option<GameSpec>,
}

/// Top-level `[game]` section — triggers the `Engine::new_game` path.
#[derive(Deserialize)]
pub struct GameSpec {
    /// Players and their starting libraries. Each player's `library` is a list
    /// of card keys from `[[cards]]` (may repeat for multiple copies).
    pub players: Vec<GamePlayerSpec>,
    /// Turn order as player ids. Defaults to `players` order if absent.
    #[serde(default)]
    pub turn_order: Vec<u32>,
    /// RNG seed for shuffles. Defaults to the top-level `seed` field.
    pub rng_seed: Option<u64>,
    /// Game rule overrides. Any omitted field inherits `GameSettings::default()`.
    #[serde(default)]
    pub settings: GameSettingsSpec,
}

#[derive(Deserialize)]
pub struct GamePlayerSpec {
    pub id: u32,
    /// Card keys (from `[[cards]]`) in library order, top to bottom. May repeat.
    #[serde(default)]
    pub library: Vec<String>,
    /// CR 903.6: card keys (from `[[cards]]`) for this player's commander(s)
    /// — placed in the command zone by `Engine::new_game` instead of being
    /// shuffled into `library`. A list (not a single key) since Partner
    /// commanders mean more than one is legal. Empty (the default) for a
    /// non-Commander game.
    #[serde(default)]
    pub commanders: Vec<String>,
    /// Overrides `settings.starting_life` for this player only.
    pub life_override: Option<u32>,
}

/// All fields optional — each overrides the corresponding `GameSettings::default()`.
/// Omitted fields keep the default value.
#[derive(Deserialize, Default)]
pub struct GameSettingsSpec {
    /// Shuffle each player's library before drawing (default: `true`).
    pub shuffle_libraries: Option<bool>,
    /// Target hand size for opening draw and redraws (default: `7`).
    pub starting_hand_size: Option<u8>,
    /// Auto-keep all hands without any mulligan decisions (default: `false`).
    pub skip_mulligan: Option<bool>,
    /// Mulligans that don't cost a card on the bottom (default: `0`).
    pub free_mulligans: Option<u32>,
    /// Maximum mulligans allowed. `None`/absent = unlimited (default).
    pub max_mulligans: Option<u32>,
    /// Starting life total (default: `20`).
    pub starting_life: Option<u32>,
    /// Poison counters needed to lose (default: `10`).
    pub poison_loss_threshold: Option<u32>,
    /// Max hand size enforced at cleanup (default: `7`).
    pub max_hand_size: Option<u32>,
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
    /// CR 604.3/208.2a: if set, this card's power is characteristic-defining
    /// (CR 613 layer 7a) rather than fixed — `power` above is ignored when
    /// this is set (a card is one or the other, not both; not rejected if
    /// somehow both are set, same "not a supported combination" stance
    /// `script`'s own doc comment below already takes for its overlap).
    /// Raw Lua source, same `function(source, controller) -> integer`
    /// convention `ScriptRuntime::invoke_cda` expects — same raw-source-
    /// string convention `trigger_condition`/`trigger_operation` already use.
    #[serde(default)]
    pub power_script: Option<String>,
    /// Same as `power_script`, for toughness.
    #[serde(default)]
    pub toughness_script: Option<String>,
    /// CR 605: if non-empty, every instance of this card is set up with one
    /// `RegisteredAbility { kind: Activated, effect: Some(AddMana(...)) }` —
    /// `ability_index = 0` in an `activate_ability` action. Each entry is one
    /// literal unit added when activated (same letters as `mana.units` on a
    /// `submit_payment` action, see `parse_mana_unit`) — e.g. `["G"]` for
    /// `{T}: Add {G}`. Only one mana ability per card this way; a card
    /// needing more than one (or a non-mana ability) needs `CardSpec`
    /// extended further.
    #[serde(default)]
    pub mana_ability: Vec<String>,
    /// CR 702 printed keywords, by name (e.g. `["Flying", "Deathtouch"]") —
    /// parsed into a `KeywordSet` via `parse_keywords`. Only the primitives
    /// `KeywordSet` itself models; see that type's doc comment.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Real Lua source for this card's one triggered ability's condition
    /// callback (see `ScriptRuntime::invoke_condition`). `None` means "no
    /// filter, always triggers" once `trigger_operation` is set — same rule
    /// `RegisteredAbility::condition` documents.
    #[serde(default)]
    pub trigger_condition: Option<String>,
    /// Real Lua source for this card's one triggered ability's operation
    /// callback (see `ScriptRuntime::invoke_operation`). Presence of this
    /// field is what registers the ability at all — `trigger_condition`
    /// alone does nothing without it. Only one triggered ability per card
    /// this way, same single-ability-per-card simplification `mana_ability`
    /// already uses for activated abilities.
    #[serde(default)]
    pub trigger_operation: Option<String>,
    /// A full self-registering card script — the `s.initial_effect(c)`
    /// module convention (see `docs/lua_scripts.md`), calling
    /// `Game.CreateTriggeredAbility(c)` itself rather than the harness
    /// building a `RegisteredAbility` by hand. Sets `CardDefinition::
    /// script_ref` to `ScriptReference::Inline(script)` directly - unlike
    /// `trigger_condition`/`trigger_operation`, this needs no post-
    /// construction `Engine`/`ScriptRuntime` step: the real engine pipeline
    /// (`PlayLand`/`ResolveStackTop`) runs it automatically once the card
    /// actually reaches the battlefield. Mutually exclusive in practice with
    /// `trigger_condition`/`trigger_operation` (both can be set, but nothing
    /// stops a card from having both a self-registered ability and a
    /// harness-wired one - not a supported combination, just not rejected).
    #[serde(default)]
    pub script: Option<String>,
    /// CR 613: if set, every instance of this card is set up with one static
    /// `ContinuousEffect` sourced from it, built directly as a Rust struct —
    /// there's no `Game.AddContinuousEffect` Lua binding yet (that's its own
    /// future slice, static-ability self-registration — see
    /// docs/implementation.md's slice backlog). Only one continuous effect
    /// per card this way, same single-per-card simplification
    /// `mana_ability`/`trigger_operation` already use.
    #[serde(default)]
    pub continuous_effect: Option<ContinuousEffectSpec>,
    /// CR 601.2c: if set, casting this card as a spell requires exactly
    /// `count` targets matching `filter` — see `TargetRequirementSpec`'s own
    /// doc comment. `None` means no targets required (the common case).
    #[serde(default)]
    pub target_requirement: Option<TargetRequirementSpec>,
}

/// `CardSpec.continuous_effect`'s shape — `layer` picks which of the layer
/// engine's built primitives (`layers.rs`) this effect is: `"type_changing"`
/// (uses `types`), `"color_changing"` (uses `colors`), `"ability_adding"`
/// (uses `keywords`), `"power_toughness_setting"` / `"power_toughness_modifying"`
/// (both use `power`/`toughness`). Always `EffectDuration::Permanent`,
/// timestamp `0` - ordering-within-a-layer fixtures aren't exercised via
/// TOML, only via the Rust integration tests in `phase5_layer_engine_basics.rs`.
#[derive(Deserialize)]
pub struct ContinuousEffectSpec {
    pub layer: String,
    #[serde(default)]
    pub power: i32,
    #[serde(default)]
    pub toughness: i32,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// `type_changing` only — card types (e.g. `["Creature"]`) this effect
    /// grants in addition to whatever the affected object already has (CR
    /// 613.1 layer 4, additive only — see `EffectPrimitive::AddCardType`).
    #[serde(default)]
    pub types: Vec<String>,
    /// `color_changing` only — colors (e.g. `["R"]`, same letters as
    /// `CardSpec.colors`) this effect grants in addition to whatever the
    /// affected object already has (CR 613.1 layer 5, additive only — see
    /// `EffectPrimitive::AddColor`).
    #[serde(default)]
    pub colors: Vec<String>,
    #[serde(default)]
    pub filter_controller: Option<String>,
    #[serde(default)]
    pub filter_card_types: Vec<String>,
    #[serde(default)]
    pub filter_self_only: bool,
}

/// `CardSpec.target_requirement`'s shape — `filter` picks which
/// `TargetFilter` variant this is: `"object"` (uses `card_types`/
/// `controller`/`zone`, CR 115.2's battlefield default applies when `zone`
/// is unset), `"player"` (any player), or `"any"` (CR 115.4's "any target" —
/// creature/player/planeswalker/battle). `count` is always exact — see
/// `TargetRequirement`'s own "up to N" TODO for why.
#[derive(Deserialize)]
pub struct TargetRequirementSpec {
    pub filter: String,
    #[serde(default)]
    pub card_types: Vec<String>,
    #[serde(default)]
    pub controller: Option<String>,
    #[serde(default)]
    pub zone: Option<String>,
    pub count: u32,
}

/// One card instance's triggered ability, collected by `build_initial_state`
/// but not registered there — no `Engine`/`ScriptRuntime` exists yet at that
/// point (it only builds a `GameState`). `run()` loads the Lua source into
/// the `Engine` that wraps this state and pushes the real `RegisteredAbility`
/// afterward (see `register_trigger_ability`).
pub struct PendingTriggerScript {
    pub source: ObjectId,
    pub condition: Option<String>,
    pub operation: String,
}

#[derive(Deserialize, Debug)]
pub struct SetupEntry {
    pub player: u32,
    pub zone: String,
    pub card: String,
    /// CR 302.6/602.5a's summoning-sickness clock. Defaults to `0`, which is
    /// always less than any real `turn_number` (the game's first turn seeds
    /// `last_active_turn_number` at `1`) - i.e. by default a setup card is
    /// never summoning sick. Set this to the scenario's current/future
    /// `turn_number` to build a fixture that *is* sick (a creature that just
    /// entered the battlefield this turn).
    #[serde(default)]
    pub controlled_since_turn: u32,
    /// CR 704.5g fixture seed: no combat-damage step exists yet to produce
    /// this for real (see docs/implementation.md's SBA-basics slice notes),
    /// so a scenario testing lethal-damage SBA has to pre-seed it directly,
    /// same as `controlled_since_turn` stands in for a missing "gain
    /// control" producer.
    #[serde(default)]
    pub damage_marked: u32,
    /// CR 903.3: marks this specific setup instance as its player's
    /// commander. Only meaningful paired with `zone = "command"`, but not
    /// enforced here — casting/legality checks are the engine's job, not
    /// the scenario loader's (same "engine trusts the setup" stance every
    /// other `[[setup]]` field already takes).
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

/// One attacker's CR 510.1c split among its 2+ blockers - only attackers
/// actually double(+)-blocked need an entry (see
/// `PlayerActionKind::AssignCombatDamage`'s doc comment).
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
        /// CR 605's "add mana of any color" choice, one WUBRG letter per
        /// mana chosen - empty for every ability that isn't
        /// `AbilityEffect::AddManaChoice`. Reuses `mana::parse_color_letter`
        /// rather than a second parser.
        #[serde(default)]
        chosen_colors: Vec<String>,
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
    /// CR 601.6: abandons a pending `ProcessState::AwaitingPayment` pause
    /// instead of paying it — same "own entry point, not
    /// `to_player_action`/`submit_action`" rationale as `SubmitPayment`,
    /// routed through `Engine::unwind_payment`.
    UnwindPayment { player: u32 },
    /// CR 514.1: answers a pending `ProcessState::AwaitingDiscard` pause —
    /// `cards` is the list of TOML card keys the player chooses to discard.
    /// Same "own entry point" rationale as `SubmitPayment` — routed through
    /// `run_discard` / `Engine::submit_discard`, not `submit_action`.
    SubmitDiscard {
        player: u32,
        #[serde(default)]
        cards: Vec<String>,
    },
    /// CR 103.5: keep this player's current hand during pregame. Routes through
    /// `Engine::submit_mulligan_decision`, not `submit_action`.
    KeepHand { player: u32 },
    /// CR 103.5: return this player's hand and draw a fresh one. Routes through
    /// `Engine::submit_mulligan_decision`.
    TakeMulligan { player: u32 },
    /// CR 103.5: bottom `first_n` cards from the player's hand (taken in hand
    /// zone order). Routes through `Engine::submit_bottom_cards`. Use
    /// `first_n = 0` to bottom nothing (only valid when count == 0).
    BottomCards { player: u32, #[serde(default)] first_n: usize },
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
            ActionSpec::ActivateAbility { player, source, ability_index, targets, chosen_colors } => {
                let targets = targets.iter().map(describe_target).collect::<Vec<_>>().join(", ");
                format!(
                    "activate_ability(player={player}, source={source:?}, ability_index={ability_index}, targets=[{targets}], chosen_colors={chosen_colors:?})"
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
            ActionSpec::AssignCombatDamage { player, assignments } => {
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
            ActionSpec::MakeChoice { player, description, selection } => {
                let selection = selection.iter().map(describe_target).collect::<Vec<_>>().join(", ");
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
            ActionSpec::KeepHand { player } => format!("keep_hand(player={player})"),
            ActionSpec::TakeMulligan { player } => format!("take_mulligan(player={player})"),
            ActionSpec::BottomCards { player, first_n } => {
                format!("bottom_cards(player={player}, first_n={first_n})")
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
    /// Checks `GameObject.tapped` — the way to confirm an `activate_ability`
    /// action (CR 605's tap-for-mana case) actually tapped its source.
    Tapped { card: String, equals: bool },
    /// Checks `PlayerState.eliminated` — the way to confirm CR 704.5a's SBA
    /// actually fired for a player at 0 or less life.
    Eliminated { player: u32, equals: bool },
    Phase { equals: String },
    Step { equals: String },
    ActivePlayer { equals: u32 },
    PriorityPlayer { equals: u32 },
    /// Checks the number of cards in a player's hand zone.
    HandSize { player: u32, equals: usize },
    /// Checks a `GameObject.counters` count by kind - the way to confirm
    /// Wither/Infect actually put `-1/-1` counters on a creature (see
    /// `parse_counter_kind` for supported names).
    ObjectCounters { card: String, kind: String, equals: i32 },
    /// Checks a `PlayerState.counters` count by kind - the way to confirm
    /// Infect actually gave a player poison counters instead of life loss.
    PlayerCounters { player: u32, kind: String, equals: i32 },
}

/// Same assertions as `AssertionSpec` but with `assert_` prefixed type names,
/// for use in `[[steps]]` where actions and assertions share one array — the
/// prefix makes each entry's role obvious when reading the file.
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
    ManaPool { player: u32, color: String, equals: u32 },
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
    ObjectCounters { card: String, kind: String, equals: i32 },
    #[serde(rename = "assert_player_counters")]
    PlayerCounters { player: u32, kind: String, equals: i32 },
}

impl AssertionStepSpec {
    fn to_assertion_spec(&self) -> AssertionSpec {
        match self {
            Self::Life { player, equals } => AssertionSpec::Life { player: *player, equals: *equals },
            Self::LedgerLen { equals } => AssertionSpec::LedgerLen { equals: *equals },
            Self::ObjectZone { card, equals } => AssertionSpec::ObjectZone { card: card.clone(), equals: equals.clone() },
            Self::ManaPool { player, color, equals } => AssertionSpec::ManaPool { player: *player, color: color.clone(), equals: *equals },
            Self::Tapped { card, equals } => AssertionSpec::Tapped { card: card.clone(), equals: *equals },
            Self::Eliminated { player, equals } => AssertionSpec::Eliminated { player: *player, equals: *equals },
            Self::Phase { equals } => AssertionSpec::Phase { equals: equals.clone() },
            Self::Step { equals } => AssertionSpec::Step { equals: equals.clone() },
            Self::ActivePlayer { equals } => AssertionSpec::ActivePlayer { equals: *equals },
            Self::PriorityPlayer { equals } => AssertionSpec::PriorityPlayer { equals: *equals },
            Self::HandSize { player, equals } => AssertionSpec::HandSize { player: *player, equals: *equals },
            Self::ObjectCounters { card, kind, equals } => {
                AssertionSpec::ObjectCounters { card: card.clone(), kind: kind.clone(), equals: *equals }
            }
            Self::PlayerCounters { player, kind, equals } => {
                AssertionSpec::PlayerCounters { player: *player, kind: kind.clone(), equals: *equals }
            }
        }
    }
}

/// One entry in `[[steps]]` — either an action or an assertion. Serde tries
/// `Action` first (matching any `ActionSpec` type name), then `Assertion`
/// (matching any `assert_*` type name). No type names overlap between the two.
#[derive(Deserialize)]
#[serde(untagged)]
pub enum ScenarioStep {
    Action(ActionSpec),
    Assertion(AssertionStepSpec),
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
            AssertionSpec::PlayerCounters { player, kind, equals } => {
                format!("player_counters(player={player}, kind={kind:?}) == {equals}")
            }
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

/// `CardSpec.power`/`.power_script` (or the `toughness` pair) -> a
/// `PTValue` — a script, if present, wins over a plain fixed value (see
/// `CardSpec.power_script`'s doc comment on why that overlap isn't
/// rejected). `display` is always `"*"` for a scripted value — scenario
/// fixtures have no use for real oracle-text display, it's UI-only and
/// unconsulted by anything this crate checks.
fn build_pt(value: Option<i32>, script: &Option<String>) -> Option<PTValue> {
    if let Some(source) = script {
        return Some(PTValue::CharacteristicDefining {
            display: "*".to_string(),
            script: Some(ScriptReference::Inline(source.clone())),
        });
    }
    value.map(PTValue::Fixed)
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

/// `ContinuousEffectSpec.colors` entries, by the same `"W"/"U"/"B"/"R"/"G"`
/// letters as `CardSpec.colors`, into a `ColorSet` (unlike `parse_color`,
/// which returns one `Color` — a `color_changing` effect can grant more
/// than one color at once).
fn parse_colors(letters: &[String]) -> ColorSet {
    let mut set = ColorSet::empty();
    for letter in letters {
        set |= match parse_color(letter) {
            Color::White => ColorSet::WHITE,
            Color::Blue => ColorSet::BLUE,
            Color::Black => ColorSet::BLACK,
            Color::Red => ColorSet::RED,
            Color::Green => ColorSet::GREEN,
        };
    }
    set
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

/// `CardSpec.keywords` entries, by CR 702 name, into `KeywordSet` bits.
fn parse_keywords(names: &[String]) -> KeywordSet {
    let mut set = KeywordSet::empty();
    for name in names {
        set |= match name.as_str() {
            "Flying" => KeywordSet::FLYING,
            "Reach" => KeywordSet::REACH,
            "Menace" => KeywordSet::MENACE,
            "Vigilance" => KeywordSet::VIGILANCE,
            "Haste" => KeywordSet::HASTE,
            "Defender" => KeywordSet::DEFENDER,
            "Flash" => KeywordSet::FLASH,
            "Deathtouch" => KeywordSet::DEATHTOUCH,
            "Indestructible" => KeywordSet::INDESTRUCTIBLE,
            "Lifelink" => KeywordSet::LIFELINK,
            "Wither" => KeywordSet::WITHER,
            "Infect" => KeywordSet::INFECT,
            "FirstStrike" => KeywordSet::FIRST_STRIKE,
            "DoubleStrike" => KeywordSet::DOUBLE_STRIKE,
            other => panic!("unknown keyword in scenario file: {other}"),
        };
    }
    set
}

fn parse_filter_controller(name: &str) -> FilterController {
    match name {
        "you" => FilterController::You,
        "opponent" => FilterController::Opponent,
        "any" => FilterController::Any,
        other => panic!("unknown filter_controller in scenario file: {other}"),
    }
}

/// `CardSpec.continuous_effect` -> a real `ContinuousEffect` sourced from
/// `source`. `spec.layer` selects which of `layers.rs`'s three built
/// primitives this is — see `ContinuousEffectSpec`'s own doc comment.
fn build_continuous_effect(source: ObjectId, spec: &ContinuousEffectSpec) -> ContinuousEffect {
    let layer = match spec.layer.as_str() {
        "type_changing" => Layer::TypeChanging,
        "color_changing" => Layer::ColorChanging,
        "ability_adding" => Layer::AbilityAdding,
        "power_toughness_setting" => Layer::PowerToughnessSetting,
        "power_toughness_modifying" => Layer::PowerToughnessModifying,
        other => panic!(
            "unsupported continuous_effect layer in scenario file: {other} - only \
             type_changing/color_changing/ability_adding/power_toughness_setting/\
             power_toughness_modifying are wired to the layer engine so far (see layers.rs)"
        ),
    };
    let effect = match layer {
        Layer::TypeChanging => EffectPrimitive::AddCardType(parse_card_types(&spec.types)),
        Layer::ColorChanging => EffectPrimitive::AddColor(parse_colors(&spec.colors)),
        Layer::AbilityAdding => EffectPrimitive::AddKeyword(parse_keywords(&spec.keywords)),
        Layer::PowerToughnessSetting => {
            EffectPrimitive::SetPT { power: spec.power, toughness: spec.toughness }
        }
        Layer::PowerToughnessModifying => {
            EffectPrimitive::ModifyPT { power: spec.power, toughness: spec.toughness }
        }
        _ => unreachable!("layer already matched to one of the five arms above"),
    };
    let filter = EffectFilter {
        controller: spec.filter_controller.as_deref().map(parse_filter_controller),
        card_types: (!spec.filter_card_types.is_empty()).then(|| parse_card_types(&spec.filter_card_types)),
        self_only: spec.filter_self_only,
    };
    ContinuousEffect {
        source,
        layer,
        affected: AffectedFilter::Primitive(filter),
        effect: EffectPayload::Primitive(effect),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
    }
}

/// `CardSpec.target_requirement` -> a real `TargetSpec::Primitive`.
/// Fully-qualified as `stackfish_engine::TargetSpec` - this crate already
/// has its own unrelated `TargetSpec` (the TOML-facing `Player(u32)`/
/// `Card(String)` reference used by `AttackerSpec`/`MakeChoice` actions,
/// resolved separately via `resolve_target`), and that local definition
/// shadows the glob-imported engine one by Rust's name-resolution rules.
/// See `TargetRequirementSpec`'s own doc comment for what each field means.
fn build_target_requirement(spec: &TargetRequirementSpec) -> stackfish_engine::TargetSpec {
    let filter = match spec.filter.as_str() {
        "object" => TargetFilter::Object {
            card_types: (!spec.card_types.is_empty()).then(|| parse_card_types(&spec.card_types)),
            controller: spec.controller.as_deref().map(parse_filter_controller),
            zone: spec.zone.as_deref().map(parse_zone_kind),
        },
        "player" => TargetFilter::Player,
        "any" => TargetFilter::AnyTarget,
        other => panic!(
            "unknown target_requirement filter in scenario file: {other} - expected \
             object/player/any"
        ),
    };
    stackfish_engine::TargetSpec::Primitive(TargetRequirement { filter, count: spec.count })
}

/// `assert_object_counters`/`assert_player_counters`'s `kind` field, by CR
/// 122.3 name, into `CounterKind`.
fn parse_counter_kind(name: &str) -> CounterKind {
    match name {
        "plus_one_plus_one" => CounterKind::PlusOnePlusOne,
        "minus_one_minus_one" => CounterKind::MinusOneMinusOne,
        "loyalty" => CounterKind::Loyalty,
        "poison" => CounterKind::Poison,
        "energy" => CounterKind::Energy,
        "experience" => CounterKind::Experience,
        "charge" => CounterKind::Charge,
        other => CounterKind::Custom(other.to_string()),
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
pub fn build_initial_state(
    file: &ScenarioFile,
) -> (GameState, HashMap<String, ObjectId>, Vec<PendingTriggerScript>) {
    let mut next_object_id: u64 = 0;
    let mut card_ids: HashMap<String, ObjectId> = HashMap::new();
    let mut trigger_scripts: Vec<PendingTriggerScript> = Vec::new();

    let mut players = Vec::new();
    let mut turn_order = Vec::new();
    for p in &file.players {
        players.push(PlayerState {
            player_id: PlayerId(p.id),
            life: p.life,
            // Snow-sourced mana isn't representable in scenario fixtures yet
            // (same gap CardSpec's own doc comment flags for hybrid/
            // Phyrexian) - snow_* fields default to 0 via the spread.
            mana_pool: ManaPool {
                white: p.mana_pool.white,
                blue: p.mana_pool.blue,
                black: p.mana_pool.black,
                red: p.mana_pool.red,
                green: p.mana_pool.green,
                colorless: p.mana_pool.colorless,
                ..ManaPool::default()
            },
            counters: CounterSet::new(),
            emblems: Vec::new(),
            commander_damage_taken: Vec::new(),
            eliminated: false,
            drew_from_empty_library: false,
            mulligans_taken: 0,
            hand_kept: false,
            commander_casts: std::collections::HashMap::new(),
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
    let mut abilities: Vec<RegisteredAbility> = Vec::new();
    let mut continuous_effects: Vec<ContinuousEffect> = Vec::new();

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
            power: build_pt(spec.power, &spec.power_script),
            toughness: build_pt(spec.toughness, &spec.toughness_script),
            loyalty: None,
            defense: None,
            script_ref: ScriptReference::Inline(spec.script.clone().unwrap_or_default()),
            keywords: parse_keywords(&spec.keywords),
            target_requirement: spec.target_requirement.as_ref().map(build_target_requirement),
            alternative_costs: Vec::new(),
            effect: None,
            // TODO: no TOML schema for modal cards yet (CardSpec has no
            // `[[cards.modes]]`-style field) - same pre-existing gap as
            // `effect` above having no TOML representation either. A modal
            // test fixture needs a direct CardDefinition struct literal.
            modes: Vec::new(),
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
            damage_marked: entry.damage_marked,
            deathtouched: false,
            counters: CounterSet::new(),
            attachments: Vec::new(),
            attached_to: None,
            timestamp: SequenceNo(0),
            controlled_since_turn: entry.controlled_since_turn,
            is_token: false,
            is_commander: entry.is_commander,
            visibility: VisibilityPolicy { public, visible_to: Vec::new() },
        };

        let zone_owner = zone_owner_for(zone_kind, owner);
        let zone = zones
            .iter_mut()
            .find(|z| z.kind == zone_kind && z.owner == zone_owner)
            .unwrap_or_else(|| panic!("no zone {entry:?} matches in this scenario's player set"));
        zone.contents.push_back(object_id);

        if !spec.mana_ability.is_empty() {
            let units = spec.mana_ability.iter().map(|s| parse_mana_unit(s)).collect();
            abilities.push(RegisteredAbility {
                source: object_id,
                kind: AbilityKind::Activated,
                condition: None,
                cost: None,
                target: None,
                operation: ScriptHandle(0),
                effect: Some(AbilityEffect::AddMana(units)),
                intervening_if: None,
            });
        }
        if let Some(operation) = &spec.trigger_operation {
            trigger_scripts.push(PendingTriggerScript {
                source: object_id,
                condition: spec.trigger_condition.clone(),
                operation: operation.clone(),
            });
        }
        if let Some(effect_spec) = &spec.continuous_effect {
            continuous_effects.push(build_continuous_effect(object_id, effect_spec));
        }

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
        combat: None,
        // CR 302.6/602.5a: mirrors what engine::Engine::advance_turn_step's
        // PreGame arm seeds on a real first step() - this harness builds
        // GameState directly rather than stepping through that transition,
        // so it has to seed the same fact itself or every setup card would
        // look summoning sick by default.
        last_active_turn_number: HashMap::from([(active_player, 1)]),
        objects,
        zones,
        stack: Vec::new(),
        continuous_effects,
        replacement_effects: Vec::new(),
        cost_modifiers: Vec::new(),
        abilities,
        ledger: Vec::new(),
        rng: RngState { seed: file.seed.expect("`seed` is required when `[game]` is not set"), draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: 0,
        next_object_id,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
    };

    (state, card_ids, trigger_scripts)
}

/// Loads `condition_src`/`operation_src` into `engine`'s script runtime and
/// pushes the resulting `RegisteredAbility` — shared by `run()`'s
/// `build_initial_state` branch and `build_new_game_engine`'s per-library
/// loop so both scenario formats support `trigger_condition`/
/// `trigger_operation` the same way.
fn register_trigger_ability(
    engine: &mut Engine,
    source: ObjectId,
    condition_src: Option<&str>,
    operation_src: &str,
) {
    let (effect, operation) = engine
        .load_operation(operation_src)
        .unwrap_or_else(|e| panic!("trigger_operation script failed to load: {e:?}"));
    let condition = condition_src.map(|src| {
        engine
            .load_condition(src)
            .unwrap_or_else(|e| panic!("trigger_condition script failed to load: {e:?}"))
    });
    engine.state_mut().abilities.push(RegisteredAbility {
        source,
        kind: AbilityKind::Triggered,
        condition,
        cost: None,
        target: None,
        operation,
        effect,
        intervening_if: None,
    });
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
                // TODO: no TOML scenario field for this yet - see the
                // matching TODO on CardDefinition.alternative_costs above.
                alternative_cost: None,
            },
        ),
        ActionSpec::ActivateAbility { player, source, ability_index, targets, chosen_colors } => (
            *player,
            PlayerActionKind::ActivateAbility {
                source: resolve_card(source, card_ids),
                ability_index: *ability_index,
                targets: targets.iter().map(|t| resolve_target(t, card_ids)).collect(),
                chosen_colors: chosen_colors
                    .iter()
                    .map(|s| stackfish_engine::types::mana::parse_color_letter(s).expect("invalid chosen color letter in scenario TOML"))
                    .collect(),
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
        ActionSpec::AssignCombatDamage { player, assignments } => (
            *player,
            PlayerActionKind::AssignCombatDamage {
                assignments: assignments
                    .iter()
                    .map(|a| {
                        let splits = a
                            .splits
                            .iter()
                            .map(|s| (resolve_card(&s.blocker, card_ids), s.amount))
                            .collect();
                        (resolve_card(&a.attacker, card_ids), splits)
                    })
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
        // These variants never reach here — run_action_spec intercepts them
        // and calls their dedicated Engine entry points.
        ActionSpec::SubmitPayment { .. } => {
            unreachable!("SubmitPayment is handled in run_action_spec, before to_player_action")
        }
        ActionSpec::UnwindPayment { .. } => {
            unreachable!("UnwindPayment is handled in run_action_spec, before to_player_action")
        }
        ActionSpec::SubmitDiscard { .. } => {
            unreachable!("SubmitDiscard is handled in run_action_spec, before to_player_action")
        }
        ActionSpec::KeepHand { .. } | ActionSpec::TakeMulligan { .. } | ActionSpec::BottomCards { .. } => {
            unreachable!("pregame actions are handled in run_action_spec, before to_player_action")
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
        // Phyrexian life payment isn't representable in scenario fixtures
        // yet (see CardSpec's own doc comment on this same gap) - always 0.
        PaymentPlan { payments: vec![CostPayment::Mana { units, phyrexian_life: 0 }] }
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
            | StepStatus::AwaitingLegendRuleChoice { .. }
            | StepStatus::AwaitingCommanderReturnChoice { .. }
            | StepStatus::AwaitingScriptChoice { .. }
            | StepStatus::AwaitingScryChoice { .. }
            | StepStatus::AwaitingSurveilChoice { .. }
            | StepStatus::AwaitingSearchChoice { .. }
            | StepStatus::AwaitingTriggerOrderChoice { .. }
            | StepStatus::AwaitingPayment { .. }
            | StepStatus::AwaitingDiscard { .. }
            | StepStatus::AwaitingMulliganDecision { .. }
            | StepStatus::AwaitingBottomCardChoice { .. }
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

/// Same shape as `run_payment`, but for the `UnwindPayment` action — answers
/// a `ProcessState::AwaitingPayment` pause via `Engine::unwind_payment`
/// instead of `Engine::submit_payment`.
fn run_unwind_payment(engine: &mut Engine) -> StepOutcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> StepOutcome {
        if let Err(err) = engine.unwind_payment() {
            return StepOutcome::Errored(format!("{err:?}"));
        }
        drain(engine)
    }));
    match result {
        Ok(outcome) => outcome,
        Err(payload) => StepOutcome::NotImplemented(panic_message(payload)),
    }
}

/// Same shape as `run_payment`, but for the `SubmitDiscard` action — answers
/// a `ProcessState::AwaitingDiscard` pause via `Engine::submit_discard`.
fn run_discard(engine: &mut Engine, cards: Vec<ObjectId>) -> StepOutcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> StepOutcome {
        if let Err(err) = engine.submit_discard(cards) {
            return StepOutcome::Errored(format!("{err:?}"));
        }
        drain(engine)
    }));
    match result {
        Ok(outcome) => outcome,
        Err(payload) => StepOutcome::NotImplemented(panic_message(payload)),
    }
}

/// Answers a `ProcessState::AwaitingMulliganDecision` pause via
/// `Engine::submit_mulligan_decision`.
fn run_mulligan(engine: &mut Engine, action: PlayerAction) -> StepOutcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> StepOutcome {
        if let Err(err) = engine.submit_mulligan_decision(action) {
            return StepOutcome::Errored(format!("{err:?}"));
        }
        drain(engine)
    }));
    match result {
        Ok(outcome) => outcome,
        Err(payload) => StepOutcome::NotImplemented(panic_message(payload)),
    }
}

/// Answers a `ProcessState::AwaitingBottomCardChoice` pause via
/// `Engine::submit_bottom_cards`. Takes the first `first_n` cards from the
/// player's hand zone (in order) rather than asking the caller to name
/// specific `ObjectId`s — library cards created by `new_game` don't have
/// TOML keys, so positional selection is the practical choice here.
fn run_bottom_cards(engine: &mut Engine, player: PlayerId, first_n: usize) -> StepOutcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> StepOutcome {
        let cards: Vec<ObjectId> = engine
            .state()
            .zone(ZoneKind::Hand, Some(player))
            .map_or(Vec::new(), |z| z.contents.iter().copied().take(first_n).collect());
        let action = PlayerAction {
            player,
            kind: PlayerActionKind::BottomCards { cards },
        };
        if let Err(err) = engine.submit_bottom_cards(action) {
            return StepOutcome::Errored(format!("{err:?}"));
        }
        drain(engine)
    }));
    match result {
        Ok(outcome) => outcome,
        Err(payload) => StepOutcome::NotImplemented(panic_message(payload)),
    }
}

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
fn build_new_game_engine(file: &ScenarioFile, game: &GameSpec) -> (Engine, HashMap<String, ObjectId>) {
    let card_specs: HashMap<&str, &CardSpec> =
        file.cards.iter().map(|c| (c.key.as_str(), c)).collect();

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
            let symbols = mana_symbols(spec.generic_cost, &spec.colors);
            CardDefinition {
                definition_id: 0,
                name: spec.name.clone(),
                mana_cost: ManaCost { symbols },
                color_indicator: None,
                supertypes: SupertypeSet::empty(),
                card_types: parse_card_types(&spec.types),
                subtypes: Vec::new(),
                oracle_text: String::new(),
                power: build_pt(spec.power, &spec.power_script),
                toughness: build_pt(spec.toughness, &spec.toughness_script),
                loyalty: None,
                defense: None,
                script_ref: ScriptReference::Inline(spec.script.clone().unwrap_or_default()),
                keywords: parse_keywords(&spec.keywords),
                target_requirement: spec.target_requirement.as_ref().map(build_target_requirement),
                // TODO: no TOML scenario field for alternative costs yet -
                // flashback/escape-style cards aren't constructible via a
                // scenario fixture this slice, only via direct struct
                // literals in Rust integration tests (see
                // phase5_flashback_alternative_costs.rs).
                alternative_costs: Vec::new(),
                // TODO: no TOML scenario field for a Rust-native effect
                // (Game.Ops.Damage/GainLife/...) yet either - same
                // "direct struct literal only" limit as alternative_costs
                // above.
                effect: None,
                // TODO: no TOML scenario field for modal spells (CR 700.2)
                // either - same "direct struct literal only" limit.
                modes: Vec::new(),
            }
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
            let symbols = mana_symbols(spec.generic_cost, &spec.colors);
            CardDefinition {
                definition_id: 0,
                name: spec.name.clone(),
                mana_cost: ManaCost { symbols },
                color_indicator: None,
                supertypes: SupertypeSet::empty(),
                card_types: parse_card_types(&spec.types),
                subtypes: Vec::new(),
                oracle_text: String::new(),
                power: build_pt(spec.power, &spec.power_script),
                toughness: build_pt(spec.toughness, &spec.toughness_script),
                loyalty: None,
                defense: None,
                script_ref: ScriptReference::Inline(spec.script.clone().unwrap_or_default()),
                keywords: parse_keywords(&spec.keywords),
                target_requirement: spec.target_requirement.as_ref().map(build_target_requirement),
                alternative_costs: Vec::new(),
                effect: None,
                modes: Vec::new(),
            }
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
                engine.state_mut().abilities.push(RegisteredAbility {
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
        }
    }
    (engine, card_ids)
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
            // The stack isn't one of state.zones (it needs StackObject
            // metadata - targets/modes/costs - not just an id list, see
            // docs/implementation.md's zone notes), so a card sitting on
            // the stack would never be found by the state.zones lookup
            // alone. Fall back to state.stack so `equals = "stack"`
            // actually means something here.
            let actual_kind = state
                .zones
                .iter()
                .find(|z| z.contents.contains(&object_id))
                .map(|z| z.kind)
                .or_else(|| {
                    state.stack.iter().any(|s| s.stack_id == object_id).then_some(ZoneKind::Stack)
                });
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
        AssertionSpec::Tapped { card, equals } => {
            let object_id = resolve_card(card, card_ids);
            let actual = state.object(object_id).map(|o| o.tapped);
            if actual == Some(*equals) {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed { expected: equals.to_string(), actual: format!("{actual:?}") }
            }
        }
        AssertionSpec::Eliminated { player, equals } => {
            let pid = PlayerId(*player);
            let actual = state.players.iter().find(|p| p.player_id == pid).map(|p| p.eliminated);
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
        AssertionSpec::HandSize { player, equals } => {
            let pid = PlayerId(*player);
            let actual = state
                .zones
                .iter()
                .find(|z| z.kind == ZoneKind::Hand && z.owner == Some(pid))
                .map(|z| z.contents.len())
                .unwrap_or(0);
            if actual == *equals {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed { expected: equals.to_string(), actual: actual.to_string() }
            }
        }
        AssertionSpec::ObjectCounters { card, kind, equals } => {
            let object_id = resolve_card(card, card_ids);
            let counter_kind = parse_counter_kind(kind);
            let actual = state.object(object_id).map(|o| o.counters.count(&counter_kind));
            if actual == Some(*equals) {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed { expected: equals.to_string(), actual: format!("{actual:?}") }
            }
        }
        AssertionSpec::PlayerCounters { player, kind, equals } => {
            let pid = PlayerId(*player);
            let counter_kind = parse_counter_kind(kind);
            let actual =
                state.players.iter().find(|p| p.player_id == pid).map(|p| p.counters.count(&counter_kind));
            if actual == Some(*equals) {
                AssertionOutcome::Passed
            } else {
                AssertionOutcome::Failed { expected: equals.to_string(), actual: format!("{actual:?}") }
            }
        }
    }
}

pub enum EntryOutcome {
    Action(StepOutcome),
    Assertion(AssertionOutcome),
}

pub struct EntryReport {
    pub description: String,
    pub outcome: EntryOutcome,
}

pub struct ScenarioReport {
    pub entries: Vec<EntryReport>,
}

impl ScenarioReport {
    /// A "hard" failure is an assertion that actually ran and produced the
    /// wrong answer — not an action that's merely unimplemented yet, which
    /// is expected and shouldn't fail a Phase 4/5 test run.
    pub fn has_hard_failures(&self) -> bool {
        self.entries.iter().any(|e| {
            matches!(&e.outcome, EntryOutcome::Assertion(AssertionOutcome::Failed { .. }))
        })
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

fn run_action_spec(
    engine: &mut Engine,
    card_ids: &mut HashMap<String, ObjectId>,
    action_spec: &ActionSpec,
) -> EntryReport {
    let ledger_before = engine.state().ledger.len();
    let outcome = match action_spec {
        ActionSpec::SubmitPayment { mana, .. } => run_payment(engine, build_payment_plan(mana)),
        ActionSpec::UnwindPayment { .. } => run_unwind_payment(engine),
        ActionSpec::SubmitDiscard { cards, .. } => {
            let card_obj_ids =
                cards.iter().map(|k| resolve_card(k, card_ids)).collect();
            run_discard(engine, card_obj_ids)
        }
        ActionSpec::KeepHand { player } => run_mulligan(engine, PlayerAction {
            player: PlayerId(*player),
            kind: PlayerActionKind::KeepHand,
        }),
        ActionSpec::TakeMulligan { player } => run_mulligan(engine, PlayerAction {
            player: PlayerId(*player),
            kind: PlayerActionKind::TakeMulligan,
        }),
        ActionSpec::BottomCards { player, first_n } => {
            run_bottom_cards(engine, PlayerId(*player), *first_n)
        }
        other => run_action(engine, to_player_action(other, card_ids)),
    };
    remap_moved_card_ids(card_ids, &engine.state().ledger[ledger_before..]);
    EntryReport { description: action_spec.describe(), outcome: EntryOutcome::Action(outcome) }
}

pub fn run(file: &ScenarioFile) -> ScenarioReport {
    let (mut engine, mut card_ids) = if let Some(game) = &file.game {
        build_new_game_engine(file, game)
    } else {
        let (state, card_ids, trigger_scripts) = build_initial_state(file);
        let mut engine = Engine::new(state);
        for pending in trigger_scripts {
            register_trigger_ability(
                &mut engine,
                pending.source,
                pending.condition.as_deref(),
                &pending.operation,
            );
        }
        (engine, card_ids)
    };
    let mut entries = Vec::new();

    if !file.steps.is_empty() {
        for step in &file.steps {
            match step {
                ScenarioStep::Action(action_spec) => {
                    entries.push(run_action_spec(&mut engine, &mut card_ids, action_spec));
                }
                ScenarioStep::Assertion(assertion_step) => {
                    let spec = assertion_step.to_assertion_spec();
                    let outcome = check_assertion(&spec, engine.state(), &card_ids);
                    entries.push(EntryReport {
                        description: spec.describe(),
                        outcome: EntryOutcome::Assertion(outcome),
                    });
                }
            }
        }
    } else {
        for action_spec in &file.actions {
            entries.push(run_action_spec(&mut engine, &mut card_ids, action_spec));
        }
        for assertion_spec in &file.assertions {
            let outcome = check_assertion(assertion_spec, engine.state(), &card_ids);
            entries.push(EntryReport {
                description: assertion_spec.describe(),
                outcome: EntryOutcome::Assertion(outcome),
            });
        }
    }

    ScenarioReport { entries }
}

pub fn run_path(path: &std::path::Path) -> ScenarioReport {
    run(&load(path))
}
