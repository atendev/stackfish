use std::collections::HashMap;

use stackfish_engine::*;
use stackfish_engine::engine::Engine;

use crate::schema::*;
use crate::actions::build_payment_plan;
use crate::run::{StepOutcome, run_action, run_payment};

/// Reduces `cost` to "N generic + these exact colored pips" - the only mana
/// shape a scenario `[[cards]]` entry's `generic_cost`/`colors` fields can
/// ever build (see `CardSpec`'s own doc comment: no hybrid/Phyrexian/snow
/// symbol can be expressed in TOML yet). `None` for anything `auto_pay` isn't
/// prepared to solve - a real `db_script_id` card's hybrid/Phyrexian mana, a
/// bare `{C}` colorless-atom symbol, or any non-mana `additional`/
/// `alternative` cost component - so the caller can report a clear "can't
/// handle this" error instead of mis-paying.
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

/// A player's floating mana, as a flat `[W, U, B, R, G, Colorless]` tally -
/// the six plain (non-snow, non-restricted) `ManaPool` fields in one array so
/// `auto_pay`'s own helpers below can index by color instead of matching on
/// `Color` at every call site. Snow/restricted mana is ignored entirely -
/// same "not representable in a scenario fixture yet" gap `CardSpec` already
/// has, so a real `ManaPool` built from one always has both at zero.
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

/// True once `tally` has at least one unit of every required colored pip,
/// with enough left over (regardless of color) to cover `generic` too.
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

/// Which of `pip_colors` `tally` can't yet pay on its own - what a tapped
/// mana source still needs to help with, used by `pick_mana_source` to
/// prefer a source that actually closes a gap over one that would just add
/// to an already-satisfied color.
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

/// Builds the exact flat `units` list `PaymentPlan::validate` expects (CR
/// 118.13a) from a tally already confirmed (by `tally_covers`) to hold
/// enough: each colored pip spent as that exact color, `generic` more spent
/// from whatever's left over afterward (W/U/B/R/G/Colorless, in that fixed
/// order - an arbitrary but deterministic tie-break, not optimized to
/// preserve any particular color for a later cost in the same scenario).
fn units_from_tally(mut tally: [u32; 6], generic: u32, pip_colors: &[Color]) -> Vec<String> {
    let mut units = Vec::new();
    for &color in pip_colors {
        let slot = color_slot(color);
        tally[slot] -= 1;
        units.push(slot_letter(slot).to_string());
    }
    let mut remaining = generic;
    for slot in 0..tally.len() {
        while remaining > 0 && tally[slot] > 0 {
            tally[slot] -= 1;
            units.push(slot_letter(slot).to_string());
            remaining -= 1;
        }
    }
    units
}

/// Finds one untapped, no-real-cost mana ability `payer` controls that can
/// help pay the still-`missing` pip colors (or, once every pip is covered,
/// just contribute toward the generic component) - `(source, ability_index)`
/// ready to hand straight to `PlayerActionKind::ActivateAbility`.
/// `ability_index` is this ability's own position among *all* of `source`'s
/// registered abilities (matching `find_ability`'s `nth`-by-source lookup),
/// not its position among just the mana ones.
///
/// Only ever considers a `CardSpec.mana_ability`-shaped ability - `kind:
/// Activated, effect: Some(AddMana(_)), cost: None` (the implicit-tap,
/// no-payment-pause shape `activate.rs` resolves synchronously) - a real
/// costed mana ability (Treasure/Lotus-Petal-style sacrifice) is left
/// untapped for a scenario author to handle by hand; see `resolve_auto_pay`'s
/// own doc comment for why extending this is a TODO rather than done here.
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
    if let Some(&color) = missing.first() {
        if let Some((source, index, _)) =
            candidates.iter().find(|(_, _, units)| units.contains(&ManaUnit::Colored(color)))
        {
            return Some((*source, *index));
        }
    }
    candidates.first().map(|(source, index, _)| (*source, *index))
}

/// The actual auto-pay algorithm: taps untapped no-cost mana sources `payer`
/// controls until the pool covers `cost`, then returns the exact
/// `PaymentPlan` to submit. Never touches anything outside `payer`'s own
/// battlefield mana sources, and never guesses at a cost shape
/// `simple_mana_requirement` doesn't recognize.
///
/// TODO: doesn't tap a real-cost mana ability (Treasure/Lotus-Petal-style) -
/// `pick_mana_source` skips those entirely, so a scenario relying on one to
/// fully pay a cost will get a clear "ran out of untapped mana sources"
/// error here instead of a nested payment being solved automatically.
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

/// `CastSpell`/`ActivateAbility`'s own `auto_pay` field: once the base action
/// has run, if it actually opened a payment pause (`Engine::
/// pending_payment_cost`), automatically resolve and submit it via
/// `resolve_auto_pay`/`run_payment` instead of leaving that to a separate
/// `submit_payment` step. A no-op (returns `outcome` unchanged) if the base
/// action didn't run cleanly, or didn't open a payment pause at all (a
/// costless ability, say) - nothing to pay either way.
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

