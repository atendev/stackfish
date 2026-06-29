# Seven-Phase LLM Programming Workflow

A structured approach to using an AI agent for code changes without losing control of your codebase. The core principle: **you decide when each phase is complete before the agent moves to the next one.** The agent never proceeds on its own.

> This document is both a human reference and agent instructions. Agent-facing notes appear in `agent-note` code blocks throughout.

---

## Overview

| Phase | Name | Goal |
|-------|------|------|
| 0 | Design Doc | Collaborate to produce a living reference document for the project |
| 1 | Structs | Define all data structures |
| 2 | Interfaces | Define all function signatures |
| 3 | To-Dos | Annotate the codebase with change locations |
| 4 | Verification Setup | Define how correctness will be proven |
| 5 | Dry Run | Implement, verify, revert, report gaps |
| 6 | Invariants | Add assertions and constraints |
| 7 | Implement | Final real implementation |

---

## Phase 0 — Design Doc

Phase 0 is a collaboration between you and the agent to produce a **living design document** that describes the project fully — what it is, what it does, how it is structured, and any relevant design decisions. This document is the source of truth for all subsequent phases and should be updated throughout the project as things change.

The human starts by providing an initial idea or plan with as much detail as possible. The agent's job is not to immediately agree and execute — it is to ask questions, identify gaps, push back on things that seem wrong, and help shape the idea into something concrete and buildable.

**What the design doc should cover** (add or remove sections as appropriate for the project):

- **Purpose** — What is this project for? Who uses it and why?
- **Feature overview** — What does it do at a high level?
- **Architecture** — How is it structured? What are the major components and how do they relate?
- **Data model** — What are the core entities and their relationships? (high level, not full struct definitions — that comes in Phase 1)
- **UI / UX** — If applicable: layout, flow, color scheme, interaction patterns
- **Constraints & non-goals** — What is explicitly out of scope? What technical constraints exist?
- **Open questions** — Anything unresolved that needs a decision before coding starts

**What to do:**
- Human provides the initial idea with as much detail as they have
- Agent reads it, asks clarifying questions, and flags anything that seems underspecified, contradictory, or likely to cause problems later
- Together, produce a first draft of the design doc
- Iterate until the human is satisfied it captures the project accurately
- Save the doc to the repo so the agent can reference and update it throughout phases 1–7

**You move on when:** The design doc exists, is agreed upon, and covers enough detail that phases 1–7 can proceed without guessing at intent.

```agent-note
You are receiving an initial idea, not a finished spec. Your job is to help turn it into one.
Read everything provided carefully before responding.
Then do the following, in order:
  1. Ask any clarifying questions needed to understand intent, scope, or constraints
  2. Flag anything that seems underspecified, technically risky, or likely to cause problems in later phases
  3. Propose structure for the design doc and draft it collaboratively with the human
Do NOT just agree with everything and start producing output. If something seems wrong or incomplete, say so.
The design doc must be saved as a file in the project (e.g. DESIGN.md) so it can be referenced and updated throughout the workflow.
As the project evolves through phases 1–7, update the design doc to reflect decisions made, structures finalized, and anything that changed from the original plan.
The design doc should always be readable by both a person and an AI with no prior context on the project.
```

---

## Phase 1 — Structs (Data Structures)

Define all new or modified data structures needed for this feature. Nothing else. No implementation, no function bodies.

**What to do:**
- Define new structs, types, enums, or records
- Modify existing structures only where necessary
- Include field names, types, and a brief comment on purpose

**You move on when:** The human has reviewed every struct and explicitly approved them. Any corrections must be incorporated before proceeding.

```agent-note
Output ONLY struct/type definitions in this phase. No functions, no logic, no implementation.
If a struct change implies a function change, note it as a comment but do not implement it.
Present definitions clearly and wait. Do not proceed to Phase 2 until the human says so.
If the human asks for corrections, apply them and re-present the full updated definitions.
```

---

## Phase 2 — Interfaces (Function Signatures)

Define the function signatures that need to be created or changed. Still no implementation — just the shape of the API.

**What to do:**
- List every function that needs to be added or modified
- Include parameter names, types, and return types
- Add a one-line docstring describing what each function does
- Note which existing functions are being changed vs. which are new

**You move on when:** The human has reviewed every signature and approved them. No implementation has been written.

```agent-note
Output ONLY function signatures and docstrings. No bodies, no logic.
Cross-check your signatures against the structs from Phase 1 — they must be consistent.
If you realize Phase 1 structs need adjustment to support these interfaces, flag it explicitly.
Do not proceed to Phase 3 until the human confirms the interfaces look correct.
```

---

## Phase 3 — To-Dos (Codebase Annotation)

Annotate the actual codebase with TODO comments at every location that needs to change. This creates a concrete map of the work before any real code is written.

**What to do:**
- Add `TODO` comments inline at every spot that needs modification
- Each TODO should reference the relevant struct or interface from phases 1–2
- Be specific — "TODO: update enemy loop to use enemy_count instead of enemies.len()" not just "TODO: fix this"
- Do not write any implementation code

**You move on when:** The human has walked through every TODO, confirmed each one is correct, and removed or added any that don't look right.

```agent-note
Place TODO comments directly in the source files. Do not summarize them in a list — they must live in the code.
Each TODO must be specific enough that any developer (or you in Phase 6) knows exactly what to write.
Vague TODOs are a failure of this phase. If you don't know what to write in a TODO, that means Phase 0–2 was incomplete — flag it.
After placing all TODOs, summarize them by file so the human can review the full scope at a glance.
Wait for human confirmation before proceeding.
```

---

---

## Phase 4 — Verification Setup

Before the dry run can happen, you need a defined, repeatable way to prove the implementation actually works. This phase establishes that mechanism. Without it, Phase 5 has no objective pass/fail criteria and devolves into the agent guessing whether things are correct.

**What to do:**
- Identify what "working correctly" means for this specific feature
- Define the exact commands, scripts, or steps that will be run to verify it
- Confirm those commands can be run by the agent in your environment
- If a test harness or entry point doesn't exist yet, create a minimal one now
- Document the expected output or behavior for a passing run

**Verification approaches by project type:**

| Project Type | Verification Method |
|---|---|
| Library / module | Unit tests covering the new interfaces |
| CLI tool | A script that runs the tool with known inputs and checks outputs |
| Game / simulation | A headless/non-rendering loop that processes scripted inputs as events |
| Server / API | A script that hits the relevant endpoints and checks responses |
| Data pipeline | A dry run on a fixed input dataset with known expected output |

**You move on when:** There is a single command (or small set of commands) that can be run to definitively say "this works" or "this doesn't." The human has confirmed it runs successfully against the current codebase before any changes.

```agent-note
Your job in this phase is to produce a runnable verification method, not just describe one.
If tests already exist, run them now to confirm they pass on the unmodified codebase. Document the baseline output.
If no verification exists, propose the minimal one needed for this feature and implement it before Phase 5.
The verification must be something YOU can run autonomously during Phase 5 — not something that requires human eyes to interpret.
Prefer deterministic, scriptable checks over manual inspection. "It compiled" is not verification. "It compiled and produced expected output X" is.
Define clearly:
  - COMMAND: the exact command to run
  - EXPECTED: what a passing result looks like
  - FAILURE: what a failing result looks like
Do not proceed to Phase 5 until the human confirms the verification method is correct and it passes on the unmodified codebase.
```

---

## Phase 5 — Dry Run (Implement, Verify, Revert, Report)

This is the most important phase. Implement the full feature, verify it works using the method from Phase 4, then throw away the code and report where the plan broke down.

**What to do:**

1. **Implement** — Write the full implementation using only the structs, interfaces, and TODO locations agreed upon in phases 1–3. Do not introduce anything new.
2. **Verify** — Run the verification command defined in Phase 4. The result must be deterministic — pass or fail.
3. **Revert** — Undo all the implementation changes, returning the codebase to its pre-implementation state.
4. **Report** — Document every place where you had to deviate from the plan:
   - Structs that needed extra fields
   - Interfaces that needed different signatures
   - Code locations that weren't covered by a TODO
   - Anything you had to figure out that wasn't specified upfront

**You move on when:** The human has reviewed the deviation report and decided whether to update phases 1–3 to fix the gaps, or accept the deviations and carry them into Phase 7.

```agent-note
This phase is a diagnostic, not a delivery. The implementation will be thrown away.
Do not skip the revert step. The codebase must be clean after this phase — no partial implementation left behind.
Use the exact verification command established in Phase 4. Do not substitute a different check.
Your deviation report is the primary output of this phase. Be exhaustive. Small deviations matter.
Format the report clearly:
  - VERIFICATION RESULT: pass or fail, with the actual output
  - STRUCT DEVIATIONS: fields added or changed beyond Phase 1
  - INTERFACE DEVIATIONS: signatures changed beyond Phase 2
  - SCOPE DEVIATIONS: files or locations touched that had no TODO
  - UNRESOLVED: anything you couldn't implement cleanly
If everything went perfectly with zero deviations, say so explicitly. That is also valuable information.
```

---

## Phase 6 — Invariants

Add assertions, guards, and constraints that encode your assumptions about the system explicitly in the code.

**What to do:**
- Identify assumptions that should always be true at runtime
- Add assertions or guard clauses that panic/error if those assumptions are violated
- Examples: array bounds checks, state machine constraints, null guards, count limits

**Note:** This is the hardest phase to get right with an AI. Review invariant suggestions carefully — the agent doesn't have runtime knowledge of what "should never happen." You know your system better than it does.

**You move on when:** You're satisfied the most critical assumptions are now explicit in the code.

```agent-note
Do not invent invariants. Only assert things that are clearly true based on the design established in phases 1–3.
For each invariant, explain in a comment WHY it must hold, not just that it does.
If you are unsure whether something is a valid invariant, present it as a question to the human rather than asserting it.
This phase is explicitly acknowledged as the weakest phase for AI assistance. Be conservative and flag uncertainty.
```

---

## Phase 7 — Implement

Now do the real implementation. Everything is confirmed. No surprises.

**What to do:**
- Implement using the finalized structs, interfaces, and TODOs
- If Phase 5 revealed deviations that were approved, incorporate them
- Follow the TODO map exactly — if something isn't covered by a TODO, stop and ask
- Run the Phase 4 verification command after completing to confirm everything passes

**You move on when:** The implementation is complete, verification passes, and the human has reviewed it.

```agent-note
You have a complete, verified plan from the previous phases. Follow it exactly.
If you encounter anything not covered by the plan, stop and flag it rather than improvising.
Improvising in Phase 7 is how scope creep and unexpected bugs enter the codebase.
After implementing, run the Phase 4 verification command and include the output in your summary.
Confirm the result maps 1:1 to the agreed plan.
```

---

## General Agent Rules

These apply across all phases:

```agent-note
NEVER proceed to the next phase without explicit human confirmation.
NEVER implement more than the current phase asks for.
NEVER assume a correction is minor enough to skip re-review — always re-present after changes.
If the human is reviewing from a phone or limited context, keep outputs concise and scannable.
Acknowledge each phase by saying which phase you are in and what your output represents.
When in doubt, do less and ask. Doing too much is harder to undo than doing too little.

DO NOT people-please. If you have a concern about an approach — a technical risk, a design inconsistency, something that will likely cause problems in a later phase — raise it. Do this even if the human seems confident or committed to their idea. State the concern clearly and specifically, then let the human decide whether to address it or proceed anyway. Their call, not yours. Staying silent to avoid friction is a failure mode. A concern raised and dismissed is fine. A concern never raised that causes a bug in Phase 6 is not.
```

---

## When to Go Manual

The workflow is a tool, not a rule. Break out of it and write code yourself when:

- The problem is easier to show than describe in English
- You've tried to get the agent to do something three or more times and it keeps missing
- The change is small, local, and you can see exactly what needs to happen
- The agent keeps introducing logic in the wrong layer (e.g., UI logic that belongs in the game/business layer)

**The goal is good code, not consistent use of the workflow.**
