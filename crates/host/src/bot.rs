//! A random legal-move bot. It is a client of a `Session`: it sends and reads
//! bytes, parses the JSON, and answers every Decision from the candidates the
//! Decision itself lists. It never reads engine state, so it exercises the same
//! protocol a Godot host uses.
//!
//! The bot is uniform where a choice has no better answer, with two biases so
//! that games finish. At priority it passes more often than it acts, and it
//! plays a land whenever it can. It does not try to win.
//!
//! TODO(proof bot): the bot never accepts a `may_cast` Decision (madness,
//! cascade), because the Decision does not list the cast's modes or targets.
//! TODO(proof bot): it cannot pay a cost that names a sacrifice, a discard, a
//! tap or a graveyard exile, because the Payment Decision does not list the
//! eligible objects. It answers "cannot pay", which unwinds the cast.
//! TODO(proof bot): hybrid, Phyrexian, snow and `{X}` costs get "cannot pay".

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{Value, json};

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

/// The mana a player has, as the bot learns it from `mana_added` events. The
/// Payment Decision does not list the pool.
#[derive(Default, Clone, Copy)]
struct Pool {
    /// White, blue, black, red, green.
    colored: [u32; 5],
    colorless: u32,
}

const COLORS: [&str; 5] = ["white", "blue", "black", "red", "green"];

impl Pool {
    fn total(&self) -> u32 {
        self.colored.iter().sum::<u32>() + self.colorless
    }

    fn add(&mut self, unit: &Value) {
        match unit["kind"].as_str() {
            Some("colored" | "snow_colored") => {
                if let Some(i) = COLORS.iter().position(|c| unit["color"] == *c) {
                    self.colored[i] += 1;
                }
            }
            Some("colorless" | "snow_colorless") => self.colorless += 1,
            // A restricted unit may pay only for some spells, so the bot never spends it.
            _ => {}
        }
    }
}

/// What a plan spends from the pool.
struct Spent {
    colored: [u32; 5],
    colorless: u32,
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
    /// The source of the last ability the bot activated. A self cost
    /// (`tap_self`, `sacrifice_self`, `discard_self`) names it in the plan.
    last_source: Option<Value>,
    last_decision: Option<u64>,
    answers: usize,
    rejected: usize,
    hash: u64,
    injecting: bool,
    illegal_rejected: usize,
    /// Illegal Answers turned down, by the shape of the Decision they answered.
    illegal_by_shape: BTreeMap<String, usize>,
    /// The mana cost of each card the bot has seen, by object id. A move event
    /// carries the identity of every card its Seat may see, including the
    /// Seat's own hand.
    costs: HashMap<String, String>,
    seats: BTreeSet<u32>,
    record: bool,
    watchers: Vec<u32>,
    events: HashMap<u32, Vec<Value>>,
    /// Every request and response, in order, when kept.
    transcript: Option<Vec<Vec<u8>>>,
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
            transcript: None,
        }
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

    /// How many illegal Answers were turned down, by Decision shape.
    pub fn illegal_by_shape(&self) -> &BTreeMap<String, usize> {
        &self.illegal_by_shape
    }

    /// Keeps every request and response, for `transcript`.
    pub fn keeping_transcript(mut self) -> Bot {
        self.transcript = Some(Vec::new());
        self
    }

    /// The requests and responses in order, as bytes. Empty unless
    /// `keeping_transcript` was set.
    pub fn transcript(&self) -> &[Vec<u8>] {
        self.transcript.as_deref().unwrap_or_default()
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
        let body = self.send(session, json!({ "v": PROTOCOL_VERSION, "kind": "advance", "body": {} }))?;
        match body["status"].as_str() {
            Some("game_over") => return Ok(Progress::GameOver(body["winner"].as_u64().map(|w| w as u32))),
            Some("decision_open") => {}
            Some("still_running") => return Ok(Progress::Running),
            _ => return Err(self.failure(&body)),
        }
        let seat = body["decision"]["player"].as_u64().ok_or_else(|| protocol("a Decision names no player"))? as u32;
        let id = id_number(&body["decision"]["id"]).ok_or_else(|| protocol("a Decision has no id"))?;
        if self.last_decision.is_some_and(|last| id <= last) {
            return Err(protocol(format!("Decision id {id} did not go up from {:?}", self.last_decision)));
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
            return Ok(Progress::GameOver(answered["winner"].as_u64().map(|w| w as u32)));
        }
        Ok(Progress::Answered)
    }

    /// Reads whatever each Seat has not received yet, so `events` is complete.
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
        if let Some(transcript) = &mut self.transcript {
            transcript.push(bytes);
            transcript.push(reply.clone());
        }
        let value: Value = serde_json::from_slice(&reply).map_err(|e| protocol(format!("a response is not JSON: {e}")))?;
        Ok(value["body"].clone())
    }

    fn poll(&mut self, session: &mut Session, seat: u32) -> Result<Value, BotError> {
        let body = self.send(session, json!({ "v": PROTOCOL_VERSION, "kind": "poll", "seat": seat, "body": {} }))?;
        if body["status"] == "error" {
            return Err(self.failure(&body));
        }
        self.take_events(seat, &body);
        Ok(body)
    }

    /// A response that is not a normal one, as an error.
    fn failure(&self, body: &Value) -> BotError {
        let error = &body["error"];
        let text = format!("{}: {}", error["code"], error["message"]);
        if error["code"] == "pipeline_error" { BotError::Fatal(text) } else { protocol(text) }
    }

    fn pool(&mut self, seat: u32) -> &mut Pool {
        self.pools.entry(seat).or_default()
    }

    /// Learns the pool from the events a Seat received, and keeps them when
    /// recording.
    fn take_events(&mut self, seat: u32, body: &Value) {
        let Some(events) = body["events"].as_array() else { return };
        for event in events {
            match event["kind"].as_str() {
                Some("mana_added") if event["player"].as_u64() == Some(u64::from(seat)) => {
                    if let Some(units) = event["units"].as_array() {
                        let pool = self.pool(seat);
                        units.iter().for_each(|unit| pool.add(unit));
                    }
                }
                // CR 500.5: mana empties at the end of each step and phase.
                Some("step_started" | "phase_started") => *self.pool(seat) = Pool::default(),
                Some("zone_change") => {
                    if let (Some(object), Some(cost)) = (event["object"].as_str(), event["identity"]["mana_cost"].as_str()) {
                        self.costs.insert(object.to_string(), cost.to_string());
                    }
                }
                _ => {}
            }
        }
        if self.record {
            self.events.entry(seat).or_default().extend(events.iter().cloned());
        }
    }

    /// Sends the answer. A rejection is replaced once by a fallback that is
    /// always legal at its shape.
    fn answer(&mut self, session: &mut Session, seat: u32, decision: &Value, reply: &Reply) -> Result<Value, BotError> {
        let sent = self.send_answer(session, seat, decision, &reply.value)?;
        let Some(message) = rejection(&sent) else {
            self.accepted(seat, reply, &sent);
            return Ok(sent);
        };
        self.rejected += 1;
        let fallback = match decision["shape"].as_str() {
            Some("action") => json!({ "kind": "action", "action": { "kind": "pass_priority" } }),
            Some("payment") => json!({ "kind": "payment", "plan": null }),
            _ => return Err(BotError::Rejected(format!("{message} (shape {}, reply {})", decision["shape"], reply.value))),
        };
        let sent = self.send_answer(session, seat, decision, &fallback)?;
        if let Some(again) = rejection(&sent) {
            return Err(BotError::Rejected(format!("the fallback was rejected too: {again}")));
        }
        self.accepted(seat, &Reply { value: fallback, spent: None }, &sent);
        Ok(sent)
    }

    /// Sends each illegal reply for `decision` and checks it is refused, the
    /// Decision stays open, and the engine is unchanged.
    fn send_illegal_answers(&mut self, session: &mut Session, seat: u32, decision: &Value, legal: &Value) -> Result<(), BotError> {
        for illegal in illegal_replies(decision, legal) {
            let before = session.snapshot();
            let request = json!({
                "v": PROTOCOL_VERSION,
                "kind": "answer",
                "seat": seat,
                "body": { "decision": decision["id"], "reply": illegal },
            });
            let body = self.send(session, request)?;
            let context = format!("Decision {} ({}), reply {illegal}", decision["id"], decision["shape"]);
            if rejection(&body).is_none() {
                return Err(BotError::NotAtomic(format!("accepted: {context}")));
            }
            if body["decision"]["id"] != decision["id"] {
                return Err(BotError::NotAtomic(format!("the Decision did not stay open: {context}")));
            }
            let after = session.snapshot();
            if before != after {
                return Err(BotError::NotAtomic(format!("the engine changed: {context}")));
            }
            self.illegal_rejected += 1;
            *self.illegal_by_shape.entry(decision["shape"].as_str().unwrap_or_default().to_string()).or_default() += 1;
        }
        Ok(())
    }

    fn send_answer(&mut self, session: &mut Session, seat: u32, decision: &Value, reply: &Value) -> Result<Value, BotError> {
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

    /// Bookkeeping after the Session accepted `reply`.
    fn accepted(&mut self, seat: u32, reply: &Reply, body: &Value) {
        if let Some(spent) = &reply.spent {
            let pool = self.pool(seat);
            for (have, used) in pool.colored.iter_mut().zip(spent.colored) {
                *have = have.saturating_sub(used);
            }
            pool.colorless = pool.colorless.saturating_sub(spent.colorless);
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
                let modes = self.modes(&decision["mode_count"], &decision["min"], &decision["max"]);
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

    /// How many to choose from `len` options when the reply takes `min` to
    /// `max` (no `max` means no upper bound). A flexible count is a coin flip
    /// between the minimum and a random count.
    fn count(&mut self, min: usize, max: Option<usize>, len: usize) -> usize {
        let upper = max.unwrap_or(len).min(len);
        let lower = min.min(upper);
        if upper > lower && self.rng.chance(1, 2) { lower + self.rng.below(upper - lower + 1) } else { lower }
    }

    fn sample(&mut self, items: &[Value], count: usize) -> Vec<Value> {
        let mut indices: Vec<usize> = (0..items.len()).collect();
        self.shuffle(&mut indices);
        indices.into_iter().take(count).map(|i| items[i].clone()).collect()
    }

    fn pick(&mut self, decision: &Value) -> Value {
        let candidates = decision["candidates"].as_array().cloned().unwrap_or_default();
        let min = decision["min"].as_u64().unwrap_or(0) as usize;
        let max = decision["max"].as_u64().map(|m| m as usize);
        let count = self.count(min, max, candidates.len());
        json!({ "kind": "pick", "picks": self.sample(&candidates, count) })
    }

    /// Distinct mode indices, in ascending order. A repeatable range still gets
    /// distinct modes, which is always legal.
    ///
    /// TODO(proof bot): a pawprint budget (CR 700.2i) is read as a plain count.
    fn modes(&mut self, mode_count: &Value, min: &Value, max: &Value) -> Vec<u64> {
        let total = mode_count.as_u64().unwrap_or(0) as usize;
        let (min, max) = (min.as_u64().unwrap_or(0) as usize, max.as_u64().unwrap_or(0) as usize);
        let count = self.count(min, Some(max), total);
        let mut indices: Vec<usize> = (0..total).collect();
        self.shuffle(&mut indices);
        let mut chosen: Vec<u64> = indices.into_iter().take(count).map(|i| i as u64).collect();
        chosen.sort_unstable();
        chosen
    }

    fn partition(&mut self, decision: &Value) -> Value {
        let cards = decision["cards"].as_array().cloned().unwrap_or_default();
        let (mut top, mut rest) = (Vec::new(), Vec::new());
        for card in cards {
            if self.rng.chance(1, 2) { top.push(card) } else { rest.push(card) }
        }
        self.shuffle(&mut top);
        json!({ "kind": "partition", "top": top, "rest": rest })
    }

    /// One list of targets per word.
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

    /// Targets for a list of slots, or `None` if a slot lacks candidates.
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

    fn cast(&mut self, entry: &Value) -> Option<Value> {
        let alternative = if entry["normal"] == true { Value::Null } else { entry["alternative_costs"].get(0)?.clone() };
        let (modes, slots): (Vec<u64>, Vec<Value>) = match entry["modes"].as_object() {
            None => (Vec::new(), entry["targets"]["slots"].as_array().cloned().unwrap_or_default()),
            Some(range) => {
                let modes = self.modes(&range["mode_count"], &range["min"], &range["max"]);
                let per_mode = entry["targets"]["modes"].as_array()?;
                let slots = modes.iter().flat_map(|m| per_mode[*m as usize].as_array().cloned().unwrap_or_default()).collect();
                (modes, slots)
            }
        };
        let targets = self.slot_targets(&slots)?;
        let x_value = if entry["x"] == true { json!(self.rng.below(3)) } else { Value::Null };
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
                (0..count).map(|_| colors[self.rng.below(colors.len())].clone()).collect()
            }
        };
        let x_value = if entry["x"] == true { json!(self.rng.below(3)) } else { Value::Null };
        Some(json!({
            "kind": "activate_ability",
            "source": entry["source"],
            "ability_index": entry["ability_index"],
            "targets": targets,
            "chosen_colors": chosen_colors,
            "x_value": x_value,
        }))
    }

    /// Picks one priority entry. A land play is likely and passing is likelier
    /// than most things. The bot casts a card whose cost the pool covers,
    /// taps mana sources while a card in reach needs more mana, and otherwise
    /// passes.
    fn action(&mut self, seat: u32, decision: &Value) -> Value {
        let pool = *self.pool(seat);
        let entries = decision["entries"].as_array().cloned().unwrap_or_default();
        let sources = entries.iter().filter(|e| e["kind"] == "activate" && e["mana_ability"] == true).count() as u32;

        // First pass: the weights that do not depend on what else is on offer.
        // A mana ability is `None` until `wants_mana` is known.
        let mut options: Vec<(Option<usize>, Value, Option<Value>)> = Vec::new();
        let mut affordable = false;
        let mut wants_mana = false;
        for entry in &entries {
            match entry["kind"].as_str() {
                Some("play_land") => options.push((Some(12), json!({ "kind": "play_land", "card": entry["card"] }), None)),
                Some("cast") => {
                    let weight = match entry["card"].as_str().and_then(|card| self.costs.get(card)) {
                        None => 2,
                        Some(cost) if plan_payment(&json!({ "mana": cost, "additional": [] }), &pool, None).is_some() => {
                            affordable = true;
                            10
                        }
                        Some(cost) => {
                            wants_mana |= mana_value(cost).is_some_and(|mv| mv <= pool.total() + sources);
                            0
                        }
                    };
                    if weight > 0
                        && let Some(action) = self.cast(entry)
                    {
                        options.push((Some(weight), action, None));
                    }
                }
                Some("activate") => {
                    if let Some(action) = self.activate(entry) {
                        let weight = if entry["mana_ability"] == true { None } else { Some(2) };
                        options.push((weight, action, Some(entry["source"].clone())));
                    }
                }
                _ => {}
            }
        }
        let mana_weight = if wants_mana { 10 } else { 2 };
        let options: Vec<(usize, Value, Option<Value>)> =
            options.into_iter().map(|(weight, action, source)| (weight.unwrap_or(mana_weight), action, source)).collect();
        let pass_weight = if affordable { 1 } else if wants_mana { 2 } else { 6 };

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
        let entries = decision["attackers"].as_array().cloned().unwrap_or_default();
        let mut attacks = Vec::new();
        for entry in entries {
            let defenders = entry["defenders"].as_array().cloned().unwrap_or_default();
            if !defenders.is_empty() && self.rng.chance(1, 2) {
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

    /// Pays from the tracked pool, or answers "cannot pay".
    fn payment(&mut self, seat: u32, decision: &Value) -> Reply {
        let pool = *self.pool(seat);
        match plan_payment(&decision["cost"], &pool, self.last_source.as_ref()) {
            Some((payments, spent)) => Reply { value: json!({ "kind": "payment", "plan": { "payments": payments } }), spent: Some(spent) },
            None => Reply { value: json!({ "kind": "payment", "plan": null }), spent: None },
        }
    }
}

/// An Answer's reply, and the mana it spends if accepted.
struct Reply {
    value: Value,
    spent: Option<Spent>,
}

fn protocol(message: impl Into<String>) -> BotError {
    BotError::Protocol(message.into())
}

/// The message of a refused Answer: a response that still shows a Decision and
/// carries an error.
fn rejection(body: &Value) -> Option<String> {
    let error = body.get("error")?;
    Some(format!("{}: {}", error["code"], error["message"]))
}

/// Gives all of each creature's damage to its first recipient, which is always
/// legal (CR 510.1c).
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

/// The mana value of a cost of plain symbols, or `None` for a hybrid,
/// Phyrexian, snow or `{X}` cost.
fn mana_value(cost: &str) -> Option<u32> {
    let mut total = 0;
    for part in cost.split('{').skip(1) {
        total += match part.trim_end_matches('}') {
            "W" | "U" | "B" | "R" | "G" | "C" => 1,
            generic => generic.parse::<u32>().ok()?,
        };
    }
    Some(total)
}

/// A plan for `cost` from `pool`, or `None` if the bot cannot make one. Mana
/// comes first, then each additional cost in the order the Decision lists
/// them.
fn plan_payment(cost: &Value, pool: &Pool, source: Option<&Value>) -> Option<(Vec<Value>, Spent)> {
    let mut left = *pool;
    let mut payments = Vec::new();
    let mut spent = Spent { colored: [0; 5], colorless: 0 };

    let mana = cost["mana"].as_str().unwrap_or_default();
    if !mana.is_empty() {
        let mut units = Vec::new();
        let mut generic = 0u32;
        for part in mana.split('{').skip(1) {
            match part.trim_end_matches('}') {
                "C" => {
                    left.colorless = left.colorless.checked_sub(1)?;
                    spent.colorless += 1;
                    units.push(json!({ "kind": "colorless" }));
                }
                symbol => match ["W", "U", "B", "R", "G"].iter().position(|c| *c == symbol) {
                    Some(i) => {
                        left.colored[i] = left.colored[i].checked_sub(1)?;
                        spent.colored[i] += 1;
                        units.push(json!({ "kind": "colored", "color": COLORS[i] }));
                    }
                    None => generic += symbol.parse::<u32>().ok()?,
                },
            }
        }
        for _ in 0..generic {
            if left.colorless > 0 {
                left.colorless -= 1;
                spent.colorless += 1;
                units.push(json!({ "kind": "colorless" }));
            } else {
                let i = (0..5).max_by_key(|i| left.colored[*i]).filter(|i| left.colored[*i] > 0)?;
                left.colored[i] -= 1;
                spent.colored[i] += 1;
                units.push(json!({ "kind": "colored", "color": COLORS[i] }));
            }
        }
        payments.push(json!({ "kind": "mana", "units": units }));
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
