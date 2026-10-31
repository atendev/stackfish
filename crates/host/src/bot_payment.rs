//! How the proof bot pays a cost. A Payment Decision gives brace notation and no pool, so
//! the bot tracks its pool from `mana_added` events. See `docs/DESIGN.md`.

use serde_json::{Value, json};

pub(crate) const COLORS: [&str; 5] = ["white", "blue", "black", "red", "green"];
const LETTERS: [&str; 5] = ["W", "U", "B", "R", "G"];

// `colored` and `colorless` count every unit; the snow counts are the part from a snow source.
// CR 106.3
#[derive(Default, Clone, Copy)]
pub(crate) struct Pool {
    colored: [u32; 5],
    colorless: u32,
    snow_colored: [u32; 5],
    snow_colorless: u32,
}

impl Pool {
    pub(crate) fn total(&self) -> u32 {
        self.colored.iter().sum::<u32>() + self.colorless
    }

    pub(crate) fn add(&mut self, unit: &Value) {
        let color = COLORS.iter().position(|c| unit["color"] == *c);
        match (unit["kind"].as_str(), color) {
            (Some("colored"), Some(i)) => self.colored[i] += 1,
            (Some("snow_colored"), Some(i)) => {
                self.colored[i] += 1;
                self.snow_colored[i] += 1;
            }
            (Some("colorless"), _) => self.colorless += 1,
            (Some("snow_colorless"), _) => {
                self.colorless += 1;
                self.snow_colorless += 1;
            }
            // A restricted unit may pay only for some spells, so the bot never spends it.
            _ => {}
        }
    }

    pub(crate) fn spend(&mut self, spent: &Pool) {
        for i in 0..5 {
            self.colored[i] = self.colored[i].saturating_sub(spent.colored[i]);
            self.snow_colored[i] = self.snow_colored[i].saturating_sub(spent.snow_colored[i]);
        }
        self.colorless = self.colorless.saturating_sub(spent.colorless);
        self.snow_colorless = self.snow_colorless.saturating_sub(spent.snow_colorless);
    }

    fn plain_colored(&self, i: usize) -> u32 {
        self.colored[i] - self.snow_colored[i]
    }

    fn plain_colorless(&self) -> u32 {
        self.colorless - self.snow_colorless
    }

    // Takes a plain unit first so snow units stay available for `{S}`. Returns whether it was snow.
    fn take_colored(&mut self, i: usize) -> Option<bool> {
        if self.plain_colored(i) > 0 {
            self.colored[i] -= 1;
            Some(false)
        } else if self.snow_colored[i] > 0 {
            self.colored[i] -= 1;
            self.snow_colored[i] -= 1;
            Some(true)
        } else {
            None
        }
    }

    fn take_colorless(&mut self) -> Option<bool> {
        if self.plain_colorless() > 0 {
            self.colorless -= 1;
            Some(false)
        } else if self.snow_colorless > 0 {
            self.colorless -= 1;
            self.snow_colorless -= 1;
            Some(true)
        } else {
            None
        }
    }
}

/// One way to pay a single symbol.
#[derive(Clone, Copy)]
enum Branch {
    /// One mana of a color, by index into `COLORS`.
    Color(usize),
    /// `{C}` (CR 107.4c): colorless mana only.
    Colorless,
    /// `{S}` (CR 107.4h): one mana from a snow source.
    Snow,
    /// This much generic mana, paid from whatever is left.
    Generic(u32),
    /// 2 life for a Phyrexian symbol (CR 107.4f).
    Life,
}

/// A spent unit, kept to build the Answer.
#[derive(Clone, Copy)]
enum Unit {
    Colored(usize, bool),
    Colorless(bool),
}

impl Unit {
    fn json(self) -> Value {
        match self {
            Unit::Colored(i, false) => json!({ "kind": "colored", "color": COLORS[i] }),
            Unit::Colored(i, true) => json!({ "kind": "snow_colored", "color": COLORS[i] }),
            Unit::Colorless(false) => json!({ "kind": "colorless" }),
            Unit::Colorless(true) => json!({ "kind": "snow_colorless" }),
        }
    }

    fn record(self, spent: &mut Pool) {
        match self {
            Unit::Colored(i, snow) => {
                spent.colored[i] += 1;
                spent.snow_colored[i] += u32::from(snow);
            }
            Unit::Colorless(snow) => {
                spent.colorless += 1;
                spent.snow_colorless += u32::from(snow);
            }
        }
    }
}

fn color_index(letter: &str) -> Option<usize> {
    LETTERS.iter().position(|l| *l == letter)
}

fn half(text: &str) -> Option<Branch> {
    if let Some(i) = color_index(text) {
        Some(Branch::Color(i))
    } else if text == "C" {
        Some(Branch::Colorless)
    } else {
        text.parse().ok().map(Branch::Generic)
    }
}

// Cheapest first: colored mana, then generic mana, then life. `{X}` gives `None` because
// `resolve_x` replaces it before planning.
fn branches(symbol: &str) -> Option<Vec<Branch>> {
    let mut found = match symbol.split('/').collect::<Vec<_>>().as_slice() {
        ["S"] => vec![Branch::Snow],
        [single] => vec![half(single)?],
        [color, "P"] => vec![Branch::Color(color_index(color)?), Branch::Life],
        [a, b] => vec![half(a)?, half(b)?],
        [a, b, "P"] => vec![
            Branch::Color(color_index(a)?),
            Branch::Color(color_index(b)?),
            Branch::Life,
        ],
        _ => return None,
    };
    let rank = |branch: &Branch| match branch {
        Branch::Generic(_) => 1,
        Branch::Life => 2,
        _ => 0,
    };
    found.sort_by_key(rank);
    Some(found)
}

fn symbols(mana: &str) -> Option<Vec<&str>> {
    mana.split('{')
        .skip(1)
        .map(|part| part.strip_suffix('}'))
        .collect()
}

pub(crate) fn resolve_x(mana: &str, x: u32) -> String {
    mana.replace("{X}", &format!("{{{x}}}"))
}

// CR 202.3e, CR 202.3f, CR 202.3g
pub(crate) fn mana_value(mana: &str) -> Option<u32> {
    let mut total = 0;
    for symbol in symbols(mana)? {
        total += match symbol {
            "X" => 0,
            _ => branches(symbol)?
                .iter()
                .map(|branch| match branch {
                    Branch::Generic(n) => *n,
                    _ => 1,
                })
                .max()?,
        };
    }
    Some(total)
}

struct Search {
    options: Vec<Vec<Branch>>,
    life_budget: u32,
}

impl Search {
    // Generic mana is paid last, from what is left, plain units before snow ones.
    fn pay(
        &self,
        index: usize,
        left: Pool,
        units: &mut Vec<Unit>,
        generic: u32,
        life: u32,
    ) -> Option<u32> {
        let Some(branches) = self.options.get(index) else {
            return self.pay_generic(left, units, generic).then_some(life);
        };
        for branch in branches {
            let mark = units.len();
            let mut next = left;
            let (mut more_generic, mut more_life) = (0, 0);
            let found = match branch {
                Branch::Color(i) => next
                    .take_colored(*i)
                    .map(|snow| units.push(Unit::Colored(*i, snow)))
                    .is_some(),
                Branch::Colorless => next
                    .take_colorless()
                    .map(|snow| units.push(Unit::Colorless(snow)))
                    .is_some(),
                Branch::Snow => {
                    if let Some(i) = (0..5).find(|i| next.snow_colored[*i] > 0) {
                        next.colored[i] -= 1;
                        next.snow_colored[i] -= 1;
                        units.push(Unit::Colored(i, true));
                        true
                    } else if next.snow_colorless > 0 {
                        next.colorless -= 1;
                        next.snow_colorless -= 1;
                        units.push(Unit::Colorless(true));
                        true
                    } else {
                        false
                    }
                }
                Branch::Generic(n) => {
                    more_generic = *n;
                    true
                }
                Branch::Life => {
                    more_life = 2;
                    life + more_life <= self.life_budget
                }
            };
            if found
                && let Some(total) = self.pay(
                    index + 1,
                    next,
                    units,
                    generic + more_generic,
                    life + more_life,
                )
            {
                return Some(total);
            }
            units.truncate(mark);
        }
        None
    }

    fn pay_generic(&self, mut left: Pool, units: &mut Vec<Unit>, generic: u32) -> bool {
        for _ in 0..generic {
            let unit = if let Some(snow) = left.take_colorless() {
                Unit::Colorless(snow)
            } else if let Some(i) = (0..5)
                .filter(|i| left.colored[*i] > 0)
                .max_by_key(|i| (left.plain_colored(*i), left.colored[*i]))
            {
                let snow = left.take_colored(i).unwrap_or(false);
                Unit::Colored(i, snow)
            } else {
                return false;
            };
            units.push(unit);
        }
        true
    }
}

/// A plan for `cost` from `pool`, or `None` if the bot cannot make one. `life_budget` caps
/// the life spent on Phyrexian symbols.
pub(crate) fn plan_payment(
    cost: &Value,
    pool: &Pool,
    life_budget: u32,
    source: Option<&Value>,
) -> Option<(Vec<Value>, Pool)> {
    let mut payments = Vec::new();
    let mut spent = Pool::default();
    let mut life_for_costs = 0;

    for additional in cost["additional"].as_array()? {
        if additional["kind"] == "life_payment" {
            life_for_costs += additional["amount"].as_u64()? as u32;
        }
    }

    let mana = cost["mana"].as_str().unwrap_or_default();
    if !mana.is_empty() {
        let search = Search {
            options: symbols(mana)?
                .into_iter()
                .map(branches)
                .collect::<Option<_>>()?,
            life_budget: life_budget.saturating_sub(life_for_costs),
        };
        let mut units = Vec::new();
        let life = search.pay(0, *pool, &mut units, 0, 0)?;
        units.iter().for_each(|unit| unit.record(&mut spent));
        payments.push(json!({
            "kind": "mana",
            "units": units.iter().map(|unit| unit.json()).collect::<Vec<_>>(),
            "phyrexian_life": life,
        }));
    }

    for additional in cost["additional"].as_array()? {
        let payment = match additional["kind"].as_str()? {
            "tap_self" => json!({ "kind": "tap", "object": source? }),
            "sacrifice_self" => json!({ "kind": "sacrifice", "object": source? }),
            "discard_self" => json!({ "kind": "discard", "object": source? }),
            "life_payment" => json!({ "kind": "life", "amount": additional["amount"] }),
            _ => return None,
        };
        payments.push(payment);
    }
    Some((payments, spent))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(units: &[Value]) -> Pool {
        let mut pool = Pool::default();
        units.iter().for_each(|unit| pool.add(unit));
        pool
    }

    fn colored(color: &str) -> Value {
        json!({ "kind": "colored", "color": color })
    }

    fn snow(color: &str) -> Value {
        json!({ "kind": "snow_colored", "color": color })
    }

    fn colorless() -> Value {
        json!({ "kind": "colorless" })
    }

    fn plan(mana: &str, units: &[Value], life: u32) -> Option<(Vec<Value>, Pool)> {
        plan_payment(
            &json!({ "mana": mana, "additional": [] }),
            &pool(units),
            life,
            None,
        )
    }

    fn mana_payment(plan: &(Vec<Value>, Pool)) -> &Value {
        &plan.0[0]
    }

    #[test]
    fn plain_symbols_still_pay() {
        let plan =
            plan("{1}{R}", &[colored("red"), colored("red")], 0).expect("two red pay {1}{R}");
        assert_eq!(mana_payment(&plan)["units"].as_array().unwrap().len(), 2);
        assert_eq!(mana_payment(&plan)["phyrexian_life"], 0);
        assert!(
            super::plan_payment(
                &json!({ "mana": "{R}", "additional": [] }),
                &Pool::default(),
                0,
                None
            )
            .is_none()
        );
    }

    #[test]
    fn colorless_symbol_needs_colorless_mana() {
        assert!(plan("{C}", &[colored("red")], 0).is_none());
        assert!(plan("{2}{C}", &[colorless(), colored("red"), colored("red")], 0).is_some());
        assert!(
            plan(
                "{2}{C}",
                &[colored("red"), colored("red"), colored("red")],
                0
            )
            .is_none()
        );
    }

    #[test]
    fn hybrid_takes_whichever_half_the_pool_has() {
        assert!(plan("{W/U}", &[colored("blue")], 0).is_some());
        assert!(plan("{W/U}", &[colored("red")], 0).is_none());
        let one = plan("{2/W}", &[colored("white"), colorless(), colorless()], 0).unwrap();
        assert_eq!(mana_payment(&one)["units"].as_array().unwrap().len(), 1);
        assert!(plan("{2/W}", &[colored("red"), colorless()], 0).is_some());
        assert!(plan("{2/W}", &[colored("red")], 0).is_none());
    }

    #[test]
    fn two_hybrid_symbols_need_different_units() {
        assert!(plan("{W/U}{W/U}", &[colored("white"), colored("blue")], 0).is_some());
        assert!(plan("{W/U}{W/U}", &[colored("white")], 0).is_none());
    }

    #[test]
    fn phyrexian_prefers_mana_and_falls_back_to_life() {
        let with_mana = plan("{B/P}", &[colored("black")], 10).unwrap();
        assert_eq!(mana_payment(&with_mana)["phyrexian_life"], 0);
        assert_eq!(
            mana_payment(&with_mana)["units"].as_array().unwrap().len(),
            1
        );

        let with_life = plan("{B/P}", &[], 10).unwrap();
        assert_eq!(mana_payment(&with_life)["phyrexian_life"], 2);
        assert!(
            mana_payment(&with_life)["units"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn phyrexian_stays_inside_the_life_budget() {
        let plan_ = plan("{1}{B/P}{B/P}", &[colored("black"), colored("red")], 2).unwrap();
        assert_eq!(mana_payment(&plan_)["phyrexian_life"], 2);
        assert!(
            plan("{B/P}", &[], 1).is_none(),
            "1 life cannot fund a 2 life pip"
        );
        assert!(plan("{B/P}{B/P}", &[], 3).is_none());
    }

    #[test]
    fn hybrid_phyrexian_has_three_ways() {
        assert!(plan("{W/U/P}", &[colored("white")], 0).is_some());
        assert!(plan("{W/U/P}", &[colored("blue")], 0).is_some());
        assert!(plan("{W/U/P}", &[], 4).is_some());
        assert!(plan("{W/U/P}", &[colored("red")], 0).is_none());
    }

    #[test]
    fn snow_symbol_needs_a_snow_unit_and_keeps_it_from_generic() {
        assert!(plan("{S}", &[colored("green")], 0).is_none());
        let paid = plan("{S}", &[colored("green"), snow("green")], 0).unwrap();
        assert_eq!(mana_payment(&paid)["units"][0]["kind"], "snow_colored");
        let both = plan("{1}{S}", &[snow("green"), colored("green")], 0).unwrap();
        let kinds: Vec<_> = mana_payment(&both)["units"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["kind"].clone())
            .collect();
        assert_eq!(kinds.iter().filter(|k| *k == "snow_colored").count(), 1);
    }

    #[test]
    fn a_colored_pip_spares_a_snow_unit_for_a_snow_symbol() {
        let paid = plan("{G}{S}", &[snow("green"), colored("green")], 0).unwrap();
        assert_eq!(mana_payment(&paid)["units"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn spending_a_plan_removes_its_units_from_the_pool() {
        let mut have = pool(&[snow("green"), colored("green"), colorless()]);
        let (_, spent) = plan_payment(
            &json!({ "mana": "{G}{S}", "additional": [] }),
            &have,
            0,
            None,
        )
        .unwrap();
        have.spend(&spent);
        assert_eq!(have.total(), 1);
    }

    #[test]
    fn x_is_resolved_before_planning() {
        assert_eq!(resolve_x("{X}{X}{R}", 2), "{2}{2}{R}");
        assert!(
            plan("{X}{R}", &[colored("red")], 0).is_none(),
            "an unresolved X is not payable"
        );
        assert!(
            plan(
                &resolve_x("{X}{R}", 2),
                &[colored("red"), colorless(), colorless()],
                0
            )
            .is_some()
        );
    }

    #[test]
    fn mana_value_follows_cr_202_3() {
        assert_eq!(mana_value("{2}{R}"), Some(3));
        assert_eq!(mana_value("{X}{G}"), Some(1));
        assert_eq!(mana_value("{2/W}{W/U}"), Some(3));
        assert_eq!(mana_value("{1}{B/P}{B/P}"), Some(3));
        assert_eq!(mana_value("{S}{C}"), Some(2));
        assert_eq!(mana_value("{Z}"), None);
    }

    #[test]
    fn additional_costs_keep_their_order_after_mana() {
        let cost = json!({ "mana": "{R}", "additional": [{ "kind": "tap_self" }, { "kind": "life_payment", "amount": 3 }] });
        let source = json!("7");
        let (payments, _) =
            plan_payment(&cost, &pool(&[colored("red")]), 0, Some(&source)).unwrap();
        let kinds: Vec<_> = payments.iter().map(|p| p["kind"].clone()).collect();
        assert_eq!(kinds, ["mana", "tap", "life"]);
    }
}
