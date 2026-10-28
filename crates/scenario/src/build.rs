use std::collections::{HashMap, VecDeque};

use stackfish_carddb::CardDb;
use stackfish_engine::*;
use stackfish_engine::engine::Engine;

use crate::schema::*;

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

/// `TargetRequirementSpec.supertypes` (CR 205.4a, "target legendary
/// creature") - same names/panic-on-unknown convention `parse_card_types`
/// just above uses; mirrors `script.rs`'s own `parse_supertype_name`,
/// duplicated rather than shared per that function's own doc comment (no
/// common string-parsing utility between the two crates today).
fn parse_supertypes(names: &[String]) -> SupertypeSet {
    let mut set = SupertypeSet::empty();
    for name in names {
        set |= match name.as_str() {
            "Basic" => SupertypeSet::BASIC,
            "Legendary" => SupertypeSet::LEGENDARY,
            "Snow" => SupertypeSet::SNOW,
            "World" => SupertypeSet::WORLD,
            other => panic!("unknown supertype in scenario file: {other}"),
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

/// Opens `file.card_db` if set — shared by `build_initial_state` and
/// `build_new_game_engine` so both scenario formats support
/// `CardSpec.db_script_id` the same way. `None` when `card_db` is unset;
/// `card_definition_from_spec` is what turns "a `[[cards]]` entry sets
/// `db_script_id` but this returned `None`" into a clear panic instead of a
/// confusing downstream error.
pub(crate) fn open_card_db(file: &ScenarioFile) -> Option<CardDb> {
    file.card_db.as_ref().map(|path| {
        let resolved = resolve_card_db_path(file, path);
        CardDb::open(&resolved)
            .unwrap_or_else(|e| panic!("failed to open card_db {} (from {path:?}): {e}", resolved.display()))
    })
}

/// A relative `card_db` path resolves against this scenario file's own
/// directory (`file.base_dir`, set by `load()`), not the process's current
/// working directory — see `ScenarioFile.base_dir`'s own doc comment for
/// why. An absolute path is returned unchanged either way. `file.base_dir`
/// is `None` for a `ScenarioFile` built directly (via `toml::from_str`
/// rather than `load()`, as every integration test in this repo does, or by
/// hand) — a relative `card_db` there falls back to the pre-existing
/// CWD-relative behavior, unchanged from before `base_dir` existed.
pub(crate) fn resolve_card_db_path(file: &ScenarioFile, path: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(path);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match &file.base_dir {
        Some(dir) => dir.join(path),
        None => path.to_path_buf(),
    }
}

/// Builds one `[[cards]]` entry's `CardDefinition` — either a real DB-backed
/// definition (`spec.db_script_id` set: looked up via `CardDb::
/// load_by_script_id`, using the DB's own real `definition_id` and every
/// other structural characteristic it knows) or the pre-existing
/// from-scratch struct literal built out of this `CardSpec`'s own TOML
/// fields. `placeholder_definition_id` only matters for the from-scratch
/// path — the two call sites disagree with each other on what to use there
/// (`build_initial_state` passes the card's real `ObjectId.0`,
/// `build_new_game_engine` passes a hardcoded `0` for every card) - a
/// pre-existing inconsistency this function deliberately preserves exactly
/// as each call site already had it, rather than silently "fixing" it as a
/// side effect of this refactor.
///
/// See `CardSpec.db_script_id`'s own doc comment for which fields the DB
/// path ignores and which side-channel fields (`script`/`mana_ability`/
/// `trigger_condition`+`trigger_operation`/`continuous_effect`) still apply
/// regardless of which path built the definition.
pub(crate) fn card_definition_from_spec(card_db: Option<&CardDb>, placeholder_definition_id: u64, spec: &CardSpec) -> CardDefinition {
    if let Some(script_id) = &spec.db_script_id {
        let db = card_db.unwrap_or_else(|| {
            panic!(
                "card '{}' sets db_script_id {script_id:?} but this scenario file has no top-level \
                 card_db path set",
                spec.key
            )
        });
        return db.load_by_script_id(script_id).unwrap_or_else(|e| {
            panic!("card '{}': db_script_id {script_id:?} failed to load: {e}", spec.key)
        });
    }

    let symbols = mana_symbols(spec.generic_cost, &spec.colors, spec.has_x);
    CardDefinition {
        definition_id: placeholder_definition_id,
        name: spec.name.clone(),
        mana_cost: ManaCost { symbols },
        color_indicator: None,
        supertypes: parse_supertypes(&spec.supertypes),
        card_types: parse_card_types(&spec.types),
        subtypes: spec.subtypes.iter().cloned().map(Subtype).collect(),
        oracle_text: String::new(),
        power: build_pt(spec.power, &spec.power_script),
        toughness: build_pt(spec.toughness, &spec.toughness_script),
        loyalty: None,
        defense: None,
        script_ref: ScriptReference::Inline(spec.script.clone().unwrap_or_default()),
        keywords: parse_keywords(&spec.keywords),
        target_requirement: spec.target_requirement.as_ref().map(build_target_requirement),
        // TODO: no TOML scenario field for alternative costs yet -
        // flashback/escape-style cards aren't constructible via a scenario
        // fixture, only via direct struct literals in Rust integration
        // tests (see phase5_flashback_alternative_costs.rs).
        alternative_costs: Vec::new(),
        buyback_cost: None,
        // TODO: no TOML scenario field for a Rust-native effect
        // (Game.Ops.Damage/GainLife/...) yet either - same "direct struct
        // literal only" limit as alternative_costs above.
        effect: None,
        modes: build_spell_modes(&spec.modes),
        mode_choice: build_mode_choice(spec),
    }
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
fn mana_symbols(generic_cost: u32, colors: &[String], has_x: bool) -> Vec<ManaSymbol> {
    let mut symbols = Vec::new();
    if has_x {
        symbols.push(ManaSymbol::X);
    }
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
pub(crate) fn parse_mana_unit(letter: &str) -> ManaUnit {
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
            "Trample" => KeywordSet::TRAMPLE,
            "Fear" => KeywordSet::FEAR,
            "Intimidate" => KeywordSet::INTIMIDATE,
            "Shadow" => KeywordSet::SHADOW,
            "Horsemanship" => KeywordSet::HORSEMANSHIP,
            "Banding" => KeywordSet::BANDING,
            "Hexproof" => KeywordSet::HEXPROOF,
            "Shroud" => KeywordSet::SHROUD,
            "AttacksEachCombat" => KeywordSet::ATTACKS_EACH_COMBAT,
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
pub(crate) fn build_continuous_effect(source: ObjectId, spec: &ContinuousEffectSpec) -> ContinuousEffect {
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
        subtypes: (!spec.filter_subtypes.is_empty())
            .then(|| spec.filter_subtypes.iter().cloned().map(Subtype).collect()),
        excluded_subtypes: (!spec.filter_excluded_subtypes.is_empty())
            .then(|| spec.filter_excluded_subtypes.iter().cloned().map(Subtype).collect()),
        supertypes: (!spec.filter_supertypes.is_empty()).then(|| parse_supertypes(&spec.filter_supertypes)),
        excluded_supertypes: (!spec.filter_excluded_supertypes.is_empty())
            .then(|| parse_supertypes(&spec.filter_excluded_supertypes)),
        self_only: spec.filter_self_only,
    };
    // CR 611.2c: `condition_count` present means this effect is
    // conditional — see `ContinuousEffectSpec::condition_count`'s own doc
    // comment.
    let condition = spec.condition_count.map(|count| {
        InterveningIfSpec::Primitive(InterveningIfPrimitive::ControllerControlsAtLeast {
            filter: FilterSpec {
                card_types: (!spec.condition_card_types.is_empty())
                    .then(|| parse_card_types(&spec.condition_card_types)),
                supertypes: None,
                subtypes: None,
                controller: Some(FilterController::You),
                random: false,
                extra: None,
            },
            count,
        })
    });
    ContinuousEffect {
        source,
        layer,
        affected: AffectedFilter::Primitive(filter),
        effect: EffectPayload::Primitive(effect),
        timestamp: SequenceNo(0),
        duration: EffectDuration::Permanent,
        condition,
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
            excluded_card_types: (!spec.excluded_card_types.is_empty())
                .then(|| parse_card_types(&spec.excluded_card_types)),
            subtypes: (!spec.subtypes.is_empty())
                .then(|| spec.subtypes.iter().cloned().map(Subtype).collect()),
            excluded_subtypes: (!spec.excluded_subtypes.is_empty())
                .then(|| spec.excluded_subtypes.iter().cloned().map(Subtype).collect()),
            supertypes: (!spec.supertypes.is_empty()).then(|| parse_supertypes(&spec.supertypes)),
            excluded_supertypes: (!spec.excluded_supertypes.is_empty())
                .then(|| parse_supertypes(&spec.excluded_supertypes)),
            controller: spec.controller.as_deref().map(parse_filter_controller),
            zone: spec.zone.as_deref().map(parse_zone_kind),
            // TODO: TargetRequirementSpec has no mana_value TOML field yet -
            // no scenario fixture needs "target spell with mana value X"
            // expressed via TOML, only via a real card script
            // (`s.target_requirement()`), same "engine/Lua first, TOML
            // follow-up if a fixture ever needs it" posture the
            // subtypes/supertypes filter fields took before their own
            // scenario-side follow-up slice.
            mana_value: None,
        },
        "player" => TargetFilter::Player { controller: spec.controller.as_deref().map(parse_filter_controller) },
        "any" => TargetFilter::AnyTarget,
        // CR 113.9/701.6a: "counter target activated or triggered ability"
        // (Stifle, Trickbind).
        "ability" => TargetFilter::Ability,
        // CR 702.21a (Ward): the spell or ability that triggered this one.
        "triggering" => TargetFilter::TriggeringStackObject,
        // Placeholder handle - no Engine/ScriptRuntime exists yet at this
        // point (see PendingTargetScript's own doc comment). Patched with
        // the real loaded handle by register_target_script, once one does.
        "script" => TargetFilter::Script(ScriptHandle(0)),
        other => panic!(
            "unknown target_requirement filter in scenario file: {other} - expected \
             object/player/any/ability/script"
        ),
    };
    stackfish_engine::TargetSpec::Primitive(TargetRequirement {
        filter,
        count: spec.count,
        up_to: spec.up_to,
        at_least: spec.at_least,
        exclude_source: spec.exclude_source,
    })
}

/// `CardSpec.modes` -> `Vec<SpellMode>` (CR 700.2). `effect` is always
/// `None` per mode - see `SpellModeSpec`'s own doc comment for why.
fn build_spell_modes(specs: &[SpellModeSpec]) -> Vec<SpellMode> {
    specs
        .iter()
        .map(|m| SpellMode {
            target_requirement: m.target_requirement.as_ref().map(build_target_requirement),
            effect: None,
            // TODO: no TOML field for a mode's own additional cost yet -
            // same pre-existing gap CardSpec itself has for the top-level
            // card's own additional costs (no TOML representation at all).
            additional_cost: None,
            pawprint_weight: m.pawprint_weight,
        })
        .collect()
}

/// `CardSpec.mode_count`/`mode_up_to`/`mode_repeatable`/
/// `mode_pawprint_budget` -> `ModeChoice` (CR 700.2d/700.2i). Meaningless
/// (but still read, for a consistent default) when `CardSpec.modes` is
/// empty.
fn build_mode_choice(spec: &CardSpec) -> ModeChoice {
    if let Some(budget) = spec.mode_pawprint_budget {
        return ModeChoice::PawprintBudget(budget);
    }
    match (spec.mode_up_to, spec.mode_repeatable) {
        (false, false) => ModeChoice::Exactly(spec.mode_count),
        (true, false) => ModeChoice::UpTo(spec.mode_count),
        (false, true) => ModeChoice::ExactlyRepeatable(spec.mode_count),
        (true, true) => ModeChoice::UpToRepeatable(spec.mode_count),
    }
}

/// `assert_object_counters`/`assert_player_counters`'s `kind` field, by CR
/// 122.3 name, into `CounterKind`.
pub(crate) fn parse_counter_kind(name: &str) -> CounterKind {
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

pub(crate) fn parse_zone_kind(name: &str) -> ZoneKind {
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

pub(crate) fn parse_phase(name: &str) -> Phase {
    match name {
        "pre_game" => Phase::PreGame,
        "beginning" => Phase::Beginning,
        "main" => Phase::Main,
        "combat" => Phase::Combat,
        "ending" => Phase::Ending,
        other => panic!("unknown phase in scenario file: {other}"),
    }
}

pub(crate) fn parse_step(name: &str) -> Step {
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
) -> (
    GameState,
    HashMap<String, ObjectId>,
    Vec<PendingTriggerScript>,
    Vec<PendingTargetScript>,
    Vec<PendingScriptTargetRequirement>,
    Vec<PendingScriptOperation>,
) {
    let card_db = open_card_db(file);
    let mut next_object_id: u64 = 0;
    // Seeds `AbilityId`s for any `mana_ability` built below - draws from a
    // local counter rather than hardcoding a placeholder (unlike
    // `build_continuous_effect`'s own `SequenceNo(0)` timestamp placeholder)
    // because `AbilityId` is minted from the *same* counter
    // `allocate_sequence_no` uses at real runtime (see that type's own doc
    // comment) - a hardcoded placeholder here could collide with a real
    // self-registered ability's id later in the same game. `next_sequence_no`
    // below is seeded past whatever this counter reaches, same "avoid
    // colliding with anything seeded here" reasoning `next_object_id` above
    // already gets.
    let mut next_ability_id: u64 = 0;
    let mut card_ids: HashMap<String, ObjectId> = HashMap::new();
    let mut trigger_scripts: Vec<PendingTriggerScript> = Vec::new();
    let mut target_scripts: Vec<PendingTargetScript> = Vec::new();
    let mut script_target_requirements: Vec<PendingScriptTargetRequirement> = Vec::new();
    let mut script_operations: Vec<PendingScriptOperation> = Vec::new();

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

        let definition = card_definition_from_spec(card_db.as_ref(), object_id.0, spec);
        // The real Lua source `s.target_requirement()`/`s.operation()` (CR
        // 601.2c/702.5a, CR 608.2m) should run against - `spec.script` for a
        // from-scratch card, or whatever `card_definition_from_spec` just
        // resolved from the DB for a `db_script_id` one (same
        // `ScriptReference::Inline` shape either path produces). Reading
        // `spec.script` directly below this used to skip every `db_script_id`
        // card outright, silently leaving its `operation()`/
        // `target_requirement()` unregistered - see this branch's own
        // `phase5/db-script-operation-fix` note in docs/implementation.
        let resolved_script: Option<String> = match &definition.script_ref {
            ScriptReference::Inline(source) if !source.is_empty() => Some(source.clone()),
            _ => None,
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
            copiable_override: None,
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
            attached_to: None, attached_to_player: None,
            timestamp: SequenceNo(0),
            controlled_since_turn: entry.controlled_since_turn,
            is_token: false,
            is_commander: entry.is_commander,
            visibility: VisibilityPolicy { public, visible_to: Vec::new() },
            copy_generation: 0,
            entered_via_escape: false,
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
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
                id: AbilityId(next_ability_id),
                source: object_id,
                kind: AbilityKind::Activated,
                condition: None,
                cost: None,
                target: None,
                operation: ScriptHandle(0),
                effect: Some(AbilityEffect::AddMana(units)),
                intervening_if: None,
            });
            next_ability_id += 1;
        }
        if let Some(operation) = &spec.trigger_operation {
            trigger_scripts.push(PendingTriggerScript {
                source: object_id,
                condition: spec.trigger_condition.clone(),
                operation: operation.clone(),
            });
        }
        if let Some(script) = spec.target_requirement.as_ref().and_then(|r| r.script.as_ref()) {
            target_scripts.push(PendingTargetScript { object: object_id, script: script.clone() });
        }
        // No TOML target_requirement at all - let the card's own script
        // declare one (`s.target_requirement()`) if it has one. An explicit
        // TOML `target_requirement` (including the `filter = "script"` case
        // just above) always wins; this only fills the gap when it's unset.
        if spec.target_requirement.is_none() {
            if let Some(source) = &resolved_script {
                script_target_requirements
                    .push(PendingScriptTargetRequirement { object: object_id, source: source.clone() });
            }
        }
        // CardSpec has no TOML field for a card's own effect at all (unlike
        // target_requirement above) - so there's no "does TOML already have
        // one" gate here, every scripted card gets a chance to declare one.
        if let Some(source) = &resolved_script {
            script_operations.push(PendingScriptOperation { object: object_id, source: source.clone() });
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
        hand_size_modifiers: Vec::new(),
        hand_reveal_modifiers: Vec::new(),
        draw_limit_modifiers: Vec::new(),
        land_play_modifiers: Vec::new(),
        abilities,
        ledger: Vec::new(),
        rng: RngState { seed: file.seed.expect("`seed` is required when `[game]` is not set"), draws_consumed: 0 },
        next_event_id: 0,
        next_sequence_no: next_ability_id,
        next_object_id,
        pending_step_skips: Vec::new(),
        once_per_turn_triggered: std::collections::HashSet::new(),
        commander_return_offered: std::collections::HashSet::new(),
        linked_ability_generation: std::collections::HashMap::new(),
        script_ability_ids: std::collections::HashSet::new(),
        ability_function_zones: std::collections::HashMap::new(),
        off_battlefield_scripts_run: std::collections::HashSet::new(),
        linked_exiles: Vec::new(),
        pending_timing_bypass: None,
        pending_free_cast: None,
    };

    (state, card_ids, trigger_scripts, target_scripts, script_target_requirements, script_operations)
}

/// Loads `condition_src`/`operation_src` into `engine`'s script runtime and
/// pushes the resulting `RegisteredAbility` — shared by `run()`'s
/// `build_initial_state` branch and `build_new_game_engine`'s per-library
/// loop so both scenario formats support `trigger_condition`/
/// `trigger_operation` the same way.
pub(crate) fn register_trigger_ability(
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
    let id = engine.state_mut().allocate_ability_id();
    engine.state_mut().abilities.push(RegisteredAbility {
        modes: Vec::new(),
        mode_choice: ModeChoice::Exactly(1),
        id,
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

/// Loads `source` into `engine`'s script runtime and patches the placeholder
/// `TargetFilter::Script(ScriptHandle(0))` `build_target_requirement` left on
/// `object`'s own `CardDefinition.target_requirement` with the real handle —
/// same timing fix as `register_trigger_ability`, just patching a field in
/// place instead of pushing a new `RegisteredAbility`. Panics if `object`'s
/// `target_requirement` isn't the script placeholder this is meant to patch
/// (a scenario bug, not a runtime one — same "trust the caller wired this up
/// via PendingTargetScript" stance `register_trigger_ability` already takes).
pub(crate) fn register_target_script(engine: &mut Engine, object: ObjectId, source: &str) {
    let handle = engine
        .load_script(source)
        .unwrap_or_else(|e| panic!("target_requirement.script failed to load: {e:?}"));
    let definition = &mut engine
        .state_mut()
        .objects
        .get_mut(&object)
        .unwrap_or_else(|| panic!("target script registration: object {object:?} not found"))
        .definition;
    match &mut definition.target_requirement {
        Some(stackfish_engine::TargetSpec::Primitive(req)) => {
            req.filter = TargetFilter::Script(handle);
        }
        other => panic!(
            "register_target_script called on object {object:?} whose target_requirement isn't \
             the script placeholder: {other:?}"
        ),
    }
}

/// Loads `source`'s optional `s.target_requirement()` function into `engine`'s
/// script runtime (`Engine::build_target_requirement`) and sets `object`'s
/// `CardDefinition.target_requirement` to whatever it returns — `None` if
/// `source` doesn't define one, same "no targets required" default a TOML
/// fixture with no `[cards.target_requirement]` table already has. Unlike
/// `register_target_script`, this overwrites the whole field rather than
/// patching a placeholder's `filter` in place, since there's no pre-existing
/// TOML shape to patch — see `PendingScriptTargetRequirement`'s own doc
/// comment for why this is only ever called when the TOML side left the
/// field unset.
pub(crate) fn register_script_target_requirement(engine: &mut Engine, object: ObjectId, source: &str) {
    let requirement = engine
        .build_target_requirement(source)
        .unwrap_or_else(|e| panic!("script target_requirement() failed to load: {e:?}"));
    engine
        .state_mut()
        .objects
        .get_mut(&object)
        .unwrap_or_else(|| panic!("script target_requirement registration: object {object:?} not found"))
        .definition
        .target_requirement = requirement;
}

/// Loads `source`'s optional `s.operation()` function into `engine`'s script
/// runtime (`Engine::build_operation`) and sets `object`'s
/// `CardDefinition.effect` to whatever it declares - `None` (unchanged) if
/// `source` doesn't define one. A `Game.Ops.*` primitive tag decodes
/// straight to that `AbilityEffect` variant, same Rust-native fast path a
/// direct struct literal would set by hand; a custom function decodes to
/// `AbilityEffect::Script(handle)` instead, so `commands::execute_command`'s
/// `CastSpell` arm can unpack the real handle onto the `StackObject` at cast
/// time (see that variant's own doc comment).
pub(crate) fn register_script_operation(engine: &mut Engine, object: ObjectId, source: &str) {
    let Some((primitive, handle)) = engine
        .build_operation(source)
        .unwrap_or_else(|e| panic!("script operation() failed to load: {e:?}"))
    else {
        return;
    };
    let effect = primitive.or(Some(AbilityEffect::Script(handle)));
    engine
        .state_mut()
        .objects
        .get_mut(&object)
        .unwrap_or_else(|| panic!("script operation registration: object {object:?} not found"))
        .definition
        .effect = effect;
}

