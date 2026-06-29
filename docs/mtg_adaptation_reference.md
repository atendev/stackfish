# MTG Engine Adaptation Reference
### (Using a YGOPro-core style architecture, redesigned for Magic-like rules)

> Purpose: A deep technical reference for adapting a deterministic C++ + embedded scripting TCG engine to an MTG-style rules model.

---

## Table of Contents

1. [Executive summary](#executive-summary)
2. [What to keep from the original engine style](#what-to-keep-from-the-original-engine-style)
3. [What must change for MTG](#what-must-change-for-mtg)
4. [Core architecture (target design)](#core-architecture-target-design)
5. [Data model upgrades (detailed)](#data-model-upgrades-detailed)
6. [Action → Command → Event pipeline](#action--command--event-pipeline)
7. [Why naive FIFO queues fail (practical caveat)](#why-naive-fifo-queues-fail-practical-caveat)
8. [Priority, stack, and pass loop](#priority-stack-and-pass-loop)
9. [Continuous effects and layer engine](#continuous-effects-and-layer-engine)
10. [Replacement and prevention engine](#replacement-and-prevention-engine)
11. [State-based actions (SBA) loop](#state-based-actions-sba-loop)
12. [Trigger collection and APNAP ordering](#trigger-collection-and-apnap-ordering)
13. [Cost/payment system design](#costpayment-system-design)
14. [Zones, visibility, and hidden information](#zones-visibility-and-hidden-information)
15. [Scripting model for card-specific behavior](#scripting-model-for-card-specific-behavior)
16. [Determinism, replay, and networking](#determinism-replay-and-networking)
17. [Performance and caching strategies](#performance-and-caching-strategies)
18. [End-to-end worked example](#end-to-end-worked-example)
19. [Migration plan from a YGO-style core](#migration-plan-from-a-ygo-style-core)
20. [Contributor guidance and pitfalls](#contributor-guidance-and-pitfalls)

---

## Executive summary

You can absolutely reuse the **engine style** (authoritative C++ core + embedded script callbacks + deterministic state machine), but you must replace the **rules kernel** with MTG-native systems:

- Priority/pass windows,
- Stack objects (not YGO chain links),
- Continuous effect layers/dependencies,
- Replacement/prevention pre-commit rewriting,
- Repeated state-based actions (SBA) checks,
- Trigger queue + APNAP ordering.

The most important architectural upgrade is a robust data model and event pipeline:
**Player Action → Engine Command → Committed Game Events**.

---

## What to keep from the original engine style

Keep these principles unchanged:

- **Single authoritative game instance** per match/game.
- **Persistent runtime card/object instances** (not re-instantiating per zone access).
- **Embedded scripting** for card-specific behaviors.
- **C++ rules authority** (scripts request actions; core enforces legality).
- **Deterministic execution** and strict ordering.
- **Headless core** with external UI/network host.

---

## What must change for MTG

1. Replace chain model with full **stack + priority system**.
2. Implement full **turn/step framework** with APNAP priority passing.
3. Build **continuous effect layer engine**.
4. Add **replacement/prevention** rewrite pipeline before event commit.
5. Add **SBA fixed-point loop**.
6. Add robust **trigger queue** and APNAP ordering for stack insertion.
7. Support MTG-specific cost/payment complexity.
8. Support strict hidden-information rules and reveal scopes.

---

## Core architecture (target design)

### Modules

1. **Game Instance**
   - Owns entire mutable state for one game.
2. **Object Store**
   - Runtime `GameObject` instances (cards/tokens/spells/abilities).
3. **Zone Containers**
   - Library, Hand, Battlefield, Graveyard, Exile, Stack, Command.
4. **Rules Processor**
   - State machine for phases/steps/priority.
5. **Command Executor**
   - Executes validated engine commands.
6. **Event Bus / Ledger**
   - Immutable committed events for triggers/replay/sync.
7. **Replacement/Prevention Engine**
   - Rewrites proposed events before commit.
8. **Continuous Layer Engine**
   - Computes derived characteristics.
9. **Trigger Engine**
   - Captures and enqueues triggered abilities.
10. **Script Runtime**
   - Embedded Lua VM with bound API.
11. **Protocol Emitter**
   - Sends prompts/results to host.

---

## Data model upgrades (detailed)

## 1) Static definitions vs runtime objects

### `CardDefinition` (immutable, shared)
- Oracle/rules text metadata
- Mana cost, type line, base stats
- Template/script binding identifiers
- Keywords/rules tags

### `GameObject` (mutable, per-instance)
- `object_id` (stable for current existence)
- `definition_id` reference
- owner/controller
- zone + ordering metadata
- counters, damage, tapped, phased, face-down
- attachments/equipment links
- timestamp
- visibility policy
- generated effect links

> Zone change that creates a “new object” should create a new `object_id` (with optional lineage reference).

## 2) Stack item type

### `StackObject`
- `stack_id`
- kind: spell / activated ability / triggered ability / copy
- source/LKI snapshot
- chosen modes/targets/X/divisions
- costs paid / alt-cost metadata
- controller
- resolution callback reference

## 3) Continuous effects

### `ContinuousEffectInstance`
- source id
- layer/sub-layer
- predicate / affected-set policy
- timestamp/dependency info
- duration/end condition
- apply callback or declarative op

## 4) Replacement/prevention effects

### `ReplacementEffectInstance`
- event pattern matcher
- applicability predicate
- rewrite/prevent callback
- optional choice policy
- consumption policy (shields, one-shot)

## 5) Event model

### `GameEvent` (immutable fact)
- `event_id`, `sequence_no`
- type (Cast, MoveZone, Damage, LifeChange, etc.)
- payload (typed fields)
- causal refs (`caused_by_command_id`, `parent_event_id`)
- timestamp/order index

---

## Action → Command → Event pipeline

## Layers of intent/execution/facts

1. **PlayerAction** (high-level intent)
   - “Cast Shock”, “Pass priority”.
2. **EngineCommand** (fully specified executable unit)
   - validated actor, object ids, targets, mode, costs, X value.
3. **GameEvent** (what actually happened)
   - immutable record after legality/replacements/SBA effects.

### Why this split works
- Replay and auditability
- Clean netcode/spectator sync
- Easier debugging and test assertions
- Clean UI prompt cycle boundaries

---

## Why naive FIFO queues fail (practical caveat)

A plain FIFO queue assumes commands are independent. MTG commands are not.

Example failure:
- Queue has “Cast Shock”, then “Attack with creature”.
- But casting opens priority windows, responses, stack resolutions, SBA, triggers.
- Attack command cannot legally execute until stack/priority conditions are satisfied.

### Correct approach
Use an **orchestrated command pipeline**:
- Execute one command,
- enter rules micro-loop (priority/replacements/SBA/triggers),
- only then advance to next legal command opportunity.

---

## Priority, stack, and pass loop

Core loop around non-empty stack/open priority:

1. Determine priority player.
2. Offer legal actions (including Pass).
3. If player acts:
   - create/validate command,
   - push stack object if applicable,
   - reset pass count.
4. If player passes:
   - increment pass count and move priority.
5. If all players pass with non-empty stack:
   - resolve top stack object.
   - run post-resolution micro-loop.
6. If all pass on empty stack:
   - advance step/phase as rules allow.

---

## Continuous effects and layer engine

Implement a deterministic layer engine:

1. Start from base/copiable values.
2. Apply layer groups in strict order.
3. Resolve timestamp/dependency within layers.
4. Materialize derived characteristics.
5. Invalidate/recompute on relevant events.

This engine should answer queries like:
- “Current power/toughness?”
- “Can this be targeted by X?”
- “Does this have flying right now?”
- “Is this still a creature?”

---

## Replacement and prevention engine

Before committing a proposed event:

1. Build candidate replacement/prevention list.
2. Apply rules for chooser/order where multiple apply.
3. Rewrite, prevent, or transform event.
4. Re-evaluate if rewrite generates new replaceable event forms.
5. Commit final event(s) only after rewrite stabilization.

This must happen before state mutation is finalized.

---

## State-based actions (SBA) loop

After any relevant mutation/resolution:

1. Evaluate all SBAs.
2. Apply all applicable SBAs simultaneously (rules-consistent batch).
3. If any change occurred, repeat SBA check.
4. Once stable, collect triggers from resulting events/state transitions.
5. Put triggers on stack (APNAP order), then resume priority.

This is a fixed-point loop, not a single pass.

---

## Trigger collection and APNAP ordering

### Trigger flow

1. Events occur and are logged.
2. Trigger engine matches triggered abilities.
3. Pending triggers queued with controller/source/LKI.
4. During trigger placement window, order by APNAP:
   - Active player’s triggers first (their chosen internal order),
   - then next players in turn order.
5. Create `StackObject` entries for each trigger.

---

## Cost/payment system design

Use structured costs, not ad-hoc script branches.

### `CostSpec`
- mana vectors (with hybrid/phyrexian/snow/etc. support)
- additional costs (sacrifice, discard, tap)
- alternative costs
- optional additional costs (kicker-like)
- reductions/increases
- “cannot spend mana as though” restrictions

### `PaymentPlan`
- chosen mana sources
- chosen additional payments
- chosen optional costs
- validated atomically before commit

---

## Zones, visibility, and hidden information

Zones should encode semantics:

- **Library**: ordered, hidden.
- **Hand**: hidden by default.
- **Battlefield**: public.
- **Graveyard/Exile**: public (with exceptions handled explicitly).
- **Stack**: public entries with locked choices.
- **Command**: special objects.

Visibility model:
- per-object reveal scopes and durations.
- viewer-aware queries (`GetVisibleState(player)`).
- avoid hidden info leakage through generic API paths.

---

## Scripting model for card-specific behavior

Keep embedded Lua (or similar) and expose MTG-native primitives.

### Script responsibilities
- define card-specific conditions/choices/effects.
- register triggers/activated/static abilities.
- request operations through engine API.

### Core responsibilities
- validate legality and timing.
- enforce rules and hidden information.
- own stack/priority/SBA/replacement semantics.

### Example API surface (conceptual)
- `Game.RegisterTriggeredAbility(...)`
- `Game.AddContinuousEffect(layer, ...)`
- `Game.ReplaceEvent(pattern, ...)`
- `Game.CastSpell(...)`
- `Game.PayCost(...)`
- `Game.SelectTargets(...)`

---

## Determinism, replay, and networking

To keep deterministic behavior:

- deterministic RNG with explicit seed events,
- fully ordered command IDs and event sequence numbers,
- no wall-clock dependence in rules decisions,
- explicit player choice messages (no implicit defaults in hidden states),
- replay by re-running command stream and verifying event hashes.

For networking:
- server/core authoritative,
- clients submit actions,
- server returns prompts/events/state deltas.

---

## Performance and caching strategies

Hotspots:
- layer recomputation,
- legality checks,
- SBA loops,
- trigger matching.

Optimizations:
- event-driven cache invalidation,
- per-object derived characteristic caches,
- indexed object sets by controller/type/zone,
- incremental trigger matching where possible,
- avoid full battlefield scans when event-local checks suffice.

Never sacrifice correctness for micro-optimizations in rules ordering.

---

## End-to-end worked example

Scenario:
- Player A casts a burn spell targeting a 2/2.
- Player B responds with +3/+3 instant.
- Stack resolves.

Flow:
1. Action: “Cast burn spell”.
2. Command validated (target locked, costs paid).
3. Event: `SpellCast`.
4. Priority loop opens; B acts with pump instant.
5. Pump pushed to stack; event emitted.
6. Both pass; pump resolves first.
7. Continuous/temporary modifier applied via effect instance.
8. SBA check (creature survives).
9. Priority loop continues; both pass.
10. Burn resolves; damage event proposed.
11. Replacement/prevention checks run.
12. Damage committed.
13. SBA loop runs (creature may die now or survive).
14. Triggers collected and stacked APNAP.
15. Continue priority/step progression.

---

## Migration plan from a YGO-style core

1. Keep core host/runtime scaffolding.
2. Introduce new MTG zone/stack/priority model.
3. Implement event ledger + command executor.
4. Add replacement/prevention pipeline.
5. Add SBA fixed-point engine.
6. Implement layer/dependency engine.
7. Port script API to MTG semantics.
8. Build conformance test suite from known rulings.
9. Profile and optimize hotspots.
10. Freeze protocol/replay format and backfill tooling.

---

## Contributor guidance and pitfalls

Common pitfalls:
- Treating priority as a simple turn toggle.
- Resolving commands without interleaved rules micro-loops.
- Applying replacements after commit (too late).
- Skipping LKI snapshots.
- Incorrect APNAP ordering for trigger stack insertion.
- Weak identity model causing “same object?” bugs.
- Overexposing hidden information in script API.

Contributor rule of thumb:
> If a change affects timing, visibility, replacement, or SBA order, assume it is high risk and require scenario tests.

---

## Final takeaway

The best adaptation is **not** “rewrite everything” and not “just swap card scripts.”
It is:

- keep the proven deterministic engine style,
- replace the duel rules kernel with MTG-native mechanics,
- enforce a strict Action → Command → Event pipeline,
- make data model/event semantics explicit enough to support layers, SBA, replacements, triggers, and replay correctness.

That yields a scalable, testable, and production-grade MTG-style engine.
