//! Adapter behavior with no Godot types, so tests can drive it from any thread.
//! A `Core` serves only its creating thread and dies on an engine panic. See `docs/DESIGN.md`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::{self, ThreadId};

use std::path::Path;

use stackfish_host::{CardDbCardSource, CardSource, Session};

static OFF_THREAD_DROPS: AtomicUsize = AtomicUsize::new(0);

/// How many `Core`s were dropped on a thread other than their owner's.
pub(crate) fn off_thread_drops() -> usize {
    OFF_THREAD_DROPS.load(Ordering::Relaxed)
}

/// Why a call returned no bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The call came from a thread other than the one that made the instance.
    WrongThread,
    /// The engine panicked. Every later call fails the same way.
    Panicked,
    /// `handle` came before a successful `create`.
    NotCreated,
    /// `create` came after a successful `create`.
    AlreadyCreated,
}

impl Failure {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Failure::WrongThread => "wrong_thread",
            Failure::Panicked => "panicked",
            Failure::NotCreated => "not_created",
            Failure::AlreadyCreated => "already_created",
        }
    }
}

pub(crate) struct Core {
    owner: ThreadId,
    session: Option<Session>,
    dead: bool,
    last_failure: Option<Failure>,
}

impl Default for Core {
    /// The `Core` belongs to the calling thread.
    fn default() -> Core {
        Core {
            owner: thread::current().id(),
            session: None,
            dead: false,
            last_failure: None,
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        if !self.on_owner_thread() {
            OFF_THREAD_DROPS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Core {
    fn on_owner_thread(&self) -> bool {
        thread::current().id() == self.owner
    }

    /// Creates the Session from a `setup` request. A refused setup returns the error
    /// response and leaves no Session, so `create` can be retried.
    pub(crate) fn create(
        &mut self,
        request: &[u8],
        cards: &dyn CardSource,
    ) -> Result<Vec<u8>, Failure> {
        self.guard()?;
        if self.session.is_some() {
            return self.fail(Failure::AlreadyCreated);
        }
        match catch_unwind(AssertUnwindSafe(|| Session::create(request, cards))) {
            Ok(Ok((session, response))) => {
                self.session = Some(session);
                self.succeed(response)
            }
            Ok(Err(response)) => self.succeed(response),
            Err(_) => self.die(),
        }
    }

    /// `create` with cards from the database at `db_path`. A missing or unreadable
    /// database returns a `setup_error` response that names the path.
    pub(crate) fn create_with_db(
        &mut self,
        request: &[u8],
        db_path: &str,
    ) -> Result<Vec<u8>, Failure> {
        self.guard()?;
        if self.session.is_some() {
            return self.fail(Failure::AlreadyCreated);
        }
        // SQLite creates a missing file, which would fail later with no path in the message.
        let opened = if Path::new(db_path).is_file() {
            CardDbCardSource::open(db_path).map_err(|e| e.to_string())
        } else {
            Err("no such file".to_string())
        };
        match opened {
            Ok(source) => self.create(request, &source),
            Err(reason) => self.succeed(setup_error(&format!(
                "could not open card database {db_path:?}: {reason}"
            ))),
        }
    }

    pub(crate) fn handle(&mut self, request: &[u8]) -> Result<Vec<u8>, Failure> {
        self.run(|session| session.handle(request))
    }

    /// The code of the last failure, or an empty string if the last call worked. From a
    /// wrong thread it is always `wrong_thread`, and the stored value stays untouched.
    pub(crate) fn last_error(&self) -> &'static str {
        if !self.on_owner_thread() {
            return Failure::WrongThread.code();
        }
        self.last_failure.map_or("", Failure::code)
    }

    fn run(&mut self, call: impl FnOnce(&mut Session) -> Vec<u8>) -> Result<Vec<u8>, Failure> {
        self.guard()?;
        let Some(session) = self.session.as_mut() else {
            return self.fail(Failure::NotCreated);
        };
        match catch_unwind(AssertUnwindSafe(|| call(session))) {
            Ok(response) => self.succeed(response),
            Err(_) => self.die(),
        }
    }

    // A wrong-thread call must not write to `self`, since another thread owns it.
    fn guard(&mut self) -> Result<(), Failure> {
        if !self.on_owner_thread() {
            return Err(Failure::WrongThread);
        }
        if self.dead {
            return self.fail(Failure::Panicked);
        }
        Ok(())
    }

    fn die<T>(&mut self) -> Result<T, Failure> {
        self.dead = true;
        // The Session is half-updated, and a panic in its drop would escape here, so leak it.
        std::mem::forget(self.session.take());
        self.fail(Failure::Panicked)
    }

    fn fail<T>(&mut self, failure: Failure) -> Result<T, Failure> {
        self.last_failure = Some(failure);
        Err(failure)
    }

    fn succeed(&mut self, response: Vec<u8>) -> Result<Vec<u8>, Failure> {
        self.last_failure = None;
        Ok(response)
    }
}

// No Session exists to build the error response, so build one by hand.
fn setup_error(message: &str) -> Vec<u8> {
    serde_json::json!({
        "v": stackfish_host::PROTOCOL_VERSION,
        "kind": "response",
        "body": {
            "status": "error",
            "error": { "code": "setup_error", "message": message }
        }
    })
    .to_string()
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use stackfish_host::{proof_cards, proof_setup_request};

    fn created_core() -> Core {
        let mut core = Core::default();
        core.create(&proof_setup_request(2, 1), &proof_cards())
            .expect("the setup is valid");
        core
    }

    fn status(response: &[u8]) -> String {
        let value: serde_json::Value =
            serde_json::from_slice(response).expect("a response is JSON");
        value["body"]["status"]
            .as_str()
            .expect("a response has a status")
            .to_string()
    }

    #[test]
    fn create_then_advance_works_on_the_owner_thread() {
        let mut core = created_core();
        let response = core
            .handle(br#"{"v":1,"kind":"advance","body":{}}"#)
            .expect("the call works");
        assert_eq!(status(&response), "decision_open");
        assert_eq!(core.last_error(), "");
    }

    #[test]
    fn a_refused_setup_returns_the_error_response_and_allows_a_retry() {
        let mut core = Core::default();
        let refused = core
            .create(b"not json", &proof_cards())
            .expect("a refusal is a response");
        assert_eq!(status(&refused), "error");
        assert_eq!(core.last_error(), "");
        assert_eq!(core.handle(b"{}"), Err(Failure::NotCreated));
        core.create(&proof_setup_request(2, 1), &proof_cards())
            .expect("the retry works");
    }

    #[test]
    fn handle_before_create_fails_with_not_created() {
        let mut core = Core::default();
        assert_eq!(core.handle(b"{}"), Err(Failure::NotCreated));
        assert_eq!(core.last_error(), "not_created");
    }

    #[test]
    fn a_second_create_fails_and_keeps_the_first_session() {
        let mut core = created_core();
        assert_eq!(
            core.create(&proof_setup_request(2, 2), &proof_cards()),
            Err(Failure::AlreadyCreated)
        );
        assert_eq!(core.last_error(), "already_created");
        assert!(
            core.handle(br#"{"v":1,"kind":"advance","body":{}}"#)
                .is_ok()
        );
        assert_eq!(core.last_error(), "");
    }

    #[test]
    fn calls_from_another_thread_fail_and_change_nothing() {
        // gdext hands GDScript's calling thread a pointer to the instance with no
        // Send check, so the test does the same.
        struct Unchecked(*mut Core);
        unsafe impl Send for Unchecked {}

        let mut core = created_core();
        let pointer = Unchecked(&mut core);
        let (create, handle, last_error) = thread::spawn(move || {
            let pointer = pointer;
            // SAFETY: the owner thread is blocked in `join`, so nothing else touches the Core.
            let core = unsafe { &mut *pointer.0 };
            (
                core.create(&proof_setup_request(2, 3), &proof_cards()),
                core.handle(br#"{"v":1,"kind":"advance","body":{}}"#),
                core.last_error(),
            )
        })
        .join()
        .expect("the thread does not panic");
        assert_eq!(create, Err(Failure::WrongThread));
        assert_eq!(handle, Err(Failure::WrongThread));
        assert_eq!(last_error, "wrong_thread");
        // The owner thread sees no trace of the refused calls, and its Session still runs.
        assert_eq!(core.last_error(), "");
        assert!(
            core.handle(br#"{"v":1,"kind":"advance","body":{}}"#)
                .is_ok()
        );
    }

    #[test]
    fn dropping_on_another_thread_is_counted() {
        struct Unchecked(Core);
        unsafe impl Send for Unchecked {}

        let before = off_thread_drops();
        let core = Unchecked(created_core());
        thread::spawn(move || {
            // Name the whole wrapper first, so the closure captures it and not just the field.
            let core = core;
            drop(core.0);
        })
        .join()
        .expect("the thread does not panic");
        assert!(off_thread_drops() > before);
    }

    #[test]
    fn a_panic_kills_the_instance() {
        let mut core = created_core();
        let result = core.run(|_| panic!("the engine broke"));
        assert_eq!(result, Err(Failure::Panicked));
        assert_eq!(core.last_error(), "panicked");
        assert_eq!(
            core.handle(br#"{"v":1,"kind":"advance","body":{}}"#),
            Err(Failure::Panicked)
        );
        assert_eq!(
            core.create(&proof_setup_request(2, 4), &proof_cards()),
            Err(Failure::Panicked)
        );
    }

    fn fixture_db() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "stackfish-godot-carddb-test-{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "
            CREATE TABLE cards (
                oracle_id TEXT PRIMARY KEY, script_id TEXT UNIQUE, name TEXT NOT NULL,
                mana_cost TEXT, type_line TEXT, oracle_text TEXT, power TEXT, toughness TEXT,
                loyalty TEXT, defense TEXT, has_multiple_faces INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE keywords (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE);
            CREATE TABLE card_keywords (
                oracle_id TEXT NOT NULL, keyword_id INTEGER NOT NULL,
                PRIMARY KEY (oracle_id, keyword_id)
            );
            CREATE TABLE card_scripts (
                id INTEGER PRIMARY KEY AUTOINCREMENT, oracle_id TEXT NOT NULL,
                version INTEGER NOT NULL, script_source TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO cards (oracle_id, script_id, name, mana_cost, type_line, oracle_text, power, toughness)
            VALUES ('alpha-1', 'alpha', 'Alpha Bear', '{1}{G}', 'Creature — Bear', '', '2', '2');
            ",
        )
        .unwrap();
        path
    }

    fn db_setup_request() -> Vec<u8> {
        let deck = serde_json::json!([{ "card": "alpha-1", "count": 40 }]);
        serde_json::to_vec(&serde_json::json!({
            "v": stackfish_host::PROTOCOL_VERSION,
            "kind": "setup",
            "body": {
                "players": [{ "deck": deck }, { "deck": deck }],
                "format": "standard",
                "seed": "1",
            }
        }))
        .unwrap()
    }

    #[test]
    fn create_with_db_opens_an_absolute_path() {
        let db_path = fixture_db();
        let mut core = Core::default();
        let response = core
            .create_with_db(&db_setup_request(), db_path.to_str().unwrap())
            .expect("the call works");
        assert_eq!(status(&response), "created");
        std::fs::remove_file(db_path).unwrap();
    }

    #[test]
    fn create_with_db_names_a_missing_path_and_leaves_no_file() {
        let db_path = std::env::temp_dir().join(format!(
            "stackfish-godot-missing-{}.sqlite",
            std::process::id()
        ));
        let db_path = db_path.to_str().unwrap();
        let mut core = Core::default();
        let response = core
            .create_with_db(&db_setup_request(), db_path)
            .expect("the call works");
        let value: serde_json::Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(value["body"]["error"]["code"], "setup_error");
        assert!(
            value["body"]["error"]["message"]
                .as_str()
                .unwrap()
                .contains(db_path),
            "{value}"
        );
        assert!(!std::path::Path::new(db_path).exists());
        // A refused setup leaves no Session, so `handle` still fails.
        assert_eq!(
            core.handle(br#"{"v":1,"kind":"advance","body":{}}"#),
            Err(Failure::NotCreated)
        );
    }
}
