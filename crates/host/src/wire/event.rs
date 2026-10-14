use serde::Serialize;
use stackfish_engine::turn::{Phase, Step};
use stackfish_engine::{
    CounterKind, GameEvent, GameEventKind, GameObject, PTValue, PlayerId, SupertypeSet,
};

use super::mana::{WireManaUnit, mana_cost_string};
use super::{U64, WireCardType, WireTarget, WireZone, card_types};

#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum WirePhase {
    PreGame,
    Beginning,
    Main,
    Combat,
    Ending,
}

impl From<Phase> for WirePhase {
    fn from(phase: Phase) -> Self {
        match phase {
            Phase::PreGame => WirePhase::PreGame,
            Phase::Beginning => WirePhase::Beginning,
            Phase::Main => WirePhase::Main,
            Phase::Combat => WirePhase::Combat,
            Phase::Ending => WirePhase::Ending,
        }
    }
}

#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum WireStep {
    Untap,
    Upkeep,
    Draw,
    PreCombatMain,
    BeginningOfCombat,
    DeclareAttackers,
    DeclareBlockers,
    FirstStrikeDamage,
    CombatDamage,
    EndOfCombat,
    PostCombatMain,
    End,
    Cleanup,
}

impl From<Step> for WireStep {
    fn from(step: Step) -> Self {
        match step {
            Step::Untap => WireStep::Untap,
            Step::Upkeep => WireStep::Upkeep,
            Step::Draw => WireStep::Draw,
            Step::PreCombatMain => WireStep::PreCombatMain,
            Step::BeginningOfCombat => WireStep::BeginningOfCombat,
            Step::DeclareAttackers => WireStep::DeclareAttackers,
            Step::DeclareBlockers => WireStep::DeclareBlockers,
            Step::FirstStrikeDamage => WireStep::FirstStrikeDamage,
            Step::CombatDamage => WireStep::CombatDamage,
            Step::EndOfCombat => WireStep::EndOfCombat,
            Step::PostCombatMain => WireStep::PostCombatMain,
            Step::End => WireStep::End,
            Step::Cleanup => WireStep::Cleanup,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireCounterKind {
    PlusOnePlusOne,
    MinusOneMinusOne,
    Loyalty,
    Poison,
    Energy,
    Experience,
    Charge,
    Custom { name: String },
}

impl From<&CounterKind> for WireCounterKind {
    fn from(kind: &CounterKind) -> Self {
        match kind {
            CounterKind::PlusOnePlusOne => WireCounterKind::PlusOnePlusOne,
            CounterKind::MinusOneMinusOne => WireCounterKind::MinusOneMinusOne,
            CounterKind::Loyalty => WireCounterKind::Loyalty,
            CounterKind::Poison => WireCounterKind::Poison,
            CounterKind::Energy => WireCounterKind::Energy,
            CounterKind::Experience => WireCounterKind::Experience,
            CounterKind::Charge => WireCounterKind::Charge,
            CounterKind::Custom(name) => WireCounterKind::Custom { name: name.clone() },
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct WireCounter {
    pub counter: WireCounterKind,
    pub amount: i32,
}

#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum WireSupertype {
    Basic,
    Legendary,
    Snow,
    World,
}

fn supertypes(set: SupertypeSet) -> Vec<WireSupertype> {
    [
        (SupertypeSet::BASIC, WireSupertype::Basic),
        (SupertypeSet::LEGENDARY, WireSupertype::Legendary),
        (SupertypeSet::SNOW, WireSupertype::Snow),
        (SupertypeSet::WORLD, WireSupertype::World),
    ]
    .into_iter()
    .filter(|(flag, _)| set.contains(*flag))
    .map(|(_, wire)| wire)
    .collect()
}

fn pt_text(value: &Option<PTValue>) -> Option<String> {
    value.as_ref().map(|v| match v {
        PTValue::Fixed(n) => n.to_string(),
        PTValue::CharacteristicDefining { display, .. } => display.clone(),
    })
}

/// The last known information (CR 603.10) of an object an event moved or
/// consumed: the printed characteristics from its definition, plus the state it
/// had. Layer effects are not applied.
#[derive(Serialize, Clone, Debug)]
pub struct WireObjectSummary {
    pub object: U64,
    pub name: String,
    pub owner: u32,
    pub controller: u32,
    pub zone: WireZone,
    pub mana_cost: String,
    pub supertypes: Vec<WireSupertype>,
    pub card_types: Vec<WireCardType>,
    pub subtypes: Vec<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub tapped: bool,
    pub is_token: bool,
    pub is_commander: bool,
    pub counters: Vec<WireCounter>,
}

impl From<&GameObject> for WireObjectSummary {
    fn from(object: &GameObject) -> Self {
        let definition = &object.definition;
        WireObjectSummary {
            object: object.object_id.into(),
            name: definition.name.clone(),
            owner: object.owner.0,
            controller: object.controller.0,
            zone: object.zone.into(),
            mana_cost: mana_cost_string(&definition.mana_cost),
            supertypes: supertypes(definition.supertypes),
            card_types: card_types(definition.card_types),
            subtypes: definition.subtypes.iter().map(|s| s.0.clone()).collect(),
            power: pt_text(&definition.power),
            toughness: pt_text(&definition.toughness),
            tapped: object.tapped,
            is_token: object.is_token,
            is_commander: object.is_commander,
            counters: object
                .counters
                .totals()
                .iter()
                .map(|(kind, amount)| WireCounter { counter: kind.into(), amount: *amount })
                .collect(),
        }
    }
}

/// One ledger entry. Every variant has a `kind` tag.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireEventKind {
    ZoneChange { object: U64, from: WireZone, to: WireZone, last_known_info: Option<WireObjectSummary> },
    LifeChanged { player: u32, delta: i32 },
    DamageDealt { source: U64, target: WireTarget, amount: u32, is_combat_damage: bool },
    CounterAdded { object: U64, counter: WireCounterKind, amount: i32 },
    Tapped { object: U64 },
    Untapped { object: U64 },
    ManaAdded { player: u32, units: Vec<WireManaUnit> },
    AttackerDeclared { attacker: U64, target: WireTarget },
    BlockerDeclared { blocker: U64, attacker: U64 },
    CostPaid { stack_id: U64, payer: u32 },
    PrioritySet { player: u32 },
    PriorityPassed { player: u32 },
    StackObjectPushed { stack_id: U64 },
    StackObjectResolved { stack_id: U64 },
    Fizzled { stack_id: U64 },
    Countered { stack_id: U64 },
    CastingUnwound { stack_id: U64 },
    TriggerQueued { source: U64, controller: u32 },
    ReplacementApplied { source: U64, original_event: U64 },
    StateBasedActionApplied { description: String },
    ScriptError { message: String },
    TurnStarted { player: u32, turn_number: u32 },
    PhaseStarted { phase: WirePhase },
    StepStarted { step: WireStep },
    PlayerEliminated { player: u32 },
    RngDrawn { purpose: String, value: U64 },
    LibraryShuffled { player: u32 },
    CardsScried { player: u32, kept_on_top: Vec<U64>, put_bottom: Vec<U64> },
    CardsSurveiled { player: u32, kept_on_top: Vec<U64>, put_graveyard: Vec<U64> },
    BecameCopy { object: U64, copied: U64 },
    TextBoxesExchanged { a: U64, b: U64 },
    Regenerated { object: U64 },
    TokenCeased { object: U64 },
    Attached { object: U64, attached_to: U64 },
    AttachedToPlayer { object: U64, player: u32 },
    Unattached { object: U64, was_attached_to: U64 },
    UmbraArmorConsumed { enchanted: U64, aura: U64 },
    Cycled { object: U64, last_known_info: WireObjectSummary },
    MadnessExiled { object: U64, last_known_info: WireObjectSummary },
    SpellCast { object: U64 },
    BecameTarget { object: U64, by: U64, by_controller: u32 },
}

fn ids(ids: &[stackfish_engine::ObjectId]) -> Vec<U64> {
    ids.iter().map(|id| U64::from(*id)).collect()
}

fn player(player: PlayerId) -> u32 {
    player.0
}

impl From<&GameEventKind> for WireEventKind {
    fn from(kind: &GameEventKind) -> Self {
        use WireEventKind as W;
        match kind {
            GameEventKind::ZoneChange { object, from, to, last_known_info } => W::ZoneChange {
                object: (*object).into(),
                from: (*from).into(),
                to: (*to).into(),
                last_known_info: last_known_info.as_ref().map(WireObjectSummary::from),
            },
            GameEventKind::LifeChanged { player: p, delta } => W::LifeChanged { player: player(*p), delta: *delta },
            GameEventKind::DamageDealt { source, target, amount, is_combat_damage } => W::DamageDealt {
                source: (*source).into(),
                target: (*target).into(),
                amount: *amount,
                is_combat_damage: *is_combat_damage,
            },
            GameEventKind::CounterAdded { object, kind, amount } => {
                W::CounterAdded { object: (*object).into(), counter: kind.into(), amount: *amount }
            }
            GameEventKind::Tapped { object } => W::Tapped { object: (*object).into() },
            GameEventKind::Untapped { object } => W::Untapped { object: (*object).into() },
            GameEventKind::ManaAdded { player: p, units } => {
                W::ManaAdded { player: player(*p), units: units.iter().map(|u| (*u).into()).collect() }
            }
            GameEventKind::AttackerDeclared { attacker, target } => {
                W::AttackerDeclared { attacker: (*attacker).into(), target: (*target).into() }
            }
            GameEventKind::BlockerDeclared { blocker, attacker } => {
                W::BlockerDeclared { blocker: (*blocker).into(), attacker: (*attacker).into() }
            }
            GameEventKind::CostPaid { stack_id, payer } => {
                W::CostPaid { stack_id: (*stack_id).into(), payer: player(*payer) }
            }
            GameEventKind::PrioritySet { player: p } => W::PrioritySet { player: player(*p) },
            GameEventKind::PriorityPassed { player: p } => W::PriorityPassed { player: player(*p) },
            GameEventKind::StackObjectPushed { stack_id } => W::StackObjectPushed { stack_id: (*stack_id).into() },
            GameEventKind::StackObjectResolved { stack_id } => W::StackObjectResolved { stack_id: (*stack_id).into() },
            GameEventKind::Fizzled { stack_id } => W::Fizzled { stack_id: (*stack_id).into() },
            GameEventKind::Countered { stack_id } => W::Countered { stack_id: (*stack_id).into() },
            GameEventKind::CastingUnwound { stack_id } => W::CastingUnwound { stack_id: (*stack_id).into() },
            GameEventKind::TriggerQueued { source, controller } => {
                W::TriggerQueued { source: (*source).into(), controller: player(*controller) }
            }
            GameEventKind::ReplacementApplied { source, original_event } => {
                W::ReplacementApplied { source: (*source).into(), original_event: U64(original_event.0) }
            }
            GameEventKind::StateBasedActionApplied { description } => {
                W::StateBasedActionApplied { description: description.clone() }
            }
            GameEventKind::ScriptError { message } => W::ScriptError { message: message.clone() },
            GameEventKind::TurnStarted { player: p, turn_number } => {
                W::TurnStarted { player: player(*p), turn_number: *turn_number }
            }
            GameEventKind::PhaseStarted { phase } => W::PhaseStarted { phase: (*phase).into() },
            GameEventKind::StepStarted { step } => W::StepStarted { step: (*step).into() },
            GameEventKind::PlayerEliminated { player: p } => W::PlayerEliminated { player: player(*p) },
            GameEventKind::RngDrawn { purpose, value } => W::RngDrawn { purpose: purpose.clone(), value: U64(*value) },
            GameEventKind::LibraryShuffled { player: p } => W::LibraryShuffled { player: player(*p) },
            GameEventKind::CardsScried { player: p, kept_on_top, put_bottom } => {
                W::CardsScried { player: player(*p), kept_on_top: ids(kept_on_top), put_bottom: ids(put_bottom) }
            }
            GameEventKind::CardsSurveiled { player: p, kept_on_top, put_graveyard } => W::CardsSurveiled {
                player: player(*p),
                kept_on_top: ids(kept_on_top),
                put_graveyard: ids(put_graveyard),
            },
            GameEventKind::BecameCopy { object, copied } => {
                W::BecameCopy { object: (*object).into(), copied: (*copied).into() }
            }
            GameEventKind::TextBoxesExchanged { a, b } => W::TextBoxesExchanged { a: (*a).into(), b: (*b).into() },
            GameEventKind::Regenerated { object } => W::Regenerated { object: (*object).into() },
            GameEventKind::TokenCeased { object } => W::TokenCeased { object: (*object).into() },
            GameEventKind::Attached { object, attached_to } => {
                W::Attached { object: (*object).into(), attached_to: (*attached_to).into() }
            }
            GameEventKind::AttachedToPlayer { object, player: p } => {
                W::AttachedToPlayer { object: (*object).into(), player: player(*p) }
            }
            GameEventKind::Unattached { object, was_attached_to } => {
                W::Unattached { object: (*object).into(), was_attached_to: (*was_attached_to).into() }
            }
            GameEventKind::UmbraArmorConsumed { enchanted, aura } => {
                W::UmbraArmorConsumed { enchanted: (*enchanted).into(), aura: (*aura).into() }
            }
            GameEventKind::Cycled { object, last_known_info } => {
                W::Cycled { object: (*object).into(), last_known_info: last_known_info.into() }
            }
            GameEventKind::MadnessExiled { object, last_known_info } => {
                W::MadnessExiled { object: (*object).into(), last_known_info: last_known_info.into() }
            }
            GameEventKind::SpellCast { object } => W::SpellCast { object: (*object).into() },
            GameEventKind::BecameTarget { object, by, by_controller } => W::BecameTarget {
                object: (*object).into(),
                by: (*by).into(),
                by_controller: player(*by_controller),
            },
        }
    }
}

/// A ledger entry with its ids. `sequence_no` gives the total order, so a host
/// can play a batch back at its own pace.
#[derive(Serialize, Clone, Debug)]
pub struct WireEvent {
    pub event_id: U64,
    pub sequence_no: U64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caused_by_command: Option<U64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_event: Option<U64>,
    #[serde(flatten)]
    pub kind: WireEventKind,
}

impl From<&GameEvent> for WireEvent {
    fn from(event: &GameEvent) -> Self {
        WireEvent {
            event_id: U64(event.event_id.0),
            sequence_no: U64(event.sequence_no.0),
            caused_by_command: event.caused_by_command.map(|id| U64(id.0)),
            parent_event: event.parent_event.map(|id| U64(id.0)),
            kind: (&event.kind).into(),
        }
    }
}
