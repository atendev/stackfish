# YGOPRO-Core Engine Architecture (edo9300/ygopro-core)

> Repository: `edo9300/ygopro-core`
> Description: *ygopro script engine*
> Primary languages: C++ (97%), C (1.7%), Lua (0.4%), plus build/tooling files.

---

## Table of Contents

1. [What this engine is](#what-this-engine-is)
2. [High-level architecture](#high-level-architecture)
3. [Language roles](#language-roles)
4. [Core runtime model](#core-runtime-model)
5. [Main duel loop (conceptual)](#main-duel-loop-conceptual)
6. [How effects and chains are represented](#how-effects-and-chains-are-represented)
7. [Card scripting lifecycle (Lua)](#card-scripting-lifecycle-lua)
8. [How card scripts call into engine internals](#how-card-scripts-call-into-engine-internals)
9. [How engine events trigger script behavior](#how-engine-events-trigger-script-behavior)
10. [Feature implementation patterns](#feature-implementation-patterns)
11. [State management and rule enforcement](#state-management-and-rule-enforcement)
12. [Determinism and replayability](#determinism-and-replayability)
13. [Networking/UI separation model](#networkingui-separation-model)
14. [Performance characteristics](#performance-characteristics)
15. [Typical flow of a single card effect](#typical-flow-of-a-single-card-effect)
16. [How to read/extend this codebase](#how-to-readextend-this-codebase)
17. [Common pitfalls for contributors](#common-pitfalls-for-contributors)
18. [Mental model summary](#mental-model-summary)

---

## What this engine is

`ygopro-core` is the **deterministic duel simulation core** used by YGOPro-family clients/servers.
Its job is to:

- maintain complete duel state,
- enforce game rules and timing windows,
- execute card scripts,
- resolve chains, damage calculation, and continuous effects,
- produce a stream of game messages/events for an outer host (UI or network layer).

A helpful framing:

- **Core engine** = authoritative rules VM + state machine.
- **Card scripts (Lua)** = declarative/procedural behavior definitions for individual cards.
- **Host/client/server** = transport, UX, deck handling, matchmaking, replay storage, etc.

---

## High-level architecture

At a high level, the engine has these conceptual modules:

1. **Duel context**
   - Owns full state of one duel (players, LP, zones, turn info, chain, phase, flags).
2. **Card objects**
   - Runtime card instances with dynamic state (location, position, controller, counters, flags, relation data).
3. **Effect objects**
   - Runtime effect instances registered by scripts or inherent rules.
   - Include trigger/condition/cost/target/operation hooks.
4. **Field / processor logic**
   - Rule machinery for summoning legality, battle, activation legality, redirection, replacement, etc.
5. **Script bridge**
   - Lua VM integration and C API bridge that exposes `Duel.*`, `Card.*`, `Effect.*`, etc.
6. **Message pipeline**
   - Serializes engine decisions/events into protocol messages for the host.

Think of it as: **event-driven rule processor + scripted effect execution**.

---

## Language roles

### C++ (dominant, ~97%)
Implements almost all of the duel engine internals:

- object model (`duel`, `card`, `effect`, groups),
- rules and state transitions,
- event scheduling and chain handling,
- battle and damage logic,
- script API registration,
- protocol message generation.

### C (~1.7%)
Usually appears around lower-level glue, portability, or legacy-compatible boundaries (depends on exact files in this fork).

### Lua (~0.4%)
Used for **card behavior scripts** (typically external script packs consumed by this core).
Lua scripts define card initialization and effect logic through callback functions.

### Shell/Batch/Makefile
Build tooling and platform-specific automation.

---

## Core runtime model

The engine is effectively a **deterministic state machine** with a queue/processor model:

- Input: player decisions + engine-scheduled events.
- Processing: rule checks, trigger collection, legality filtering, chain/action resolution.
- Output: canonical state mutations + serialized messages.

Important traits:

- **Deterministic**: same initial state + same choices => same result.
- **Authoritative**: scripts can request operations, but core enforces legality.
- **Two-layer logic**:
  1. hardcoded shared rules in C++,
  2. card-specific logic in Lua callbacks.

---

## Main duel loop (conceptual)

A simplified conceptual loop:

1. Initialize duel state and shuffle/draw/start conditions.
2. Enter phase/step progression.
3. Open timing window.
4. Gather legal actions (activate effect, summon, set, etc.).
5. Receive player response/choice.
6. If effect activation:
   - validate conditions/cost/targets,
   - create chain link.
7. On chain resolution:
   - re-check legality where required,
   - execute operation callback,
   - apply continuous/replacement/post-resolution events.
8. Process pending events and trigger effects.
9. Update state-based checks (destroy by rule, invalid equips, etc.).
10. Emit messages and continue until duel end condition.

This is not a simple while-loop in one file; in practice it is implemented as a **processor pipeline/state machine** with many operation codes/states.

---

## How effects and chains are represented

An effect in this engine typically contains:

- **Type** (ignition, trigger, quick, continuous, field, etc.),
- **Code/category** (what it can do / what event it reacts to),
- **Range / property flags**,
- **Condition function** (Lua callback),
- **Cost function**,
- **Target function**,
- **Operation function**,
- count limits and reset conditions.

When activated, it becomes a **chain link** with captured activation context:

- activating player,
- selected targets,
- labels/temporary values,
- relation constraints.

During resolution, engine reads chain link context and executes operation with rules enforcement.

---

## Card scripting lifecycle (Lua)

Most cards follow this pattern:

1. Script file defines `initial_effect(c)`.
2. Inside `initial_effect`, script creates one or more `Effect` objects.
3. Each effect is configured (`SetType`, `SetCode`, `SetRange`, `SetProperty`, etc.).
4. Callback handlers are assigned (`SetCondition`, `SetCost`, `SetTarget`, `SetOperation`).
5. Effect is registered to card (`c:RegisterEffect(e)`).

At runtime:

- Engine loads script (once per card code context, depending on implementation details).
- Calls `initial_effect`.
- Registered effects become part of duel runtime.
- On matching events/timings, engine invokes callbacks in Lua with context.

---

## How card scripts call into engine internals

Lua functions like `Duel.SpecialSummon`, `Duel.SelectMatchingCard`, `Duel.Destroy`, `Card.IsRelateToEffect`, etc. are **bindings** to C++ engine methods.

Flow:

1. Lua callback calls API function.
2. Lua-C bridge validates arguments and converts Lua userdata (Card, Group, Effect).
3. Core C++ performs rule-legal operation or returns failure.
4. Results are pushed back to Lua (boolean, integers, groups, selected cards).

Critical point: **Lua asks; C++ decides legality**.

---

## How engine events trigger script behavior

The core publishes internal events such as:

- summon success/failure variants,
- card sent to grave/banished/returned,
- phase-step transitions,
- attack declarations/damage steps,
- chain events,
- custom event codes.

For each event window:

1. Identify candidate trigger effects.
2. Check activation legality (timing, once-per-turn, negation state, range, etc.).
3. Build possible chain actions.
4. Ask player for optional activations.
5. Resolve in chain order.

This allows flexible card behaviors while preserving timing correctness.

---

## Feature implementation patterns

### 1) Summoning mechanics
Implemented as hard-rule procedures in core, with script hooks for card-specific exceptions/extra conditions.

### 2) Continuous effects
Registered effects that alter properties/rules while active. Engine recalculates or layers these based on location, immunity, and effect precedence.

### 3) Replacement/prevention
Core checks “would happen” transitions and asks replacement effects if applicable before committing state changes.

### 4) Targeting and immunity
Core validates target legality at activation and/or resolution as required by rules text templates and effect properties.

### 5) Reset semantics
Effects and temporary modifiers carry reset flags (phase, event, leave-field, turn owner, etc.) so cleanup is automatic and deterministic.

---

## State management and rule enforcement

Core tracks:

- zones (MZONE, SZONE, GY, hand, deck, extra, banished),
- controller/owner distinctions,
- positions and sequences,
- turn/phase/step,
- chain data,
- pending operations/events,
- flags/counters/relations.

A lot of engine complexity comes from **invariants**:

- card relations must be valid,
- illegal game states must be corrected/prevented,
- effect lifetime must be exact,
- hidden information boundaries must be respected.

---

## Determinism and replayability

The engine is designed to be replay-safe:

- PRNG use is controlled and seeded,
- all meaningful choices are explicit inputs,
- outputs are serialized in ordered protocol messages.

This enables:

- spectator sync,
- server-authoritative validation,
- replay reconstruction from initial seed + decision stream.

---

## Networking/UI separation model

`ygopro-core` is intentionally **headless**:

- no graphics,
- no direct UI logic,
- no matchmaking/lobby assumptions.

Instead it emits duel messages and requires host application to:

- present prompts,
- collect user choices,
- feed responses back into engine,
- display resulting state changes.

That separation keeps rules logic centralized and portable.

---

## Performance characteristics

Why C++ core + Lua scripts works well:

- heavy rule processing in native code,
- scripts stay small and event-driven,
- userdata references avoid expensive deep copies,
- engine-centric validation avoids chaotic script-side branching.

Hot paths typically include:

- trigger scanning in dense board states,
- continuous effect recalculation,
- chain legality checks,
- battle/damage-step micro-rules.

---

## Typical flow of a single card effect

Example template (trigger effect):

1. Event happens (e.g., card destroyed).
2. Engine sees effect with matching trigger code.
3. Calls `condition(e,tp,eg,ep,ev,re,r,rp)` in Lua.
4. If true, during activation:
   - calls `cost(...)` if present,
   - calls `target(...)` to select/declare targets.
5. Chain link created.
6. On resolution, engine calls `operation(...)`.
7. Operation uses `Duel.*` APIs (e.g., special summon/destroy/draw).
8. Core applies results, post-events, and state checks.
9. Any new triggers generated enter subsequent windows/chain opportunities.

---

## How to read/extend this codebase

Recommended approach:

1. **Start from duel/field processor entry points**
   - locate where duel progression and process states are advanced.
2. **Trace script bridge registration**
   - find where Lua functions are registered (`Duel`, `Card`, `Effect`, `Group` APIs).
3. **Map effect lifecycle**
   - creation, registration, activation legality, chain insertion, resolution, reset.
4. **Inspect message emission layer**
   - understand how state changes are communicated outward.
5. **Then study complex rule modules**
   - summoning procedures, battle step, replacement logic, immunity filtering.

For card authors:

- copy an existing script with similar timing semantics,
- keep condition/cost/target/operation separation clean,
- rely on engine helpers instead of manual state assumptions.

---

## Common pitfalls for contributors

1. **Assuming activation legality equals resolution legality**
   - many effects must re-check at resolution.
2. **Incorrect reset flags**
   - causes lingering modifiers or premature cleanup.
3. **Mishandling controller vs owner**
   - especially for moved/stolen/equipped cards.
4. **Ignoring hidden-information constraints**
   - selection helpers must respect visibility.
5. **Over-scripted rule logic**
   - core should enforce universal rules; scripts should express card-specific behavior.

---

## Mental model summary

If you remember one thing:

> `ygopro-core` is a deterministic C++ rules engine that executes Lua-defined card behaviors through a strict legality-checked bridge, advancing duel state via an event/chain-driven processor and emitting protocol messages for external clients.

That model explains nearly every subsystem:

- **C++**: truth, invariants, timing, legality, serialization.
- **Lua**: card intent, modular behavior, effect callbacks.
- **Host app**: user I/O and presentation.

---

## Appendix: Conceptual pseudo-interfaces

```cpp name=conceptual_engine_interfaces.cpp
// Conceptual only (not exact source signatures)
class Duel {
public:
  void Process();                  // advance processor state machine
  void HandlePlayerInput(...);     // consume response from host
  void EmitMessage(...);           // push protocol messages
};

class Card {
public:
  void RegisterEffect(Effect* e);
};

class Effect {
public:
  void SetType(uint32_t type);
  void SetCode(uint32_t code);
  void SetCondition(lua_Function f);
  void SetCost(lua_Function f);
  void SetTarget(lua_Function f);
  void SetOperation(lua_Function f);
};
```

```lua name=conceptual_card_script.lua
-- Conceptual card script structure
function s.initial_effect(c)
  local e1=Effect.CreateEffect(c)
  e1:SetType(EFFECT_TYPE_IGNITION)
  e1:SetRange(LOCATION_MZONE)
  e1:SetCondition(s.cond)
  e1:SetCost(s.cost)
  e1:SetTarget(s.tg)
  e1:SetOperation(s.op)
  c:RegisterEffect(e1)
end
```

---

If you want, I can next produce:
- a **file-by-file map** of this exact repository (entry points, key classes, call graph), and/or
- a **“how to add a new mechanic” contributor guide** tied to concrete engine modules.
