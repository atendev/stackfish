---
id: 071
title: Move the targeting and layer tests the scenario schema can express to TOML
type: task
mode: HITL
status: closed
blocked_by: [070]
claimed: false
---

## Question

Move the roughly 20 to 30 Rust tests in `targeting_basics`, `targeting_up_to_n`, `targeting_at_least_n` and `layer_engine_basics` whose setup and assertions the scenario schema already covers into TOML scenarios, and delete the Rust copies.

## Notes

- Optional. Found by 060. Waits on 070 so the moved tests keep useful failure messages.
- Move only tests that end in assertions the schema has (life, zone, tapped, counters and the like). Anything checking effective characteristics or specific errors stays in Rust.

## Resolution

Four of the 32 tests moved, not the 20 to 30 the ticket estimated. The other 28 end in assertions the scenario schema lacks:
- the rejection variant (`expect_error` accepts any error),
- an exact `damage_marked`,
- `Fizzled` or `StackObjectResolved` events,
- a stack object's targets,
- effective power, toughness and keywords.
Others need a mid-test state edit to make a target leave play, or a per-effect timestamp. `docs/implementation/99-backlog.md` lists these gaps.

The four moves:
- `casting_with_zero_targets_is_legal_under_up_to_n` became `targeting/targeting_up_to_n.toml`. The TOML checks that the spell is in the graveyard, not that the graveyard holds exactly one card. The graveyard starts empty, so the two checks are the same here.
- `a_granted_indestructible_survives_lethal_damage_via_sba` became `layers/layer_engine_granted_indestructible.toml`.
- `with_the_boost_registered_the_same_creature_survives_sba` was deleted. `layers/layer_engine_basics.toml` already covers it.
- `a_modify_pt_boost_saves_a_creature_from_the_lethal_toughness_sba` was deleted. It was the unboosted control, a 2/2 with 2 damage dying, and `sba/sba_basics.toml` already covers that.

`resolution_succeeds_when_target_remains_legal` stays in Rust. Its TOML counterpart casts a bolt with no effect, so a fizzle would leave the same zones.

A manual check flipped `up_to` to false and removed the Indestructible grant. Both files failed with their entries named. Built in `76fc094` on `phase5/migrate-fitting-tests-to-toml`.
