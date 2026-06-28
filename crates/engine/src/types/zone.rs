use std::collections::VecDeque;

use crate::{ObjectId, PlayerId};

/// Per CR 400. `Command` includes the commander sub-zone in commander games.
/// The Stack is the one exception not represented as a `Zone` here — it
/// needs `StackObject` metadata (targets, modes, costs paid), not just an
/// ordered id list, so it's tracked separately on `GameState`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ZoneKind {
    Library,
    Hand,
    Battlefield,
    Graveyard,
    Stack,
    Exile,
    Command,
}

/// One zone instance. Per-player zones (library/hand/graveyard/command) have
/// `owner = Some(_)`; shared zones (battlefield/exile) have `owner = None`.
/// `contents` is the single source of truth for what's in the zone and in
/// what order — nothing else (notably not `GameObject`) duplicates that
/// ordering. `GameObject` only carries the `ZoneKind` classification, not a
/// `Zone` itself, to avoid embedding this list inside every object in it.
pub struct Zone {
    pub kind: ZoneKind,
    pub owner: Option<PlayerId>,
    pub contents: VecDeque<ObjectId>,
}

impl Zone {
    pub fn new(kind: ZoneKind, owner: Option<PlayerId>) -> Self {
        todo!()
    }

    pub fn push_top(&mut self, object: ObjectId) {
        todo!()
    }

    pub fn push_bottom(&mut self, object: ObjectId) {
        todo!()
    }

    /// Removes `object` from wherever it sits in `contents`, if present.
    pub fn remove(&mut self, object: ObjectId) -> Option<ObjectId> {
        todo!()
    }

    pub fn contains(&self, object: ObjectId) -> bool {
        todo!()
    }

    pub fn len(&self) -> usize {
        todo!()
    }

    pub fn top(&self) -> Option<ObjectId> {
        todo!()
    }
}
