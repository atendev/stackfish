# 16-decision-facade

`crates/engine/src/decision/` is the layer a host talks to instead of the engine's 33 `submit_*` and `decline_*`
methods. Those methods and the typed `ProcessState` pauses are unchanged. The design is in
`.plan/closed/002-decision-answer-shape.md`, `003`, and `005`.

## API

- `Engine::open_decision(&mut self) -> Option<Decision>` builds the open Decision from `ProcessState`. It returns `None`
  while the engine is running, after game over, and before the first `step()` has run pregame setup.
- `Engine::answer(&mut self, player, Answer) -> Result<(), Rejection>` checks and routes an Answer. It does not step the
  engine. The caller steps until the next pause.
- A Decision has an id, the answering player, a display-only mechanic label, and a `ReplyShape`. An Answer is a
  Decision id plus a `Reply`, and each `ReplyShape` has one matching `Reply` variant.

## Reply shapes

| Shape | Reply | Pauses |
| --- | --- | --- |
| `Pick { candidates, min, max }` | `Pick(Vec<Candidate>)` | legend rule, attach choices, Devour, Amplify, discard choice, copy choice, search, script `SelectTarget`, cleanup discard, mulligan bottom, replacement order |
| `YesNo` | `YesNo(bool)` | mulligan (`true` takes one), commander return, script `MayChoose` |
| `Order { items }` | `Order(Vec<usize>)` | trigger order |
| `Modes { mode_count, min, max, repeatable, solo_only }` | `Modes(Vec<u32>)` | spell mode choice, trigger mode choice |
| `Partition { cards, rest }` | `Partition { top, rest }` | Scry, Surveil |
| `Payment { cost }` | `Payment(Option<PaymentPlan>)` | cast payment (`None` unwinds), unless-paid (`None` declines) |
| `Targets { spec, candidates }` | `Targets(Vec<Vec<Target>>)` | trigger targets, one list per target word |
| `MayCast { card }` | `MayCast(Option<CastAnnouncement>)` | Madness, Cascade |
| `Action { entries }` | `Action(PlayerAction)` | priority, checked against the entries (see `17-legal-actions.md`) |
| `DeclareAttackers { attackers }` | `DeclareAttackers(Vec<(ObjectId, Target)>)` | declare attackers step |
| `DeclareBlockers { blockers }` | `DeclareBlockers(Vec<(ObjectId, ObjectId)>)` | declare blockers step, one defending player at a time |
| `AssignDamage { splits }` | `AssignDamage(Vec<(ObjectId, Vec<(ObjectId, u32)>)>)` | combat damage splits |

`Candidate` is `Target(Target) | Replacement(ReplacementCandidate)`. It reuses the engine's `Target`, so it also covers
`Target::Ability`.

## Rejections

Checks run in this order: a Decision is open (`not_open`), the id matches (`stale_decision`), the player matches
(`wrong_seat`), the reply variant matches the shape (`wrong_answer_shape`), then the choice is legal
(`invalid_choice`). The facade checks Pick membership, bounds and duplicates itself. Every other typed engine error maps
to `invalid_choice` with its `Debug` text in the message. The engine's `submit_*` methods leave the pause untouched when
they fail, so a rejected Answer changes nothing and the Decision keeps its id. A priority, attack, block or damage Answer
now also goes through `validate_command` inside `submit_action` (ticket 035), so a reply the entries list but the
engine's rules refuse comes back here as `invalid_choice` instead of failing in `step()`. The one exception is
`internal`: a Madness or Cascade cast that broke an engine invariant. It means the engine is fatal and gives no
"nothing changed" promise (`22-error-split.md`).

## Decision ids

A `DecisionTracker` on `Engine` hands out ids starting at 1. `open_decision` allocates lazily and returns the same id
until an Answer is accepted. The next pause then gets the next id, even if it is the same kind of pause. The Session
takes over the counter in B3.

## Notes for later slices

- `open::describe` has no wildcard arm. A new `ProcessState` variant fails to compile until it is mapped to a shape or
  listed as not waiting on a player.
- Script `SelectTarget` carries a `TargetFilter`, not a list. The facade enumerates legal objects (in id order) and
  players with `target_is_legal`.
- Unit tests build pauses directly in `engine/decision_tests.rs`. `tests/phase5_decision_answer_facade.rs` drives a real
  mulligan, bottom choice and priority passes.
