use std::collections::HashMap;

use stackfish_engine::*;
use stackfish_engine::engine::Engine;

use crate::schema::*;
use crate::actions::build_payment_plan;
use crate::run::{StepOutcome, run_action, run_payment};

// Reduces `cost` to generic mana plus exact colored pips, the only shape a `[[cards]]` entry
// can build. `None` for hybrid, Phyrexian, `{C}` or any non-mana component, so the caller
// reports it instead of mis-paying.
fn simple_mana_requirement(cost: &CostSpec) -> Option<(u32, Vec<Color>)> {
    if !cost.additional.is_empty() || !cost.alternative.is_empty() {
        return None;
    }
    let mana = cost.mana.as_ref()?;
    let mut generic = 0u32;
    let mut colors = Vec::new();
    for symbol in &mana.symbols {
        match symbol {
            ManaSymbol::Atom(AtomicManaSymbol::Generic(n)) => generic += n,
            ManaSymbol::Atom(AtomicManaSymbol::Colored(c)) => colors.push(*c),
            _ => return None,
        }
    }
    Some((generic, colors))
}

// Counts as `[W, U, B, R, G, Colorless]`. Snow and restricted mana is ignored because a
// fixture pool never holds it.
fn mana_tally(pool: &ManaPool) -> [u32; 6] {
    [pool.white, pool.blue, pool.black, pool.red, pool.green, pool.colorless]
}

fn color_slot(color: Color) -> usize {
    match color {
        Color::White => 0,
        Color::Blue => 1,
        Color::Black => 2,
        Color::Red => 3,
        Color::Green => 4,
    }
}

fn slot_letter(slot: usize) -> &'static str {
    ["W", "U", "B", "R", "G", "C"][slot]
}

fn tally_covers(tally: &[u32; 6], generic: u32, pip_colors: &[Color]) -> bool {
    let mut remaining = *tally;
    for &color in pip_colors {
        let slot = color_slot(color);
        if remaining[slot] == 0 {
            return false;
        }
        remaining[slot] -= 1;
    }
    remaining.iter().sum::<u32>() >= generic
}

// Lets `pick_mana_source` prefer a source that closes a gap.
fn missing_pip_colors(tally: &[u32; 6], pip_colors: &[Color]) -> Vec<Color> {
    let mut remaining = *tally;
    let mut missing = Vec::new();
    for &color in pip_colors {
        let slot = color_slot(color);
        if remaining[slot] > 0 {
            remaining[slot] -= 1;
        } else {
            missing.push(color);
        }
    }
    missing
}

// Generic mana comes from what is left in W/U/B/R/G/Colorless order, a deterministic tie-break.
// CR 118.13a
fn units_from_tally(mut tally: [u32; 6], generic: u32, pip_colors: &[Color]) -> Vec<String> {
    let mut units = Vec::new();
    for &color in pip_colors {
        let slot = color_slot(color);
        tally[slot] -= 1;
        units.push(slot_letter(slot).to_string());
    }
    let mut remaining = generic;
    for (slot, count) in tally.iter_mut().enumerate() {
        while remaining > 0 && *count > 0 {
            *count -= 1;
            units.push(slot_letter(slot).to_string());
            remaining -= 1;
        }
    }
    units
}

// Considers only an activated `AddMana` ability with no cost. `ability_index` counts all of
// the source's abilities, matching `find_ability`, not just the mana ones.
// TODO: consider a mana ability that has its own cost; see `resolve_auto_pay`
fn pick_mana_source(
    state: &GameState,
    payer: PlayerId,
    tally: &[u32; 6],
    pip_colors: &[Color],
) -> Option<(ObjectId, u32)> {
    let missing = missing_pip_colors(tally, pip_colors);
    let mut candidates: Vec<(ObjectId, u32, &Vec<ManaUnit>)> = Vec::new();
    let mut index_by_source: HashMap<ObjectId, u32> = HashMap::new();
    for ability in &state.abilities {
        let index = {
            let counter = index_by_source.entry(ability.source).or_insert(0);
            let current = *counter;
            *counter += 1;
            current
        };
        if ability.kind != AbilityKind::Activated || ability.cost.is_some() {
            continue;
        }
        let Some(AbilityEffect::AddMana(units)) = &ability.effect else { continue };
        let Some(source_object) = state.object(ability.source) else { continue };
        if source_object.zone != ZoneKind::Battlefield
            || source_object.tapped
            || source_object.controller != payer
        {
            continue;
        }
        candidates.push((ability.source, index, units));
    }
    if let Some(&color) = missing.first()
        && let Some((source, index, _)) =
            candidates.iter().find(|(_, _, units)| units.contains(&ManaUnit::Colored(color)))
        {
            return Some((*source, *index));
        }
    candidates.first().map(|(source, index, _)| (*source, *index))
}

/// Taps `payer`'s untapped no-cost mana sources until the pool covers `cost`, then returns
/// the `PaymentPlan` to submit. It never guesses at a cost shape it does not recognize.
// TODO: tap a mana ability that has its own cost (Treasure, Lotus Petal); `pick_mana_source` skips them
pub(crate) fn resolve_auto_pay(engine: &mut Engine, payer: PlayerId, cost: &CostSpec) -> Result<PaymentPlan, String> {
    let (generic, pip_colors) = simple_mana_requirement(cost).ok_or_else(|| {
        "auto_pay can't handle this cost shape (hybrid/Phyrexian/snow mana, or a non-mana \
         additional/alternative cost) - build the payment plan by hand instead"
            .to_string()
    })?;

    loop {
        let tally = mana_tally(&engine
            .state()
            .player(payer)
            .unwrap_or_else(|| panic!("auto_pay: payer {payer:?} has no player state"))
            .mana_pool);
        if tally_covers(&tally, generic, &pip_colors) {
            break;
        }
        let Some((source, ability_index)) = pick_mana_source(engine.state(), payer, &tally, &pip_colors) else {
            let have: u32 = tally.iter().sum();
            let need = generic + pip_colors.len() as u32;
            return Err(format!(
                "auto_pay ran out of untapped mana sources: {have} unit(s) floating, cost needs {need}"
            ));
        };
        let tap = PlayerAction {
            player: payer,
            kind: PlayerActionKind::ActivateAbility {
                source,
                ability_index,
                targets: Vec::new(),
                chosen_colors: Vec::new(),
                x_value: None,
            },
        };
        if let StepOutcome::Errored(err) = run_action(engine, tap) {
            return Err(format!("auto_pay failed tapping a mana source: {err}"));
        }
    }

    let tally = mana_tally(&engine
        .state()
        .player(payer)
        .unwrap_or_else(|| panic!("auto_pay: payer {payer:?} has no player state"))
        .mana_pool);
    let units = units_from_tally(tally, generic, &pip_colors);
    Ok(build_payment_plan(&ManaPaymentSpec { units }))
}

/// Pays an open payment pause after a `CastSpell` or `ActivateAbility` with `auto_pay`. It
/// returns `outcome` unchanged if the action failed or opened no pause.
pub(crate) fn chain_auto_pay(engine: &mut Engine, payer: PlayerId, outcome: StepOutcome) -> StepOutcome {
    if !matches!(outcome, StepOutcome::Ran) {
        return outcome;
    }
    let Some((cost, _source)) = engine.pending_payment_cost() else {
        return outcome;
    };
    let cost = cost.clone();
    match resolve_auto_pay(engine, payer, &cost) {
        Ok(plan) => run_payment(engine, plan),
        Err(message) => StepOutcome::Errored(message),
    }
}

