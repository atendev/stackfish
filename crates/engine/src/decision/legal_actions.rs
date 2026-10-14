//! `Engine::legal_actions` and its combat counterparts. These list the entries
//! a Decision offers. `open_decision` calls them, and a host may call them
//! directly to see what the engine would offer right now.

use crate::commands;
use crate::engine::Engine;
use crate::types::ids::PlayerId;

use super::{AttackerEntry, BlockerEntry, DamageSplitEntry, Entry};

impl Engine {
    /// Everything the player who holds priority may do: play a land, cast a
    /// card, activate an ability or pass. `Pass` is always last. An entry is
    /// listed when timing, zone, restrictions and target existence allow it.
    /// The list does not say whether the player can pay (ticket 003).
    pub fn legal_actions(&self) -> Vec<Entry> {
        commands::priority_entries(self.state(), self.script_runtime())
    }

    /// The active player's possible attackers while attackers can be
    /// declared, each with the defenders it may attack. Empty otherwise.
    pub fn legal_attackers(&self) -> Vec<AttackerEntry> {
        commands::attacker_entries(self.state(), self.script_runtime())
    }

    /// `player`'s possible blockers while they still owe a declaration, each
    /// with the attackers it may block. Empty otherwise.
    pub fn legal_blockers(&self, player: PlayerId) -> Vec<BlockerEntry> {
        commands::blocker_entries(self.state(), self.script_runtime(), player)
    }

    /// The player who chooses next in this combat damage step and the splits
    /// that player still has to choose, or `None` when no split is waiting.
    pub fn pending_damage_splits(&self) -> Option<(PlayerId, Vec<DamageSplitEntry>)> {
        commands::damage_split_entries(self.state(), self.script_runtime())
    }
}
