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

    pub fn is_empty(&self) -> bool {
        self.contents.is_empty()
    }

    pub fn top(&self) -> Option<ObjectId> {
        self.contents.front().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_zone_has_kind_owner_and_empty_contents() {
        let owner = PlayerId(0);
        let zone = Zone::new(ZoneKind::Hand, Some(owner));
        assert_eq!(zone.kind, ZoneKind::Hand);
        assert_eq!(zone.owner, Some(owner));
        assert!(zone.contents.is_empty());
    }

    #[test]
    fn push_top_adds_to_front() {
        let mut zone = Zone::new(ZoneKind::Library, Some(PlayerId(0)));
        let a = ObjectId(0);
        let b = ObjectId(1);
        zone.push_top(a);
        zone.push_top(b);
        assert_eq!(zone.contents, VecDeque::from([b, a]));
    }

    #[test]
    fn push_bottom_adds_to_back() {
        let mut zone = Zone::new(ZoneKind::Graveyard, Some(PlayerId(0)));
        let a = ObjectId(0);
        let b = ObjectId(1);
        zone.push_bottom(a);
        zone.push_bottom(b);
        assert_eq!(zone.contents, VecDeque::from([a, b]));
    }

    #[test]
    fn remove_returns_object_and_shifts_others() {
        let mut zone = Zone::new(ZoneKind::Hand, Some(PlayerId(0)));
        let a = ObjectId(0);
        let b = ObjectId(1);
        let c = ObjectId(2);
        zone.push_bottom(a);
        zone.push_bottom(b);
        zone.push_bottom(c);

        assert_eq!(zone.remove(b), Some(b));
        assert_eq!(zone.contents, VecDeque::from([a, c]));
    }

    #[test]
    fn remove_missing_object_returns_none() {
        let mut zone = Zone::new(ZoneKind::Hand, Some(PlayerId(0)));
        zone.push_bottom(ObjectId(0));
        assert_eq!(zone.remove(ObjectId(99)), None);
    }

    #[test]
    fn contains_and_len_track_contents() {
        let mut zone = Zone::new(ZoneKind::Battlefield, None);
        let a = ObjectId(0);
        assert!(!zone.contains(a));
        assert_eq!(zone.len(), 0);

        zone.push_top(a);
        assert!(zone.contains(a));
        assert_eq!(zone.len(), 1);
    }

    #[test]
    fn top_returns_front_or_none() {
        let mut zone = Zone::new(ZoneKind::Library, Some(PlayerId(0)));
        assert_eq!(zone.top(), None);

        let a = ObjectId(0);
        let b = ObjectId(1);
        zone.push_bottom(a);
        zone.push_top(b);
        assert_eq!(zone.top(), Some(b));
    }
}
