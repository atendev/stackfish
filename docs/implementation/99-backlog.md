# 99-backlog

## Live TODOs / unresolved gaps

- **Layer 2 internal filtering**: `applicable_effects_in_layer`/`applies_to` still read raw `GameObject.controller`, not `effective_controller` — deliberate to avoid recursive layer-2 resolution.
- **Layer 1 (copy)**: bare copy only; no CR 707.9 exception, no ETB-replacement "enters as copy" path.
- **Layer 3 (text-changing)**: only CR 612.8 name-setting is real; general word substitution / Exchange of Words / Volrath's Shapeshifter / Spy Kit unmodeled.
- **Layer 7d (counters)**: pre-netted into base P/T before continuous effects; not timestamp-interleaved per CR 613.4 literal text.
- **CR 613.6-8 dependency**: `EffectPayload::Script` effects never participate in dependency ordering.
- **Targeting**: `TargetSpec::Script`/`invoke_target` still `todo!()`; "N or more" counts, "another target", negative filters unmodeled.
- **Activated abilities**: positional `ability_index` by registration order, not a stable id.
- **Replacement effects**: `Game.Ops.DrawCards(n)` bypasses registered replacement checking; ETB-tapped/counter modifiers and protection-as-replacement unmodeled.
- **Planeswalkers/Battles/DFCs/Split/Sagas**: explicit DESIGN.md non-goals.
- **Dead Phase-3 stubs**: `Subtype::new`/`as_str`, `CardDefinition::has_type`/`has_supertype`/`has_subtype`, several `mana.rs` constructors are `todo!()` with zero callers.
- **Face-down permanents**: `card_summary_to_lua` does not hide real info; no morph-style mechanic exists yet, so currently unreachable.
- **Stack owner**: `StackObject` has no `owner` field; controller used as proxy in player elimination.
- **Cost modifier order**: same-timestamp modifier order is not player-chosen.
- **Enchant/Equip (CR 702.16b)**: attach machinery exists; Enchant side closed, Equip side closed, but some Aura edge rules (303.4d/f/h/i/k/m) not modeled.
- **Landwalk / Banding**: still open.

## Mistakes caught (don't repeat)

- Hybrid Phyrexian does **not** need a 3-slot container; `Phyrexian(Color)` atom already carries two payment options.
- PowerShell 5.1 `-replace` with a ScriptBlock is unsupported; use line-by-line array splicing for position-sensitive edits.
- Replacement stabilization loop needs a per-event `excluded` set, or a `Repeatable`/`LimitedUses` effect can match its own unchanged output forever and hang.
