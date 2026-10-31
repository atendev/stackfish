//! Replies a Decision must turn down. `Bot::injecting_illegal_answers` sends
//! these before each real Answer to prove a rejection changes nothing.

use serde_json::{Value, json};

const BOGUS: &str = "999999";

fn bogus_object() -> Value {
    json!({ "kind": "object", "id": BOGUS })
}

/// Illegal replies for a Decision, derived from `legal`, the reply the bot would send. A
/// yes or no has no illegal reply of its shape, so it only gets the wrong-shape one.
pub(crate) fn illegal_replies(decision: &Value, legal: &Value) -> Vec<Value> {
    let wrong_shape = if legal["kind"] == "yes_no" {
        json!({ "kind": "pick", "picks": [] })
    } else {
        json!({ "kind": "yes_no", "value": true })
    };
    let mut replies = vec![wrong_shape];
    let mut extend = |reply: Value| replies.push(reply);
    match legal["kind"].as_str().unwrap_or_default() {
        "pick" => {
            let mut reply = legal.clone();
            reply["picks"]
                .as_array_mut()
                .expect("a pick reply has picks")
                .push(bogus_object());
            extend(reply);
        }
        "order" => {
            let count = decision["items"].as_array().map_or(0, Vec::len);
            let mut reply = legal.clone();
            reply["order"]
                .as_array_mut()
                .expect("an order reply has an order")
                .push(json!(count));
            extend(reply);
        }
        "modes" => {
            let mode_count = decision["mode_count"].as_u64().unwrap_or(0);
            let mut reply = legal.clone();
            reply["modes"]
                .as_array_mut()
                .expect("a modes reply has modes")
                .push(json!(mode_count));
            extend(reply);
        }
        "partition" => {
            let mut reply = legal.clone();
            reply["top"]
                .as_array_mut()
                .expect("a partition reply has a top")
                .push(json!(BOGUS));
            extend(reply);
        }
        "payment" => {
            let units: Vec<Value> = (0..50).map(|_| json!({ "kind": "colorless" })).collect();
            extend(
                json!({ "kind": "payment", "plan": { "payments": [{ "kind": "mana", "units": units }] } }),
            );
        }
        "targets" => {
            let mut reply = legal.clone();
            let lists = reply["targets"]
                .as_array_mut()
                .expect("a targets reply has lists");
            match lists.first_mut().and_then(Value::as_array_mut) {
                Some(first) => first.push(bogus_object()),
                None => lists.push(Value::Array(vec![bogus_object()])),
            }
            extend(reply);
        }
        "may_cast" => {
            extend(
                json!({ "kind": "may_cast", "cast": { "modes": [99], "targets": [], "x_value": null } }),
            );
        }
        "action" => {
            extend(json!({ "kind": "action", "action": { "kind": "play_land", "card": BOGUS } }));
            extend(json!({
                "kind": "action",
                "action": { "kind": "cast_spell", "card": BOGUS, "x_value": null, "alternative_cost": null },
            }));
            extend(json!({
                "kind": "action",
                "action": { "kind": "activate_ability", "source": BOGUS, "ability_index": 0, "x_value": null },
            }));
        }
        "declare_attackers" => {
            let mut reply = legal.clone();
            reply["attackers"]
                .as_array_mut()
                .expect("an attackers reply has attackers")
                .push(json!({ "attacker": BOGUS, "defender": { "kind": "player", "id": 0 } }));
            extend(reply);
        }
        "declare_blockers" => {
            let mut reply = legal.clone();
            reply["blocks"]
                .as_array_mut()
                .expect("a blockers reply has blocks")
                .push(json!({ "blocker": BOGUS, "attacker": "999998" }));
            extend(reply);
        }
        "assign_damage" => {
            let mut reply = legal.clone();
            reply["assignments"]
                .as_array_mut()
                .expect("a damage reply has assignments")
                .push(json!({ "source": BOGUS, "splits": [] }));
            extend(reply);
            // The right creatures, one point too many.
            let mut over = legal.clone();
            if let Some(amount) = over["assignments"]
                .get_mut(0)
                .and_then(|a| a["splits"].get_mut(0))
            {
                let given = amount["amount"].as_u64().unwrap_or(0);
                amount["amount"] = json!(given + 1);
                extend(over);
            }
        }
        _ => {}
    }
    replies
}
