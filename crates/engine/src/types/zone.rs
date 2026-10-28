use std::collections::VecDeque;

use crate::{ObjectId, PlayerId};

/// CR 400. `Stack` has no `Zone`: stack objects carry targets, modes and
/// costs paid, so `GameState` tracks them separately.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ZoneKind {
    Library,
    Hand,
    Battlefield,
    Graveyard,
    Stack,
    Exile,
    Command,
}

/// Per-player zones have `owner = Some(_)`; shared zones have `None`.
/// `contents` is the only record of order. `GameObject` holds just the
/// `ZoneKind`.
#[derive(Clone, PartialEq, Debug)]
pub struct Zone {
    pub kind: ZoneKind,
    pub owner: Option<PlayerId>,
    pub contents: VecDeque<ObjectId>,
}

impl Zone {
    pub fn new(kind: ZoneKind, owner: Option<PlayerId>) -> Self {
        Zone { kind, owner, contents: VecDeque::new() }
    }

    pub fn push_top(&mut self, object: ObjectId) {
        self.contents.push_front(object);
    }

    pub fn push_bottom(&mut self, object: ObjectId) {
        self.contents.push_back(object);
    }

    pub fn remove(&mut self, object: ObjectId) -> Option<ObjectId> {
        let idx = self.contents.iter().position(|&id| id == object)?;
        self.contents.remove(idx)
    }

    pub fn contains(&self, object: ObjectId) -> bool {
        self.contents.contains(&object)
    }

    pub fn len(&self) -> usize {
        self.contents.len()
    }

    pub fn top(&self) -> Option<ObjectId> {
        self.contents.front().copied()
    }
}
