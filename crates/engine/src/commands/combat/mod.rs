//! `execute_command`'s combat-related arms (`DeclareAttackers`/
//! `DeclareBands`/`DeclareBlockers`/`AssignCombatDamage`), split out of the
//! original monolithic `commands.rs` into `declare`/`damage`.

mod damage;
mod declare;

pub(in crate::commands) use damage::execute_assign_combat_damage;
pub(in crate::commands) use declare::{execute_declare_attackers, execute_declare_bands, execute_declare_blockers};
