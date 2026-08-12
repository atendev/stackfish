use crate::types::card::{CardTypeSet, KeywordSet, ModeChoice, SpellMode, SupertypeSet};
use crate::types::cost::{CostSpec, FilterSpec};
use crate::types::effect::EffectFilter;
use crate::types::counter::CounterKind;
use crate::types::event::GameEventKind;
use crate::types::ids::{AbilityId, ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::mana::{ColorSet, ManaUnit};
use crate::types::target::TargetSpec;
use crate::types::turn::Step;
use crate::types::zone::ZoneKind;

/// Per ygopro's effect lifecycle (`docs/ygo_pro_reference.md`), generalized
/// for MTG: condition/cost/target/operation are separate script callbacks
/// rather than one monolithic function, so the engine can check (e.g.)
/// condition without committing to cost/target/operation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AbilityKind {
    Triggered,
    Activated,
    Static,
}

/// Structural stand-in for an ability's effect when no script bridge exists
/// to actually run one (every `RegisteredAbility` still needs `operation` —
/// this is narrower: just enough to model CR 605's mana abilities, the one
/// concrete effect shape this engine resolves without a script callback).
/// `None` on `RegisteredAbility::effect` means "this ability's effect can
/// only be carried out via the script bridge" — not yet possible, same gap
/// as `operation` everywhere else.
#[derive(Clone, Debug)]
pub enum AbilityEffect {
    /// CR 605: adds exactly these literal units to the activator's mana
    /// pool. Reuses `ManaUnit` (see `mana.rs`) — same "literal units, not a
    /// cost shape" vocabulary `CostPayment::Mana` already uses, just added
    /// instead of spent.
    AddMana(Vec<ManaUnit>),
    /// CR 605's "add one mana of any color" shape (and its "add N mana,
    /// each any color among {...}" generalization) — unlike `AddMana` above,
    /// the color of each of the `count` mana produced isn't fixed on the
    /// ability itself; it's chosen when the ability is activated (CR
    /// 602.2b's choice-as-part-of-activation process), carried on
    /// `PlayerActionKind::ActivateAbility::chosen_colors`. `commands.rs`'s
    /// `ActivateAbility` arm validates `chosen_colors.len() == count` and
    /// every entry is a member of `choices` before converting each to
    /// `ManaUnit::Colored` and adding to the pool, same as `AddMana` does
    /// for its own fixed units. No `SnowColored` choice variant yet (see
    /// this slice's own UNRESOLVED note in docs/implementation.md).
    AddManaChoice { count: u32, choices: ColorSet },
    /// The `Game.Ops.GainLife(n)` primitive — gains the controller exactly
    /// `n` life, no script invocation needed. Mirrors the hand-written
    /// `{ gain_life = n }` Lua operation table from the prior slice, just
    /// resolved on the Rust side instead of via `ScriptRuntime::invoke_operation`.
    GainLife(i32),
    /// The `Game.Ops.Damage(n)` primitive — deals exactly `n` damage to the
    /// resolving `StackObject`'s first chosen target (CR 608.2m). Unlike
    /// `GainLife` above, this does *not* mutate `GameState` directly —
    /// `ResolveStackTop` emits a real `GameEventKind::DamageDealt` event and
    /// lets it flow through the normal `ReplacementCheck`→`CommitEvents`
    /// pipeline, the same one combat damage already uses, so
    /// prevention/replacement effects (CR 614-616) and Lifelink/Wither/
    /// Infect/Deathtouch (`commands::apply_damage_event`) apply for free.
    /// Single fixed target only — no "divide among any number of targets"
    /// (Arc Trail-style) or "each opponent" mass-damage shape; matches
    /// `targeting-basics`'s own existing narrowing (see
    /// docs/implementation.md).
    Damage(u32),
    /// The `Game.Ops.Destroy()` primitive — destroys the resolving
    /// `StackObject`'s first chosen target (CR 701.8). Unlike `Damage`,
    /// this can't be modeled as "push a not-yet-applied event and let
    /// `CommitEvents` retroactively apply it" — every other `ZoneChange`
    /// event in this codebase describes a move that already happened, so
    /// `ResolveStackTop` performs the real CR 614.13 `WouldDie` replacement
    /// check and `sba::commit_death` inline (mirroring `resolve_deaths`'s
    /// own per-object body), the same way `Engine::submit_payment` already
    /// does for a Sacrifice cost payment. CR 702.12b: an indestructible
    /// target is untouched — checked before even proposing the
    /// replacement-checked event, not a replacement effect itself. No
    /// fields — a toggle, not a value, same shape as `SwitchPT`.
    Destroy,
    /// CR 509.1h: "target attacking creature becomes blocked" — blocked by
    /// no specific creature (CR 510.1c: deals no combat damage unless it has
    /// trample), mutated directly into `CombatState.effect_blocked` via
    /// `commands::mark_effect_blocked` — no event kind exists for this, same
    /// "already happened, no `CommitEvents` needed" convention `Pump`/
    /// `GainControl` already use for their own direct `GameState` mutation.
    /// CR 702.22i rides along for free: `mark_effect_blocked` itself
    /// propagates to every other living member of the target's own band.
    /// No fields — a toggle, not a value, same shape as `Destroy`. Narrowed:
    /// `target_requirement` can only express "target creature," not "target
    /// *attacking* creature" (no `TargetFilter` predicate for combat state
    /// exists, and the Lua state snapshot doesn't expose combat data either)
    /// — targeting a non-attacking creature is a silent no-op at resolution
    /// (checked inside `mark_effect_blocked` itself) rather than a real CR
    /// 608.2b illegal-target fizzle.
    BecomeBlocked,
    /// The `Game.Ops.DestroyAll(filter)` primitive (CR 701.8, CR 601.2c's
    /// "all"-effects-don't-target) — destroys every battlefield permanent
    /// matching `filter` unconditionally, e.g. Armageddon's "Destroy all
    /// lands." Same `EffectFilter` shape (`controller`/`card_types`/
    /// `self_only`) `Game.CreateStaticAbility`'s own `:SetAffected` already
    /// uses — reuses `types::effect::matches_effect_filter` (extracted from
    /// `ContinuousEffect::applies_to` for exactly this reuse) rather than a
    /// second copy of the same controller/card_types matching logic.
    /// `ResolveStackTop` collects every matching battlefield object and
    /// hands the list to the same `continue_scripted_zone_move(
    /// ScriptedZoneMoveKind::Destroy, ...)` pipeline `Destroy` above already
    /// delegates to — CR 702.12b indestructibility, CR 616.1 ordering-choice
    /// pauses, and the CR 614.13 `WouldDie` replacement check all fall out
    /// for free, no new destruction logic needed. `subtypes`/`excluded_*`
    /// aren't part of `EffectFilter` yet, so "destroy all Zombies"/"destroy
    /// all nonland permanents" aren't expressible this way yet — narrower
    /// than `TargetFilter::Object`, named as a follow-up if a real card
    /// needs it.
    DestroyAll(EffectFilter),
    /// The `Game.Ops.Counters(kind, n)` primitive — puts `amount` counters
    /// of `kind` on the resolving `StackObject`'s first chosen target (CR
    /// 122.1a). Like `Damage`, not `Destroy`: `GameEventKind::CounterAdded`
    /// already existed as dead code (never constructed, never matched) with
    /// exactly this shape, so `ResolveStackTop` pushes it unapplied and
    /// `CommitEvents` retroactively applies it, the same pipeline `Damage`
    /// uses for `DamageDealt`. Object targets only — no player-facing
    /// counters (Energy/Experience) this slice, since real "put a counter on
    /// target creature/artifact" text vastly outnumbers those and
    /// `CounterAdded` only carries a single `ObjectId`, not a generic
    /// `Target`. No replacement/prevention modeling for counter placement
    /// either (no `EventFilter::CounterAdded` shape exists) — same gap
    /// `GainLife` already has, not fixed here.
    Counters { kind: CounterKind, amount: i32 },
    /// The `Game.Ops.Pump(power, toughness, duration)` primitive — "target
    /// creature gets +X/+Y until <duration>" (e.g. CR 702's fight/pump
    /// effects). Every legal target independently gets its own
    /// `ContinuousEffect` (layer 7c, `EffectPrimitive::ModifyPT`) pushed
    /// straight into `state.continuous_effects` — mutates state immediately,
    /// same "already happened, no `CommitEvents` needed" convention
    /// `DrawCards`/`Shuffle` already use, since there's no
    /// event kind for "a continuous effect was added" (static-ability
    /// registration isn't ledger-logged either). Each pushed effect's
    /// `ContinuousEffect.source` is set to the *target's* `ObjectId`, not
    /// the spell/ability's own source, paired with `EffectFilter::
    /// self_only: true` — reusing exactly the "self_only pins to
    /// `ContinuousEffect.source`" mechanism `EffectFilter`'s own doc comment
    /// already names as its stance for "the affected object is the effect's
    /// own attachment target," just applied to a spell's chosen target
    /// instead of an aura's. Free side effect: if the target later leaves
    /// whatever zone it occupied when this effect was created, its
    /// `ObjectId` stops resolving (every zone change mints a fresh one) and
    /// `layers::applicable_effects_in_layer` already treats an unresolvable
    /// source as inapplicable — the effect can't outlive its own target.
    /// `duration` is `EffectDuration`'s real drain paths
    /// (`UntilEndOfTurn`/`UntilYourNextTurn`/`UntilEndOfCombat`/
    /// `Permanent` — see `phase5/effect-duration-expiration`); a
    /// non-Object legal target is skipped, same "silent no-op" stance every
    /// other object-targeted primitive here takes.
    Pump { power: i32, toughness: i32, duration: crate::types::effect::EffectDuration },
    /// The `Game.Ops.GainControl(duration)` primitive — CR 108.4/613.1
    /// layer 2, e.g. "Gain control of target creature until end of turn"
    /// (Threaten-style). Same per-target `ContinuousEffect` construction
    /// `Pump` above uses (`self_only: true` pinned to the *target's* own
    /// `ObjectId`, mutates `state.continuous_effects` immediately, no
    /// `CommitEvents` involved), just `Layer::ControlChangingEffects`/
    /// `EffectPrimitive::GainControl` instead of layer 7c/`ModifyPT`. The
    /// new controller is always the resolving spell/ability's own
    /// controller (`StackObject::controller` at resolution time) - no
    /// separate player argument from Lua, since every real "gain control"
    /// card gives control to its own caster. A non-Object legal target is
    /// skipped, same "silent no-op" stance every other object-targeted
    /// primitive here takes.
    ///
    /// Deliberately narrow: only this resolving-spell/ability creation path
    /// exists. A *static* ability's own "you control enchanted creature"
    /// (Control Magic-style, CR 303.4h auras) needs real attach/enchant
    /// machinery this engine doesn't have yet (`GameObject.attached_to`/
    /// `.attachments` are inert placeholder fields, never written to
    /// anywhere) - a separate, larger gap, not attempted here. Also,
    /// `GameObject.controlled_since_turn` isn't updated by this effect
    /// (CR 302.6 summoning sickness technically restarts for the new
    /// controller) - deliberately left alone rather than half-fixed, since
    /// naively stamping it here would leave a stale value behind once the
    /// effect's duration expires and control reverts via the layer engine,
    /// a new bug rather than a fix. Named, not guessed at.
    GainControl(crate::types::effect::EffectDuration),
    /// The `Game.Ops.Regenerate()` primitive — CR 701.19a: "Regenerate
    /// target creature." Every legal target independently gets its own
    /// one-shot regeneration shield (`ReplacementEffect`, `EventFilter::
    /// WouldDie` paired with `ReplacementPayload::Regenerate`, pinned via
    /// `self_only: true` exactly like `Pump` pins its own `ContinuousEffect`)
    /// pushed straight into `state.replacement_effects` — same "mutates
    /// state immediately, no event kind for this" convention `Pump` already
    /// established, since static-ability-sourced shields (CR 701.19b)
    /// aren't logged either.
    ///
    /// CR 701.19c: a target with `KeywordSet::CANT_BE_REGENERATED` gets no
    /// shield at all - checked at the moment the shield would be created,
    /// not as a property of the `ReplacementEffect` itself. The ability/
    /// spell that resolved this primitive still fully resolved (its cost was
    /// already paid, same as any other primitive here) - only the shield
    /// silently fails to come into being, matching 701.19c's own wording
    /// ("doesn't preclude such abilities from being activated... rather,
    /// causes shields to not be applied"). A non-Object legal target is
    /// skipped, same "silent no-op" stance every other object-targeted
    /// primitive here takes.
    Regenerate,
    /// The `Game.Ops.DrawCards(n)` primitive — the resolving `StackObject`'s
    /// controller draws exactly `n` cards (CR 120.3/121), no target (same
    /// "always the controller" shape `GainLife` already uses, not `Damage`/
    /// `Destroy`/`Counters`'s object-target shape — "Draw a card"/"you draw
    /// two cards" vastly outnumbers "target player draws"). Like every
    /// other real zone move in this engine, each draw is proposed via
    /// `commands::propose_draw`, replacement-checked via `apply_replacements`
    /// (CR 614/616), and committed via `commands::commit_zone_move` — see
    /// `Engine::continue_draw_cards`. Drawing from an empty library silently
    /// stops rather than looping past it - CR 704.5b's empty-library-loss SBA
    /// isn't modeled (same TODO `draw_step` already carries). No `Target::
    /// Player` "target player draws" variant; replacement effects that
    /// change *which player* draws (e.g. Notion Thief) are not representable
    /// because `ReplacementPayload::ReplaceZoneDestination` only rewrites `to`,
    /// not the destination hand's owner.
    DrawCards(u32),
    /// The `Game.Ops.Sacrifice()` primitive — sacrifices the resolving
    /// `StackObject`'s first chosen target from the battlefield (CR 701.17).
    /// Unlike `Destroy`, indestructibility does **not** prevent sacrifice (CR
    /// 702.12b is Destroy-specific). The implementation is identical to the
    /// `Destroy` arm minus the indestructibility check: proposes
    /// `Battlefield → Graveyard`, runs `apply_replacements` (CR 614.13's
    /// "if it would die, exile it instead" applies to sacrifice), then calls
    /// `sba::commit_death` — which, unlike the `Exile` case, works correctly
    /// here because the destination is the owner's per-player graveyard
    /// (`Some(owner)`), not the shared Exile zone. `Dies`/`LeavesBattlefield`
    /// triggers fire the same way they do off an SBA-driven death.
    ///
    /// TODO: "sacrifice a permanent you control" (player chooses at resolution,
    /// no targeting) requires Lua coroutines — blocked on backlog item 9.
    /// TODO: self-sacrifice ("sacrifice this permanent," no target) is a useful
    /// but distinct shape deferred as a named follow-up.
    Sacrifice,
    /// The `Game.Ops.Exile()` primitive — exiles the resolving `StackObject`'s
    /// first chosen target from the battlefield (CR 701.18). Unlike `Destroy`,
    /// exile ignores indestructibility entirely (CR 702.12b is Destroy-only).
    /// Like `Destroy`, a `ZoneChange` always describes a move that already
    /// happened, so `ResolveStackTop` runs the real `apply_replacements`
    /// pipeline and then inlines the zone move — but cannot reuse
    /// `sba::commit_death` because that function hardcodes `Some(owner)` for
    /// its destination zone lookup, which fails for the shared Exile zone
    /// (`owner = None`). Battlefield targets only — "exile target
    /// creature/permanent" vastly outnumbers "exile target card from a
    /// graveyard," and the latter requires knowing which zone to remove from
    /// at resolution time; deferred as a named follow-up. No fields — a
    /// toggle, not a value, same shape as `Destroy`.
    Exile,
    /// The `Game.Ops.Counter()` primitive (CR 701.6a) — "counter target
    /// spell." The target is a `Target::Object` referring to another
    /// `StackObject`'s own `stack_id` (a spell on the stack already has a
    /// real `GameObject` at `zone: Stack` sharing that same `ObjectId` — see
    /// `commands::cast::execute_cast_spell` — so `TargetFilter::Object {
    /// zone: Some(ZoneKind::Stack), .. }` already targets it correctly with
    /// no engine change; card_types/controller narrow it the same way any
    /// other `Object` filter does, e.g. Negate's "target noncreature spell").
    ///
    /// Unlike every other single-target `AbilityEffect` here, the affected
    /// object is *not* `top.source`/the resolving spell's own target in the
    /// zone-mutation sense `Destroy`/`Exile`/`Sacrifice` use (those all act
    /// on a Battlefield object reachable through `continue_scripted_zone_move`)
    /// — it's a *different*, still-present entry in `state.stack` itself.
    /// `resolve_counter_effect` finds and removes that `StackObject` by
    /// matching `stack_id` (a plain `Vec::retain`/position-remove, since
    /// `state.stack` is a bare `Vec`, not a queue with its own removal
    /// primitive), then — only if it was a `Spell` (an ability "just stops
    /// existing," same as a fizzled ability, no `ZoneChange`) — reuses
    /// `Engine::move_resolving_spell` for the card's own zone move (CR
    /// 701.6a: owner's graveyard, or exile if *that* spell's own
    /// `exile_on_leaving_stack` is set, e.g. a flashbacked spell that gets
    /// countered still ends up exiled per CR 702.34a - not the *countering*
    /// spell's own flag). No `apply_replacements` call - same "a spell
    /// leaving the stack for graveyard/exile isn't a `WouldDie`-shaped
    /// replacement-checked move" stance the CR 608.2b fizzle branch already
    /// takes for the identical zone-move shape. No CR 616.1 ordering pause
    /// either - a single stack entry being removed, not several simultaneous
    /// battlefield deaths.
    ///
    /// CR 608.2b already covers "the targeted spell left the stack before
    /// this resolves" for free: `target_is_legal`'s zone check fails once
    /// the target's `ObjectId` no longer resolves to a `zone: Stack` object
    /// (every zone change mints a fresh id), so `legal_targets` comes back
    /// empty and the *Counterspell itself* fizzles via the ordinary path,
    /// never reaching this arm at all.
    ///
    /// "Counter target spell with mana value X (or less/or greater)"
    /// (Convolute's fixed-number form, or the X-comparison form) is real now
    /// too, via `TargetFilter::Object::mana_value` - see that field's own
    /// doc comment for the full shape (`ManaValueRequirement`/
    /// `ManaValueBound`, threading the casting/resolving spell's own
    /// announced `x_value` through `commands::target_is_legal`).
    ///
    /// TODO: "counter target activated or triggered ability" (Stifle,
    /// Trickbind) isn't reachable this slice - an ability's `StackObject` has
    /// no matching `GameObject` for a `TargetFilter::Object` to find, and no
    /// ability-shaped target vocabulary exists yet. TODO: "counter unless
    /// [that player] pays {X}" (Mana Leak, Condescend) needs a real
    /// mid-resolution payment ask - not built.
    Counter,
    /// The `Game.Ops.CreateToken()` primitive — creates one or more token
    /// permanents on the battlefield under the controller's control (CR 111).
    /// Named tokens (Clue, Food) are fully implemented; Treasure is blocked
    /// on mana-choice coroutines (backlog item 9). Creature tokens support
    /// arbitrary subtypes, colors, P/T, keywords, and optional additional
    /// card types (Artifact, Enchantment) and supertypes (Legendary).
    ///
    /// TODO: CR 111.7 — tokens that leave the battlefield cease to exist;
    /// enforcement deferred (tokens currently go to graveyard like normal
    /// permanents). Blocked on a zone-watch hook in SBA/replacement layer.
    CreateToken { spec: TokenSpec, count: u32 },
    /// The `Game.Ops.Shuffle()` primitive — the resolving `StackObject`'s
    /// controller shuffles their own library (CR 701.19b: "a player who
    /// shuffles a library shuffles their own unless instructed otherwise").
    /// No target — same "always the controller" shape `DrawCards`/`GainLife`
    /// already use, not `Damage`/`Destroy`/`Counters`'s object-target shape.
    /// Resolved by calling the existing `Engine::shuffle_library` helper
    /// (previously pregame-setup-only) directly, which already mutates state
    /// and logs the real `LibraryShuffled`/`RngDrawn` events itself — no
    /// `CommitEvents` special-case needed, same "commit inline" convention
    /// `DrawCards` uses. No fields — a toggle, not a value, same shape as
    /// `Destroy`/`Sacrifice`/`Exile`.
    ///
    /// TODO: `Target::Player` "target player shuffles their library" is rare
    /// but real — deferred as a named follow-up, same precedent `DrawCards`
    /// itself already set for its own target gap.
    Shuffle,
    /// The `Game.Ops.Scry(n)` primitive (CR 701.22a) — the resolving
    /// `StackObject`'s controller looks at the top `n` cards of their own
    /// library, then puts any number on the bottom (any order) and the rest
    /// on top (any order). Unlike every other primitive above, this can't
    /// resolve in one step: `ResolveStackTop` peeks the top `n` ids
    /// (read-only) and pauses on a new `ProcessState::AwaitingScryChoice`
    /// rather than mutating anything itself — `Engine::submit_scry_choice`
    /// carries out the actual reorder once the controller answers. CR
    /// 701.22b: `n == 0` (or an empty library) is a no-op — no pause, no
    /// event, same "no scry event occurs" text this rule states explicitly.
    ///
    /// TODO: CR 701.22d ("whenever a player scries" trigger) — the resulting
    /// `GameEventKind::CardsScried` event already reaches `CollectTriggers`,
    /// but no `ConditionPrimitive::Scries` exists yet to match it (extends
    /// backlog item 7, same "primitive ships before its own trigger
    /// condition" precedent `Damage`/`DrawCards` already set).
    /// TODO: CR 701.22c (simultaneous multi-player scry, APNAP ordering) —
    /// single-player-effect shape only, same narrowing every prior slice
    /// takes.
    Scry(u32),
    /// The `Game.Ops.Surveil(n)` primitive (CR 701.25a) — identical to
    /// `Scry` except the "not kept on top" pile goes to the controller's
    /// graveyard instead of the bottom of their library. Unlike `Scry`'s
    /// pure reorder, the graveyard-bound pile is a real zone move (CR 400.7
    /// new `ObjectId` per card), same convention `submit_discard` already
    /// established. CR 701.25c: `n == 0` is a no-op, same as `Scry`.
    ///
    /// TODO: CR 701.25d ("whenever a player surveils" trigger) — same
    /// deferred-condition gap as `Scry`'s own TODO above.
    /// TODO: CR 701.25b ("look at additional cards while you surveil") not
    /// modeled — always exactly `n` cards.
    Surveil(u32),
    /// The `Game.Ops.Search(filter, count, destination, shuffle_after)`
    /// primitive (CR 701.23a) — the resolving `StackObject`'s controller
    /// looks through their *entire* library (not just a peeked top-N window
    /// like `Scry`/`Surveil`) for up to `count` cards matching `filter`,
    /// puts them into `destination`, then (if `shuffle_after`) shuffles.
    /// Unlike `Scry`/`Surveil`, no visibility-machinery gap blocks this at
    /// all — `GameState`/`Zone` already expose full contents freely; the
    /// only project-wide hidden-info gap (DESIGN.md's open question) is the
    /// *Lua* callback boundary, which this Rust-native primitive never
    /// crosses, same as every other `AbilityEffect` here.
    ///
    /// CR 701.23b: never mandatory for a quality search — the controller may
    /// find fewer than `count`, including zero. `ResolveStackTop` pauses on
    /// `ProcessState::AwaitingSearchChoice` with `candidates` already
    /// pre-filtered to the ids matching `filter` (evaluated with the
    /// controller as `viewer`) — `Engine::submit_search_choice` just checks
    /// the answer is a subset of `candidates` of size <= `count`, no
    /// re-filtering needed there.
    ///
    /// TODO: CR 701.23d (a search for an undefined quality/a bare quantity,
    /// e.g. "look at the top card" phrasing) is a different, mandatory-find
    /// shape — not built; this variant is quality-search (701.23a/b) only.
    /// TODO: CR 701.23e (reveal the found card) — no reveal/visibility-
    /// transition concept exists in this engine yet; purely cosmetic today
    /// anyway since hidden info isn't enforced at the Lua boundary either.
    /// TODO: CR 701.23f (an effect that redirects "search" into "search the
    /// top N instead") — would need its own `EventFilter::Search`
    /// replacement shape; doesn't exist.
    /// TODO: CR 701.23i (simultaneous multiplayer search, APNAP) —
    /// single-player-effect shape only, same narrowing `Scry`/`Surveil`'s
    /// own 701.22c/701.25b gaps already took.
    Search { filter: FilterSpec, count: u32, destination: SearchDestination, shuffle_after: bool },
    /// The `Game.Ops.BecomeCopy()` primitive (CR 707.2/707.3, layer 1) — the
    /// resolving `StackObject`'s own source becomes a copy of its first
    /// chosen target's current copiable values (e.g. Vesuvan Doppelganger's
    /// "become a copy of target creature"). No fields — a toggle, not a
    /// value, same shape as `Destroy`/`Sacrifice`/`Exile`/`Shuffle`; the
    /// target lives on the `StackObject`, not here.
    ///
    /// Unlike `Destroy`/`Sacrifice`/`Exile`, this never moves anything
    /// between zones — CR 707.4 is explicit that becoming a copy while
    /// remaining on the battlefield does not trigger ETB/LTB — so
    /// `ResolveStackTop` just mutates the source `GameObject.
    /// copiable_override` in place (see that field's own doc comment for
    /// why layer 1 is a real mutation, not a `ContinuousEffect` query like
    /// every other layer) and re-runs `run_card_script` against the new
    /// snapshot after purging whatever the source had self-registered
    /// under its old identity (CR 707.2a: "doesn't wind up with two values
    /// of each ability").
    ///
    /// CR 707.9's exceptions/modifications, applied to the copied snapshot
    /// before it's frozen into `copiable_override` - see `CopyExceptions`'
    /// own doc comment for what's representable. TODO(CR 707.6): "as ~
    /// enters" choices aren't re-prompted here - out of scope for this path,
    /// since it never re-enters the battlefield; only relevant once a real
    /// ETB-replacement "enters as a copy" path (Clone itself) exists, a
    /// separate follow-up.
    BecomeCopy(CopyExceptions),
    /// The `Game.Ops.Attach` primitive (CR 702.6a, Equip: "`[Cost]`: Attach
    /// this permanent to target creature you control. Activate only as a
    /// sorcery.") — attaches the resolving `StackObject`'s own source
    /// (`top.source`, the Equipment) to its first (only, per CR 702.6a)
    /// legal target via `commands::attach_permanent`, the same function
    /// `phase5/aura-attach` built for a resolving Aura's own CR 303.4b
    /// entry-attach. No fields — a toggle, same shape as `Destroy`/
    /// `Sacrifice`/`Exile`/`Regenerate`/`BecomeCopy`; the target lives on
    /// the `StackObject`, not here. Never moves anything between zones -
    /// `attach_permanent` mutates `attached_to`/`attachments` in place, so
    /// this logs a `GameEventKind::Attached` alongside the mutation, not a
    /// `ZoneChange`.
    ///
    /// CR 702.6a's "Activate only as a sorcery" restriction is enforced in
    /// `commands::execute_command`'s `ActivateAbility` arm by matching on
    /// `effect == Some(AbilityEffect::Attach)` directly, rather than a new
    /// `RegisteredAbility` field - a generic per-ability timing restriction
    /// would touch every one of this crate's ~94 existing `RegisteredAbility`
    /// struct literals for a restriction only this one effect shape needs
    /// today, and CR 702.6a always bundles Attach with the sorcery
    /// restriction in every real card's reminder text anyway. Deferred to a
    /// real field if a second ability shape ever needs the same timing
    /// restriction independently of its effect.
    ///
    /// TODO(CR 702.6c): "Equip [quality]"/"Equip [quality] creature" further
    /// restrictions beyond the base "creature you control" filter aren't
    /// modeled - same subtype-filter gap `target.rs` already names for
    /// Auras' own "Enchant Human"-style restriction. TODO(CR 702.6d):
    /// multiple equip abilities on one permanent aren't distinguished from
    /// each other - CR 704.5n's own recheck (`sba::
    /// collect_illegally_attached_equipment`) doesn't consult *which*
    /// ability attached it anyway, only whether the attachment is still
    /// legal per CR 301.5/702.16, so this mostly doesn't matter in
    /// practice. TODO(CR 702.6e): Equip planeswalker isn't modeled -
    /// planeswalkers are backlog item 16's own explicit non-goal.
    Attach,
    /// The `Game.Ops.ExchangeTextBoxes` primitive (CR 612.5) — exchanges
    /// the resolving `StackObject`'s first two chosen targets' "text boxes."
    /// No fields — a toggle, same shape as `BecomeCopy`/`Attach`; the
    /// targets live on the `StackObject`, not here.
    ///
    /// Deliberately *not* a `CopyExceptions`-shaped variant of `BecomeCopy`:
    /// CR 612.1 scopes text-changing effects to "rules text ... and/or ...
    /// the type line," and CR 612.5 itself says only "text boxes" - name,
    /// mana cost, color, P/T, and card types/supertypes/subtypes all stay
    /// exactly as printed on each permanent. Only `oracle_text`, `keywords`,
    /// and `script_ref` (this engine's operative stand-in for "rules text" -
    /// see `ResolveStackTop`'s dispatch for why) cross over. This is
    /// explicitly not a CR 707 copy effect (CR 612 and CR 707 are separate
    /// rule families) - no `CopyExceptions` interaction, no "copy of a
    /// copy" concern.
    ///
    /// Same underlying machinery `BecomeCopy` established for changing a
    /// *live* permanent's actual behavior (not just its displayed
    /// characteristics): `commands::purge_granted_abilities` then a fresh
    /// `run_card_script`, done symmetrically for both targets against a
    /// synthetic definition (each target's own definition, with only the
    /// three text-box fields swapped in from the other). Like `BecomeCopy`,
    /// this is a one-time mutation with no duration/reversion - permanent
    /// until another effect changes it, not a new gap.
    ///
    /// Unlike a uniform multi-word spell (e.g. Plague Spores' "destroy
    /// target creature and target land," CR 608.2b's own worked example,
    /// where each surviving legal target is still independently destroyed),
    /// this is inherently pairwise - if either of the two targets is no
    /// longer legal by the time this resolves, there's no partner to
    /// exchange with, so the whole effect is a no-op rather than doing
    /// something one-sided. `ResolveStackTop` rechecks both targets directly
    /// against `top.target_requirement`'s two `TargetRequirement`s rather
    /// than consuming the generic flattened `legal_targets` list (which
    /// loses the positional pairing a role-differentiated effect like this
    /// one needs - see `TargetSpec::Multiple`'s own doc comment on why
    /// role-differentiated multi-word effects need bespoke handling).
    ExchangeTextBoxes,
    /// CR 700.2d — 2+ chosen modes of a modal spell/ability, combined.
    /// `commands::execute_command`'s `CastSpell` arm builds this only when
    /// `modes.len() >= 2`; a single chosen mode still uses that mode's own
    /// `effect` bare (unwrapped), unchanged from pre-`phase5/modal-
    /// multi-mode` behavior. One entry per chosen mode, in *card-print*
    /// order (not the order the player listed them in `modes`) — matches
    /// Wizards' own ruling that a modal spell's chosen effects happen in
    /// the order printed on the card. `None` at a slot means that mode has
    /// no effect of its own (e.g. purely a targeting restriction with no
    /// resolution-time action) — contributes nothing when `ResolveStackTop`
    /// walks this list, but its slot must still line up 1:1 with the
    /// parallel per-mode target chunk `ResolveStackTop` re-derives from
    /// `StackObject::target_requirement` (always `TargetSpec::Multiple`
    /// when this variant is used, with one `TargetRequirement` per chosen
    /// mode, `count: 0` synthesized for a mode with no `target_requirement`
    /// of its own, so `commands::split_targets_by_requirement` always
    /// produces exactly one chunk per entry here — see that call site's own
    /// doc comment).
    Modal(Vec<Option<AbilityEffect>>),
    /// A plain spell's own resolution effect (CR 608.2m), declared by its
    /// script's optional `s.operation()` function (a sibling to
    /// `s.register_abilities(c)`, not a replacement) when that function
    /// returns a custom Lua function rather than a `Game.Ops.*` primitive
    /// tag — the same "primitive fast path vs. script fallback" shape
    /// `RegisteredAbility::effect`/`::operation` already use for triggered/
    /// activated abilities, folded into this single field since
    /// `CardDefinition` has no sibling `operation: ScriptHandle` field of
    /// its own (deliberately not added — see `ScriptRuntime::
    /// build_operation`'s own doc comment for why a new `CardDefinition`
    /// field was avoided).
    ///
    /// Never actually reaches `ResolveStackTop`'s or `ActivateAbility`'s own
    /// `AbilityEffect` dispatch — `commands::execute_command`'s `CastSpell`
    /// arm unpacks this into `StackObject`'s separate `operation`/`effect`
    /// fields at push time (`Script(handle)` -> `operation: handle, effect:
    /// None`), the same shape those two dispatches already treat `None` as
    /// "invoke `operation` for real." Listed on both anyway for
    /// exhaustiveness only, same "every variant named, no wildcard"
    /// convention `Modal` above already established for a similarly
    /// can't-actually-originate-there case.
    Script(ScriptHandle),
    /// CR 702.79a Persist / 702.93a Undying — "return it to the battlefield
    /// under its owner's control with a [-1/-1 | +1/+1] counter on it."
    /// Only ever meaningful on a `Dies`-triggered `RegisteredAbility`
    /// (`ability.source` is the departed, pre-death `ObjectId` by the time
    /// this resolves) — `ResolveStackTop` locates the actual graveyard
    /// object via `ObjectLineage::previous == Some(top.source)` (the same
    /// link `sba::commit_death` mints for exactly this kind of lookup), a
    /// no-op if it's no longer there (exiled in response, etc. — matches
    /// real Magic: nothing to return). Deliberately narrow, same posture
    /// every other first-proof slice in this file takes: doesn't propose
    /// the entry through `replacement::apply_replacements` (no CR 616
    /// "would enter, redirect elsewhere" support for this specific
    /// producer — no real Persist/Undying card needs it) and doesn't call
    /// `apply_enters_battlefield_modifiers` (so a card combining this with
    /// its own Devour/Bloodthirst/EntersTapped text won't compose — also no
    /// real card). Ordinary "when this creature enters" triggers (this
    /// card's own or anyone else's) still fire for free, since those are
    /// driven off the plain `ZoneChange` event this pushes to the ledger,
    /// not off either of the skipped mechanisms above.
    ReturnSelfWithCounter(CounterKind),
    /// CR 702.35a's second ability — "you may cast it by paying [cost]
    /// rather than paying its mana cost. If you don't, put it into your
    /// graveyard." Only ever meaningful on a `MadnessExiled`-triggered
    /// `RegisteredAbility` (`ability.source` is the departed, pre-exile
    /// hand `ObjectId` by the time this resolves, same "trigger's own
    /// source is stale, look up the live descendant" shape `ReturnSelfWithCounter`
    /// above already established) — `ResolveStackTop` locates the actual
    /// exile object via `ObjectLineage::previous == Some(top.source)`, a
    /// no-op if it's no longer there. Unlike every other `AbilityEffect`
    /// variant, resolving this doesn't finish synchronously — it always
    /// pauses (`ProcessState::AwaitingMadnessChoice`) for a real decision,
    /// same "always pauses" posture `Sacrifice`/`Destroy`/`Exile` already
    /// take, just with a genuinely new two-way resume (`Engine::
    /// decline_madness_cast`/`submit_madness_cast`) instead of a zone-move
    /// queue. See `engine::resolve::madness`'s own doc comment for the full
    /// shape.
    MadnessCastOrDiscard,
    /// CR 702.85a Cascade — "exile cards from the top of your library until
    /// you exile a nonland card whose mana value is less than this spell's
    /// mana value. You may cast that card without paying its mana cost if
    /// the resulting spell's mana value is less than this spell's mana
    /// value. Then put all cards exiled this way that weren't cast on the
    /// bottom of your library in a random order." Only ever meaningful on a
    /// `Cast`-triggered `RegisteredAbility` self-registered on the cascade
    /// spell itself while it's on the stack — `ability.source`/`top.source`
    /// is the spell object itself here (unlike `ReturnSelfWithCounter`/
    /// `MadnessCastOrDiscard`, this trigger's own source is still live, no
    /// `ObjectLineage` lookup needed). Always pauses (`ProcessState::
    /// AwaitingCascadeChoice`) once the library walk finds a candidate (or
    /// exhausts the library) — see `engine::resolve::cascade`'s own doc
    /// comment for the full shape, including the narrowing this slice takes
    /// on CR 702.85a's own second "if the resulting spell's mana value is
    /// less" re-check.
    Cascade,
    /// The `Game.Ops.ForEachPlayer(x)` primitive (`phase5/mass-choice-
    /// effects`, CR 101.4's APNAP-order "each player ..." shape — the rules
    /// text's own worked example is literally "each player sacrifices a
    /// creature"). `x` is either a script function or a bare `SacrificeRandom`/
    /// `DiscardRandom` tag (`ForEachPlayerTag`'s own doc comment covers both
    /// shapes and why the Lua binding restricts the tag case to just those
    /// two) — this field is `None` for the function shape (the real payload
    /// there is `RegisteredAbility::operation`/`StackObject::operation`, a
    /// real `ScriptHandle` for once rather than the usual `ScriptHandle(0)`
    /// placeholder every other static tag pairs with) and `Some(inner)` for
    /// the bare-tag shape, carrying `inner` directly since there's no
    /// script handle to stash it under.
    ///
    /// Function shape: `engine::resolve::each_player::
    /// resolve_for_each_player_effect` invokes that same script function
    /// once per player in CR 101.4 APNAP order (active player, then each
    /// other player in turn order), rebasing which `PlayerId` counts as
    /// `controller`/"you" each time rather than adding any new "whose
    /// perspective" mechanism — `TargetFilter::Object::controller`'s
    /// "you"/"opponent" already resolves against whatever `controller` was
    /// passed into `ScriptRuntime::invoke_operation`, not a value hardcoded
    /// to the spell's original caster, so this composes for free with
    /// `Game.ChooseN`/`Game.SelectTargets`/`Game.Ops.SacrificeChosen`/
    /// `DiscardChosen` called from inside the wrapped function. CR 101.4's
    /// "then the actions happen simultaneously": a per-player invocation's
    /// own `sacrifice_chosen`/`discard_chosen` result-table keys (see
    /// `OperationOutcome::PendingZoneMoves`) are *not* applied right after
    /// that player's own choice — `resolve_for_each_player_effect`
    /// accumulates every player's ids into one combined batch per verb and
    /// performs the real zone moves only after the last player's choice, so
    /// a mid-loop replacement effect (or a later player's own choice) still
    /// sees every permanent/card that's *about* to leave, not one that
    /// already has.
    ///
    /// Bare-tag shape: no script involved at all — `resolve_for_each_player_
    /// effect` draws each player's own random picks directly (`Engine::
    /// pick_sacrifice_random_ids`/`pick_discard_random_ids`, the same
    /// `next_rng`-driven helpers `resolve_sacrifice_random_effect`/
    /// `resolve_discard_random_effect` use for the single-player case), pure
    /// synchronous Rust with no possibility of pausing mid-computation, then
    /// merges every player's picks into one queue before the single real
    /// `continue_scripted_zone_move` call — same "simultaneous" guarantee as
    /// the function shape, for free, since there's no per-player commit
    /// point to even consider skipping.
    ///
    /// TODO: only `SacrificeRandom`/`DiscardRandom` are accepted as a bare
    /// tag (checked in the `Game.Ops.ForEachPlayer` Lua binding itself, a
    /// loud load-time error otherwise) — no real card needs any other
    /// `Game.Ops.*` primitive applied per-player without a script function
    /// wrapping it.
    ForEachPlayer(Option<Box<AbilityEffect>>),
    /// The `Game.Ops.SacrificeRandom(filter, count)` primitive (CR 701.21,
    /// "at random" — a true random procedure, not a player choice, unlike
    /// `Game.ChooseN`+`Game.Ops.SacrificeChosen`). `filter` matches
    /// `AbilityEffect::DestroyAll`'s own `EffectFilter` shape (`controller`
    /// resolved against the resolving `StackObject`'s own controller, same
    /// "you"/"opponent"/"any" vocabulary), evaluated fresh at resolution —
    /// no targeting. `count` candidates are drawn without replacement via
    /// `Engine::next_rng` (same deterministic, ledger-logged source
    /// `shuffle_library` already uses), fewer than `count` sacrificed if
    /// fewer than `count` candidates match (mirrors CR 701.17b's mill
    /// "can't mill more than exist" stance - no real card needs this engine
    /// to error out instead). Resolved via the same `continue_scripted_
    /// zone_move(Sacrifice, ...)` pipeline `Sacrifice`/`DestroyAll` already
    /// use, so CR 616.1 ordering and dies-triggers apply identically.
    SacrificeRandom { filter: EffectFilter, count: u32 },
    /// The `Game.Ops.DiscardRandom(filter, count)` primitive (CR 701.9b:
    /// "some effects... require a random discard"). `filter` matches
    /// `SacrificeRandom`'s own shape (`phase5/mass-choice-effects` follow-up
    /// — an empty table matches every card, same "no restriction" default
    /// every other `EffectFilter` consumer uses) - e.g. "discard a random
    /// land" is `Game.Ops.DiscardRandom({ card_types = { "Land" } }, 1)`.
    /// Always the resolving `StackObject`'s own controller's hand. Same
    /// "fewer than `count` if the hand is smaller" stance as
    /// `SacrificeRandom`, mirroring CR 701.17b's mill precedent. Resolved
    /// via `continue_scripted_zone_move(Discard, ...)`.
    DiscardRandom { filter: EffectFilter, count: u32 },
}

/// Where `AbilityEffect::Search`'s found cards go — narrowed to the two
/// overwhelmingly common real destinations. `Battlefield { tapped, .. }`
/// covers fetch lands (most enter tapped, some don't); no Graveyard/Exile
/// destination this slice (rare in practice, deferred).
#[derive(Clone, Copy, Debug)]
pub enum SearchDestination {
    Hand,
    /// CR 303.4f/g/h (`phase5/aura-enters-attached`): `enters_attached`
    /// applies only to a found Aura card (`commands::is_aura`) — it enters
    /// already attached, choosing among legal enchant candidates as it
    /// enters (0 candidates: CR 303.4g, stays in the library instead;
    /// `false`, the default/common case, is every plain fetch-land-style
    /// search unaffected by this). A non-Aura card with this set simply
    /// enters unattached (CR 303.4h) — the field is a no-op for it.
    Battlefield { tapped: bool, enters_attached: bool },
}

/// Which named token to create — covers the fixed-spec artifact tokens.
#[derive(Clone, Debug)]
pub enum NamedToken {
    /// A Clue artifact token with "{2}, Sacrifice this artifact: Draw a card."
    Clue,
    /// A Food artifact token with "{2}, Sacrifice this artifact: Gain 3 life."
    Food,
    // TODO: Treasure ("{T}, Sacrifice this artifact: Add one mana of any color")
    // blocked on mana-choice player input / coroutines (backlog item 9).
}

/// Full spec for a creature (or artifact creature, enchantment creature, etc.)
/// token generated by `Game.Tokens.Creature` or `Game.Tokens.ArtifactCreature`.
#[derive(Clone, Debug)]
pub struct CreatureTokenSpec {
    pub subtypes: Vec<String>,
    pub colors: ColorSet,
    pub power: i32,
    pub toughness: i32,
    pub keywords: KeywordSet,
    /// Extra card types beyond Creature (e.g. ARTIFACT, ENCHANTMENT).
    pub additional_types: CardTypeSet,
    /// Supertypes such as LEGENDARY.
    pub additional_supertypes: SupertypeSet,
    /// Override for the token's name; defaults to joined subtypes if None.
    pub name: Option<String>,
}

/// The discriminated spec passed from Lua to `ResolveStackTop`.
#[derive(Clone, Debug)]
pub enum TokenSpec {
    Named(NamedToken),
    Creature(CreatureTokenSpec),
    /// CR 707.10f/111: "create a token that's a copy of [object]" — the
    /// `ObjectId` being copied. Unlike `Named`/`Creature`, this token's
    /// `CardDefinition` isn't built from this spec at all (see
    /// `build_token_definition`'s own doc comment) — `ResolveStackTop`'s
    /// `CreateToken` arm reads the copied object's *current* copiable
    /// values (its `copiable_override` if it's itself a copy, per CR
    /// 707.3, else its own `definition`) instead, and re-runs
    /// `ScriptRuntime::run_card_script` against that snapshot so the
    /// token's copied triggered/static abilities actually self-register
    /// (CR 707.2a: abilities are copiable) — every other `TokenSpec` skips
    /// that call entirely since a Named/Creature token has no script.
    /// CR 707.9's exceptions/modifications, applied to the copied snapshot
    /// before it's frozen as the token's own `definition` - see
    /// `CopyExceptions`' own doc comment for what's representable.
    CopyOf(ObjectId, CopyExceptions),
}

/// CR 707.9's copy-effect exceptions/modifications, applied to a copy
/// snapshot at the moment it's taken (by `BecomeCopy`/`CopyOf`) before it's
/// frozen into `GameObject.copiable_override`/a token's own `definition`.
/// CR 707.9's real shapes are open-ended (arbitrary card text) - narrowed to
/// the two overwhelmingly common real ones, matching this codebase's own
/// precedent everywhere else of modeling a fixed, parametrized primitive
/// rather than a general text-modification system:
/// - 707.9b (modify a characteristic): `set_power_toughness`, e.g. "except
///   it has base power and toughness 1/1" - the single most common real
///   exception on Clone-variant cards.
/// - 707.9a (gain an ability as part of copying): `add_keywords`, e.g.
///   "except it also has flying" - same shape, applied to the snapshot's
///   `keywords` field directly (union, same "in addition to" convention
///   `EffectPrimitive::AddKeyword` already uses for continuous effects).
///
/// - 707.9c/d (some characteristics specifically *not* copied, original
///   retained - the inverse of 9a/9b): `retain_name`/`retain_color`, e.g.
///   "except it retains its own name" - the two most common real "retains
///   original X" shapes (CR 707.9d itself singles out color for special
///   handling, clearing any color indicator too). Narrower than the full
///   CR 707.9c (any of the seven copiable characteristics) - no real card
///   combination needs retained P/T/types/subtypes yet, same "no real card"
///   posture `set_power_toughness`/`add_keywords` themselves were scoped
///   with originally.
///
/// - 707.9a's *other* real shape (gain a whole new ability, not just a
///   keyword, as part of copying): `grant_recurring_copy_trigger`, Vesuvan
///   Doppelganger's own "except it has 'at the beginning of your upkeep, you
///   may have this creature become a copy of target creature'" — narrowed to
///   this one concrete shape rather than a general "grant an arbitrary
///   ability" system (no Lua-authorable ability blueprint type exists to
///   describe one). See `commands::grant_linked_copy_ability`'s own doc
///   comment for how the granted ability itself is built and why it's a
///   hardcoded Rust shape rather than something `run_card_script` produces.
///
/// `Default`'s all-empty value is a bare copy - unaffected, same result as
/// before this type existed. CR 707.9e/g's own schema prerequisite — "was
/// this the most recent copy effect applied to this object" — is real now
/// too (`phase5/copy-generation`, `GameObject.copy_generation`/`GameState.
/// linked_ability_generation`); `grant_recurring_copy_trigger`'s own granted
/// ability is tagged with it, though `commands::purge_granted_abilities`
/// already deletes the *previous* granted ability outright on every real
/// re-copy in this engine (both producers call it before re-registering),
/// so the generation tag's own supersession check stays structurally
/// unreached through either of this engine's two copy producers — named,
/// not silently assumed, while building this exception.
///
/// - 707.9e (an "additional effect" exception, not a characteristic
///   modification): `enters_with_additional_counters`, e.g. Spark Double's
///   "if it's a creature, it enters with an additional +1/+1 counter on it"
///   - a different shape from every field above, since none of those touch
///   anything but the copy snapshot's `CardDefinition` fields, while this
///   one is a real `GameObject`-level mutation (a counter). Applied by both
///   real copy producers right after their own `copy_generation` bump, as a
///   plain `GameEventKind::CounterAdded` push - reuses `CommitEvents`'
///   existing generic counter-apply path, no new event variant needed.
///   707.9e's own second sentence (the exception's effect doesn't happen if
///   *another* copy effect applies to the object first) is real now too
///   (`phase5/chained-copy-effects`) - see `Engine::submit_copy_choice`'s own
///   doc comment for the chain-detection/`carry_forward_if_declined`
///   mechanism. `CounterAmount::X` (`phase5/altered-ego-x-counters`, CR
///   707.9e's own Altered Ego worked example) is the variable-amount sibling
///   to `Fixed` - resolved against the *entering object's own* `x_value`
///   (`StackObject.x_value`, captured once at `AwaitingCopyChoice`'s raise
///   site and carried unchanged through any chain, same invariant-through-
///   the-chain treatment `colors_spent`/`stack_id`/`controller` already get -
///   the X belongs to Altered Ego itself, not to whichever creature ends up
///   copied at any one link), not a per-round value re-derived from whatever
///   was just copied.
#[derive(Clone, Debug)]
pub struct CopyExceptions {
    pub set_power_toughness: Option<(i32, i32)>,
    pub add_keywords: KeywordSet,
    pub retain_name: bool,
    pub retain_color: bool,
    pub grant_recurring_copy_trigger: bool,
    pub enters_with_additional_counters: Option<(CounterKind, CounterAmount)>,
}

/// The amount half of `CopyExceptions::enters_with_additional_counters` -
/// `Fixed` (Spark Double's "an additional +1/+1 counter") or `X` (Altered
/// Ego's "X additional +1/+1 counters", resolved against the entering
/// object's own `StackObject.x_value` at copy-application time - see
/// `CopyExceptions`'s own doc comment for why that's invariant through a
/// chain). A single field with a two-shaped amount, not two separate
/// `Option` fields, so "fixed and X both set at once" isn't representable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterAmount {
    Fixed(u32),
    X,
}

impl Default for CopyExceptions {
    fn default() -> Self {
        CopyExceptions {
            set_power_toughness: None,
            add_keywords: KeywordSet::empty(),
            retain_name: false,
            retain_color: false,
            grant_recurring_copy_trigger: false,
            enters_with_additional_counters: None,
        }
    }
}

impl CopyExceptions {
    /// Applies these exceptions to `snapshot` in place, right before it's
    /// frozen - shared by `BecomeCopy` and `CreateToken`'s `CopyOf` arm so
    /// the two real copy-producing call sites can't drift.
    ///
    /// `original` is the copying object's own true, pre-copy
    /// `CardDefinition` (`GameObject::definition`, never overwritten by any
    /// copy machinery - distinct from `copiable_override`) - `None` only for
    /// `TokenSpec::CopyOf`, where a freshly created token has no prior "own
    /// identity" to retain from at all. `retain_name`/`retain_color` are a
    /// deliberate, tested no-op in that case, not silently broken.
    pub fn apply(&self, snapshot: &mut crate::types::card::CardDefinition, original: Option<&crate::types::card::CardDefinition>) {
        if let Some((power, toughness)) = self.set_power_toughness {
            snapshot.power = Some(crate::types::card::PTValue::Fixed(power));
            snapshot.toughness = Some(crate::types::card::PTValue::Fixed(toughness));
        }
        snapshot.keywords |= self.add_keywords;
        if let Some(original) = original {
            if self.retain_name {
                snapshot.name = original.name.clone();
            }
            if self.retain_color {
                // CR 707.9d: an explicit color indicator matching the
                // original's own resolved color - "any color indicator of
                // that object is also not copied" is satisfied by this
                // being the *original's* indicator-or-mana-cost-derived
                // color, not whatever the copied source's own indicator was.
                snapshot.color_indicator = Some(original.colors());
            }
        }
    }
}

/// CR 612.5's "text box" - `oracle_text`, `keywords`, and `script_ref` - the
/// three `CardDefinition` fields that stand in for "rules text" in this
/// engine (see `AbilityEffect::ExchangeTextBoxes`'s own doc comment for why
/// exactly these three and not more). Overwrites `snapshot`'s in place with
/// `other`'s; everything else on `snapshot` (name, mana cost, P/T, card
/// types/subtypes/supertypes) is left untouched, unlike `CopyExceptions`'
/// full-copy-by-default shape above.
pub fn apply_text_box(snapshot: &mut crate::types::card::CardDefinition, other: &crate::types::card::CardDefinition) {
    snapshot.oracle_text = other.oracle_text.clone();
    snapshot.keywords = other.keywords;
    snapshot.script_ref = other.script_ref.clone();
}

/// The two condition shapes `Game.Conditions.*` covers this slice — a
/// Rust-native fast path so common trigger shapes don't need a full Lua
/// condition function written out.
///
/// `Clone`-only (not `Copy`) since `DiesMatching`/`LeavesBattlefieldMatching`
/// carry a `FilterSpec`, which owns a `Vec`/`String` — a real, anticipated
/// consequence of adding those two variants (backlog item 7), not an
/// oversight. Ripples to `ConditionSpec` below (same reason) and to
/// `script.rs`'s `ConditionTag`/`OncePerTurnTag` wrappers, which drop their
/// own `Copy` derive to match. `PartialEq`/`Eq` stay real — `FilterSpec`
/// gained matching derives alongside this change, since every one of its
/// own fields already supported them.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ConditionPrimitive {
    /// "Whenever this permanent enters the battlefield" (CR 603 ETB
    /// triggers) — matches a `ZoneChange` event moving `source` to the
    /// battlefield. Mirrors the hand-written
    /// `event.object == source and event.to == "battlefield"` Lua condition
    /// from the prior slice exactly.
    EntersBattlefield,
    /// No filter — always matches. A real variant (not just `None`) so a
    /// Lua author has an explicit, correct spelling for "unconditional";
    /// see `RegisteredAbility::condition`'s doc for why both spellings
    /// exist.
    Always,
    /// "Whenever this deals damage" (CR 603.2, matched against
    /// `GameEventKind::DamageDealt`) — `source` is the one dealing the
    /// damage. Reachable today because combat damage (`AssignCombatDamage`)
    /// already routes its `DamageDealt` events through the real
    /// `CommitEvents`→`SbaLoop`→`CollectTriggers` pipeline (see the
    /// `fix-replacement-bypasses` slice) — unlike a phase/step-boundary
    /// condition, which `AdvanceTurnStep` currently can't reach at all (see
    /// this file's own TODO note below and docs/implementation.md's backlog).
    /// No combat-vs-noncombat distinction: `DamageDealt` itself doesn't
    /// carry one, so this matches any damage this object deals, by whatever
    /// means eventually produces the event.
    DealsDamage,
    /// "Whenever this is dealt damage" — the mirror image of `DealsDamage`,
    /// matching on the event's `target` instead of its `source`. Only
    /// matches a `Target::Object` target (a player being dealt damage isn't
    /// "this" for a permanent's own ability).
    IsDealtDamage,
    /// "At the beginning of your upkeep" (CR 502.1's most common trigger
    /// shape) — matches `GameEventKind::StepStarted{step: Step::Upkeep}`
    /// only when the ability's controller is also the active player.
    /// Reachable now that `AdvanceTurnStep` (engine.rs) routes its events
    /// through the real `ReplacementCheck`→`CommitEvents`→`SbaLoop`→
    /// `CollectTriggers` pipeline instead of `log_events` (see that
    /// function's own doc comment) — the concrete condition this slice adds
    /// to prove the plumbing fix actually works, not just the fix itself.
    BeginningOfYourUpkeep,
    /// "At the beginning of each upkeep" — the less common, controller-
    /// agnostic sibling of `BeginningOfYourUpkeep` (e.g. Waste Not-style
    /// effects). Matches every `Step::Upkeep` `StepStarted` regardless of
    /// whose turn it is.
    BeginningOfEachUpkeep,
    /// "At the beginning of your untap step" (CR 502.4's own trigger shape —
    /// genuinely rare on real cards, since the step grants no priority, but
    /// CR 603.3's last sentence still applies: a trigger whose condition is
    /// met here just waits to go on the stack until the next time a player
    /// would receive priority, which `AdvanceTurnStep`'s `Untap` branch now
    /// achieves by routing through the real pipeline with
    /// `PipelineExit::AdvanceTurnStep` instead of `BackToPriority` — see
    /// that branch's own doc comment). Matches
    /// `GameEventKind::StepStarted{step: Step::Untap}` only when the
    /// ability's controller is also the active player, same "your" shape
    /// `BeginningOfYourUpkeep` already established.
    BeginningOfYourUntap,
    /// "At the beginning of each untap step" — the controller-agnostic
    /// sibling of `BeginningOfYourUntap`, same relationship
    /// `BeginningOfEachUpkeep` has to `BeginningOfYourUpkeep`.
    BeginningOfEachUntap,
    /// "When this creature dies" (CR 700.4/603.10) — matches a `ZoneChange`
    /// event moving `source` from the battlefield to a graveyard. Only ever
    /// reachable via `collect_triggers`'s CR 603.10 last-known-information
    /// fallback: by the time this is checked, `source` has already left
    /// `state.objects`/`state.abilities` (purged by `commands::
    /// retire_object` as part of the very death this condition is matching)
    /// - see that function's own doc comment. Matched against `last_known_
    /// info.object_id`, not the event's own `object` field, because a
    /// battlefield->graveyard move mints a fresh `ObjectId` for the
    /// graveyard-side object (CR 400.7) - `object` in the committed event is
    /// that *new* id, never `source`.
    Dies,
    /// "When this leaves the battlefield" (CR 603.10) — the broader sibling
    /// of `Dies`: matches any `ZoneChange` moving `source` off the
    /// battlefield, regardless of destination (graveyard, exile, hand,
    /// library, ...), not just a death. Same last-known-information
    /// matching shape as `Dies`.
    LeavesBattlefield,
    /// "Whenever you draw a card" (CR 121) — matches a `ZoneChange` event
    /// moving *any* card from library to hand, filtered to only the ability
    /// controller's own draws via `last_known_info.controller` (the pre-move
    /// object clone every `ZoneChange` carries — no live state lookup
    /// needed). Deliberately not `source`-scoped like `EntersBattlefield`:
    /// this condition doesn't care what `source` is, only who controls it
    /// and who drew. Matches by zone-shape only (`Library`→`Hand`) — there's
    /// no separate "this was a draw" flag on the event, so a hypothetical
    /// future non-draw Library→Hand effect (CR 121.5 draws a distinction
    /// real MTG cares about) would false-positive here; no such effect
    /// exists in this engine yet.
    DrawsCard,
    /// "Whenever a player draws a card" — the controller-agnostic sibling of
    /// `DrawsCard` (e.g. Underworld Dreams-style effects), same relationship
    /// `BeginningOfEachUpkeep` has to `BeginningOfYourUpkeep`.
    EachDrawsCard,
    /// "Whenever you discard a card" — matches a `ZoneChange` event moving
    /// *any* card from hand to graveyard, filtered to only the ability
    /// controller's own discards via `last_known_info.controller`, same
    /// "zone-shape only" matching `DrawsCard` already established (a
    /// Hand→Graveyard move is always a discard in this engine today — no
    /// other effect produces that shape yet, matching `DrawsCard`'s own
    /// "no separate flag on the event" caveat). Reachable now that
    /// `Engine::submit_discard` (CR 514.1's cleanup hand-size discard)
    /// routes its events through `CollectTriggers` instead of committing
    /// them directly — the same `AdvanceTurnStep`-shaped pipeline-bypass gap
    /// `upkeep-triggers`/this backlog item's Untap fix already closed
    /// elsewhere. A cost-payment `Discard` (`commands::
    /// move_to_graveyard_for_cost`) already reached the real pipeline before
    /// this fix, so both discard sources are covered.
    DiscardsCard,
    /// "Whenever a player discards a card" — the controller-agnostic sibling
    /// of `DiscardsCard` (e.g. Waste Not/Megrim-style effects), same
    /// relationship `EachDrawsCard` has to `DrawsCard`.
    EachDiscardsCard,
    /// "When this card is discarded" — the self-referential sibling
    /// `DiscardsCard`/`EachDiscardsCard` don't cover (those are
    /// player-relative: "whenever you/a player discards *a* card", not
    /// "whenever *this* card is discarded"). Same CR 603.10 look-back-in-time
    /// shape `Dies`/`LeavesBattlefield` already use for a battlefield
    /// departure, extended by symmetry to a hand departure — CR 603.10a's own
    /// enumerated list doesn't explicitly name "leaves the hand" among its
    /// categories, but the mechanism (match by `last_known_info.object_id`
    /// against a source that's already been purged from `state.abilities` by
    /// the time this is checked) is identical, so this reuses it rather than
    /// inventing a second one. Reachable via both discard sources today - a
    /// cost-payment `Discard` (`commands::move_to_graveyard_for_cost`) and
    /// the cleanup hand-size discard (`Engine::submit_discard`) already stash
    /// their departed `Triggered` abilities for the `collect_triggers` LKI
    /// fallback to see (closed by the same fix that added `DiscardsCard`).
    Discarded,
    /// "When you cycle this card" (CR 702.29c/f) — narrower self-referential
    /// sibling of `Discarded`: unlike `Discarded` (matches *any* hand ->
    /// graveyard departure of this card, whatever the cause), this matches
    /// only `GameEventKind::Cycled`, a purpose-built marker event logged
    /// alongside (not instead of) the ordinary `ZoneChange` only when the
    /// discard specifically paid a `AdditionalCost::DiscardSelf` cost
    /// (`Engine::continue_cost_zone_moves`'s `cycling_source` parameter) —
    /// the same discriminator this engine already uses to mean "this is a
    /// Cycling/Typecycling ability's own cost" (`commands::activate`'s
    /// hand-zone override). CR 702.29c's own "triggers from whatever zone
    /// the card winds up in after it's cycled" falls out for free: this
    /// primitive never reads `to` at all. Reuses the same CR 603.10
    /// departed/LKI fallback shape `Discarded` itself uses, since the
    /// cycled card's own abilities are purged the moment it leaves hand,
    /// same as any other departure.
    Cycled,
    /// "When this card is exiled this way" (CR 702.35a's second ability) —
    /// same narrower-self-referential-sibling-of-`Discarded` shape `Cycled`
    /// above already established, matching only `GameEventKind::
    /// MadnessExiled`, a purpose-built marker event logged alongside the
    /// ordinary `ZoneChange` only when a discard's own madness redirect
    /// (`ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile)`,
    /// registered by the card on itself) actually won
    /// (`replacement::madness_marker_event`). Reuses the same CR 603.10
    /// departed/LKI fallback shape `Cycled`/`Discarded` both use, since the
    /// card's own madness triggered ability is purged the moment it leaves
    /// hand, same as any other departure.
    MadnessExiled,
    /// "When you cast this spell" (CR 601.2i, CR 702.85a Cascade's own first
    /// clause) — matches `GameEventKind::SpellCast{object} if object ==
    /// source`. Unlike `Cycled`/`MadnessExiled`/`Discarded`, this is a
    /// self-referential condition on a still-*live* source (the spell is
    /// still sitting on the stack when this fires - nothing purges it), so
    /// it's matched by `triggers::collect_triggers`'s ordinary main loop,
    /// no CR 603.10 departed/LKI fallback needed at all.
    ///
    /// No real self-registration producer exists for this yet - tried
    /// generically (a `CommandOutcome::stack_entries` field, `Engine::
    /// handle_pending_action` re-running a card's own `run_card_script` a
    /// second time once it hits the stack, mirroring `battlefield_entries`)
    /// but reverted: every existing card's `register_abilities(c)` is
    /// written assuming it runs *once* per object lifecycle - running it a
    /// second time at cast time, on top of the *existing* battlefield-entry
    /// call, double-registers every ordinary ETB trigger too (`commands::
    /// remap_abilities` carries the cast-time registration forward across
    /// the stack->battlefield move, and the pre-existing `run_card_script`
    /// call at battlefield-entry then registers a second, independent copy
    /// on the same final object) - caught directly by `phase5_escape_
    /// provenance.rs`'s own pre-existing test regressing into a genuine CR
    /// 603.3b two-copy ordering-choice pause instead of a single clean
    /// trigger. Direct `RegisteredAbility` construction only for now, same
    /// posture other alternative-cost-family slices already take for their
    /// own Lua-inaccessible pieces.
    Cast,
    /// "At the beginning of combat on your turn" (CR 507) — matches
    /// `GameEventKind::StepStarted{step: Step::BeginningOfCombat}` only when
    /// the ability's controller is also the active player. Same reachability
    /// story as `BeginningOfYourUpkeep`: `AdvanceTurnStep` already routes
    /// this step's `StepStarted` through the real pipeline, neither `Untap`
    /// nor `Cleanup` special-casing applies to it.
    BeginningOfYourCombat,
    /// "At the beginning of each combat" — the controller-agnostic sibling
    /// of `BeginningOfYourCombat`.
    BeginningOfEachCombat,
    /// "At the beginning of your end step" (CR 512/513) — matches
    /// `GameEventKind::StepStarted{step: Step::End}` only when the ability's
    /// controller is also the active player. Same reachability story as
    /// `BeginningOfYourUpkeep`.
    BeginningOfYourEndStep,
    /// "At the beginning of each end step" — the controller-agnostic sibling
    /// of `BeginningOfYourEndStep`.
    BeginningOfEachEndStep,
    /// "Whenever you scry" (CR 701.22d) — matches `GameEventKind::CardsScried`
    /// with `event.player == controller`. Reachable now that
    /// `Engine::submit_scry_choice` logs that event (`phase5/scry-surveil`
    /// slice); this is the deferred trigger-condition half that slice's own
    /// TODO named.
    Scries,
    /// "Whenever a player scries" — the controller-agnostic sibling of
    /// `Scries`, same relationship `EachDrawsCard` has to `DrawsCard`.
    EachScries,
    /// "Whenever you surveil" (CR 701.25d) — same shape as `Scries`, against
    /// `GameEventKind::CardsSurveiled`.
    Surveils,
    /// "Whenever a player surveils" — the controller-agnostic sibling of
    /// `Surveils`.
    EachSurveils,
    /// "Whenever a creature you control dies" (CR 700.4/603.10) — the
    /// filter-scoped sibling of `Dies`: unlike `Dies` (matches only when the
    /// *ability's own source* is what died), this matches *any* qualifying
    /// death, checked via `filter.matches(&lki, controller)` — `controller`
    /// is the ability's own controller, so `FilterSpec { controller:
    /// Some(FilterController::You), .. }` naturally gives "you control" for
    /// free (no separate `Each*` sibling needed the way `Dies` never had
    /// one - `FilterController::Any` already covers that case). The
    /// ability's `source` here is a separate, still-live permanent watching
    /// for deaths (Blood Artist-style) - this is checked in
    /// `collect_triggers`'s ordinary live-source loop, never the CR 603.10
    /// departed/LKI fallback loop `Dies`/`LeavesBattlefield` themselves rely
    /// on (that loop explicitly whitelists only those two identity-scoped
    /// variants).
    DiesMatching(FilterSpec),
    /// "Whenever a permanent matching filter leaves the battlefield" — the
    /// filter-scoped sibling of `LeavesBattlefield`, same relationship
    /// `DiesMatching` has to `Dies`.
    LeavesBattlefieldMatching(FilterSpec),
}

// TODO(backlog item 7, CR 603.10 continued): Dies/LeavesBattlefield are wired
// for the SBA-driven death path (sba::commit_death) and Sacrifice/Discard
// cost payment (commands::move_to_graveyard_for_cost, via commit_payment ->
// Engine::submit_payment) - see move_to_graveyard_for_cost's own doc comment
// for why the Discard half stays dormant. Two call sites still discard their
// departed abilities outright: submit_discard's CR 514.1 hand-size discard,
// and draw_step's library-side retirement - both would be inert to wire
// today (neither can produce a Battlefield-departure LKI).
// ~~"Whenever a creature you control dies" (filtered by something other than
// "is this exact object")~~ - done, DiesMatching/LeavesBattlefieldMatching
// above (phase5/trigger-completeness-cleanup slice).
//
// TODO(trigger system completeness, backlog item 7): once-per-turn/
// intervening-if legality (CR 603.2-603.3) is still untouched. Untap-step
// events still can't trigger anything (CR 502.4 grants no priority there, so
// there's no window to place a trigger before) and Cleanup's own
// CleanupSbaCheck flow still doesn't run collect_triggers at all (a second,
// differently-shaped fix, left for its own slice - see
// docs/implementation.md's backlog item 7a).

impl ConditionPrimitive {
    /// Zero Lua VM calls — evaluated directly against the event that's being
    /// checked. `source` is the registered ability's source object (not
    /// necessarily the event's own object); `controller` is that source's
    /// controller and `active_player` is whoever's turn it currently is —
    /// both needed for `BeginningOfYourUpkeep`'s "your" relativity, which
    /// `DealsDamage`/`IsDealtDamage`/`EntersBattlefield` never needed.
    pub fn matches(
        &self,
        source: ObjectId,
        controller: PlayerId,
        active_player: PlayerId,
        event: &GameEventKind,
    ) -> bool {
        match self {
            ConditionPrimitive::Always => true,
            ConditionPrimitive::EntersBattlefield => matches!(
                event,
                GameEventKind::ZoneChange { object, to, .. }
                    if *object == source && *to == ZoneKind::Battlefield
            ),
            ConditionPrimitive::DealsDamage => matches!(
                event,
                GameEventKind::DamageDealt { source: dealer, .. } if *dealer == source
            ),
            ConditionPrimitive::IsDealtDamage => matches!(
                event,
                GameEventKind::DamageDealt { target, .. } if *target == Target::Object(source)
            ),
            ConditionPrimitive::BeginningOfYourUpkeep => {
                controller == active_player
                    && matches!(event, GameEventKind::StepStarted { step: Step::Upkeep })
            }
            ConditionPrimitive::BeginningOfEachUpkeep => {
                matches!(event, GameEventKind::StepStarted { step: Step::Upkeep })
            }
            ConditionPrimitive::BeginningOfYourUntap => {
                controller == active_player
                    && matches!(event, GameEventKind::StepStarted { step: Step::Untap })
            }
            ConditionPrimitive::BeginningOfEachUntap => {
                matches!(event, GameEventKind::StepStarted { step: Step::Untap })
            }
            ConditionPrimitive::Dies => matches!(
                event,
                GameEventKind::ZoneChange { from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, last_known_info: Some(lki), .. }
                    if lki.object_id == source
            ),
            ConditionPrimitive::LeavesBattlefield => matches!(
                event,
                GameEventKind::ZoneChange { from: ZoneKind::Battlefield, last_known_info: Some(lki), .. }
                    if lki.object_id == source
            ),
            ConditionPrimitive::DrawsCard => matches!(
                event,
                GameEventKind::ZoneChange { from: ZoneKind::Library, to: ZoneKind::Hand, last_known_info: Some(lki), .. }
                    if lki.controller == controller
            ),
            ConditionPrimitive::EachDrawsCard => matches!(
                event,
                GameEventKind::ZoneChange { from: ZoneKind::Library, to: ZoneKind::Hand, .. }
            ),
            ConditionPrimitive::DiscardsCard => matches!(
                event,
                GameEventKind::ZoneChange { from: ZoneKind::Hand, to: ZoneKind::Graveyard, last_known_info: Some(lki), .. }
                    if lki.controller == controller
            ),
            ConditionPrimitive::EachDiscardsCard => matches!(
                event,
                GameEventKind::ZoneChange { from: ZoneKind::Hand, to: ZoneKind::Graveyard, .. }
            ),
            ConditionPrimitive::Discarded => matches!(
                event,
                GameEventKind::ZoneChange { from: ZoneKind::Hand, to: ZoneKind::Graveyard, last_known_info: Some(lki), .. }
                    if lki.object_id == source
            ),
            ConditionPrimitive::Cycled => matches!(
                event,
                GameEventKind::Cycled { object, .. } if *object == source
            ),
            ConditionPrimitive::MadnessExiled => matches!(
                event,
                GameEventKind::MadnessExiled { object, .. } if *object == source
            ),
            ConditionPrimitive::Cast => matches!(
                event,
                GameEventKind::SpellCast { object } if *object == source
            ),
            ConditionPrimitive::BeginningOfYourCombat => {
                controller == active_player
                    && matches!(event, GameEventKind::StepStarted { step: Step::BeginningOfCombat })
            }
            ConditionPrimitive::BeginningOfEachCombat => {
                matches!(event, GameEventKind::StepStarted { step: Step::BeginningOfCombat })
            }
            ConditionPrimitive::BeginningOfYourEndStep => {
                controller == active_player
                    && matches!(event, GameEventKind::StepStarted { step: Step::End })
            }
            ConditionPrimitive::BeginningOfEachEndStep => {
                matches!(event, GameEventKind::StepStarted { step: Step::End })
            }
            ConditionPrimitive::Scries => matches!(
                event,
                GameEventKind::CardsScried { player, .. } if *player == controller
            ),
            ConditionPrimitive::EachScries => matches!(event, GameEventKind::CardsScried { .. }),
            ConditionPrimitive::Surveils => matches!(
                event,
                GameEventKind::CardsSurveiled { player, .. } if *player == controller
            ),
            ConditionPrimitive::EachSurveils => matches!(event, GameEventKind::CardsSurveiled { .. }),
            // CR 700.4/603.10: unlike Dies (matched by identity against
            // `source` in the departed/LKI fallback loop), this is checked
            // in the ordinary live-source loop against a separate watching
            // ability - `controller` here is that ability's own controller,
            // giving FilterSpec's existing `FilterController::You`/`Any` the
            // "you control"/"any player controls" distinction for free.
            ConditionPrimitive::DiesMatching(filter) => matches!(
                event,
                GameEventKind::ZoneChange { from: ZoneKind::Battlefield, to: ZoneKind::Graveyard, last_known_info: Some(lki), .. }
                    if filter.matches(lki, lki.controller, controller)
            ),
            ConditionPrimitive::LeavesBattlefieldMatching(filter) => matches!(
                event,
                GameEventKind::ZoneChange { from: ZoneKind::Battlefield, last_known_info: Some(lki), .. }
                    if filter.matches(lki, lki.controller, controller)
            ),
        }
    }
}

/// A condition/operation source string resolves to one of these shapes
/// at load time (`ScriptRuntime::load_condition`) — a recognized
/// `Game.Conditions.*` tag (`Primitive`, no further Lua calls needed ever
/// again), a genuine custom Lua function (`Script`, invoked for real via
/// `ScriptRuntime::invoke_condition` each time, same as before this slice),
/// or `PrimitiveOncePerTurn` (CR 603.2h: "This ability triggers only once
/// each turn" layered on top of any `ConditionPrimitive`, via
/// `Game.Conditions.OncePerTurn(tag)` wrapping any other `Game.Conditions.*`
/// tag). Matching itself is identical to `Primitive`'s own — the cap is
/// enforced separately, in `triggers::collect_triggers`/`Engine`'s
/// `CollectTriggers` stage, against `GameState::once_per_turn_triggered`
/// (keyed by the ability's own stable `AbilityId`, reset each turn). Not
/// offered for a `Script` condition this slice - a custom Lua condition
/// function has no engine-side `AbilityId`-keyed slot to consult without
/// also threading one through `ScriptContext`, deferred as a named
/// follow-up.
/// `Clone`-only (not `Copy`) — see `ConditionPrimitive`'s own doc comment for
/// why (its `DiesMatching`/`LeavesBattlefieldMatching` variants own a
/// `FilterSpec`).
#[derive(Clone, Debug)]
pub enum ConditionSpec {
    Primitive(ConditionPrimitive),
    Script(ScriptHandle),
    PrimitiveOncePerTurn(ConditionPrimitive),
}

/// CR 603.4's intervening-if clause ("When/Whenever/At X, if Y, Z") is a
/// pure state predicate, not event-shaped like `ConditionPrimitive` (whose
/// `matches` always takes a `&GameEventKind`) — checked once when the
/// trigger event fires (folded into `triggers::collect_triggers`, alongside
/// but separate from the ability's own trigger-event `condition`) and again
/// at resolution (`Engine`'s `ResolveStackTop`, CR 608.2a). Reused as-is for
/// CR 611.2c's conditional continuous effects (`ContinuousEffect::condition`)
/// — the same "evaluate a live board-state predicate, get a bool" shape both
/// features need, see that field's own doc comment.
///
/// `ControllerControlsAtLeast` resolves "you" as `object_controller`, always
/// the raw `GameObject.controller` field rather than `layers::
/// effective_controller` — same Layer 2 gap `ContinuousEffect::applies_to`'s
/// own `FilterController` check already has (see `Layer::ControlChangingEffects`'s
/// doc comment: nothing in live gameplay resolves effective control yet), and
/// avoids `applicable_effects_in_layer` recursing into `effective_controller`
/// (which itself calls `applicable_effects_in_layer`) to evaluate a
/// condition.
///
/// TODO: more shapes (hand size, "still on the battlefield") are the same
/// category of follow-up every other primitive in this file started narrow
/// before growing.
///
/// `Clone`-only (not `Copy`) — `ControllerControlsAtLeast` carries a
/// `FilterSpec`, same reason `ConditionPrimitive` dropped `Copy` for
/// `DiesMatching`/`LeavesBattlefieldMatching`. Ripples to `InterveningIfSpec`
/// below (same reason).
#[derive(Clone, Debug)]
pub enum InterveningIfPrimitive {
    ControllerAtOrBelowLife(i32),
    /// CR 611.2c's overwhelmingly common real shape: "as long as you
    /// control `count` or more permanents matching `filter`."
    ControllerControlsAtLeast { filter: FilterSpec, count: u32 },
    /// CR 603.4/702.138b: "if it escaped" — Kroxa/Uro's own real card
    /// pattern (an ordinary ETB trigger's intervening-if, not CR 702.138c/
    /// d's own "escapes with [counters/ability]" templates, which are a
    /// different, narrower shape - see `ReplacementPayload::
    /// EntersWithCountersEscape` for those). Checked against the ability's
    /// own `source` object's `GameObject::entered_via_escape` - `false`
    /// (never holds) if `source` no longer resolves live, same
    /// conservative default `ControllerControlsAtLeast` already takes for
    /// an unresolvable state.
    Escaped,
    /// CR 603.4/702.79a/702.93a: "if it had no [kind] counters on it" —
    /// checked against the object *as it was the moment it died*, not live
    /// state (by the time this is ever evaluated, a `Dies`-sourced
    /// `source` has already been purged/renumbered by `retire_object`, the
    /// same CR 603.10 problem `ConditionPrimitive::Dies` itself solves with
    /// a last-known-info fallback). Reads `lki` when the caller has one
    /// (always true in practice — the only real producer is a departed
    /// `Dies` trigger, both at collection in `triggers::collect_triggers`'s
    /// own LKI-fallback loop and at the CR 608.2a resolution recheck via
    /// `StackObject::source_lki`); falls back to a live `state.object(
    /// source)` lookup otherwise, same conservative-default shape `Escaped`
    /// above already takes for its own unresolvable-source case.
    HadNoCountersOfKind(CounterKind),
}

impl InterveningIfPrimitive {
    pub fn holds(
        &self,
        state: &crate::types::game::GameState,
        controller: PlayerId,
        source: ObjectId,
        lki: Option<&crate::types::object::GameObject>,
    ) -> bool {
        match self {
            InterveningIfPrimitive::ControllerAtOrBelowLife(threshold) => {
                state.player(controller).is_some_and(|p| p.life <= *threshold)
            }
            InterveningIfPrimitive::ControllerControlsAtLeast { filter, count } => {
                let matching = state
                    .objects
                    .values()
                    .filter(|o| o.zone == ZoneKind::Battlefield)
                    .filter(|o| filter.matches(o, o.controller, controller))
                    .count();
                matching as u32 >= *count
            }
            InterveningIfPrimitive::Escaped => {
                state.object(source).is_some_and(|o| o.entered_via_escape)
            }
            InterveningIfPrimitive::HadNoCountersOfKind(kind) => match lki {
                Some(lki) => lki.counters.count(kind) == 0,
                None => state.object(source).is_some_and(|o| o.counters.count(kind) == 0),
            },
        }
    }
}

/// See `InterveningIfPrimitive`'s own doc comment. Same `Primitive`/`Script`
/// shape `ConditionSpec` uses. `Script` (backlog item 7,
/// `phase5/trigger-completeness-cleanup` slice) is checked via
/// `ScriptRuntime::invoke_intervening_if`, which passes a minimal state
/// snapshot (`script.rs`'s `state_to_lua` — currently just per-player life
/// totals, mirroring the one `Primitive` shape above) into the Lua call as
/// its 3rd argument: `function(source, controller, snapshot)`. No live
/// object-characteristic queries yet (e.g. "this creature's power") - named
/// as the next follow-up, not guessed at here.
/// `Clone`-only (not `Copy`) — see `InterveningIfPrimitive`'s own doc comment.
#[derive(Clone, Debug)]
pub enum InterveningIfSpec {
    Primitive(InterveningIfPrimitive),
    Script(ScriptHandle),
}

/// A card's registered ability, as set up by its script's `register_abilities`-
/// equivalent (`Game.RegisterTriggeredAbility` etc — see `script_api.rs`).
/// Lives in `GameState.abilities` as a flat list (same pattern as
/// `continuous_effects`/`replacement_effects`), not nested under
/// `GameObject`, so the engine can scan all registered abilities against an
/// event/timing window without walking every object first.
#[derive(Clone)]
pub struct RegisteredAbility {
    /// Stable across this ability's lifetime — see `AbilityId`'s own doc
    /// comment. Minted via `GameState::allocate_ability_id` at registration
    /// time; a hand-built test fixture that never goes through real
    /// self-registration picks its own arbitrary (but ideally
    /// file-unique) value instead.
    pub id: AbilityId,
    pub source: ObjectId,
    pub kind: AbilityKind,
    /// `None` means "no filter, always matches" — same meaning as
    /// `Some(ConditionSpec::Primitive(ConditionPrimitive::Always))`. Two
    /// spellings exist on purpose: `None` is what every non-triggered
    /// (`Activated`/`Static`) ability uses, since it never had a condition
    /// concept to begin with; `Always` is a real, explicit choice a Lua
    /// trigger author writes (`return Game.Conditions.Always`) when they
    /// mean "no filter" deliberately — loading rejects a bare Lua `nil`
    /// result as an error rather than silently treating a typo'd
    /// `Game.Conditions.Typo` as unconditional (see
    /// `ScriptRuntime::load_condition`).
    pub condition: Option<ConditionSpec>,
    /// `None` means "no cost at all" — `ActivateAbility` treats this the
    /// same as an empty `CostSpec` (mana: None, additional: empty), still
    /// routing through `AwaitingPayment` for an explicit (even if empty)
    /// acknowledgment, same CR 118.5a stance `CastSpell`'s mana cost already
    /// takes. `Some(spec)` is CR 602.2b's real cost — `spec.additional` is
    /// what `Game.CreateActivatedAbility`'s `:AddCost` accumulates for
    /// tap/sacrifice/discard/life components; `spec.mana` is set the same
    /// way from a `Game.Costs.Mana("{1}{R}")` component, parsed via
    /// `ManaCost::parse` (mana.rs) — no longer always `None`, since the
    /// ability-mana-cost-strings slice closed that gap.
    pub cost: Option<CostSpec>,
    /// `None` means "no targets required" — same "cast as though it did
    /// not require those targets" stance `CardDefinition::target_requirement`
    /// takes. `Some(TargetSpec::Primitive(req))` is what `ActivateAbility`
    /// validates non-mana-ability targets against — `req.filter` can itself
    /// be a `TargetFilter::Script` scripted legality predicate as of
    /// `phase5/scripted-target-filter`, set via `:SetTarget(function)`.
    pub target: Option<TargetSpec>,
    pub operation: ScriptHandle,
    /// See `AbilityEffect`'s doc comment — `Some` for an effect shape this
    /// engine can resolve without a script call at all (CR 605 mana
    /// abilities' `AddMana`, or a `Game.Ops.*` primitive like `GainLife`
    /// loaded via `ScriptRuntime::load_operation`). `None` means resolution
    /// falls back to invoking `operation` for real through the script
    /// bridge.
    pub effect: Option<AbilityEffect>,
    /// CR 603.4's intervening-if clause, orthogonal to `condition` above (the
    /// trigger-event check) — see `InterveningIfSpec`'s own doc comment.
    /// `None` for every ability without "if" text, which is most of them —
    /// same "no concept for this ability" stance `cost`/`target` already
    /// take when `None`. Checked in `triggers::collect_triggers` at trigger
    /// time and baked onto the resulting `StackObject` (`PutTriggersApnap`)
    /// for `ResolveStackTop` to recheck at resolution (CR 608.2a).
    pub intervening_if: Option<InterveningIfSpec>,
    /// CR 603.3c/700.2b: a modal triggered ability's own mode list — empty
    /// (the common case) for every non-modal ability, same "empty means
    /// doesn't apply" stance `CardDefinition::modes` already takes for a
    /// non-modal spell. Reuses `SpellMode`/`ModeChoice` wholesale rather than
    /// a parallel type, since the shape (a target requirement + effect per
    /// option, chosen `mode_choice` of them) is identical to a spell's own —
    /// only *when* the choice is made differs (CR 603.3c: as the ability is
    /// put on the stack, via `ProcessState::AwaitingTriggerModeChoice`/
    /// `Engine::submit_trigger_mode_choice`, since a triggered ability isn't
    /// initiated by a single synchronous player action the way `CastSpell`
    /// is). `SpellMode::additional_cost` is never populated on a
    /// trigger-declared mode and never consulted here — CR 700.2h's
    /// additional-cost text is tied to *casting* a spell (601.2b/f-h), not to
    /// a triggered ability going on the stack, so that field stays
    /// meaningless (not wired) for this ability kind, same "field exists,
    /// this ability kind just never sets/reads it" stance `cost`/`target`
    /// already take for a mana ability.
    pub modes: Vec<SpellMode>,
    /// Meaningless (but must still be set, `RegisteredAbility` has no
    /// `Default` impl) when `modes` is empty — same convention
    /// `CardDefinition::mode_choice` already established.
    ///
    /// CR 700.2e "chosen by opponent" is deliberately not supported for a
    /// triggered ability's own modes this slice — `ModeChoice::
    /// ExactlyChosenByOpponent`/`UpToChosenByOpponent` exist as variants (the
    /// enum is shared with `CardDefinition`), but `push_ordered_triggers`
    /// doesn't special-case them, so they resolve through the same
    /// caster/controller-chosen path as `Exactly`/`UpTo` — meaning a trigger
    /// declaring one of those two variants would incorrectly ask its own
    /// controller instead of the other player. No real card needs this
    /// combination yet, so it's a named gap, not silently wrong in a way
    /// anything currently exercises.
    pub mode_choice: ModeChoice,
}

#[cfg(test)]
mod tests {
    use super::*;

    // DealsDamage/IsDealtDamage/EntersBattlefield/Always never consult
    // controller/active_player - any two PlayerIds work as filler here.
    const CONTROLLER: PlayerId = PlayerId(0);
    const ACTIVE_PLAYER: PlayerId = PlayerId(0);

    #[test]
    fn deals_damage_matches_when_source_is_the_dealer() {
        let source = ObjectId(0);
        let other = ObjectId(1);
        let event = GameEventKind::DamageDealt { source, target: Target::Object(other), amount: 3, is_combat_damage: false };
        assert!(ConditionPrimitive::DealsDamage.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn deals_damage_does_not_match_a_different_dealer() {
        let source = ObjectId(0);
        let dealer = ObjectId(1);
        let event =
            GameEventKind::DamageDealt { source: dealer, target: Target::Object(source), amount: 3, is_combat_damage: false };
        assert!(!ConditionPrimitive::DealsDamage.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn is_dealt_damage_matches_when_source_is_the_target() {
        let source = ObjectId(0);
        let dealer = ObjectId(1);
        let event =
            GameEventKind::DamageDealt { source: dealer, target: Target::Object(source), amount: 3, is_combat_damage: false };
        assert!(ConditionPrimitive::IsDealtDamage.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn is_dealt_damage_does_not_match_a_different_target() {
        let source = ObjectId(0);
        let other = ObjectId(1);
        let event = GameEventKind::DamageDealt { source, target: Target::Object(other), amount: 3, is_combat_damage: false };
        assert!(!ConditionPrimitive::IsDealtDamage.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn is_dealt_damage_does_not_match_a_player_target() {
        // A player being dealt damage isn't "this" for a permanent's own
        // ability - IsDealtDamage only ever matches a Target::Object.
        let source = ObjectId(0);
        let dealer = ObjectId(1);
        let event = GameEventKind::DamageDealt { source: dealer, target: Target::Player(PlayerId(0)), amount: 3, is_combat_damage: false };
        assert!(!ConditionPrimitive::IsDealtDamage.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn deals_damage_and_is_dealt_damage_dont_match_unrelated_events() {
        let source = ObjectId(0);
        let event = GameEventKind::LifeChanged { player: PlayerId(0), delta: -1 };
        assert!(!ConditionPrimitive::DealsDamage.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
        assert!(!ConditionPrimitive::IsDealtDamage.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn beginning_of_your_upkeep_matches_when_controller_is_active_player() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let event = GameEventKind::StepStarted { step: Step::Upkeep };
        assert!(ConditionPrimitive::BeginningOfYourUpkeep.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_your_upkeep_does_not_match_someone_elses_upkeep() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted { step: Step::Upkeep };
        assert!(!ConditionPrimitive::BeginningOfYourUpkeep.matches(source, controller, active_player, &event));
    }

    #[test]
    fn beginning_of_your_upkeep_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let event = GameEventKind::StepStarted { step: Step::Draw };
        assert!(!ConditionPrimitive::BeginningOfYourUpkeep.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_each_upkeep_matches_regardless_of_whose_turn_it_is() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted { step: Step::Upkeep };
        assert!(ConditionPrimitive::BeginningOfEachUpkeep.matches(source, controller, active_player, &event));
    }

    #[test]
    fn beginning_of_each_upkeep_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let event = GameEventKind::StepStarted { step: Step::End };
        assert!(!ConditionPrimitive::BeginningOfEachUpkeep.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn beginning_of_your_untap_matches_when_controller_is_active_player() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let event = GameEventKind::StepStarted { step: Step::Untap };
        assert!(ConditionPrimitive::BeginningOfYourUntap.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_your_untap_does_not_match_someone_elses_untap() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted { step: Step::Untap };
        assert!(!ConditionPrimitive::BeginningOfYourUntap.matches(source, controller, active_player, &event));
    }

    #[test]
    fn beginning_of_your_untap_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let event = GameEventKind::StepStarted { step: Step::Upkeep };
        assert!(!ConditionPrimitive::BeginningOfYourUntap.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_each_untap_matches_regardless_of_whose_turn_it_is() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted { step: Step::Untap };
        assert!(ConditionPrimitive::BeginningOfEachUntap.matches(source, controller, active_player, &event));
    }

    #[test]
    fn beginning_of_each_untap_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let event = GameEventKind::StepStarted { step: Step::Upkeep };
        assert!(!ConditionPrimitive::BeginningOfEachUntap.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    // Dies/LeavesBattlefield are only ever matched against last_known_info
    // (the object itself is already gone by the time these are checked -
    // see collect_triggers's CR 603.10 fallback) - a minimal GameObject
    // snapshot stands in for that.
    fn dummy_last_known_info(object_id: ObjectId) -> crate::types::object::GameObject {
        use crate::types::card::CardDefinition;
        use crate::types::card::{CardTypeSet, KeywordSet, ModeChoice, PTValue, ScriptReference, SupertypeSet};
        use crate::types::counter::CounterSet;
        use crate::types::ids::ObjectLineage;
        use crate::types::mana::ManaCost;
        use crate::types::object::{GameObject, VisibilityPolicy};

        GameObject {
            object_id,
            lineage: ObjectLineage { previous: None },
            definition: CardDefinition {
                definition_id: 1,
                name: "Test Creature".to_string(),
                mana_cost: ManaCost { symbols: Vec::new() },
                color_indicator: None,
                supertypes: SupertypeSet::empty(),
                card_types: CardTypeSet::CREATURE,
                subtypes: Vec::new(),
                oracle_text: String::new(),
                power: Some(PTValue::Fixed(1)),
                toughness: Some(PTValue::Fixed(1)),
                loyalty: None,
                defense: None,
                script_ref: ScriptReference::ByKey("test_creature".to_string()),
                keywords: KeywordSet::empty(),
                target_requirement: None,
                alternative_costs: Vec::new(),
                buyback_cost: None,
                effect: None,
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
            },
            copiable_override: None,
            owner: CONTROLLER,
            controller: CONTROLLER,
            zone: ZoneKind::Battlefield,
            tapped: false,
            flipped: false,
            face_down: false,
            phased_out: false,
            damage_marked: 0,
            deathtouched: false,
            counters: CounterSet::new(),
            attachments: Vec::new(),
            attached_to: None, attached_to_player: None,
            timestamp: crate::types::ids::SequenceNo(0),
            controlled_since_turn: 0,
            is_commander: false,
            is_token: false,
            visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
            copy_generation: 0,
            entered_via_escape: false,
        }
    }

    #[test]
    fn dies_matches_battlefield_to_graveyard_by_last_known_object_id() {
        let source = ObjectId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999), // the new id minted for the graveyard-side object
            from: ZoneKind::Battlefield,
            to: ZoneKind::Graveyard,
            last_known_info: Some(dummy_last_known_info(source)),
        };
        assert!(ConditionPrimitive::Dies.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn dies_does_not_match_a_different_objects_death() {
        let source = ObjectId(0);
        let other = ObjectId(1);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Battlefield,
            to: ZoneKind::Graveyard,
            last_known_info: Some(dummy_last_known_info(other)),
        };
        assert!(!ConditionPrimitive::Dies.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn dies_does_not_match_a_non_death_departure() {
        // Battlefield -> Exile (e.g. a CR 614.13 "exile instead" replacement)
        // is a LeavesBattlefield, not a Dies.
        let source = ObjectId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Battlefield,
            to: ZoneKind::Exile,
            last_known_info: Some(dummy_last_known_info(source)),
        };
        assert!(!ConditionPrimitive::Dies.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn leaves_battlefield_matches_any_destination() {
        let source = ObjectId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Battlefield,
            to: ZoneKind::Exile,
            last_known_info: Some(dummy_last_known_info(source)),
        };
        assert!(ConditionPrimitive::LeavesBattlefield.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn leaves_battlefield_does_not_match_a_move_that_never_left_the_battlefield() {
        let source = ObjectId(0);
        let event = GameEventKind::ZoneChange {
            object: source,
            from: ZoneKind::Hand,
            to: ZoneKind::Battlefield,
            last_known_info: Some(dummy_last_known_info(source)),
        };
        assert!(!ConditionPrimitive::LeavesBattlefield.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    fn drawn_card_last_known_info(drawer: PlayerId) -> crate::types::object::GameObject {
        let mut lki = dummy_last_known_info(ObjectId(42));
        lki.owner = drawer;
        lki.controller = drawer;
        lki.zone = ZoneKind::Library;
        lki
    }

    #[test]
    fn draws_card_matches_the_controllers_own_draw() {
        let source = ObjectId(0);
        let drawer = PlayerId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Library,
            to: ZoneKind::Hand,
            last_known_info: Some(drawn_card_last_known_info(drawer)),
        };
        assert!(ConditionPrimitive::DrawsCard.matches(source, drawer, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn draws_card_does_not_match_a_different_players_draw() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let drawer = PlayerId(1);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Library,
            to: ZoneKind::Hand,
            last_known_info: Some(drawn_card_last_known_info(drawer)),
        };
        assert!(!ConditionPrimitive::DrawsCard.matches(source, controller, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn draws_card_does_not_match_a_non_draw_zone_change() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Hand,
            to: ZoneKind::Graveyard,
            last_known_info: Some(drawn_card_last_known_info(controller)),
        };
        assert!(!ConditionPrimitive::DrawsCard.matches(source, controller, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn each_draws_card_matches_regardless_of_who_drew() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let drawer = PlayerId(1);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Library,
            to: ZoneKind::Hand,
            last_known_info: Some(drawn_card_last_known_info(drawer)),
        };
        assert!(ConditionPrimitive::EachDrawsCard.matches(source, controller, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn each_draws_card_does_not_match_a_non_draw_zone_change() {
        let source = ObjectId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Hand,
            to: ZoneKind::Graveyard,
            last_known_info: None,
        };
        assert!(!ConditionPrimitive::EachDrawsCard.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    fn discarded_card_last_known_info(discarder: PlayerId) -> crate::types::object::GameObject {
        let mut lki = dummy_last_known_info(ObjectId(42));
        lki.owner = discarder;
        lki.controller = discarder;
        lki.zone = ZoneKind::Hand;
        lki
    }

    #[test]
    fn discards_card_matches_the_controllers_own_discard() {
        let source = ObjectId(0);
        let discarder = PlayerId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Hand,
            to: ZoneKind::Graveyard,
            last_known_info: Some(discarded_card_last_known_info(discarder)),
        };
        assert!(ConditionPrimitive::DiscardsCard.matches(source, discarder, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn discards_card_does_not_match_a_different_players_discard() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let discarder = PlayerId(1);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Hand,
            to: ZoneKind::Graveyard,
            last_known_info: Some(discarded_card_last_known_info(discarder)),
        };
        assert!(!ConditionPrimitive::DiscardsCard.matches(source, controller, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn discards_card_does_not_match_a_non_discard_zone_change() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Library,
            to: ZoneKind::Hand,
            last_known_info: Some(discarded_card_last_known_info(controller)),
        };
        assert!(!ConditionPrimitive::DiscardsCard.matches(source, controller, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn each_discards_card_matches_regardless_of_who_discarded() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let discarder = PlayerId(1);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Hand,
            to: ZoneKind::Graveyard,
            last_known_info: Some(discarded_card_last_known_info(discarder)),
        };
        assert!(ConditionPrimitive::EachDiscardsCard.matches(source, controller, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn each_discards_card_does_not_match_a_non_discard_zone_change() {
        let source = ObjectId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Library,
            to: ZoneKind::Hand,
            last_known_info: None,
        };
        assert!(!ConditionPrimitive::EachDiscardsCard.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn discarded_matches_hand_to_graveyard_by_last_known_object_id() {
        let source = ObjectId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999), // the new id minted for the graveyard-side object
            from: ZoneKind::Hand,
            to: ZoneKind::Graveyard,
            last_known_info: Some(dummy_last_known_info(source)),
        };
        assert!(ConditionPrimitive::Discarded.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn discarded_does_not_match_a_different_cards_discard() {
        let source = ObjectId(0);
        let other = ObjectId(1);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Hand,
            to: ZoneKind::Graveyard,
            last_known_info: Some(dummy_last_known_info(other)),
        };
        assert!(!ConditionPrimitive::Discarded.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn discarded_does_not_match_a_battlefield_death() {
        // Battlefield -> Graveyard is a Dies, not a Discarded - same card
        // identity, different departure zone.
        let source = ObjectId(0);
        let event = GameEventKind::ZoneChange {
            object: ObjectId(999),
            from: ZoneKind::Battlefield,
            to: ZoneKind::Graveyard,
            last_known_info: Some(dummy_last_known_info(source)),
        };
        assert!(!ConditionPrimitive::Discarded.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn beginning_of_your_combat_matches_when_controller_is_active_player() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let event = GameEventKind::StepStarted { step: Step::BeginningOfCombat };
        assert!(ConditionPrimitive::BeginningOfYourCombat.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_your_combat_does_not_match_someone_elses_combat() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted { step: Step::BeginningOfCombat };
        assert!(!ConditionPrimitive::BeginningOfYourCombat.matches(source, controller, active_player, &event));
    }

    #[test]
    fn beginning_of_your_combat_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let event = GameEventKind::StepStarted { step: Step::Upkeep };
        assert!(!ConditionPrimitive::BeginningOfYourCombat.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_each_combat_matches_regardless_of_whose_turn_it_is() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted { step: Step::BeginningOfCombat };
        assert!(ConditionPrimitive::BeginningOfEachCombat.matches(source, controller, active_player, &event));
    }

    #[test]
    fn beginning_of_each_combat_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let event = GameEventKind::StepStarted { step: Step::End };
        assert!(!ConditionPrimitive::BeginningOfEachCombat.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn beginning_of_your_end_step_matches_when_controller_is_active_player() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let event = GameEventKind::StepStarted { step: Step::End };
        assert!(ConditionPrimitive::BeginningOfYourEndStep.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_your_end_step_does_not_match_someone_elses_end_step() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted { step: Step::End };
        assert!(!ConditionPrimitive::BeginningOfYourEndStep.matches(source, controller, active_player, &event));
    }

    #[test]
    fn beginning_of_your_end_step_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let event = GameEventKind::StepStarted { step: Step::BeginningOfCombat };
        assert!(!ConditionPrimitive::BeginningOfYourEndStep.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_each_end_step_matches_regardless_of_whose_turn_it_is() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted { step: Step::End };
        assert!(ConditionPrimitive::BeginningOfEachEndStep.matches(source, controller, active_player, &event));
    }

    #[test]
    fn beginning_of_each_end_step_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let event = GameEventKind::StepStarted { step: Step::Upkeep };
        assert!(!ConditionPrimitive::BeginningOfEachEndStep.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    fn state_with_life(player: PlayerId, life: i32) -> crate::types::game::GameState {
        use crate::types::counter::CounterSet;
        use crate::types::game::{GameState, RngState};
        use crate::types::mana::ManaPool;
        use crate::types::player::PlayerState;
        use crate::types::turn::Phase;
        use std::collections::HashMap;

        GameState {
            players: vec![PlayerState {
                player_id: player,
                life,
                mana_pool: ManaPool::default(),
                counters: CounterSet::new(),
                emblems: Vec::new(),
                commander_damage_taken: Vec::new(),
                eliminated: false,
                drew_from_empty_library: false,
                mulligans_taken: 0,
                hand_kept: false,
                commander_casts: std::collections::HashMap::new(),
            }],
            turn_order: vec![player],
            active_player: player,
            turn_number: 1,
            phase: Phase::Main,
            step: Step::PreCombatMain,
            priority_player: player,
            consecutive_passes: 0,
            lands_played_this_turn: HashMap::new(),
            combat: None,
            last_active_turn_number: HashMap::new(),
            objects: HashMap::new(),
            zones: Vec::new(),
            stack: Vec::new(),
            continuous_effects: Vec::new(),
            replacement_effects: Vec::new(),
            cost_modifiers: Vec::new(),
            hand_size_modifiers: Vec::new(),
            hand_reveal_modifiers: Vec::new(),
            draw_limit_modifiers: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState { seed: 1, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 0,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
            linked_ability_generation: std::collections::HashMap::new(),
            pending_timing_bypass: None,
            pending_free_cast: None,
        }
    }

    #[test]
    fn controller_at_or_below_life_holds_when_life_is_at_the_threshold() {
        let p0 = PlayerId(0);
        let state = state_with_life(p0, 5);
        assert!(InterveningIfPrimitive::ControllerAtOrBelowLife(5).holds(&state, p0, ObjectId(0), None));
    }

    #[test]
    fn controller_at_or_below_life_holds_when_life_is_below_the_threshold() {
        let p0 = PlayerId(0);
        let state = state_with_life(p0, 3);
        assert!(InterveningIfPrimitive::ControllerAtOrBelowLife(5).holds(&state, p0, ObjectId(0), None));
    }

    #[test]
    fn controller_at_or_below_life_does_not_hold_when_life_is_above_the_threshold() {
        let p0 = PlayerId(0);
        let state = state_with_life(p0, 6);
        assert!(!InterveningIfPrimitive::ControllerAtOrBelowLife(5).holds(&state, p0, ObjectId(0), None));
    }

    #[test]
    fn controller_at_or_below_life_does_not_hold_for_an_unknown_player() {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let state = state_with_life(p0, 1);
        assert!(!InterveningIfPrimitive::ControllerAtOrBelowLife(5).holds(&state, p1, ObjectId(0), None));
    }

    /// A minimal battlefield permanent for `ControllerControlsAtLeast`
    /// fixtures - same idea as `dummy_last_known_info` above, parameterized
    /// on controller/card_types instead of hardcoded to `CONTROLLER`/
    /// `CREATURE`.
    fn battlefield_object(id: ObjectId, controller: PlayerId, card_types: CardTypeSet) -> crate::types::object::GameObject {
        use crate::types::card::CardDefinition;
        use crate::types::card::{KeywordSet, ModeChoice, PTValue, ScriptReference, SupertypeSet};
        use crate::types::counter::CounterSet;
        use crate::types::ids::ObjectLineage;
        use crate::types::mana::ManaCost;
        use crate::types::object::{GameObject, VisibilityPolicy};

        GameObject {
            object_id: id,
            lineage: ObjectLineage { previous: None },
            definition: CardDefinition {
                definition_id: 1,
                name: "Test Permanent".to_string(),
                mana_cost: ManaCost { symbols: Vec::new() },
                color_indicator: None,
                supertypes: SupertypeSet::empty(),
                card_types,
                subtypes: Vec::new(),
                oracle_text: String::new(),
                power: Some(PTValue::Fixed(1)),
                toughness: Some(PTValue::Fixed(1)),
                loyalty: None,
                defense: None,
                script_ref: ScriptReference::ByKey("test_permanent".to_string()),
                keywords: KeywordSet::empty(),
                target_requirement: None,
                alternative_costs: Vec::new(),
                buyback_cost: None,
                effect: None,
                modes: Vec::new(),
                mode_choice: ModeChoice::Exactly(1),
            },
            copiable_override: None,
            owner: controller,
            controller,
            zone: ZoneKind::Battlefield,
            tapped: false,
            flipped: false,
            face_down: false,
            phased_out: false,
            damage_marked: 0,
            deathtouched: false,
            counters: CounterSet::new(),
            attachments: Vec::new(),
            attached_to: None, attached_to_player: None,
            timestamp: crate::types::ids::SequenceNo(0),
            controlled_since_turn: 0,
            is_commander: false,
            is_token: false,
            visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
            copy_generation: 0,
            entered_via_escape: false,
        }
    }

    fn artifacts_filter() -> FilterSpec {
        FilterSpec {
            card_types: Some(CardTypeSet::ARTIFACT),
            supertypes: None,
            subtypes: None,
            controller: Some(crate::types::cost::FilterController::You),
            random: false,
            extra: None,
        }
    }

    #[test]
    fn controls_at_least_holds_when_the_count_is_exactly_met() {
        let p0 = PlayerId(0);
        let mut state = state_with_life(p0, 20);
        state.objects.insert(ObjectId(0), battlefield_object(ObjectId(0), p0, CardTypeSet::ARTIFACT));
        state.objects.insert(ObjectId(1), battlefield_object(ObjectId(1), p0, CardTypeSet::ARTIFACT));
        state.objects.insert(ObjectId(2), battlefield_object(ObjectId(2), p0, CardTypeSet::ARTIFACT));
        let primitive = InterveningIfPrimitive::ControllerControlsAtLeast { filter: artifacts_filter(), count: 3 };
        assert!(primitive.holds(&state, p0, ObjectId(0), None));
    }

    #[test]
    fn controls_at_least_does_not_hold_when_below_the_count() {
        let p0 = PlayerId(0);
        let mut state = state_with_life(p0, 20);
        state.objects.insert(ObjectId(0), battlefield_object(ObjectId(0), p0, CardTypeSet::ARTIFACT));
        state.objects.insert(ObjectId(1), battlefield_object(ObjectId(1), p0, CardTypeSet::ARTIFACT));
        let primitive = InterveningIfPrimitive::ControllerControlsAtLeast { filter: artifacts_filter(), count: 3 };
        assert!(!primitive.holds(&state, p0, ObjectId(0), None));
    }

    #[test]
    fn controls_at_least_drops_the_instant_a_matching_permanent_leaves() {
        let p0 = PlayerId(0);
        let mut state = state_with_life(p0, 20);
        state.objects.insert(ObjectId(0), battlefield_object(ObjectId(0), p0, CardTypeSet::ARTIFACT));
        state.objects.insert(ObjectId(1), battlefield_object(ObjectId(1), p0, CardTypeSet::ARTIFACT));
        state.objects.insert(ObjectId(2), battlefield_object(ObjectId(2), p0, CardTypeSet::ARTIFACT));
        let primitive = InterveningIfPrimitive::ControllerControlsAtLeast { filter: artifacts_filter(), count: 3 };
        assert!(primitive.holds(&state, p0, ObjectId(0), None), "3 artifacts meets the threshold");

        // Re-derived fresh, no stale state - removing one below the
        // threshold flips the same query without needing to invalidate a
        // cache anywhere.
        state.objects.remove(&ObjectId(2));
        assert!(!primitive.holds(&state, p0, ObjectId(0), None), "down to 2 artifacts, no longer holds");
    }

    #[test]
    fn controls_at_least_with_a_you_filter_ignores_an_opponents_matching_permanents() {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let mut state = state_with_life(p0, 20);
        state.objects.insert(ObjectId(0), battlefield_object(ObjectId(0), p0, CardTypeSet::ARTIFACT));
        state.objects.insert(ObjectId(1), battlefield_object(ObjectId(1), p1, CardTypeSet::ARTIFACT));
        state.objects.insert(ObjectId(2), battlefield_object(ObjectId(2), p1, CardTypeSet::ARTIFACT));
        let primitive = InterveningIfPrimitive::ControllerControlsAtLeast { filter: artifacts_filter(), count: 3 };
        assert!(
            !primitive.holds(&state, p0, ObjectId(0), None),
            "p0 controls only 1 artifact - the opponent's 2 don't count toward p0's threshold"
        );
    }
}
