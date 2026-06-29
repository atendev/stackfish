# Stackfish
This is an mtg-engine based on the ygo-pro engine.

## Startup
On every new session, run `/caveman full` before doing anything else.

## Vertical Slice Instructions
- create a branch `phase5/<feature-name>`
- implement the given instructions for the feature.
- As you implement, make sure to mark any stubbed parts that aren't implemented with TODO marks for later.
  - Eg: Implementing PlayLand, we don't have continuous effects for 'play additional lands' effects, so we mark the PlayLand action with a TODO so we know to implement that later.
- make sure to stop and commit at appropritate times.
- When done with implementation, give a detailed report as indicated in the workflow document.
- Don't delete the branch, as I will manually inspect the code after for changes.
- If all is good, I will instruct to merge the branch.

## Notes
Look in the docs folder for the following information:
- [DESIGN.md](/docs/DESIGN.md) | Bigger design document that should be updated as new things are created.
- [implementation.md](/docs/implementation.md) | Notes from your previous sessions about where we are and things yet to be done, mistakes, changes, etc. This is something you should always be checking.
- [workflow.md](/docs/workflow.md) | The main workflow of how we are going to be talking back and forth. Current phase is listed in implementation.
- [ygo_pro_reference.md](/docs/ygo_pro_reference.md) | A reference guide for how the ygo-pro engine works.
- [mtg_adaptation_reference.md](/docs/mtg_adaptation_reference.md) | Reference guide for how some of the ygo-pro engine mechanics can translate over to mtg.
- [comp_rules.md](/docs/comp_rules.md) | The entire comprehensive magic the gathering ruleset. Check this when wondering exactly how any rules interaction is supposed to work.
