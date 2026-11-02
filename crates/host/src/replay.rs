//! The replay file a host exports with `export_replay` and loads with `Replay`: the setup
//! bytes, the accepted Answers, card fingerprints and the engine build. See `docs/DESIGN.md`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::card_source::CardSource;
use crate::envelope::{ErrorCode, PROTOCOL_VERSION, Response};
use crate::session::Session;

/// The `replay_v` of every replay this crate writes. It moves apart from the wire `v`.
pub const REPLAY_VERSION: u32 = 1;

/// Names the engine a replay was recorded on. A replay from another build is refused,
/// because the same Answers can play out differently there.
pub fn engine_build_id() -> String {
    format!(
        "stackfish {} rev {}",
        env!("CARGO_PKG_VERSION"),
        stackfish_engine::BEHAVIOUR_REV
    )
}

/// One accepted `answer` request: its Seat and its body's `decision` and `reply`, verbatim.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecordedAnswer {
    pub seat: u32,
    pub decision: Value,
    pub reply: Value,
}

/// The value of a response's `replay` field. It holds every hidden choice and the seed, so
/// a host must never pass it to a player Seat.
// TODO: add an omniscient stream digest so a loader can catch an Answer swapped for another legal one
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReplayFile {
    pub replay_v: u32,
    pub engine_build: String,
    /// The fingerprint of every key the setup names.
    pub cards: BTreeMap<String, String>,
    /// The `setup` request bytes as the host sent them. It holds the seed.
    pub setup: String,
    pub answers: Vec<RecordedAnswer>,
}

fn refuse(code: ErrorCode, message: impl Into<String>) -> Vec<u8> {
    Response::error(code, message).to_bytes()
}

/// A parsed replay. `Session::from_replay` plays it to its end in one call. A host that
/// steps through a game calls `start`, then sends `answer_request(i)` at each Decision.
pub struct Replay {
    file: ReplayFile,
}

impl Replay {
    /// `Err` holds a `malformed_message` response.
    pub fn parse(bytes: &[u8]) -> Result<Replay, Vec<u8>> {
        serde_json::from_slice(bytes)
            .map(|file| Replay { file })
            .map_err(|e| {
                refuse(
                    ErrorCode::MalformedMessage,
                    format!("not a valid replay: {e}"),
                )
            })
    }

    /// Refuses with `setup_invalid` when the replay version, the engine build or any card's
    /// fingerprint differs from what `cards` serves. Every mismatched key is named.
    pub fn verify(&self, cards: &dyn CardSource) -> Result<(), Vec<u8>> {
        let file = &self.file;
        if file.replay_v != REPLAY_VERSION {
            return Err(refuse(
                ErrorCode::SetupInvalid,
                format!(
                    "replay_v {} is not supported, this Session reads {REPLAY_VERSION}",
                    file.replay_v
                ),
            ));
        }
        let build = engine_build_id();
        if file.engine_build != build {
            return Err(refuse(
                ErrorCode::SetupInvalid,
                format!(
                    "the replay was recorded on engine {:?}, this is {build:?}",
                    file.engine_build
                ),
            ));
        }
        let body = Session::setup_body(file.setup.as_bytes()).map_err(|r| r.to_bytes())?;
        let used = crate::setup::keys(&body);
        let recorded: BTreeSet<String> = file.cards.keys().cloned().collect();
        let mismatched: BTreeSet<&String> =
            used.symmetric_difference(&recorded)
                .chain(used.intersection(&recorded).filter(|key| {
                    cards.fingerprint(key).ok().as_ref() != file.cards.get(key.as_str())
                }))
                .collect();
        if !mismatched.is_empty() {
            let keys: Vec<&str> = mismatched.into_iter().map(String::as_str).collect();
            return Err(refuse(
                ErrorCode::SetupInvalid,
                format!(
                    "card data differs from the recording for {}",
                    keys.join(", ")
                ),
            ));
        }
        Ok(())
    }

    /// Verifies, then builds the Session from the recorded setup with no Answer applied.
    pub fn start(&self, cards: &dyn CardSource) -> Result<(Session, Vec<u8>), Vec<u8>> {
        self.verify(cards)?;
        Session::create(self.file.setup.as_bytes(), cards)
    }

    /// How many Answers the replay recorded, so a stepping host knows when it is done.
    pub fn answer_count(&self) -> usize {
        self.file.answers.len()
    }

    /// The `i`th recorded Answer as `answer` request bytes.
    pub fn answer_request(&self, i: usize) -> Option<Vec<u8>> {
        let answer = self.file.answers.get(i)?;
        let request = serde_json::json!({
            "v": PROTOCOL_VERSION,
            "kind": "answer",
            "seat": answer.seat,
            "body": { "decision": answer.decision, "reply": answer.reply },
        });
        Some(serde_json::to_vec(&request).expect("a JSON value always serializes"))
    }

    pub(crate) fn answers(&self) -> &[RecordedAnswer] {
        &self.file.answers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replay_with_an_unknown_field_is_malformed() {
        let bytes =
            br#"{"replay_v":1,"engine_build":"x","cards":{},"setup":"","answers":[],"extra":1}"#;
        let refused: Value = serde_json::from_slice(&Replay::parse(bytes).err().unwrap()).unwrap();
        assert_eq!(refused["body"]["error"]["code"], "malformed_message");
    }

    #[test]
    fn a_wrong_replay_version_is_refused_before_anything_else() {
        let replay = Replay {
            file: ReplayFile {
                replay_v: 2,
                engine_build: engine_build_id(),
                cards: BTreeMap::new(),
                setup: String::new(),
                answers: Vec::new(),
            },
        };
        let refused = replay.verify(&crate::FixtureCardSource::new()).unwrap_err();
        let refused: Value = serde_json::from_slice(&refused).unwrap();
        assert_eq!(refused["body"]["error"]["code"], "setup_invalid");
        assert!(refused.to_string().contains("replay_v 2"), "{refused}");
    }
}
