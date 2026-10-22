//! `execute_command`'s combat-related arms (`DeclareAttackers`/
//! `DeclareBands`/`DeclareBlockers`/`AssignCombatDamage`), split out of the
//! original monolithic `commands.rs` into `declare`/`damage`.

mod damage;
mod declare;
mod entries;

pub(crate) use damage::{apply_assign_combat_damage, execute_assign_combat_damage, validate_assign_combat_damage, DamagePlan};
pub(crate) use declare::mark_effect_blocked;
pub(crate) use declare::{
    apply_declare_attackers, apply_declare_bands, apply_declare_blockers, validate_declare_attackers,
    validate_declare_bands, validate_declare_blockers, AttackersPlan, BandsPlan, BlockersPlan,
};
pub(crate) use entries::{attacker_entries, blocker_entries, damage_split_entries, next_blocking_player};
