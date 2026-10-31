//! A random legal-move bot that plays through a `Session` using only bytes and the
//! candidates each Decision lists. It biases toward passing and playing lands so games end.
//! See `docs/DESIGN.md`.

// TODO: accept `may_cast` Decisions; they do not list the cast's modes or targets
// TODO: pay costs that name a sacrifice, discard, tap or graveyard exile; the Payment Decision lists no eligible objects
// TODO: spend more than life minus one on a Phyrexian symbol, and learn the starting life instead of assuming 20

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{Value, json};

use crate::bot_payment::{Pool, mana_value, plan_payment, resolve_x};
use crate::envelope::PROTOCOL_VERSION;
use crate::illegal::illegal_replies;
use crate::session::Session;

/// A small deterministic generator (splitmix64), so the bot needs no crate and
/// the same seed gives the same game.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n`. `n` must be above zero.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// True with probability `num / den`.
    fn chance(&mut self, num: usize, den: usize) -> bool {
        self.below(den) < num
    }
}

/// Why the bot stopped without finishing the game.
#[derive(Debug)]
pub enum BotError {
    /// A response was not what the protocol promises.
    Protocol(String),
    /// The Session reported a pipeline error and cannot go on.
    Fatal(String),
    /// The Session rejected an Answer and the bot has no fallback for it.
    Rejected(String),
    /// The game went on past the answer limit.
    AnswerLimit(usize),
    /// An illegal Answer sent on purpose (`injecting_illegal_answers`) was
    /// accepted, changed the engine, or closed its Decision.
    NotAtomic(String),
}

impl std::fmt::Display for BotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BotError::Protocol(m) => write!(f, "protocol error: {m}"),
            BotError::Fatal(m) => write!(f, "fatal pipeline error: {m}"),
            BotError::Rejected(m) => write!(f, "answer rejected: {m}"),
            BotError::AnswerLimit(n) => write!(f, "no end after {n} answers"),
            BotError::NotAtomic(m) => write!(f, "a rejection was not atomic: {m}"),
        }
    }
}

impl std::error::Error for BotError {}

/// What one `step` did.
#[derive(Debug, PartialEq, Eq)]
pub enum Progress {
    /// The bot answered one Decision.
    Answered,
    /// The engine ran without reaching a Decision, for example at its step cap.
    Running,
    /// The game ended. `None` is a draw.
    GameOver(Option<u32>),
}

/// How a finished game went.
#[derive(Debug)]
pub struct Outcome {
    pub winner: Option<u32>,
    pub answers: usize,
    /// Answers the Session rejected and the bot replaced with a fallback.
    pub rejected: usize,
    /// FNV-1a over every request and response, so two runs compare in one
    /// number.
    pub transcript_hash: u64,
    /// Illegal Answers sent on purpose and turned down, as
    /// `injecting_illegal_answers` asks. Zero otherwise.
    pub illegal_rejected: usize,
}

pub struct Bot {
    rng: Rng,
    pools: HashMap<u32, Pool>,
    /// Each Seat's life, from `life_changed` events on top of `starting_life`.
    life: HashMap<u32, i64>,
    starting_life: i64,
    /// The source of the last ability the bot activated. A self cost
    /// (`tap_self`, `sacrifice_self`, `discard_self`) names it in the plan.
    last_source: Option<Value>,
    last_decision: Option<u64>,
    answers: usize,
    rejected: usize,
    hash: u64,
    injecting: bool,
    illegal_rejected: usize,
    /// Illegal Answers turned down, by the shape and mechanic of the Decision they answered, such as `partition (Scry)`.
    illegal_by_shape: BTreeMap<String, usize>,
    /// The mana cost of each card the bot has seen, by object id. A move event
    /// carries the identity of every card its Seat may see, including the
    /// Seat's own hand.
    costs: HashMap<String, String>,
    seats: BTreeSet<u32>,
    record: bool,
    watchers: Vec<u32>,
    events: HashMap<u32, Vec<Value>>,
}

const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;

fn fnv(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

fn id_number(id: &Value) -> Option<u64> {
    id.as_str()?.parse().ok()
}

impl Bot {
    /// A bot whose choices follow from `seed`.
    pub fn new(seed: u64) -> Bot {
        Bot {
            rng: Rng(seed ^ 0xA5A5_5A5A_DEAD_BEEF),
            pools: HashMap::new(),
            life: HashMap::new(),
            starting_life: 20,
            last_source: None,
            last_decision: None,
            answers: 0,
            rejected: 0,
            hash: FNV_OFFSET,
            injecting: false,
            illegal_rejected: 0,
            illegal_by_shape: BTreeMap::new(),
            costs: HashMap::new(),
            seats: BTreeSet::new(),
            record: false,
            watchers: Vec::new(),
            events: HashMap::new(),
        }
    }

    /// Tells the bot the life each player starts with. The default is 20, so a
    /// commander game needs 40 for the bot to pay Phyrexian symbols with life.
    pub fn with_starting_life(mut self, life: u32) -> Bot {
        self.starting_life = i64::from(life);
        self
    }

    /// Keeps every event each Seat receives, for `events`. `watchers` are Seats
    /// that never answer (spectators), which the bot polls on every step.
    pub fn recording(mut self, watchers: &[u32]) -> Bot {
        self.record = true;
        self.watchers = watchers.to_vec();
        self
    }

    /// Sends illegal Answers before each real one: at every Decision, each reply
    /// from `illegal::illegal_replies`. Each must be turned down with the
    /// Decision still open and the engine exactly as it was. The real game is
    /// unaffected, so it plays out as it would without them.
    pub fn injecting_illegal_answers(mut self) -> Bot {
        self.injecting = true;
        self
    }

    /// How many illegal Answers were turned down, by Decision shape and mechanic.
    pub fn illegal_by_shape(&self) -> &BTreeMap<String, usize> {
        &self.illegal_by_shape
    }

    /// The events `seat` received, in order. Empty unless `recording` was set.
    pub fn events(&self, seat: u32) -> &[Value] {
        self.events.get(&seat).map_or(&[], Vec::as_slice)
    }

    /// Plays until the game ends. `max_answers` bounds the loop.
    pub fn play(&mut self, session: &mut Session, max_answers: usize) -> Result<Outcome, BotError> {
        let mut loops = 0usize;
        loop {
            match self.step(session)? {
                Progress::GameOver(winner) => {
                    self.flush(session)?;
                    return Ok(Outcome {
                        winner,
                        answers: self.answers,
                        rejected: self.rejected,
                        transcript_hash: self.hash,
                        illegal_rejected: self.illegal_rejected,
                    });
                }
                Progress::Answered | Progress::Running => {
                    loops += 1;
                    if loops > max_answers {
                        return Err(BotError::AnswerLimit(max_answers));
                    }
                }
            }
        }
    }

    /// Finds the open Decision, reads it as its chooser, and answers it.
    pub fn step(&mut self, session: &mut Session) -> Result<Progress, BotError> {
        let body = self.send(
            session,
            json!({ "v": PROTOCOL_VERSION, "kind": "advance", "body": {} }),
        )?;
        match body["status"].as_str() {
            Some("game_over") => {
                return Ok(Progress::GameOver(
                    body["winner"].as_u64().map(|w| w as u32),
                ));
            }
            Some("decision_open") => {}
            Some("still_running") => return Ok(Progress::Running),
            _ => return Err(self.failure(&body)),
        }
        let seat = body["decision"]["player"]
            .as_u64()
            .ok_or_else(|| protocol("a Decision names no player"))? as u32;
        let id =
            id_number(&body["decision"]["id"]).ok_or_else(|| protocol("a Decision has no id"))?;
        if self.last_decision.is_some_and(|last| id <= last) {
            return Err(protocol(format!(
                "Decision id {id} did not go up from {:?}",
                self.last_decision
            )));
        }
        self.last_decision = Some(id);
        self.seats.insert(seat);

        for watcher in self.watchers.clone() {
            self.poll(session, watcher)?;
        }
        let polled = self.poll(session, seat)?;
        let decision = polled["decision"].clone();
        if decision["id"] != body["decision"]["id"] {
            return Err(protocol("the chooser's poll shows a different Decision"));
        }

        let reply = self.reply_for(seat, &decision);
        if self.injecting {
            self.send_illegal_answers(session, seat, &decision, &reply.value)?;
        }
        let answered = self.answer(session, seat, &decision, &reply)?;
        self.answers += 1;
        if answered["status"] == "game_over" {
            return Ok(Progress::GameOver(
                answered["winner"].as_u64().map(|w| w as u32),
            ));
        }
        Ok(Progress::Answered)
    }

    fn flush(&mut self, session: &mut Session) -> Result<(), BotError> {
        if !self.record {
            return Ok(());
        }
        let mut seats: Vec<u32> = self.seats.iter().copied().collect();
        seats.extend(self.watchers.iter().copied());
        for seat in seats {
            self.poll(session, seat)?;
        }
        Ok(())
    }

    fn send(&mut self, session: &mut Session, request: Value) -> Result<Value, BotError> {
        let bytes = serde_json::to_vec(&request).expect("a request holds only JSON values");
        let reply = session.handle(&bytes);
        self.hash = fnv(fnv(self.hash, &bytes), &reply);
        let value: Value = serde_json::from_slice(&reply)
            .map_err(|e| protocol(format!("a response is not JSON: {e}")))?;
        Ok(value["body"].clone())
    }

    fn poll(&mut self, session: &mut Session, seat: u32) -> Result<Value, BotError> {
        let body = self.send(
            session,
            json!({ "v": PROTOCOL_VERSION, "kind": "poll", "seat": seat, "body": {} }),
        )?;
        if body["status"] == "error" {
            return Err(self.failure(&body));
        }
        self.take_events(seat, &body);
        Ok(body)
    }

    fn failure(&self, body: &Value) -> BotError {
        let error = &body["error"];
        let text = format!("{}: {}", error["code"], error["message"]);
        if error["code"] == "pipeline_error" {
            BotError::Fatal(text)
        } else {
            protocol(text)
        }
    }

    fn pool(&mut self, seat: u32) -> &mut Pool {
        self.pools.entry(seat).or_default()
    }

    // A payment plan may spend all but the last point of life.
    fn life_budget(&self, seat: u32) -> u32 {
        let life = self.starting_life + self.life.get(&seat).copied().unwrap_or(0);
        (life - 1).max(0) as u32
    }

    fn take_events(&mut self, seat: u32, body: &Value) {
        let Some(events) = body["events"].as_array() else {
            return;
        };
        for event in events {
            match event["kind"].as_str() {
                Some("mana_added") if event["player"].as_u64() == Some(u64::from(seat)) => {
                    if let Some(units) = event["units"].as_array() {
                        let pool = self.pool(seat);
                        units.iter().for_each(|unit| pool.add(unit));
                    }
                }
                Some("life_changed") if event["player"].as_u64() == Some(u64::from(seat)) => {
                    *self.life.entry(seat).or_default() += event["delta"].as_i64().unwrap_or(0);
                }
                // Damage to a player is not a `life_changed` event.
                // TODO: gaining life emits no event, so the tracked life can sit below the real one
                Some("damage_dealt")
                    if event["target"]["kind"] == "player"
                        && event["target"]["id"].as_u64() == Some(u64::from(seat)) =>
                {
                    *self.life.entry(seat).or_default() -= event["amount"].as_i64().unwrap_or(0);
                }
                // CR 500.5
                Some("step_started" | "phase_started") => *self.pool(seat) = Pool::default(),
                Some("zone_change") => {
                    if let (Some(object), Some(cost)) = (
                        event["object"].as_str(),
                        event["identity"]["mana_cost"].as_str(),
                    ) {
                        self.costs.insert(object.to_string(), cost.to_string());
                    }
                }
                _ => {}
            }
        }
        if self.record {
            self.events
                .entry(seat)
                .or_default()
                .extend(events.iter().cloned());
        }
    }

    // A rejected reply is retried once with a fallback that is always legal for its shape.
    fn answer(
        &mut self,
        session: &mut Session,
        seat: u32,
        decision: &Value,
        reply: &Reply,
    ) -> Result<Value, BotError> {
        let sent = self.send_answer(session, seat, decision, &reply.value)?;
        let Some(message) = rejection(&sent) else {
            self.accepted(seat, reply, &sent);
            return Ok(sent);
        };
        self.rejected += 1;
        let fallback = match decision["shape"].as_str() {
            Some("action") => json!({ "kind": "action", "action": { "kind": "pass_priority" } }),
            Some("payment") => json!({ "kind": "payment", "plan": null }),
            _ => {
                return Err(BotError::Rejected(format!(
                    "{message} (shape {}, reply {})",
                    decision["shape"], reply.value
                )));
            }
        };
        let sent = self.send_answer(session, seat, decision, &fallback)?;
        if let Some(again) = rejection(&sent) {
            return Err(BotError::Rejected(format!(
                "the fallback was rejected too: {again}"
            )));
        }
        self.accepted(
            seat,
            &Reply {
                value: fallback,
                spent: None,
            },
            &sent,
        );
        Ok(sent)
    }

    fn send_illegal_answers(
        &mut self,
        session: &mut Session,
        seat: u32,
        decision: &Value,
        legal: &Value,
    ) -> Result<(), BotError> {
        for illegal in illegal_replies(decision, legal) {
            let before = session.snapshot();
            let request = json!({
                "v": PROTOCOL_VERSION,
                "kind": "answer",
                "seat": seat,
                "body": { "decision": decision["id"], "reply": illegal },
            });
            let body = self.send(session, request)?;
            let context = format!(
                "Decision {} ({}), reply {illegal}",
                decision["id"], decision["shape"]
            );
            if rejection(&body).is_none() {
                return Err(BotError::NotAtomic(format!("accepted: {context}")));
            }
            if body["decision"]["id"] != decision["id"] {
                return Err(BotError::NotAtomic(format!(
                    "the Decision did not stay open: {context}"
                )));
            }
            let after = session.snapshot();
            if before != after {
                return Err(BotError::NotAtomic(format!(
                    "the engine changed: {context}"
                )));
            }
            self.illegal_rejected += 1;
            let key = format!(
                "{} ({})",
                decision["shape"].as_str().unwrap_or_default(),
                decision["mechanic"].as_str().unwrap_or_default()
            );
            *self.illegal_by_shape.entry(key).or_default() += 1;
        }
        Ok(())
    }

    fn send_answer(
        &mut self,
        session: &mut Session,
        seat: u32,
        decision: &Value,
        reply: &Value,
    ) -> Result<Value, BotError> {
        let request = json!({
            "v": PROTOCOL_VERSION,
            "kind": "answer",
            "seat": seat,
            "body": { "decision": decision["id"], "reply": reply },
        });
        let body = self.send(session, request)?;
        if body["status"] == "error" {
            return Err(self.failure(&body));
        }
        Ok(body)
    }

    fn accepted(&mut self, seat: u32, reply: &Reply, body: &Value) {
        if let Some(spent) = &reply.spent {
            self.pool(seat).spend(spent);
        }
        self.take_events(seat, body);
    }

    fn reply_for(&mut self, seat: u32, decision: &Value) -> Reply {
        let plain = |value: Value| Reply { value, spent: None };
        match decision["shape"].as_str().unwrap_or_default() {
            "pick" => plain(self.pick(decision)),
            "yes_no" => plain(json!({ "kind": "yes_no", "value": self.rng.chance(1, 5) })),
            "order" => {
                let count = decision["items"].as_array().map_or(0, Vec::len);
                let mut order: Vec<usize> = (0..count).collect();
                self.shuffle(&mut order);
                plain(json!({ "kind": "order", "order": order }))
            }
            "modes" => {
                let modes = self.modes(
                    &decision["mode_count"],
                    &decision["min"],
                    &decision["max"],
                    &decision["solo_only"],
                );
                plain(json!({ "kind": "modes", "modes": modes }))
            }
            "partition" => plain(self.partition(decision)),
            "payment" => self.payment(seat, decision),
            "targets" => plain(self.targets(decision)),
            "may_cast" => plain(json!({ "kind": "may_cast", "cast": null })),
            "action" => plain(self.action(seat, decision)),
            "declare_attackers" => plain(self.attackers(decision)),
            "declare_blockers" => plain(self.blockers(decision)),
            "assign_damage" => plain(assign_damage(decision)),
            other => plain(json!({ "kind": other })),
        }
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.rng.below(i + 1);
            items.swap(i, j);
        }
    }

    // A flexible range is a coin flip between the minimum and a random count.
    fn count(&mut self, min: usize, max: Option<usize>, len: usize) -> usize {
        let upper = max.unwrap_or(len).min(len);
        let lower = min.min(upper);
        if upper > lower && self.rng.chance(1, 2) {
            lower + self.rng.below(upper - lower + 1)
        } else {
            lower
        }
    }

    fn sample(&mut self, items: &[Value], count: usize) -> Vec<Value> {
        let mut indices: Vec<usize> = (0..items.len()).collect();
        self.shuffle(&mut indices);
        indices
            .into_iter()
            .take(count)
            .map(|i| items[i].clone())
            .collect()
    }

    fn pick(&mut self, decision: &Value) -> Value {
        let candidates = decision["candidates"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let min = decision["min"].as_u64().unwrap_or(0) as usize;
        let max = decision["max"].as_u64().map(|m| m as usize);
        let count = self.count(min, max, candidates.len());
        json!({ "kind": "pick", "picks": self.sample(&candidates, count) })
    }

    /// Distinct mode indices in ascending order, which is legal even for a repeatable
    /// range. A `solo_only` mode is picked only alone.
    // TODO: a pawprint budget is read as a plain count. CR 700.2i
    fn modes(
        &mut self,
        mode_count: &Value,
        min: &Value,
        max: &Value,
        solo_only: &Value,
    ) -> Vec<u64> {
        let total = mode_count.as_u64().unwrap_or(0) as usize;
        let (min, max) = (
            min.as_u64().unwrap_or(0) as usize,
            max.as_u64().unwrap_or(0) as usize,
        );
        let solo: Vec<u64> = solo_only
            .as_array()
            .map(|list| list.iter().filter_map(Value::as_u64).collect())
            .unwrap_or_default();
        let joinable = (0..total).filter(|&i| !solo.contains(&(i as u64))).count();
        let mut count = self.count(min, Some(max), total);
        if count >= 2 {
            count = count.min(joinable);
        }
        let mut indices: Vec<usize> = (0..total)
            .filter(|&i| count < 2 || !solo.contains(&(i as u64)))
            .collect();
        self.shuffle(&mut indices);
        let mut chosen: Vec<u64> = indices.into_iter().take(count).map(|i| i as u64).collect();
        chosen.sort_unstable();
        chosen
    }

    fn partition(&mut self, decision: &Value) -> Value {
        let cards = decision["cards"].as_array().cloned().unwrap_or_default();
        let (mut top, mut rest) = (Vec::new(), Vec::new());
        for card in cards {
            if self.rng.chance(1, 2) {
                top.push(card)
            } else {
                rest.push(card)
            }
        }
        self.shuffle(&mut top);
        json!({ "kind": "partition", "top": top, "rest": rest })
    }

    fn targets(&mut self, decision: &Value) -> Value {
        let words = decision["words"].as_array().cloned().unwrap_or_default();
        let lists: Vec<Value> = words
            .iter()
            .map(|word| {
                let candidates = word["candidates"].as_array().cloned().unwrap_or_default();
                let min = word["min"].as_u64().unwrap_or(0) as usize;
                let max = word["max"].as_u64().map(|m| m as usize);
                let count = self.count(min, max, candidates.len());
                Value::Array(self.sample(&candidates, count))
            })
            .collect();
        json!({ "kind": "targets", "targets": lists })
    }

    fn slot_targets(&mut self, slots: &[Value]) -> Option<Vec<Value>> {
        let mut chosen = Vec::new();
        for slot in slots {
            let candidates = slot["candidates"].as_array().cloned().unwrap_or_default();
            let min = slot["min"].as_u64().unwrap_or(0) as usize;
            if candidates.len() < min {
                return None;
            }
            let max = slot["max"].as_u64().map(|m| m as usize);
            let count = self.count(min, max, candidates.len());
            chosen.extend(self.sample(&candidates, count));
        }
        Some(chosen)
    }

    fn x_value(&mut self, cost: Option<&str>, can_pay: &dyn Fn(&str, u32) -> bool) -> Value {
        let most = match cost {
            Some(cost) => (0..3).rev().find(|x| can_pay(cost, *x)).unwrap_or(0),
            None => 2,
        };
        json!(self.rng.below(most as usize + 1))
    }

    fn cast(&mut self, entry: &Value, x_value: Value) -> Option<Value> {
        let alternative = if entry["normal"] == true {
            Value::Null
        } else {
            entry["alternative_costs"].get(0)?.clone()
        };
        let (modes, slots): (Vec<u64>, Vec<Value>) = match entry["modes"].as_object() {
            None => (
                Vec::new(),
                entry["targets"]["slots"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            ),
            Some(range) => {
                let modes = self.modes(
                    &range["mode_count"],
                    &range["min"],
                    &range["max"],
                    range.get("solo_only").unwrap_or(&Value::Null),
                );
                let per_mode = entry["targets"]["modes"].as_array()?;
                let slots = modes
                    .iter()
                    .flat_map(|m| {
                        per_mode[*m as usize]
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                    })
                    .collect();
                (modes, slots)
            }
        };
        // TODO: pick X and targets together; an `x_dependent` slot lists targets legal for some X, so the engine can reject the pair
        let targets = self.slot_targets(&slots)?;
        Some(json!({
            "kind": "cast_spell",
            "card": entry["card"],
            "modes": modes,
            "targets": targets,
            "x_value": x_value,
            "alternative_cost": alternative,
            "pay_buyback": false,
        }))
    }

    fn activate(&mut self, entry: &Value) -> Option<Value> {
        let slots = entry["targets"].as_array().cloned().unwrap_or_default();
        let targets = self.slot_targets(&slots)?;
        let chosen_colors: Vec<Value> = match entry["color_choice"].as_object() {
            None => Vec::new(),
            Some(choice) => {
                let colors = choice["colors"].as_array()?.clone();
                let count = choice["count"].as_u64().unwrap_or(0) as usize;
                (0..count)
                    .map(|_| colors[self.rng.below(colors.len())].clone())
                    .collect()
            }
        };
        let x_value = if entry["x"] == true {
            json!(self.rng.below(3))
        } else {
            Value::Null
        };
        Some(json!({
            "kind": "activate_ability",
            "source": entry["source"],
            "ability_index": entry["ability_index"],
            "targets": targets,
            "chosen_colors": chosen_colors,
            "x_value": x_value,
        }))
    }

    /// Picks one priority entry: casts what the pool covers, taps mana sources while a card
    /// in reach needs more, and otherwise passes.
    fn action(&mut self, seat: u32, decision: &Value) -> Value {
        let pool = *self.pool(seat);
        let budget = self.life_budget(seat);
        let can_pay = |cost: &str, x: u32| {
            let cost = json!({ "mana": resolve_x(cost, x), "additional": [] });
            plan_payment(&cost, &pool, budget, None).is_some()
        };
        let entries = decision["entries"].as_array().cloned().unwrap_or_default();
        let sources = entries
            .iter()
            .filter(|e| e["kind"] == "activate" && e["mana_ability"] == true)
            .count() as u32;

        // A mana ability's weight stays `None` until `wants_mana` is known.
        let mut options: Vec<(Option<usize>, Value, Option<Value>)> = Vec::new();
        let mut affordable = false;
        let mut wants_mana = false;
        for entry in &entries {
            match entry["kind"].as_str() {
                Some("play_land") => options.push((
                    Some(12),
                    json!({ "kind": "play_land", "card": entry["card"] }),
                    None,
                )),
                Some("cast") => {
                    let cost = entry["card"]
                        .as_str()
                        .and_then(|card| self.costs.get(card))
                        .cloned();
                    let weight = match &cost {
                        None => 2,
                        Some(cost) if can_pay(cost, 0) => {
                            affordable = true;
                            10
                        }
                        Some(cost) => {
                            wants_mana |=
                                mana_value(cost).is_some_and(|mv| mv <= pool.total() + sources);
                            0
                        }
                    };
                    if weight > 0 {
                        let x_value = if entry["x"] == true {
                            self.x_value(cost.as_deref(), &can_pay)
                        } else {
                            Value::Null
                        };
                        if let Some(action) = self.cast(entry, x_value) {
                            options.push((Some(weight), action, None));
                        }
                    }
                }
                Some("activate") => {
                    if let Some(action) = self.activate(entry) {
                        let weight = if entry["mana_ability"] == true {
                            None
                        } else {
                            Some(2)
                        };
                        options.push((weight, action, Some(entry["source"].clone())));
                    }
                }
                _ => {}
            }
        }
        let mana_weight = if wants_mana { 10 } else { 2 };
        let options: Vec<(usize, Value, Option<Value>)> = options
            .into_iter()
            .map(|(weight, action, source)| (weight.unwrap_or(mana_weight), action, source))
            .collect();
        let pass_weight = if affordable {
            1
        } else if wants_mana {
            2
        } else {
            6
        };

        let total: usize = options.iter().map(|(w, ..)| w).sum::<usize>() + pass_weight;
        let mut roll = self.rng.below(total);
        self.last_source = None;
        for (weight, action, source) in options {
            if roll < weight {
                self.last_source = source;
                return json!({ "kind": "action", "action": action });
            }
            roll -= weight;
        }
        json!({ "kind": "action", "action": { "kind": "pass_priority" } })
    }

    fn attackers(&mut self, decision: &Value) -> Value {
        let entries = decision["attackers"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut attacks = Vec::new();
        for entry in entries {
            let defenders = entry["defenders"].as_array().cloned().unwrap_or_default();
            let required = entry["required"].as_bool().unwrap_or(false);
            if !defenders.is_empty() && (required || self.rng.chance(1, 2)) {
                let defender = defenders[self.rng.below(defenders.len())].clone();
                attacks.push(json!({ "attacker": entry["attacker"], "defender": defender }));
            }
        }
        json!({ "kind": "declare_attackers", "attackers": attacks })
    }

    fn blockers(&mut self, decision: &Value) -> Value {
        let entries = decision["blockers"].as_array().cloned().unwrap_or_default();
        let mut blocks = Vec::new();
        for entry in entries {
            let attackers = entry["attackers"].as_array().cloned().unwrap_or_default();
            if !attackers.is_empty() && self.rng.chance(1, 3) {
                let attacker = attackers[self.rng.below(attackers.len())].clone();
                blocks.push(json!({ "blocker": entry["blocker"], "attacker": attacker }));
            }
        }
        json!({ "kind": "declare_blockers", "blocks": blocks })
    }

    fn payment(&mut self, seat: u32, decision: &Value) -> Reply {
        let pool = *self.pool(seat);
        match plan_payment(
            &decision["cost"],
            &pool,
            self.life_budget(seat),
            self.last_source.as_ref(),
        ) {
            Some((payments, spent)) => Reply {
                value: json!({ "kind": "payment", "plan": { "payments": payments } }),
                spent: Some(spent),
            },
            None => Reply {
                value: json!({ "kind": "payment", "plan": null }),
                spent: None,
            },
        }
    }
}

struct Reply {
    value: Value,
    spent: Option<Pool>,
}

fn protocol(message: impl Into<String>) -> BotError {
    BotError::Protocol(message.into())
}

fn rejection(body: &Value) -> Option<String> {
    let error = body.get("error")?;
    Some(format!("{}: {}", error["code"], error["message"]))
}

// CR 510.1c
// All damage to the first recipient is always a legal split.
fn assign_damage(decision: &Value) -> Value {
    let splits = decision["splits"].as_array().cloned().unwrap_or_default();
    let assignments: Vec<Value> = splits
        .iter()
        .map(|split| {
            let recipients = split["recipients"].as_array().cloned().unwrap_or_default();
            let power = split["power"].as_u64().unwrap_or(0);
            let given: Vec<Value> = recipients
                .iter()
                .enumerate()
                .map(|(i, recipient)| json!({ "recipient": recipient, "amount": if i == 0 { power } else { 0 } }))
                .collect();
            json!({ "source": split["source"], "splits": given })
        })
        .collect();
    json!({ "kind": "assign_damage", "assignments": assignments })
}
