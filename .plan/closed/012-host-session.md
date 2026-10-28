---
id: 012
title: Build B3 crates/host Session, serde codec and CardSource
type: task
mode: HITL
status: closed
blocked_by: []
claimed: false
---

## Question

Build `crates/host` with the Session, serde and codec, plus the CardSource trait, setup types, format presets and creation validation.

## Notes

- Branch: `phase5/host-session`. Follow the CLAUDE.md slice workflow: requirements report first, approval before every commit, closing report, user merges.
- Decisions: 006 (CardSource turns `oracle_id` UUIDs into `CardDefinition`s; setup message holds the player list in turn order, decklists and optional commanders, format preset with overrides, and seed; the Session validates structure only; mulligans are Decisions after the first Advance), 007 (wire format: `serde_json`, mirror types in `crates/host`, `v`/`kind`/`body` envelope, `u64` as decimal strings, ten stable error codes), and 001 (Advance with optional step budget; a rejected Answer changes nothing; a pipeline error is fatal).
- The requirements report should raise making the Session's default step cap a Setup override (from 009).
- Mark any stubbed parts with TODO.

## Resolution

Built `crates/host` on `phase5/host-session` and merged it (`73f4a4a`).

- `Session::create` parses a setup request and builds the `GameConfig` through a `CardSource`. `Session::handle` takes `advance` and `answer` requests as JSON bytes. Standard and commander presets take per-field overrides, and `step_cap` is a setup field (default 100000).
- One Seat per player, created automatically. Mirror types cover all 41 events, the 12 reply shapes, entries and the four priority actions. The Decision id counter stays on `Engine`.
- Added error code `decision_pending` for an `advance` while a Decision is open. `v` is checked on every request, and `seat` is optional on `advance`.
- Left open: per-Seat visibility (B4), replay export, the carddb-backed source, spectator Seats, friendlier rejection text, and payment cost filters. Notes are in `docs/implementation/18-host-session.md`.
