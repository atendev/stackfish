use crate::types::card::{CardTypeSet, KeywordSet, SupertypeSet};
use crate::types::cost::{CostSpec, FilterSpec};
use crate::types::counter::CounterKind;
use crate::types::event::GameEventKind;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
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
    /// TODO(CR 707.9): no exception/modification support - always a bare
    /// copy. TODO(CR 707.6): "as ~ enters" choices aren't re-prompted here
    /// - out of scope for this path, since it never re-enters the
    /// battlefield; only relevant once a real ETB-replacement "enters as a
    /// copy" path (Clone itself) exists, a separate follow-up.
    BecomeCopy,
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
}

/// Where `AbilityEffect::Search`'s found cards go — narrowed to the two
/// overwhelmingly common real destinations. `Battlefield { tapped }` covers
/// fetch lands (most enter tapped, some don't); no Graveyard/Exile
/// destination this slice (rare in practice, deferred).
#[derive(Clone, Copy, Debug)]
pub enum SearchDestination {
    Hand,
    Battlefield { tapped: bool },
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
    /// TODO(CR 707.9): no exception/modification support - always a bare
    /// copy of the target's current values.
    CopyOf(ObjectId),
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
/// (keyed by `(source, ability_index)`, reset each turn). Not offered for a
/// `Script` condition this slice - a custom Lua condition function has no
/// engine-side ability_index-keyed slot to consult without also threading
/// one through `ScriptContext`, deferred as a named follow-up.
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
}

impl InterveningIfPrimitive {
    pub fn holds(&self, state: &crate::types::game::GameState, controller: PlayerId) -> bool {
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

/// A card's registered ability, as set up by its script's `initial_effect`-
/// equivalent (`Game.RegisterTriggeredAbility` etc — see `script_api.rs`).
/// Lives in `GameState.abilities` as a flat list (same pattern as
/// `continuous_effects`/`replacement_effects`), not nested under
/// `GameObject`, so the engine can scan all registered abilities against an
/// event/timing window without walking every object first.
#[derive(Clone)]
pub struct RegisteredAbility {
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
    /// takes. `Some(TargetSpec::Primitive(_))` is what `ActivateAbility`
    /// validates non-mana-ability targets against this slice;
    /// `Some(TargetSpec::Script(_))` stays dead code (see `TargetSpec`'s own
    /// doc comment).
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
        use crate::types::card::{CardTypeSet, KeywordSet, PTValue, ScriptReference, SupertypeSet};
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
                effect: None,
                modes: Vec::new(),
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
            attached_to: None,
            timestamp: crate::types::ids::SequenceNo(0),
            controlled_since_turn: 0,
            is_commander: false,
            is_token: false,
            visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
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
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState { seed: 1, draws_consumed: 0 },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 0,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
        }
    }

    #[test]
    fn controller_at_or_below_life_holds_when_life_is_at_the_threshold() {
        let p0 = PlayerId(0);
        let state = state_with_life(p0, 5);
        assert!(InterveningIfPrimitive::ControllerAtOrBelowLife(5).holds(&state, p0));
    }

    #[test]
    fn controller_at_or_below_life_holds_when_life_is_below_the_threshold() {
        let p0 = PlayerId(0);
        let state = state_with_life(p0, 3);
        assert!(InterveningIfPrimitive::ControllerAtOrBelowLife(5).holds(&state, p0));
    }

    #[test]
    fn controller_at_or_below_life_does_not_hold_when_life_is_above_the_threshold() {
        let p0 = PlayerId(0);
        let state = state_with_life(p0, 6);
        assert!(!InterveningIfPrimitive::ControllerAtOrBelowLife(5).holds(&state, p0));
    }

    #[test]
    fn controller_at_or_below_life_does_not_hold_for_an_unknown_player() {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let state = state_with_life(p0, 1);
        assert!(!InterveningIfPrimitive::ControllerAtOrBelowLife(5).holds(&state, p1));
    }

    /// A minimal battlefield permanent for `ControllerControlsAtLeast`
    /// fixtures - same idea as `dummy_last_known_info` above, parameterized
    /// on controller/card_types instead of hardcoded to `CONTROLLER`/
    /// `CREATURE`.
    fn battlefield_object(id: ObjectId, controller: PlayerId, card_types: CardTypeSet) -> crate::types::object::GameObject {
        use crate::types::card::CardDefinition;
        use crate::types::card::{KeywordSet, PTValue, ScriptReference, SupertypeSet};
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
                effect: None,
                modes: Vec::new(),
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
            attached_to: None,
            timestamp: crate::types::ids::SequenceNo(0),
            controlled_since_turn: 0,
            is_commander: false,
            is_token: false,
            visibility: VisibilityPolicy { public: true, visible_to: Vec::new() },
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
        assert!(primitive.holds(&state, p0));
    }

    #[test]
    fn controls_at_least_does_not_hold_when_below_the_count() {
        let p0 = PlayerId(0);
        let mut state = state_with_life(p0, 20);
        state.objects.insert(ObjectId(0), battlefield_object(ObjectId(0), p0, CardTypeSet::ARTIFACT));
        state.objects.insert(ObjectId(1), battlefield_object(ObjectId(1), p0, CardTypeSet::ARTIFACT));
        let primitive = InterveningIfPrimitive::ControllerControlsAtLeast { filter: artifacts_filter(), count: 3 };
        assert!(!primitive.holds(&state, p0));
    }

    #[test]
    fn controls_at_least_drops_the_instant_a_matching_permanent_leaves() {
        let p0 = PlayerId(0);
        let mut state = state_with_life(p0, 20);
        state.objects.insert(ObjectId(0), battlefield_object(ObjectId(0), p0, CardTypeSet::ARTIFACT));
        state.objects.insert(ObjectId(1), battlefield_object(ObjectId(1), p0, CardTypeSet::ARTIFACT));
        state.objects.insert(ObjectId(2), battlefield_object(ObjectId(2), p0, CardTypeSet::ARTIFACT));
        let primitive = InterveningIfPrimitive::ControllerControlsAtLeast { filter: artifacts_filter(), count: 3 };
        assert!(primitive.holds(&state, p0), "3 artifacts meets the threshold");

        // Re-derived fresh, no stale state - removing one below the
        // threshold flips the same query without needing to invalidate a
        // cache anywhere.
        state.objects.remove(&ObjectId(2));
        assert!(!primitive.holds(&state, p0), "down to 2 artifacts, no longer holds");
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
            !primitive.holds(&state, p0),
            "p0 controls only 1 artifact - the opponent's 2 don't count toward p0's threshold"
        );
    }
}
