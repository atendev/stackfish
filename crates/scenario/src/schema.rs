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
    /// Path to the external card-database SQLite file (`stackfish-carddb`) -
    /// set this once and any `[[cards]]` entry can pull a real
    /// `CardDefinition` from it via `CardSpec.db_script_id` instead of
    /// hand-writing every field. `None` (the default) means no `[[cards]]`
    /// entry in this file may set `db_script_id` - attempting to do so
    /// panics with a clear message rather than silently falling back to the
    /// from-scratch TOML fields.
    ///
    /// A relative path is resolved against `base_dir` below (this scenario
    /// file's own directory), not the process's current working directory -
    /// see that field's own doc comment for why that distinction matters. An
    /// absolute path is used as-is either way.
    #[serde(default)]
    pub card_db: Option<String>,
    /// This scenario file's own directory - `None` when `load()` wasn't used
    /// to build this `ScenarioFile` (a hand-built one, or one parsed
    /// directly from a string via `toml::from_str`, as every integration
    /// test in this repo does). `#[serde(skip)]`: not a TOML field, set by
    /// `load()` after parsing from the path it was given. Exists solely so
    /// `card_db` above can resolve relative to *this file*, the same way a
    /// shell resolves a relative include path against the including
    /// script's own directory - not against wherever the process happened
    /// to be launched from (which for `cargo test` is the crate root, not
    /// the individual scenario file's own subdirectory under `tests/
    /// scenarios/`, a mismatch that would otherwise make every relative
    /// `card_db` fragile and confusing).
    #[serde(skip)]
    pub base_dir: Option<std::path::PathBuf>,
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
/// color letters + an optional bare `{X}` (`has_x`), rather than parsing a
/// `"{2}{R}"`-style cost string. Real cost-string parsing doesn't exist
/// anywhere yet; this sidesteps needing it just to write scenario fixtures.
/// Revisit if scenarios need hybrid/Phyrexian mana — not representable in
/// this shape yet.
#[derive(Deserialize)]
pub struct CardSpec {
    pub key: String,
    /// Required unless `db_script_id` is set - see that field's own doc
    /// comment for why this and every other characteristic field below
    /// becomes optional in that mode.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub generic_cost: u32,
    #[serde(default)]
    pub colors: Vec<String>,
    /// CR 107.3/601.2b - if true, this card's mana cost gets a bare
    /// `{X}` symbol (in addition to `generic_cost`/`colors`, not instead of
    /// them - e.g. `{X}{R}` is `has_x = true, colors = ["R"]`). Casting it
    /// then requires `PlayerActionKind::CastSpell::x_value` (the `x_value`
    /// field on a `cast_spell` action) to announce a value, which
    /// `ManaCost::resolve_x` substitutes in as a generic amount before
    /// `PaymentPlan::validate` sees the cost - same mechanism
    /// `phase5_x_costs.rs` exercises directly against the engine, just
    /// reachable from a scenario fixture now.
    #[serde(default)]
    pub has_x: bool,
    /// Required unless `db_script_id` is set.
    #[serde(default)]
    pub types: Vec<String>,
    /// Looks this card up in the top-level `card_db` SQLite file
    /// (`stackfish_carddb::CardDb::load_by_script_id`) instead of building a
    /// `CardDefinition` from this spec's own fields — the DB's
    /// `cards.script_id` column, a stable human-typed slug (e.g.
    /// `"lightning_bolt"`), not the card's printed `name` or its UUID
    /// `oracle_id`. Lets a scenario test against the exact real card instead
    /// of retyping its mana cost/types/power/toughness/keywords by hand.
    ///
    /// When set, every other characteristic field on this struct (`name`,
    /// `generic_cost`/`colors`, `types`, `supertypes`, `subtypes`, `power`/
    /// `power_script`, `toughness`/`toughness_script`, `keywords`,
    /// `target_requirement`) is ignored — not rejected if also set, same
    /// "not a supported combination" stance this file already takes for
    /// `power`/`power_script`'s own overlap. `script`, `mana_ability`,
    /// `trigger_condition`/`trigger_operation`, and `continuous_effect` are
    /// unaffected either way: those build separate `RegisteredAbility`/
    /// `ContinuousEffect`/pending-script entries keyed off the object's id,
    /// not fields on `CardDefinition` itself, so a scenario can still attach
    /// a synthetic ability to a real DB-backed card to probe an interaction.
    #[serde(default)]
    pub db_script_id: Option<String>,
    /// CR 205.4a — this card's own printed supertypes (e.g. `["Legendary"]`).
    /// Distinct from `TargetRequirementSpec.supertypes`, which restricts what
    /// a spell/ability is legal to target, not what a card itself is.
    #[serde(default)]
    pub supertypes: Vec<String>,
    /// CR 205.4b — this card's own printed subtypes (e.g. `["Human", "Wizard"]`
    /// for a creature, `["Aura"]` for an enchantment). Unvalidated against any
    /// fixed list (see `Subtype`'s own doc comment — the subtype space is
    /// open-ended), unlike `types`/`supertypes` above which panic on an
    /// unrecognized name. Distinct from `TargetRequirementSpec.subtypes`,
    /// same rationale as `supertypes` above.
    #[serde(default)]
    pub subtypes: Vec<String>,
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
    /// A full self-registering card script — the `s.register_abilities(c)`
    /// module convention (see `docs/lua/card-script-basics.md`), calling
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
    /// CR 700.2 modal spells — empty for the overwhelming majority of cards
    /// (non-modal), same "empty means non-modal" convention
    /// `CardDefinition.modes` itself documents. Each entry is one mode's own
    /// `target_requirement` (`SpellModeSpec`'s own doc comment covers what a
    /// mode can and can't carry from TOML). `commands.rs`'s existing mode-
    /// selection logic (`PlayerActionKind::CastSpell::modes`) is what
    /// actually consumes this at cast time — unchanged by this field.
    #[serde(default)]
    pub modes: Vec<SpellModeSpec>,
    /// CR 700.2d — how many of `modes` above must be chosen. Ignored when
    /// `modes` is empty. Defaults to `1` (the pre-existing "Choose one —"
    /// behavior) so every scenario fixture written before
    /// `phase5/modal-multi-mode` still parses unchanged.
    #[serde(default = "default_mode_count")]
    pub mode_count: u32,
    /// CR 700.2d's "up to" shape (e.g. "Choose one or both —" is
    /// `mode_count = 2, mode_up_to = true`) vs. the default "choose exactly
    /// `mode_count`". Ignored when `modes` is empty.
    #[serde(default)]
    pub mode_up_to: bool,
    /// CR 700.2d's reminder-text-gated "you may choose the same mode more
    /// than once" exception (e.g. Profane Command). Ignored when `modes` is
    /// empty. Defaults to `false` so every scenario fixture written before
    /// this field existed still parses unchanged.
    #[serde(default)]
    pub mode_repeatable: bool,
    /// CR 700.2i's pawprint-budget shape (e.g. Fickle Fiend/Kolaghan's
    /// Command-style "choose modes with combined pawprint value N or
    /// less") - when set, `build_mode_choice` returns `ModeChoice::
    /// PawprintBudget` and ignores `mode_count`/`mode_up_to`/
    /// `mode_repeatable` entirely (meaningless for this shape - no real
    /// card combines pawprint budgets with any of them, same narrowing
    /// `ModeChoice::PawprintBudget`'s own doc comment already takes).
    /// `None` (the default) keeps every pre-existing count-based fixture
    /// parsing unchanged.
    #[serde(default)]
    pub mode_pawprint_budget: Option<u32>,
}

fn default_mode_count() -> u32 {
    1
}

/// One entry of `CardSpec.modes` (CR 700.2's "Choose one —" options).
/// `target_requirement` mirrors the top-level `CardSpec.target_requirement`
/// field exactly, `None` meaning this mode requires no targets (e.g. "You
/// gain 3 life"). No `effect` field — same pre-existing gap
/// `CardSpec.effect` itself has (no TOML representation for a Rust-native
/// `AbilityEffect` yet, only via `script`); a mode built from TOML always
/// resolves with `effect: None`, same as any other TOML-built spell today.
#[derive(Deserialize)]
pub struct SpellModeSpec {
    #[serde(default)]
    pub target_requirement: Option<TargetRequirementSpec>,
    /// CR 700.2i: this mode's own printed pawprint ({P}) weight, only
    /// meaningful alongside `CardSpec.mode_pawprint_budget` - same
    /// "meaningless (0) until needed" convention `SpellMode::pawprint_
    /// weight` itself documents. Defaults to `0` so every scenario fixture
    /// written before this field existed still parses unchanged.
    #[serde(default)]
    pub pawprint_weight: u32,
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
    /// CR 205.4b — e.g. `["Wall"]` for "Walls get -1/-0". Matches as an
    /// "or" across the list, same convention `TargetRequirementSpec.subtypes`
    /// already documents.
    #[serde(default)]
    pub filter_subtypes: Vec<String>,
    /// CR 601.2c/115/205.4b's negated form — e.g. `["Wall"]` for "creatures
    /// that aren't Walls get +1/+1".
    #[serde(default)]
    pub filter_excluded_subtypes: Vec<String>,
    /// CR 205.4a — e.g. `["Legendary"]` for "legendary creatures get +1/+1".
    #[serde(default)]
    pub filter_supertypes: Vec<String>,
    /// CR 601.2c/115/205.4a's negated form — e.g. `["Legendary"]` for
    /// "nonlegendary creatures get -1/-1".
    #[serde(default)]
    pub filter_excluded_supertypes: Vec<String>,
    #[serde(default)]
    pub filter_self_only: bool,
    /// CR 611.2c: if set (alongside `condition_count`), this effect only
    /// applies while its source's controller controls `condition_count` or
    /// more permanents with any of these card types — the one
    /// `InterveningIfPrimitive::ControllerControlsAtLeast` shape this slice
    /// ships. `None`/absent (the common case) means the effect is
    /// unconditional, same as before this slice.
    #[serde(default)]
    pub condition_card_types: Vec<String>,
    #[serde(default)]
    pub condition_count: Option<u32>,
}

/// `CardSpec.target_requirement`'s shape — `filter` picks which
/// `TargetFilter` variant this is: `"object"` (uses `card_types`/
/// `subtypes`/`controller`/`zone`, CR 115.2's battlefield default applies
/// when `zone` is unset), `"player"` (any player), `"any"` (CR 115.4's
/// "any target" — creature/player/planeswalker/battle), or `"ability"` (CR
/// 113.9's "counter target activated or triggered ability", Stifle/
/// Trickbind). `count`'s meaning
/// depends on `up_to`/`at_least` (mutually exclusive) — see
/// `TargetRequirement`'s own doc for the full three-way shape (exact/"up to
/// N"/"N or more", CR 601.2c/115.6).
#[derive(Deserialize)]
pub struct TargetRequirementSpec {
    pub filter: String,
    #[serde(default)]
    pub card_types: Vec<String>,
    /// CR 601.2c/115's negated form — e.g. `["Land"]` for "target nonland
    /// permanent". See `TargetFilter::Object::excluded_card_types`'s own
    /// doc comment.
    #[serde(default)]
    pub excluded_card_types: Vec<String>,
    /// CR 205.4b — e.g. `["Human"]` for "target Human". Matches as an "or"
    /// across the list, same convention `TargetFilter::Object::subtypes`
    /// itself documents.
    #[serde(default)]
    pub subtypes: Vec<String>,
    /// CR 601.2c/115/205.4b's negated form — e.g. `["Wall"]` for "target
    /// creature that isn't a Wall". See `TargetFilter::Object::
    /// excluded_subtypes`'s own doc comment.
    #[serde(default)]
    pub excluded_subtypes: Vec<String>,
    /// CR 205.4a — e.g. `["Legendary"]` for "target legendary creature".
    #[serde(default)]
    pub supertypes: Vec<String>,
    /// CR 601.2c/115/205.4a's negated form — e.g. `["Legendary"]` for
    /// "target nonlegendary creature". See `TargetFilter::Object::
    /// excluded_supertypes`'s own doc comment.
    #[serde(default)]
    pub excluded_supertypes: Vec<String>,
    #[serde(default)]
    pub controller: Option<String>,
    #[serde(default)]
    pub zone: Option<String>,
    pub count: u32,
    #[serde(default)]
    pub up_to: bool,
    /// CR 601.2c/115.6's "N or more" shape - see `TargetRequirement::at_least`'s
    /// own doc comment.
    #[serde(default)]
    pub at_least: bool,
    /// CR 115.4/"another target" - see `TargetRequirement::exclude_source`'s
    /// own doc comment.
    #[serde(default)]
    pub exclude_source: bool,
    /// CR 601.2c/602.2b's scripted legality predicate (`TargetFilter::
    /// Script`) — only meaningful when `filter = "script"`; raw Lua source,
    /// `function(object, source, controller, state) -> bool`, same shape
    /// `ActivatedAbilityBuilder::SetTarget`'s function form already uses.
    /// Loaded into a real `ScriptHandle` post-construction by
    /// `register_target_script` — same "no `Engine`/`ScriptRuntime` exists
    /// yet at `CardDefinition`-build time" timing problem `trigger_condition`/
    /// `trigger_operation` already solve via `register_trigger_ability`.
    #[serde(default)]
    pub script: Option<String>,
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

/// One card instance's scripted `target_requirement`, collected by
/// `build_initial_state` for the same reason `PendingTriggerScript` is —
/// `run()` loads the Lua source afterward and patches the placeholder
/// `TargetFilter::Script(ScriptHandle(0))` `build_target_requirement` left on
/// this object's own `CardDefinition.target_requirement` (see
/// `register_target_script`).
pub struct PendingTargetScript {
    pub object: ObjectId,
    pub script: String,
}

/// One card instance whose `target_requirement` is left for its own script
/// to declare (CR 702.5a/303.4a — an Aura's "Enchant [quality]" line, etc.),
/// collected by `build_initial_state` for the same reason `PendingTargetScript`
/// is: no `Engine`/`ScriptRuntime` exists yet at that point. Distinct from
/// `PendingTargetScript` — that one patches a TOML-declared `filter =
/// "script"` predicate's placeholder handle; this one has no TOML
/// `target_requirement` at all, and instead calls the card's own `script`
/// source's optional `s.target_requirement()` function (`Engine::
/// build_target_requirement`) to fill the whole field in, once an `Engine`
/// exists (see `register_script_target_requirement`). Only collected when
/// `CardSpec.target_requirement` is unset and this card resolved to a real
/// script — either `CardSpec.script` (TOML-inline) or a `db_script_id`
/// lookup's own `CardDefinition.script_ref` (`phase5/db-script-operation-
/// fix` — the DB path used to be silently skipped here, reading only
/// `spec.script`). An explicit TOML `target_requirement` always wins,
/// unchanged for every pre-existing fixture.
pub struct PendingScriptTargetRequirement {
    pub object: ObjectId,
    pub source: String,
}

/// One card instance whose `CardDefinition.effect` (CR 608.2m, a plain
/// spell's own resolution effect) is left for its own script to declare via
/// the optional `s.operation()` function, collected by `build_initial_state`
/// for the same reason `PendingScriptTargetRequirement` is - no `Engine`/
/// `ScriptRuntime` exists yet at that point. Unlike `target_requirement`,
/// `CardSpec` has no TOML field for a card's own effect at all (see its own
/// doc comment - direct Rust struct literal only, until now), so there's no
/// "does the TOML side already have one" gate to check - every card that
/// resolved to a real script (TOML-inline `CardSpec.script`, or a
/// `db_script_id` lookup's `CardDefinition.script_ref`, same as
/// `PendingScriptTargetRequirement` above) gets one of these, and
/// `register_script_operation` simply leaves `effect: None` unchanged if that
/// script has no `operation()` function of its own.
pub struct PendingScriptOperation {
    pub object: ObjectId,
    pub source: String,
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
        /// Shortcut: if the cast opens a `ProcessState::AwaitingPayment`
        /// pause, automatically pay it instead of needing a separate
        /// `submit_payment`/`auto_pay` step - see `chain_auto_pay`'s own doc
        /// comment for exactly what it will and won't do. Default `false`
        /// (existing files are unaffected).
        #[serde(default)]
        auto_pay: bool,
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
        /// CR 107.3a/602.2b: same X-announcement contract as `CastSpell::
        /// x_value` above, for an ability whose own activation cost has an
        /// `{X}` in it.
        x_value: Option<u32>,
        /// Same shortcut as `CastSpell::auto_pay` above, for an activated
        /// ability's own cost.
        #[serde(default)]
        auto_pay: bool,
    },
    /// Shortcut: `ActivateAbility { ability_index: 0, targets: [], chosen_colors: [] }`
    /// - the extremely common "tap this land for its one mana ability" case.
    /// Only ever expands to ability index 0, same "one mana ability per
    /// card" simplification `CardSpec.mana_ability` already takes.
    TapForMana { player: u32, source: String },
    /// Shortcut: repeatedly passes priority for whichever player currently
    /// holds it (no `player` field - it isn't the caller's to choose) until
    /// `state.phase` matches `phase`. See `run_advance`'s own doc comment
    /// for exactly what it will and won't push through.
    AdvanceToPhase { phase: String },
    /// Same shortcut as `AdvanceToPhase`, for `state.step`.
    AdvanceToStep { step: String },
    DeclareAttackers {
        player: u32,
        #[serde(default)]
        attackers: Vec<AttackerSpec>,
    },
    /// CR 508.1e/702.22c: groups just-declared attackers into bands - see
    /// `PlayerActionKind::DeclareBands`'s own doc comment for legality. Each
    /// inner list is one band, named by card key.
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
    /// Backlog item 9 (`Game.SelectTargets`/`Game.ChooseN`): answers a
    /// pending `ProcessState::AwaitingScriptChoice { request: SelectTarget,
    /// .. }` pause — a card script asking mid-resolution for one target
    /// matching its own `TargetFilter`, regardless of which primitive
    /// (`Game.SelectTargets` directly, or a `Game.ChooseN` loop iteration)
    /// raised it. `target` is a bare player id or card key, same
    /// `TargetSpec` convention every other action's targets use. Same "own
    /// entry point" rationale as `SubmitPayment` — routed through
    /// `Engine::submit_script_choice`, not `submit_action`. `player` is
    /// documentation only (matches the request's own `controller`, which
    /// the engine already knows) — not validated against it, same stance
    /// `SubmitPayment`/`SubmitDiscard` already take.
    SubmitScriptChoice { player: u32, target: TargetSpec },
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
            ActionSpec::CastSpell { player, card, targets, modes, x_value, auto_pay } => {
                let targets = targets.iter().map(describe_target).collect::<Vec<_>>().join(", ");
                format!(
                    "cast_spell(player={player}, card={card:?}, targets=[{targets}], modes={modes:?}, x_value={x_value:?}, auto_pay={auto_pay})"
                )
            }
            ActionSpec::ActivateAbility { player, source, ability_index, targets, chosen_colors, x_value, auto_pay } => {
                let targets = targets.iter().map(describe_target).collect::<Vec<_>>().join(", ");
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
                    .map(|band| format!("[{}]", band.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>().join(", ")))
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
            ActionSpec::SubmitScriptChoice { player, target } => {
                format!("submit_script_choice(player={player}, target={})", describe_target(target))
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
    /// Checks `PlayerState.commander_damage_taken` (CR 903.10a) for one
    /// commander source - the way to confirm a commander's combat damage
    /// actually accumulated (and that a non-commander source, or a
    /// commander's non-combat damage, didn't).
    CommanderDamage { player: u32, source_card: String, equals: u32 },
    /// Checks `CombatState::bands` (CR 508.1e/702.22c) - the way to confirm
    /// `declare_bands` actually grouped the named cards into one band,
    /// without needing to infer it indirectly from a damage-split outcome.
    /// `equals` is the band's full membership (including `card` itself), by
    /// key, order-independent. `equals = []` asserts `card` is in no band at
    /// all (either combat has no bands this turn, or `card` isn't in any of
    /// them).
    BandMembers { card: String, equals: Vec<String> },
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
    #[serde(rename = "assert_commander_damage")]
    CommanderDamage { player: u32, source_card: String, equals: u32 },
    #[serde(rename = "assert_band_members")]
    BandMembers { card: String, equals: Vec<String> },
}

impl AssertionStepSpec {
    pub(crate) fn to_assertion_spec(&self) -> AssertionSpec {
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
            Self::CommanderDamage { player, source_card, equals } => {
                AssertionSpec::CommanderDamage { player: *player, source_card: source_card.clone(), equals: *equals }
            }
            Self::BandMembers { card, equals } => {
                AssertionSpec::BandMembers { card: card.clone(), equals: equals.clone() }
            }
        }
    }
}

/// One entry in `[[steps]]` — either an action or an assertion, dispatched on
/// its own `type` field: `assert_*` goes to `Assertion`, anything else to
/// `Action`. Hand-rolled instead of `#[serde(untagged)]` because an untagged
/// enum swallows every variant's real deserialization error and reports only
/// "data did not match any variant" with no indication of which field was
/// wrong or on which step — see the `deserialize` impl below for the fix.
pub enum ScenarioStep {
    Action {
        spec: ActionSpec,
        /// Inverts this step's pass/fail polarity: the engine rejecting the
        /// action becomes the expected, passing result, and the engine
        /// accepting it becomes the hard failure instead. For asserting an
        /// action is illegal (e.g. an invalid band shape) without a
        /// dedicated assertion primitive for every piece of state that
        /// would otherwise require inspecting. Default false (the engine
        /// rejecting an action is a hard failure, same as always).
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
            let expect_error =
                value.get("expect_error").and_then(toml::Value::as_bool).unwrap_or(false);
            Ok(ScenarioStep::Action { spec, expect_error })
        }
    }
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
            AssertionSpec::CommanderDamage { player, source_card, equals } => {
                format!("commander_damage(player={player}, source_card={source_card:?}) == {equals}")
            }
            AssertionSpec::BandMembers { card, equals } => {
                format!("band_members(card={card:?}) == {equals:?}")
            }
        }
    }
}
