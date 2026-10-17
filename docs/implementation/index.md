# Implementation Notes Index

Compact reference for my own use. Each file covers one subsystem.

- [00-overview](00-overview.md) — repo layout, phase 1 data model, phase 2-4 baseline, `stackfish-testutil` fixture-builder convention
- [01-pipeline](01-pipeline.md) — `ProcessState`, pass priority, replacement/trigger pause plumbing
- [02-zone-moves](02-zone-moves.md) — play land, cast spell, payment, beginning/ending phases, CR 400.7 id minting
- [03-combat](03-combat.md) — attackers, blockers, combat damage, first/double strike
- [04-sba](04-sba.md) — state-based actions, player loss, legend/world rule, tokens, regeneration, max hand size
- [05-script-bridge](05-script-bridge.md) — trigger/self-registration, primitives, continuous effect scripting, state query
- [06-abilities](06-abilities.md) — mana abilities, activated ability costs, mana cost strings, mana add choice, static durations
- [07-targeting](07-targeting.md) — target validation, up-to-N, multi-target, subtype/supertype filters, hexproof/shroud, Lua array target specs
- [08-replacements](08-replacements.md) — replacement/prevention effects, redirect damage, cost/cleanup/draw replacement checks
- [09-layers](09-layers.md) — layer engine: types/colors, keywords, P/T set/modify/CDA/switch, control, dependency, text
- [10-triggers](10-triggers.md) — ETB, damage, upkeep, dies, draw/combat/end, once-per-turn, untap, discard, intervening-if
- [11-ops](11-ops.md) — `Game.Ops.*`: damage, destroy, counters, draw, exile, sacrifice, token, shuffle, scry, search, tap/untap, mill/lose life/sacrifice self, reanimation, fight/bite, exile until leaves
- [12-coroutines](12-coroutines.md) — `Game.SelectTargets`, player targets, may abilities, modal spells, payment unwind
- [13-commander-multiplayer](13-commander-multiplayer.md) — multiplayer defending player, elimination, commander tax/damage, color identity
- [14-protection-attach](14-protection-attach.md) — protection qualities, auras, equipment, attach/evasion keywords, hexproof/shroud vs attachment
- [15-card-database-loader](15-card-database-loader.md) — `stackfish-carddb`: SQLite external-card-DB connector, `ManaCost::parse`/type_line/keyword mapping, CDA script gap
- [16-decision-facade](16-decision-facade.md) — `decision` module: `Engine::open_decision`/`answer`, reply shapes, rejection codes, Decision ids
- [17-legal-actions](17-legal-actions.md) — `Engine::legal_actions`/`legal_attackers`/`legal_blockers`/`pending_damage_splits`, entry types, combat Decisions, Answer checks
- [18-host-session](18-host-session.md) — `crates/host`: `Session` over JSON bytes, `CardSource`, setup/format presets, wire mirror types, error codes
- [19-proof-game](19-proof-game.md) — `proof_cards`, the random `Bot`, the full-game byte tests, and the three engine bugs it found
- [99-backlog](99-backlog.md) — current unresolved gaps and mistakes caught
