//! The combat arms of `execute_command`: `declare` for attackers, bands and
//! blockers, `damage` for damage assignment. See `DESIGN.md` for the rest.

mod damage;
mod declare;
mod entries;
mod predicate;

pub(crate) use damage::{apply_assign_combat_damage, execute_assign_combat_damage, validate_assign_combat_damage, DamagePlan};
pub(crate) use declare::mark_effect_blocked;
pub(crate) use declare::{
    apply_declare_attackers, apply_declare_bands, apply_declare_blockers, validate_declare_attackers,
    validate_declare_bands, validate_declare_blockers, AttackersPlan, BandsPlan, BlockersPlan,
};
pub(crate) use entries::{attacker_entries, blocker_entries, damage_split_entries, next_blocking_player};
pub(crate) use predicate::pass_would_skip_a_required_attack;
