use crate::types::card::{CardTypeSet, KeywordSet, ModeChoice, SpellMode, SupertypeSet};
use crate::types::cost::{CostSpec, FilterSpec};
use crate::types::counter::CounterKind;
use crate::types::effect::EffectFilter;
use crate::types::event::GameEventKind;
use crate::types::ids::{AbilityId, ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::mana::{ColorSet, ManaUnit};
use crate::types::target::TargetSpec;
use crate::types::turn::Step;
use crate::types::zone::ZoneKind;

/// Per the ygo-pro effect lifecycle (`docs/ygo_pro_reference.md`),
/// generalized for MTG: condition, cost, target and operation are separate
/// script callbacks, so the engine can check a condition without committing
/// to the rest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AbilityKind {
    Triggered,
    Activated,
    Static,
}

/// CR 118.12, 118.12a: "[do something] unless [a player does something
/// else]", shared by every pay-or-else keyword. `AbilityEffect::UnlessPaid`'s
/// `cost` is an ordinary `CostSpec`, validated and committed through the same
/// `PaymentPlan` and `commit_payment` machinery as `submit_payment`.
/// `otherwise` is a small closed set of consequences that fires once every
/// candidate payer declines in turn (`AwaitingUnlessPaidChoice`'s
/// `payer_queue`, in CR 101.4 APNAP order). Ward and Echo each have one fixed
/// payer, and the queue lets a future "any player may pay" keyword reuse it.
/// Widen the set when a real card needs a new shape.
#[derive(Clone, Debug, PartialEq)]
pub enum UnlessPaidConsequence {
    /// CR 701.6a, 118.12 (Mana Leak, Condescend) and CR 702.21a (Ward):
    /// counters the ability's own target with `counter_stack_object`, as
    /// `AbilityEffect::Counter` does.
    Counter,
    /// CR 702.30a (Echo): sacrifices the ability's source with
    /// `resolve_sacrifice_effect`, with no target of its own.
    SacrificeSelf,
}

/// CR 110.2a: which player controls a permanent an effect puts onto the
/// battlefield. `You` is the default ("under your control", the controller of
/// the resolving spell or ability); `Owner` is "under its owner's control".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryControl {
    You,
    Owner,
}

/// Structural stand-in for an ability's effect that the engine resolves
/// without a script callback, starting with CR 605 mana abilities. `None` on
/// `RegisteredAbility::effect` means only the script bridge can carry the
/// effect out.
#[derive(Clone, Debug, PartialEq)]
pub enum AbilityEffect {
    /// CR 605: adds exactly these literal units to the activator's mana pool,
    /// the vocabulary `CostPayment::Mana` uses for spending.
    AddMana(Vec<ManaUnit>),
    /// CR 605: "add one mana of any color" and its "add N mana, each any color
    /// among {...}" form. The colors are chosen at activation (CR 602.2b) and
    /// carried in `ActivateAbility::chosen_colors`. The `ActivateAbility` arm
    /// checks `chosen_colors.len() == count` and that every entry is in
    /// `choices`, then adds `ManaUnit::Colored` units.
    // TODO: no `SnowColored` choice variant
    AddManaChoice { count: u32, choices: ColorSet },
    /// `Game.Ops.GainLife(n)`: the controller gains `n` life. A negative `n`
    /// loses life (CR 119.3), but `LoseLife` is the intended spelling, since
    /// it can target a player and pushes a `LifeChanged` event.
    /// TODO: `GainLife` pushes no `LifeChanged` event, so CR 119.9's "gains life" trigger and CR 119.10's replacement have nothing to listen to
    GainLife(i32),
    /// `Game.Ops.Damage(n)`: deals `n` damage to the resolving object's first
    /// chosen target (CR 608.2m). It emits a real `DamageDealt` event through
    /// `ReplacementCheck` and `CommitEvents`, as combat damage does, so
    /// prevention and replacement (CR 614-616) and Lifelink, Wither, Infect
    /// and Deathtouch (`commands::apply_damage_event`) apply. A single fixed
    /// target only, with no "divide among" or "each opponent" shapes.
    Damage(u32),
    /// `Game.Ops.Destroy()`: destroys the first chosen target (CR 701.8). It
    /// can't push an unapplied event as `Damage` does, because every
    /// `ZoneChange` describes a move already made. `ResolveStackTop` instead
    /// runs the CR 614.13 `WouldDie` replacement check and `sba::commit_death`
    /// inline, as `Engine::submit_payment` does for a Sacrifice cost. An
    /// indestructible target is untouched (CR 702.12b), checked before
    /// proposing the event. A toggle with no fields.
    Destroy,
    /// CR 509.1h: "target attacking creature becomes blocked", by no specific
    /// creature (no combat damage unless trample, CR 510.1c). Mutates
    /// `CombatState.effect_blocked` through `commands::mark_effect_blocked`
    /// with no event, like `Pump` and `GainControl`. CR 702.22i propagates to
    /// the band inside that function. A toggle with no fields.
    // TODO: `target_requirement` can express "target creature" but not "attacking" (no combat-state `TargetFilter`, and the Lua snapshot has no combat data), so a non-attacking target is a silent no-op and not a CR 608.2b fizzle
    BecomeBlocked,
    /// `Game.Ops.DestroyAll(filter)` (CR 701.8, 601.2c: "all" effects don't
    /// target): destroys every matching battlefield permanent (Armageddon).
    /// Takes the `EffectFilter` of `:SetAffected` and uses
    /// `matches_effect_filter`. `ResolveStackTop` hands the list to
    /// `continue_scripted_zone_move(ScriptedZoneMoveKind::Destroy, ..)`, so
    /// indestructibility (CR 702.12b), ordering pauses (CR 616.1) and the
    /// `WouldDie` check come for free.
    DestroyAll(EffectFilter),
    /// `Game.Ops.Counters(kind, n)`: puts `amount` counters on the first
    /// chosen target (CR 122.1a). Like `Damage`, `ResolveStackTop` pushes
    /// `CounterAdded` unapplied and `CommitEvents` applies it. Object targets
    /// only, because `CounterAdded` carries an `ObjectId`, so Energy and
    /// Experience are out.
    // TODO: no `EventFilter::CounterAdded`, so counter placement can't be replaced or prevented
    Counters { kind: CounterKind, amount: i32 },
    /// `Game.Ops.Pump(power, toughness, duration)`: "target creature gets
    /// +X/+Y until <duration>". Each legal target gets its own layer 7c
    /// `ModifyPT` `ContinuousEffect` pushed straight into
    /// `state.continuous_effects`, with no `CommitEvents`, since no event kind
    /// says "a continuous effect was added". `ContinuousEffect.source` is the
    /// target's `ObjectId` with `self_only: true`, so the effect can't outlive
    /// its target: a zone change mints a new id, and
    /// `layers::applicable_effects_in_layer` ignores an unresolvable source.
    /// `duration` drains through `EffectDuration` (`UntilEndOfTurn`,
    /// `UntilYourNextTurn`, `UntilEndOfCombat`, `Permanent`). A non-object
    /// target is skipped silently, as in every object-targeted primitive here.
    Pump {
        power: i32,
        toughness: i32,
        duration: crate::types::effect::EffectDuration,
    },
    /// `Game.Ops.GainControl(duration)` (CR 108.4, 613.1 layer 2): "gain
    /// control of target creature until end of turn". Builds a per-target
    /// `ContinuousEffect` like `Pump`, but with `Layer::ControlChangingEffects`
    /// and `EffectPrimitive::GainControl`. The new controller is always
    /// `StackObject::controller` at resolution. A non-object target is skipped
    /// silently. This covers only the resolving-spell path.
    // TODO: `GameObject.controlled_since_turn` is not updated here, though CR 302.6 restarts summoning sickness for the new controller; stamping it would leave a stale value once the effect expires
    GainControl(crate::types::effect::EffectDuration),
    /// `Game.Ops.Regenerate()` (CR 701.19a): each legal target gets its own
    /// one-shot shield (`EventFilter::WouldDie` with
    /// `ReplacementPayload::Regenerate`, pinned by `self_only: true` as `Pump`
    /// does) pushed straight into `state.replacement_effects`, with no event,
    /// since static-sourced shields (CR 701.19b) aren't logged either.
    /// CR 701.19c: a target with `CANT_BE_REGENERATED` gets no shield, checked
    /// when the shield would be created. The ability still resolves. A
    /// non-object target is skipped silently.
    Regenerate,
    /// `Game.Ops.DrawCards(n)`: the controller draws `n` cards (CR 120.3,
    /// 121), with no target. Each draw goes through `commands::propose_draw`,
    /// `apply_replacements` (CR 614, 616) and `commands::commit_zone_move`;
    /// see `Engine::continue_draw_cards`. Drawing from an empty library
    /// silently stops.
    // TODO: "target player draws", and replacements that change which player draws (Notion Thief), since `ReplaceZoneDestination` rewrites only `to`
    DrawCards(u32),
    /// `Game.Ops.Mill(n)` (CR 701.17a): the affected player mills the top `n`
    /// cards. The player is the controller when the ability declared no target
    /// spec (`StackObject::target_requirement`), otherwise the first legal
    /// `Target::Player` at resolution. If that target is gone or illegal, or
    /// the spec chose none ("up to one target player"), nothing happens, with
    /// no controller fallback.
    ///
    /// CR 701.17b: an oversized mill moves as many cards as possible, and an
    /// empty library mills nothing. Milling is not drawing, so it never sets
    /// `drew_from_empty_library` or causes the CR 704.5b loss. The batch is
    /// the top `min(n, library size)` cards, read once up front, and moves
    /// through `ScriptedZoneMoveKind::Mill`. Each card is its own
    /// `ZoneChange { Library -> Graveyard }` run through `apply_replacements`
    /// (so a Leyline of the Void redirect applies and a CR 616.1 ordering
    /// choice can pause mid-mill) and committed with
    /// `commands::commit_zone_move`, which mints a new `ObjectId` (CR 400.7).
    // TODO: no `Game.Conditions.*` entry for "whenever a card is milled"; the `ZoneChange` event exists and nothing listens for Library to Graveyard
    // TODO: no way to act on "the milled card" (CR 701.17c, 701.17d), mill as a cost (CR 701.17b), dredge (CR 702.52) or rad counters (CR 728)
    // TODO: a replacement that changes how many cards a mill moves, since the batch is fixed before the first card moves
    Mill(u32),
    /// `Game.Ops.LoseLife(n)` (CR 119.3): the affected player loses `n` life,
    /// chosen as in `Mill`. `Player::lose_life` subtracts without clamping,
    /// leaving 0 or less to the CR 704.5a SBA. `n == 0` changes nothing and
    /// pushes no event. It pushes `LifeChanged { player, delta: -n }`, already
    /// applied to state like `Tapped`, and is the first resolution-time
    /// producer of that event (cost payment is the other).
    // TODO: nothing consumes `LifeChanged`: no "whenever you lose life" condition and no "would lose life" replacement event (CR 614)
    // TODO: no "can't lose life" restriction (CR 119.8)
    // TODO: damage to a player (CR 120.3a), `GainLife` and lifelink do not push `LifeChanged`
    // TODO: no "lose life equal to X" amount and no Two-Headed Giant team life total (CR 119.4a)
    LoseLife(u32),
    /// `Game.Ops.Sacrifice()` (CR 701.21a): sacrifices the first chosen
    /// target. Indestructibility doesn't stop it (CR 702.12b is
    /// Destroy-specific). Same as `Destroy` minus that check: it proposes
    /// Battlefield to graveyard, runs `apply_replacements` (so CR 614.13's
    /// "exile it instead" applies) and calls `sba::commit_death`, which works
    /// here because the destination is the owner's per-player graveyard.
    /// `Dies` and `LeavesBattlefield` triggers fire as for an SBA death.
    // TODO: "sacrifice a permanent you control" (player chooses at resolution, no targeting) needs Lua coroutines; the untargeted self form is `SacrificeSelf`
    Sacrifice,
    /// `Game.Ops.SacrificeSelf` (CR 701.21a): "sacrifice this permanent", the
    /// effect form of `AdditionalCost::SacrificeSelf`. The source goes through
    /// `ScriptedZoneMoveKind::Sacrifice`, the same replacement-checked move
    /// `Sacrifice` uses, so indestructible doesn't stop it, regeneration
    /// can't intercept it and Dies and LeavesBattlefield triggers fire.
    ///
    /// CR 701.21a: a player can't sacrifice a permanent they don't control.
    /// Nothing targeted the source, so the resolver checks it: if it is on the
    /// battlefield under a different effective controller, nothing happens. A
    /// source that already left is a new object (CR 400.7), so its old id
    /// doesn't resolve and nothing happens.
    // TODO: `UnlessPaidConsequence::SacrificeSelf` (Echo) goes through `resolve_sacrifice_effect` and makes no controller check
    // TODO: "sacrifice a permanent you control" with a player choice needs a choice pause (see `Sacrifice`)
    SacrificeSelf,
    /// `Game.Ops.Exile()` (CR 701.18): exiles the first chosen battlefield
    /// target. Exile ignores indestructibility (CR 702.12b is Destroy-only).
    /// `ResolveStackTop` runs `apply_replacements` and inlines the move,
    /// because `sba::commit_death` hardcodes `Some(owner)` for the destination
    /// zone and the shared Exile zone has `owner = None`. Battlefield targets
    /// only. A toggle with no fields.
    Exile,
    /// `Game.Ops.ReturnToHand()` (CR 400.3): "return target permanent to its
    /// owner's hand". The chosen targets move through
    /// `continue_scripted_zone_move` (`ScriptedZoneMoveKind::Bounce`), so the
    /// move is replacement-checked (CR 614) and fires leaves-the-battlefield
    /// triggers. The card goes to its owner's hand, and a returned token
    /// ceases to exist (CR 111.7, 704.5d). The target may be on the
    /// battlefield, in a graveyard or in exile
    /// (`ScriptedZoneMoveKind::accepts_source_zone`), read from the object at
    /// resolution, and the target filter's `zone` lets a script aim at a
    /// graveyard card. Untargeted forms are `ReturnSelfToHand` and
    /// `ReturnAllToHand`.
    ReturnToHand,
    /// `Game.Ops.ReturnSelfToHand`: "return this card from your graveyard to
    /// your hand" (CR 400.3). The source moves through the same `Bounce`
    /// pipeline as `ReturnToHand`. If it left the zone it was in, it is a new
    /// object (CR 400.7) and the effect does nothing, which is not a fizzle
    /// since nothing was targeted.
    ReturnSelfToHand,
    /// `Game.Ops.ReturnAllToHand(filter, zone)` (CR 400.3, 601.2c: "all"
    /// effects don't target). Collects every object in `zone` (battlefield,
    /// graveyard across all players, or exile) matching `filter` and moves the
    /// list through `ScriptedZoneMoveKind::Bounce`, so the moves are
    /// simultaneous, replacement-checked and fire departed triggers.
    /// `filter.controller` is read off the object's controller, which equals
    /// its owner outside the battlefield.
    ReturnAllToHand {
        filter: EffectFilter,
        zone: ZoneKind,
    },
    /// `Game.Ops.ExileSelf`: "exile this card from your graveyard" (CR 406.2).
    /// The source goes through `ScriptedZoneMoveKind::Exile`. A source that
    /// changed zones is a new object (CR 400.7), so the effect does nothing.
    /// The targeted "until it leaves the battlefield" form is
    /// `ExileUntilSourceLeaves`.
    // TODO: face-down exile (CR 406.3)
    ExileSelf,
    /// `Game.Ops.ExileAll(filter, zone)` (CR 406.2, 601.2c): collects every
    /// object in `zone` (battlefield or graveyard, across all players)
    /// matching `filter` and moves the list through
    /// `ScriptedZoneMoveKind::Exile`, so the moves are simultaneous,
    /// replacement-checked and fire departed triggers.
    // TODO: Hand and Library sources (`accepts_source_zone` rejects them)
    ExileAll {
        filter: EffectFilter,
        zone: ZoneKind,
    },
    /// `Game.Ops.ExileUntilSourceLeaves` (CR 610.3): "exile target creature
    /// until ~ leaves the battlefield". Targeted and battlefield-only like
    /// `Exile`, but each exiled card is linked to the ability's source in
    /// `GameState::linked_exiles`. Once the source is off the battlefield the
    /// card returns under its owner's control (CR 610.3c) through
    /// `ScriptedZoneMoveKind::Reanimate`, when `finish_sba_loop` settles. If
    /// the source left before this resolves, nothing is exiled (CR 610.3a,
    /// 610.3b).
    // TODO: the cleanup-step SBA check doesn't return linked exiles
    // TODO: no "phases out until" form (CR 610.4)
    // TODO: no untargeted form, graveyard source, other durations (until end of turn, until your next turn) or Oblivion Ring style separate-trigger return (CR 607)
    ExileUntilSourceLeaves,
    /// `Game.Ops.ReturnToBattlefield(opts?)`: "return target creature card
    /// from a graveyard to the battlefield" (Reanimate, Zombify, Unburial
    /// Rites). Every legal object target moves through
    /// `ScriptedZoneMoveKind::Reanimate`, so each entry is a replacement-checked
    /// `ZoneChange` (CR 614, 616) and then gets the entry treatment of a
    /// resolving permanent spell: a fresh CR 400.7 object, summoning sick, its
    /// card script run, and `apply_enters_battlefield_modifiers` (enters
    /// tapped, with counters, Bloodthirst, Escape counters). ETB triggers fire
    /// off the ledger `ZoneChange`.
    ///
    /// `control` is CR 110.2a: the effect's controller by default
    /// ([`EntryControl::You`]) or the owner ([`EntryControl::Owner`]).
    /// `tapped` enters it tapped. The source zone (graveyard or exile) is read
    /// from the object at resolution.
    ///
    /// An instant or sorcery card stays put (CR 400.4a). An Aura with exactly
    /// one legal host enters attached to it (CR 303.4f), and with none it
    /// stays put (CR 303.4g). Tokens never get here (CR 111.7). The untargeted
    /// sibling is `ReturnAllToBattlefield`. Persist and Undying use
    /// `ReturnSelfWithCounter`, which skips replacement checks.
    // TODO: an Aura with two or more legal hosts needs the controller to choose one; there is no non-stack entry-attach pause, so it stays in the graveyard (CR 303.4f)
    // TODO: Clone-style "enters as a copy", Devour and Amplify need a choice pause not tied to a stack object, so a reanimated card with one enters as itself (CR 707.6, 702.82a, 702.38a, 614.12a)
    // TODO: no "enters with an additional counter" option (CR 122.6)
    // TODO: no "enters attacking" or "enters blocking" option (CR 508.4)
    // TODO: no "under target opponent's control" and no "may" return
    ReturnToBattlefield { control: EntryControl, tapped: bool },
    /// `Game.Ops.ReturnAllToBattlefield(filter, zone?, opts?)`: "return all
    /// creature cards from all graveyards to the battlefield" (Rise of the Dark
    /// Realms, Living Death; CR 601.2c). Collects every matching object in
    /// `zone` (graveyard across all players, or exile) and moves the list
    /// through `ScriptedZoneMoveKind::Reanimate`, each entry behaving as in
    /// `ReturnToBattlefield`. `filter.controller` is read off the object's
    /// controller, its owner outside the battlefield.
    // TODO: entries are processed one after another and not as one simultaneous event, so an earlier arrival's replacement can affect a later one (CR 603.6a, 614.12)
    ReturnAllToBattlefield {
        filter: EffectFilter,
        zone: ZoneKind,
        control: EntryControl,
        tapped: bool,
    },
    /// `Game.Ops.Tap` (CR 701.26a): taps every legal object target. Only an
    /// untapped battlefield permanent changes (CR 110.5d), and a no-op pushes
    /// no `Tapped` event, so a "becomes tapped" trigger can't fire on it
    /// (CR 603.2e). The flag is set directly and the event pushed as already
    /// happened, as the tap-cost, attack and untap-step producers do, and not
    /// applied in `CommitEvents` like `Counters`. Tapping never touches
    /// `CombatState` (CR 506.4b). Untargeted siblings are `TapSelf` and
    /// `TapAll`.
    // TODO: no "would become tapped" replacement event, so the flip is not replacement-checked (CR 614)
    // TODO: no Lua `becomes tapped` condition, since `ConditionPrimitive` has none (CR 603.2e)
    Tap,
    /// `Game.Ops.Untap` (CR 701.26b): the mirror of `Tap`. Only a tapped
    /// battlefield permanent changes, and `Untapped` is pushed only on a real
    /// flip.
    // TODO: no stun counters and no "would become untapped" replacement event, so an untap always succeeds (CR 122.1d, 614)
    // TODO: no Lua `becomes untapped` condition (CR 603.2e)
    Untap,
    /// `Game.Ops.TapSelf` (CR 701.26a): taps the ability's own source. A
    /// source that left the battlefield is a new object (CR 400.7), so nothing
    /// happens. Same flip rules and TODOs as `Tap`.
    TapSelf,
    /// The `Game.Ops.UntapSelf` primitive - "untap this permanent" (CR
    /// 701.26b). Mirror of `TapSelf`; same TODOs as `Untap`.
    UntapSelf,
    /// `Game.Ops.TapAll(filter)` (CR 701.26a, 601.2c): "tap all creatures your
    /// opponents control". It doesn't target, so hexproof and protection don't
    /// stop it. Taps the untapped battlefield objects matching `filter`, with
    /// no zone argument since tapped status exists only there (CR 110.5d).
    /// `filter.controller` is read off the raw `object.controller`, like every
    /// `matches_effect_filter` caller. Same TODOs as `Tap`.
    TapAll { filter: EffectFilter },
    /// The `Game.Ops.UntapAll(filter)` primitive - "untap all creatures you
    /// control" (CR 701.26b). Mirror of `TapAll`; same TODOs as `Untap`.
    UntapAll { filter: EffectFilter },
    /// `Game.Ops.Counter()` (CR 701.6a): "counter target spell". The target is
    /// a `Target::Object` naming another `StackObject`'s `stack_id`. A spell on
    /// the stack has a `GameObject` at `zone: Stack` with the same id
    /// (`commands::cast::execute_cast_spell`), so
    /// `TargetFilter::Object { zone: Some(ZoneKind::Stack), .. }` already
    /// targets it, narrowed by card types and controller (Negate's "target
    /// noncreature spell"). Convolute's "mana value X (or less, or greater)"
    /// uses `TargetFilter::Object::mana_value`, and Stifle or Trickbind's
    /// "counter target activated or triggered ability" uses `Target::Ability`
    /// and `TargetFilter::Ability`. `resolve_counter_effect` accepts either.
    ///
    /// Unlike the other single-target effects, the affected object is a
    /// different, still-present entry in `state.stack`, not a battlefield
    /// object reachable through `continue_scripted_zone_move`.
    /// `resolve_counter_effect` finds it by `stack_id` and removes it. Only a
    /// `Spell` then moves, through `Engine::move_resolving_spell` (CR 701.6a:
    /// owner's graveyard, or exile if that spell's own `exile_on_leaving_stack`
    /// is set, as with a flashbacked spell, CR 702.34a). An ability just stops
    /// existing. There is no `apply_replacements` call, as in the CR 608.2b
    /// fizzle branch, and no CR 616.1 ordering pause, since one stack entry
    /// leaves.
    ///
    /// CR 608.2b already covers "the target left the stack": the zone check in
    /// `target_is_legal` fails once the id no longer resolves to a stack
    /// object, so `legal_targets` is empty and the Counterspell fizzles before
    /// reaching this arm. "Counter unless that player pays {X}" (Mana Leak,
    /// Condescend) is `UnlessPaid` with `UnlessPaidConsequence::Counter`.
    Counter,
    /// CR 118.12, 118.12a: the "[do something] unless [a player does
    /// something else]" template. See `UnlessPaidConsequence`. The target
    /// declaration matches plain `Counter` when `otherwise` is `Counter`
    /// (`Target::Object` or `Target::Ability`), and `SacrificeSelf` declares
    /// none.
    ///
    /// Resolution pauses (`resolve_unless_paid_effect`,
    /// `AwaitingUnlessPaidChoice`) because CR 118.12's "if [a player] does" is
    /// a decision by a player who may not control this ability (Ward's
    /// targeting player). `Engine::submit_unless_paid_payment` and
    /// `decline_unless_paid` (`choice_answers.rs`) answer it. Paying commits
    /// the real cost at once through `PaymentPlan` and `commit_payment`, with
    /// no cast-style unwind, since declining unwinds nothing (CR 601.6 doesn't
    /// apply). Declining advances `payer_queue`, and an empty queue dispatches
    /// `otherwise`: `Counter` reuses `counter_stack_object`, and
    /// `SacrificeSelf` reuses `resolve_sacrifice_effect` against the ability's
    /// source through `ScriptedZoneMoveThen::StackResolution`.
    // TODO: Condescend's {X} is an amount announced when the countering spell was cast; `ManaCost` can't parse `{X}` from a Lua string and this variant can't read `top.x_value` at resolution, so only Mana Leak's fixed number works
    // TODO: `payer_queue` has one element for both producers (Ward's targeted player, Echo's controller); an "any player may pay" or "each opponent may pay" keyword isn't wired from Lua or TOML
    UnlessPaid {
        cost: CostSpec,
        otherwise: UnlessPaidConsequence,
    },
    /// `Game.Ops.CreateToken()`: creates `count` token permanents under the
    /// controller's control (CR 111). Named tokens (Clue, Food, Treasure) are
    /// complete, and creature tokens support arbitrary subtypes, colors, P/T,
    /// keywords, extra card types (Artifact, Enchantment) and supertypes
    /// (Legendary). Tokens that leave the battlefield cease to exist
    /// (CR 111.7, `sba::collect_ceasing_tokens`).
    CreateToken { spec: TokenSpec, count: u32 },
    /// `Game.Ops.Shuffle()`: the controller shuffles their own library
    /// (CR 701.19b). It calls `Engine::shuffle_library`, which mutates state
    /// and logs `LibraryShuffled` and `RngDrawn` itself, so `CommitEvents`
    /// needs no special case. A toggle with no fields.
    // TODO: `Target::Player` "target player shuffles their library"
    Shuffle,
    /// `Game.Ops.Scry(n)` (CR 701.22a): the controller looks at the top `n`
    /// cards of their library, then puts any number on the bottom and the rest
    /// on top, in any order. It can't resolve in one step: `ResolveStackTop`
    /// peeks the top `n` ids and pauses on `AwaitingScryChoice`, and
    /// `Engine::submit_scry_choice` does the reorder. `n == 0` or an empty
    /// library is a no-op with no pause and no event (CR 701.22b).
    // TODO: simultaneous multi-player scry in APNAP order (CR 701.22c)
    Scry(u32),
    /// `Game.Ops.Surveil(n)` (CR 701.25a): `Scry`, but the pile not kept on
    /// top goes to the graveyard. That pile is a real zone move with a new
    /// `ObjectId` per card (CR 400.7), as in `submit_discard`. `n == 0` is a
    /// no-op (CR 701.25c).
    // TODO: "look at additional cards while you surveil" (CR 701.25b); it is always exactly `n` cards
    Surveil(u32),
    /// `Game.Ops.Search(filter, count, destination, shuffle_after)`
    /// (CR 701.23a): the controller looks through their whole library, not a
    /// peeked window, for up to `count` cards matching `filter`, puts them in
    /// `destination` and shuffles if `shuffle_after`. `GameState` exposes full
    /// contents, and the only hidden-information gap is the Lua callback
    /// boundary, which this primitive never crosses.
    ///
    /// CR 701.23b: a quality search is never mandatory, so the controller may
    /// find fewer than `count`, including zero. `ResolveStackTop` pauses on
    /// `AwaitingSearchChoice` with `candidates` pre-filtered to matches (the
    /// controller as `viewer`), and `Engine::submit_search_choice` only checks
    /// that the answer is a subset of `candidates` of size at most `count`.
    // TODO: a search for an undefined quality or a bare quantity, a mandatory find (CR 701.23d); this variant is quality-search only
    // TODO: reveal the found card (CR 701.23e); there is no reveal concept yet
    // TODO: an effect that redirects "search" to "the top N instead" (CR 701.23f); it needs its own `EventFilter::Search`
    // TODO: simultaneous multiplayer search in APNAP order (CR 701.23i)
    Search {
        filter: FilterSpec,
        count: u32,
        destination: SearchDestination,
        shuffle_after: bool,
    },
    /// `Game.Ops.BecomeCopy()` (CR 707.2, 707.3, layer 1): the source becomes
    /// a copy of its first chosen target's current copiable values (Vesuvan
    /// Doppelganger). A toggle with no fields, and the target lives on the
    /// `StackObject`.
    ///
    /// It moves nothing between zones, so no ETB or LTB triggers (CR 707.4).
    /// `ResolveStackTop` mutates `GameObject.copiable_override` in place
    /// (layer 1 is a real mutation; see that field) and reruns
    /// `run_card_script` against the new snapshot, after purging what the
    /// source self-registered under its old identity (CR 707.2a). CR 707.9
    /// exceptions are applied to the snapshot before it freezes; see
    /// `CopyExceptions`.
    // TODO: "as ~ enters" choices are not re-prompted (CR 707.6); this path never re-enters the battlefield
    BecomeCopy(CopyExceptions),
    /// `Game.Ops.Attach` (CR 702.6a, Equip): attaches the source Equipment to
    /// its only legal target through `commands::attach_permanent`, the
    /// function a resolving Aura's entry-attach uses (CR 303.4b). A toggle with
    /// no fields. It moves no zones: `attach_permanent` mutates `attached_to`
    /// and `attachments` in place, and this logs `Attached`, not a
    /// `ZoneChange`.
    ///
    /// CR 702.6a's "activate only as a sorcery" is enforced in the
    /// `ActivateAbility` arm by matching `effect == Some(AbilityEffect::Attach)`,
    /// not by a new `RegisteredAbility` field, which would touch ~94 literal
    /// sites for one effect shape. Make it a field if a second ability needs
    /// the same timing restriction independently of its effect.
    // TODO: "Equip [quality]" restrictions beyond "creature you control" (CR 702.6c), the same subtype-filter gap as Aura "Enchant Human" in `target.rs`
    // TODO: multiple equip abilities on one permanent aren't told apart (CR 702.6d); `sba::collect_illegally_attached_equipment` doesn't consult which one attached it
    // TODO: Equip planeswalker (CR 702.6e), since planeswalkers are a non-goal
    Attach,
    /// `Game.Ops.ExchangeTextBoxes` (CR 612.5): exchanges the first two chosen
    /// targets' text boxes. A toggle with no fields, and the targets live on
    /// the `StackObject`.
    ///
    /// Not a `BecomeCopy` variant: CR 612.1 scopes text-changing effects to
    /// rules text and the type line, and CR 612.5 says only "text boxes", so
    /// name, mana cost, color, P/T and types stay as printed. Only
    /// `oracle_text`, `keywords` and `script_ref` (the engine's stand-in for
    /// rules text) cross over. It isn't a CR 707 copy effect.
    ///
    /// It reuses `BecomeCopy`'s machinery for changing a live permanent's
    /// behavior: `commands::purge_granted_abilities` then a fresh
    /// `run_card_script`, for both targets against a synthetic definition with
    /// the three fields swapped. It is a one-time mutation with no duration.
    ///
    /// The effect is pairwise: if either target is no longer legal at
    /// resolution, the whole effect does nothing. `ResolveStackTop` rechecks
    /// both against `top.target_requirement`'s two `TargetRequirement`s,
    /// because the flattened `legal_targets` loses the positional pairing (see
    /// `TargetSpec::Multiple`).
    ExchangeTextBoxes,
    /// `Game.Ops.Fight` (CR 701.14): two creatures each deal damage equal to
    /// their power to the other. No fields. The target spec picks the pair:
    /// - `TargetSpec::Multiple` with two exact count-1 words ("target creature
    ///   you control fights target creature you don't control"): the first
    ///   word's creature fights the second's, and both words may hold the same
    ///   creature, which fights itself.
    /// - one `TargetSpec::Primitive` with count 2 ("two target creatures fight
    ///   each other").
    /// - one `TargetSpec::Primitive` with count 1 ("it fights target
    ///   creature"): the ability's source fights the target.
    ///
    /// Any other shape does nothing. Each creature is the source of its own
    /// `DamageDealt` (`is_combat_damage: false`, CR 701.14d), so deathtouch,
    /// lifelink, wither, infect, protection, prevention, replacement and
    /// "deals damage" triggers see the creature and not the spell. Both
    /// amounts are read through the layer engine and fixed before either event
    /// is pushed, so the damage is simultaneous and SBAs run afterward. A
    /// creature with power 0 or less deals no damage and emits no event, and a
    /// self-fight is one event of twice its power (CR 701.14c).
    ///
    /// CR 701.14b: if either creature is gone, no longer a creature or an
    /// illegal target, neither deals damage. The ability still counts as
    /// resolved while one target is legal.
    // TODO: a self-source fight whose source left the battlefield does nothing instead of using last known information (CR 608.2h)
    // TODO: no "fights up to one target creature" (an `up_to` or `at_least` word) and no "fights each creature"
    Fight,
    /// `Game.Ops.Bite`: "target creature you control deals damage equal to its
    /// power to target creature you don't control". Not a fight (CR 701.14), so
    /// the dealer takes nothing back. No fields. Targets are a
    /// `TargetSpec::Multiple` of two exact count-1 words (first deals, second
    /// receives), or one count-1 `Primitive` (the source deals to the target).
    /// A count-2 word does nothing. The damage is one `DamageDealt` from the
    /// first creature (`is_combat_damage: false`), so every effect that
    /// applies to `Fight` damage applies. The dealer must still be a creature
    /// on the battlefield, and power 0 or less deals nothing.
    // TODO: a dealer that left the battlefield does nothing instead of using last known information for its power (CR 608.2h), and the other limits of `Fight`
    Bite,
    /// CR 700.2d: 2+ chosen modes of a modal spell or ability, combined. The
    /// `CastSpell` arm builds it only when `modes.len() >= 2`, and a single
    /// chosen mode uses its bare `effect`. One entry per chosen mode, in
    /// card-print order and not the order the player listed them, matching the
    /// ruling that chosen effects happen as printed. `None` is a mode with no
    /// effect of its own, contributing nothing at resolution, but its slot
    /// must line up 1:1 with the per-mode target chunks that `ResolveStackTop`
    /// re-derives from `StackObject::target_requirement` (always
    /// `TargetSpec::Multiple` here). Mode i owns `words[i]` consecutive target
    /// words: one for a mode with no target requirement (a `count: 0`
    /// placeholder, CR 700.2c) or a single `Primitive`, and n for a `Multiple`
    /// of n words. `commands::split_targets_by_requirement` gives one chunk per
    /// word. The sum of `words` is the length of the flat list.
    Modal {
        effects: Vec<Option<AbilityEffect>>,
        words: Vec<usize>,
    },
    /// CR 608.2c: several effects of one spell or ability, in the order
    /// written (`Game.Ops.Chain(a, b, ...)`). Each entry is accepted by
    /// `commands::is_modal_resolvable`, so a chain never nests, holds a
    /// `ForEachPlayer` or holds a custom function. Every entry sees the
    /// ability's one target requirement and legal targets, and an entry that
    /// doesn't use a target ignores them. It runs through the loop `Modal`
    /// uses (`engine::resolve::modal`), so an entry that pauses (a CR 616.1
    /// ordering choice, a search, a scry) resumes the rest afterward. An entry
    /// that can't do anything leaves the next to run (CR 609.3).
    ///
    /// `words` is `None` for `Game.Ops.Chain`, where every entry shares the
    /// whole target set. `Game.Ops.ChainEach` sets `Some(counts)`: entry i owns
    /// `counts[i]` consecutive target words of the `TargetSpec` (a `Primitive`
    /// is one word, a `Multiple` one per requirement), as `Modal`'s `words`
    /// does. An entry that owns none acts on its controller. The counts must
    /// sum to the declared word count, which `script::decode::check_chain_words`
    /// checks at registration or spell load.
    // TODO: a word shared by two entries ("destroy target creature, then its controller gains 3 life"), and a chain inside `ForEachPlayer` or as a mode of a `Modal`
    Chain {
        entries: Vec<AbilityEffect>,
        words: Option<Vec<usize>>,
    },
    /// A plain spell's resolution effect (CR 608.2m), declared by the script's
    /// optional `s.operation()` (a sibling of `s.register_abilities(c)`) when
    /// it returns a custom Lua function and not a `Game.Ops.*` tag. It is the
    /// primitive-or-script split of `RegisteredAbility::effect` and
    /// `::operation`, folded into one field because `CardDefinition` has no
    /// `operation` field (see `ScriptRuntime::build_operation`).
    ///
    /// It never reaches the `AbilityEffect` dispatch of `ResolveStackTop` or
    /// `ActivateAbility`: the `CastSpell` arm unpacks it into `StackObject`'s
    /// `operation` and `effect` (`Script(handle)` becomes `operation: handle,
    /// effect: None`). It is listed there only for exhaustiveness, like
    /// `Modal`.
    Script(ScriptHandle),
    /// CR 702.79a Persist and 702.93a Undying: "return it to the battlefield
    /// under its owner's control with a [-1/-1 | +1/+1] counter on it". Only
    /// meaningful on a `Dies`-triggered `RegisteredAbility`, whose
    /// `ability.source` is the pre-death `ObjectId`, so `ResolveStackTop` finds
    /// the graveyard object through `ObjectLineage::previous == Some(top.source)`,
    /// the link `sba::commit_death` mints. It does nothing if the card is gone
    /// (exiled in response).
    ///
    /// Narrow: it skips `replacement::apply_replacements` (no CR 616 "would
    /// enter, redirect") and `apply_enters_battlefield_modifiers`, so a card
    /// combining it with Devour, Bloodthirst or enters-tapped text won't
    /// compose. ETB triggers still fire from the plain `ZoneChange` it pushes.
    ReturnSelfWithCounter(CounterKind),
    /// CR 702.35a's second ability: "you may cast it by paying [cost] rather
    /// than its mana cost. If you don't, put it into your graveyard." Only
    /// meaningful on a `MadnessExiled`-triggered `RegisteredAbility`, whose
    /// `ability.source` is the stale pre-exile hand id, so `ResolveStackTop`
    /// finds the exile object through `ObjectLineage::previous == Some(top.source)`
    /// (as `ReturnSelfWithCounter` does) and does nothing if it's gone. It
    /// always pauses (`AwaitingMadnessChoice`) with a two-way resume
    /// (`Engine::decline_madness_cast`, `submit_madness_cast`). See
    /// `engine::resolve::madness`.
    MadnessCastOrDiscard,
    /// CR 702.85a Cascade: exile from the top of the library until a nonland
    /// card with lower mana value, cast it free if its mana value is still
    /// lower, and bottom the rest in random order. Only meaningful on a
    /// `Cast`-triggered `RegisteredAbility` self-registered on the cascade
    /// spell while it is on the stack, so `top.source` is live and needs no
    /// lineage lookup. It pauses (`AwaitingCascadeChoice`) once the library
    /// walk finds a candidate or runs out. See `engine::resolve::cascade`,
    /// including how it narrows CR 702.85a's second mana-value recheck.
    Cascade,
    /// `Game.Ops.ForEachPlayer(x)` (CR 101.4: "each player sacrifices a
    /// creature" in APNAP order). `x` is a script function or a bare
    /// `SacrificeRandom`, `DiscardRandom`, `Mill` or `LoseLife` tag (see
    /// `ForEachPlayerTag`). This field is `None` for the function shape, whose
    /// payload is `RegisteredAbility::operation` or `StackObject::operation`,
    /// and `Some(inner)` for a bare tag.
    ///
    /// Function shape: `engine::resolve::each_player::resolve_for_each_player_effect`
    /// invokes the function once per player in CR 101.4 APNAP order, rebasing
    /// `controller` ("you") each time. `TargetFilter::Object::controller`
    /// resolves against the `controller` passed to
    /// `ScriptRuntime::invoke_operation`, so `Game.ChooseN`,
    /// `Game.SelectTargets`, `Game.Ops.SacrificeChosen` and `DiscardChosen`
    /// compose inside the wrapped function. The actions happen simultaneously
    /// (CR 101.4): each invocation's `SacrificeChosen` and `DiscardChosen`
    /// ids (`OperationOutcome::PendingZoneMoves`) accumulate into one batch per
    /// verb, and the zone moves run after the last player's choice, so a
    /// mid-loop replacement or later choice still sees everything about to
    /// leave.
    ///
    /// Bare-tag shape: no script. `resolve_for_each_player_effect` draws each
    /// player's random picks directly (`Engine::pick_sacrifice_random_ids`,
    /// `pick_discard_random_ids`, the `next_rng` helpers of the single-player
    /// case) in synchronous Rust that cannot pause, and merges them into one
    /// queue before the single `continue_scripted_zone_move` call. `Mill(n)`
    /// merges every player's top N library ids into one
    /// `ScriptedZoneMoveKind::Mill` batch, and `LoseLife(n)` applies per player
    /// in APNAP order with one `LifeChanged` each. In this wrapper they act on
    /// each player and never read the ability's target spec.
    // TODO: only `SacrificeRandom`, `DiscardRandom`, `Mill` and `LoseLife` are accepted as a bare tag (a load-time error in the `Game.Ops.ForEachPlayer` binding); no real card needs another per-player primitive without a wrapping function
    ForEachPlayer(Option<Box<AbilityEffect>>),
    /// `Game.Ops.SacrificeRandom(filter, count)` (CR 701.21, "at random", a
    /// random procedure and not a choice, unlike `Game.ChooseN` with
    /// `Game.Ops.SacrificeChosen`). `filter` is the `EffectFilter` of
    /// `DestroyAll`, with `controller` resolved against the resolving object's
    /// controller, evaluated at resolution with no targeting. `count`
    /// candidates are drawn without replacement through `Engine::next_rng` (the
    /// deterministic, ledger-logged source `shuffle_library` uses), and fewer
    /// are sacrificed if fewer match, as for mill (CR 701.17b). It runs through
    /// `continue_scripted_zone_move(Sacrifice, ..)`, so CR 616.1 ordering and
    /// dies-triggers apply.
    SacrificeRandom { filter: EffectFilter, count: u32 },
    /// `Game.Ops.DiscardRandom(filter, count)` (CR 701.9b, random discard).
    /// `filter` has `SacrificeRandom`'s shape, and an empty table matches every
    /// card ("discard a random land" is
    /// `Game.Ops.DiscardRandom({ card_types = { "Land" } }, 1)`). Always the
    /// resolving controller's hand, and fewer than `count` if the hand is
    /// smaller. It runs through `continue_scripted_zone_move(Discard, ..)`.
    DiscardRandom { filter: EffectFilter, count: u32 },
    /// CR 701.9, 701.9b: "target player discards `count` cards", where that
    /// player chooses which. The target is declared normally
    /// (`s.target_requirement()`, a single `TargetFilter::Player`). Distinct
    /// from `DiscardRandom` (no choice) and `Game.Ops.DiscardChosen` (this
    /// ability's controller answers through `Game.ChooseN`; see
    /// `engine::resolve::discard`): the discarding player picks here, through a
    /// mid-resolution pause (`AwaitingDiscardChoice`,
    /// `Engine::submit_discard_choice`). One card is asked for and resolved,
    /// then asked again if `count` says so, with the hand re-read before each
    /// ask. An empty hand ends the effect early (CR 701.9).
    // TODO: "discard your hand" (an unbounded count); `count: u32` covers only a fixed number such as Mind Rot's two
    // TODO: several target words ("target player discards a card and target player discards a card"); single target only
    Discard { count: u32 },
}

/// Where `AbilityEffect::Search`'s found cards go, limited to the two common
/// real destinations. `Battlefield { tapped, .. }` covers fetch lands (most
/// enter tapped).
// TODO: Graveyard and Exile destinations
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SearchDestination {
    Hand,
    /// CR 303.4f, 303.4g, 303.4h: applies only to a found Aura
    /// (`commands::is_aura`), which enters already attached and chooses among
    /// legal enchant candidates as it enters. With none, it stays in the
    /// library (CR 303.4g). A non-Aura with this set enters unattached.
    /// `false` is the plain fetch-land search.
    Battlefield {
        tapped: bool,
        enters_attached: bool,
    },
}

/// Which named token to create — covers the fixed-spec artifact tokens.
#[derive(Clone, Debug, PartialEq)]
pub enum NamedToken {
    /// A Clue artifact token with "{2}, Sacrifice this artifact: Draw a card."
    Clue,
    /// A Food artifact token with "{2}, Sacrifice this artifact: Gain 3 life."
    Food,
    /// CR 111.10a: a Treasure artifact token with "{T}, Sacrifice this
    /// artifact: Add one mana of any color", using `AbilityEffect::AddManaChoice`.
    Treasure,
}

/// Full spec for a creature (or artifact creature, enchantment creature, etc.)
/// token generated by `Game.Tokens.Creature`.
#[derive(Clone, Debug, PartialEq)]
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
#[derive(Clone, Debug, PartialEq)]
pub enum TokenSpec {
    Named(NamedToken),
    Creature(CreatureTokenSpec),
    /// CR 707.10f, 111: "create a token that's a copy of [object]", naming the
    /// object copied. The token's `CardDefinition` isn't built from this spec
    /// (see `build_token_definition`). The `CreateToken` arm reads the copied
    /// object's current copiable values (its `copiable_override` if it is
    /// itself a copy, CR 707.3, else its `definition`) and reruns
    /// `ScriptRuntime::run_card_script` against that snapshot so copied
    /// abilities self-register (CR 707.2a). Other `TokenSpec`s skip that call,
    /// having no script. CR 707.9 exceptions apply to the snapshot before it
    /// freezes as the token's `definition`; see `CopyExceptions`.
    CopyOf(ObjectId, CopyExceptions),
}

/// CR 707.9 copy-effect exceptions, applied to a copy snapshot when it is
/// taken (by `BecomeCopy` or `CopyOf`) before it freezes into
/// `GameObject.copiable_override` or a token's `definition`. The real shapes
/// are open-ended card text, so this models fixed, parametrized primitives for
/// the common ones:
/// - CR 707.9b (modify a characteristic): `set_power_toughness`, "except it
///   has base power and toughness 1/1", the most common Clone-variant
///   exception.
/// - CR 707.9a (gain an ability while copying): `add_keywords`, "except it
///   also has flying", unioned into the snapshot's `keywords` as
///   `EffectPrimitive::AddKeyword` does.
/// - CR 707.9c, 707.9d (keep original characteristics): `retain_name` and
///   `retain_color`, "except it retains its own name". CR 707.9d also clears
///   any color indicator. Narrower than all seven copiable characteristics,
///   since no real card needs retained P/T, types or subtypes yet.
/// - CR 707.9a (gain a whole new ability): `grant_recurring_copy_trigger`,
///   Vesuvan Doppelganger's "except it has 'at the beginning of your upkeep,
///   you may have this creature become a copy of target creature'". This one
///   concrete shape stands in for a general ability grant, since no
///   Lua-authorable ability blueprint exists. See
///   `commands::grant_linked_copy_ability` for how it is built and why it is
///   hardcoded Rust.
/// - CR 707.9e (an "additional effect"): `enters_with_additional_counters`,
///   Spark Double's "it enters with an additional +1/+1 counter". It differs
///   from the fields above in being a `GameObject`-level mutation (a counter).
///   Both copy producers apply it after their `copy_generation` bump as a
///   `CounterAdded` push through `CommitEvents`. CR 707.9e's second sentence
///   (it doesn't happen if another copy effect applies first) is handled by
///   the chain detection and `carry_forward_if_declined` mechanism in
///   `Engine::submit_copy_choice`. `CounterAmount::X` (Altered Ego, CR 707.9e)
///   is the variable sibling of `Fixed`, resolved against the entering
///   object's own `x_value`: captured once at the `AwaitingCopyChoice` raise
///   site and kept through any chain, like `colors_spent`, `stack_id` and
///   `controller`, since the X belongs to Altered Ego and not to whichever
///   creature is copied at a link.
///
/// `Default` is a bare copy. CR 707.9e and 707.9g's generation check is real
/// (`GameObject.copy_generation`, `GameState.linked_ability_generation`), and
/// `grant_recurring_copy_trigger`'s ability is tagged with it. Both producers
/// call `commands::purge_granted_abilities` before re-registering, so the
/// supersession check is unreached through either today.
#[derive(Clone, Debug, PartialEq)]
pub struct CopyExceptions {
    pub set_power_toughness: Option<(i32, i32)>,
    pub add_keywords: KeywordSet,
    pub retain_name: bool,
    pub retain_color: bool,
    pub grant_recurring_copy_trigger: bool,
    pub enters_with_additional_counters: Option<(CounterKind, CounterAmount)>,
}

/// The amount of `CopyExceptions::enters_with_additional_counters`: `Fixed`
/// (Spark Double) or `X` (Altered Ego), resolved against the entering object's
/// own `StackObject.x_value` when the copy applies. One field with a
/// two-shaped amount, so "fixed and X both set" can't be represented.
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
    /// Applies these exceptions to `snapshot` in place before it freezes,
    /// shared by `BecomeCopy` and the `CopyOf` arm of `CreateToken` so the two
    /// producers can't drift. `original` is the copying object's own pre-copy
    /// `CardDefinition` (`GameObject::definition`, never overwritten by copy
    /// machinery), `None` only for `TokenSpec::CopyOf`, where a new token has
    /// no identity to retain. `retain_name` and `retain_color` are a tested
    /// no-op in that case.
    pub fn apply(
        &self,
        snapshot: &mut crate::types::card::CardDefinition,
        original: Option<&crate::types::card::CardDefinition>,
    ) {
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
                // CR 707.9d: the original's indicator-or-mana-cost color, not the copied source's indicator.
                snapshot.color_indicator = Some(original.colors());
            }
        }
    }
}

/// CR 612.5's "text box": `oracle_text`, `keywords` and `script_ref`, the three
/// `CardDefinition` fields that stand in for rules text (see
/// `AbilityEffect::ExchangeTextBoxes`). Overwrites `snapshot`'s with `other`'s
/// and leaves the rest alone, unlike the full copy of `CopyExceptions`.
pub fn apply_text_box(
    snapshot: &mut crate::types::card::CardDefinition,
    other: &crate::types::card::CardDefinition,
) {
    snapshot.oracle_text = other.oracle_text.clone();
    snapshot.keywords = other.keywords;
    snapshot.script_ref = other.script_ref.clone();
}

/// The condition shapes `Game.Conditions.*` covers, a Rust-native fast path so
/// common trigger shapes need no Lua condition function.
///
/// `Clone` only, not `Copy`, because `DiesMatching` and
/// `LeavesBattlefieldMatching` carry a `FilterSpec`. That ripples to
/// `ConditionSpec` and to the `ConditionTag` and `OncePerTurnTag` wrappers in
/// `script.rs`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ConditionPrimitive {
    /// CR 603: "whenever this permanent enters the battlefield", a
    /// `ZoneChange` moving `source` to the battlefield.
    EntersBattlefield,
    /// CR 702.21a (Ward): "whenever this permanent becomes the target of a
    /// spell or ability an opponent controls" - matches a `BecameTarget`
    /// event naming `source` whose spell or ability is controlled by anyone
    /// but the trigger's controller. There is no team model, so "opponent"
    /// is any other player.
    BecomesTargetOfOpponent,
    /// No filter, always matches. An explicit spelling of "unconditional"; see
    /// `RegisteredAbility::condition`.
    Always,
    /// CR 603.2: "whenever this deals damage", matched against `DamageDealt`
    /// with `source` as the dealer. Reachable because combat damage routes its
    /// events through `CommitEvents`, `SbaLoop` and `CollectTriggers`.
    DealsDamage,
    /// "Whenever this is dealt damage" — the mirror image of `DealsDamage`,
    /// matching on the event's `target` instead of its `source`. Only
    /// matches a `Target::Object` target (a player being dealt damage isn't
    /// "this" for a permanent's own ability).
    IsDealtDamage,
    /// CR 502.1: "at the beginning of your upkeep", matching
    /// `StepStarted { step: Step::Upkeep }` only when the ability's controller
    /// is the active player. `AdvanceTurnStep` routes its events through the
    /// real `ReplacementCheck`, `CommitEvents`, `SbaLoop` and `CollectTriggers`
    /// pipeline, so it is reachable.
    BeginningOfYourUpkeep,
    /// "At the beginning of each upkeep" — the less common, controller-
    /// agnostic sibling of `BeginningOfYourUpkeep` (e.g. Waste Not-style
    /// effects). Matches every `Step::Upkeep` `StepStarted` regardless of
    /// whose turn it is.
    BeginningOfEachUpkeep,
    /// CR 502.4: "at the beginning of your untap step", rare since the step
    /// grants no priority. CR 603.3 holds the trigger until a player next
    /// receives priority, which the `Untap` branch of `AdvanceTurnStep`
    /// achieves with `PipelineExit::AdvanceTurnStep`. Matches
    /// `StepStarted { step: Step::Untap }` only when the controller is the
    /// active player.
    BeginningOfYourUntap,
    /// "At the beginning of each untap step" — the controller-agnostic
    /// sibling of `BeginningOfYourUntap`, same relationship
    /// `BeginningOfEachUpkeep` has to `BeginningOfYourUpkeep`.
    BeginningOfEachUntap,
    /// CR 700.4, 603.10: "when this creature dies", a `ZoneChange` moving
    /// `source` from the battlefield to a graveyard. Reachable only through
    /// the last-known-information fallback of `collect_triggers`, because
    /// `source` is already gone from `state.objects` and `state.abilities`
    /// (purged by `commands::retire_object`). Matched against
    /// `last_known_info.object_id`, since the event's own `object` is the new
    /// graveyard id (CR 400.7).
    Dies,
    /// CR 603.10: "when this leaves the battlefield", the broader sibling of
    /// `Dies`: any `ZoneChange` of `source` off the battlefield. Same
    /// last-known-information matching.
    LeavesBattlefield,
    /// CR 121: "whenever you draw a card", a `ZoneChange` of any card from
    /// library to hand, filtered to the controller through
    /// `last_known_info.controller`. Not `source`-scoped. It matches by zone
    /// shape because the event has no "this was a draw" flag, so a future
    /// non-draw Library-to-Hand effect (CR 121.5) would false-positive.
    DrawsCard,
    /// "Whenever a player draws a card" — the controller-agnostic sibling of
    /// `DrawsCard` (e.g. Underworld Dreams-style effects), same relationship
    /// `BeginningOfEachUpkeep` has to `BeginningOfYourUpkeep`.
    EachDrawsCard,
    /// "Whenever you discard a card": a `ZoneChange` of any card from hand to
    /// graveyard, filtered to the controller through
    /// `last_known_info.controller`. Zone shape only, as `DrawsCard`, since
    /// nothing else moves hand to graveyard today. Both discard sources reach
    /// the pipeline: `Engine::submit_discard` (CR 514.1) and a cost-payment
    /// `Discard` (`commands::move_to_graveyard_for_cost`).
    DiscardsCard,
    /// "Whenever a player discards a card" — the controller-agnostic sibling
    /// of `DiscardsCard` (e.g. Waste Not/Megrim-style effects), same
    /// relationship `EachDrawsCard` has to `DrawsCard`.
    EachDiscardsCard,
    /// "When this card is discarded": the self-referential sibling of
    /// `DiscardsCard` and `EachDiscardsCard`, which are player-relative. It
    /// uses the CR 603.10 look-back mechanism of `Dies`, extended to a hand
    /// departure: match by `last_known_info.object_id` against a source
    /// already purged from `state.abilities`. CR 603.10a doesn't list leaving
    /// the hand, but the mechanism is identical. Reachable from a cost-payment
    /// `Discard` and the cleanup discard, which stash their departed
    /// `Triggered` abilities for the `collect_triggers` fallback.
    Discarded,
    /// CR 702.29c, 702.29f: "when you cycle this card". Narrower than
    /// `Discarded`: it matches only `Cycled`, a marker event logged beside the
    /// ordinary `ZoneChange` when the discard paid `AdditionalCost::DiscardSelf`
    /// (`Engine::continue_cost_zone_moves`'s `cycling_source`, the
    /// discriminator `commands::activate` uses for a Cycling cost). CR
    /// 702.29c's "triggers from whatever zone the card ends up in" follows
    /// because it never reads `to`. It reuses the CR 603.10 departed/LKI
    /// fallback of `Discarded`.
    Cycled,
    /// CR 702.35a's second ability: "when this card is exiled this way".
    /// Matches only the `MadnessExiled` marker event, logged beside the
    /// ordinary `ZoneChange` when a discard's madness redirect
    /// (`ReplacementPayload::ReplaceZoneDestination(ZoneKind::Exile)`,
    /// registered by the card on itself) won (`replacement::madness_marker_event`).
    /// Same CR 603.10 departed/LKI fallback as `Cycled` and `Discarded`.
    MadnessExiled,
    /// CR 601.2i, 702.85a: "when you cast this spell", matching
    /// `SpellCast { object }` with `object == source`. The spell is still live
    /// on the stack, so `triggers::collect_triggers` matches it in the
    /// ordinary main loop with no CR 603.10 departed/LKI fallback.
    // TODO: a self-registration producer; rerunning a card's `run_card_script` at cast time double-registers every ordinary ETB trigger, since `commands::remap_abilities` carries the cast-time registration across the stack-to-battlefield move, so only direct `RegisteredAbility` construction works today
    Cast,
    /// CR 507: "at the beginning of combat on your turn", matching
    /// `StepStarted { step: Step::BeginningOfCombat }` only when the controller
    /// is the active player. Reachable as `BeginningOfYourUpkeep` is.
    BeginningOfYourCombat,
    /// "At the beginning of each combat" — the controller-agnostic sibling
    /// of `BeginningOfYourCombat`.
    BeginningOfEachCombat,
    /// CR 512, 513: "at the beginning of your end step", matching
    /// `StepStarted { step: Step::End }` for the active player. Reachable as
    /// `BeginningOfYourUpkeep` is.
    BeginningOfYourEndStep,
    /// "At the beginning of each end step" — the controller-agnostic sibling
    /// of `BeginningOfYourEndStep`.
    BeginningOfEachEndStep,
    /// CR 701.22d: "whenever you scry", matching `CardsScried` with
    /// `event.player == controller`, logged by `Engine::submit_scry_choice`.
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
    /// CR 700.4, 603.10: "whenever a creature you control dies", the
    /// filter-scoped sibling of `Dies`. `Dies` matches only when the ability's
    /// own source died, and this matches any qualifying death through
    /// `filter.matches(&lki, controller)`, where `controller` is the ability's
    /// controller, so `FilterController::You` gives "you control" and `Any`
    /// needs no `Each*` sibling. The source is a separate live permanent
    /// watching for deaths (Blood Artist), so the ordinary live-source loop of
    /// `collect_triggers` checks it and not the CR 603.10 fallback, which
    /// whitelists only `Dies` and `LeavesBattlefield`.
    DiesMatching(FilterSpec),
    /// "Whenever a permanent matching filter leaves the battlefield" — the
    /// filter-scoped sibling of `LeavesBattlefield`, same relationship
    /// `DiesMatching` has to `Dies`.
    LeavesBattlefieldMatching(FilterSpec),
}

// TODO: `submit_discard`'s CR 514.1 hand-size discard and `draw_step`'s library-side retirement still discard their departed abilities outright; wiring them is inert today, since neither produces a Battlefield-departure LKI (CR 603.10)
// TODO: untap-step events can't trigger anything (CR 502.4 grants no priority), and Cleanup's `CleanupSbaCheck` flow doesn't run `collect_triggers`

impl ConditionPrimitive {
    /// Zero Lua VM calls: evaluated against the event being checked. `source`
    /// is the registered ability's source, not necessarily the event's object.
    /// `controller` and `active_player` are needed for the "your" relativity of
    /// `BeginningOfYourUpkeep` and its siblings.
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
                GameEventKind::ZoneChange {
                    from: ZoneKind::Library,
                    to: ZoneKind::Hand,
                    ..
                }
            ),
            ConditionPrimitive::DiscardsCard => matches!(
                event,
                GameEventKind::ZoneChange { from: ZoneKind::Hand, to: ZoneKind::Graveyard, last_known_info: Some(lki), .. }
                    if lki.controller == controller
            ),
            ConditionPrimitive::EachDiscardsCard => matches!(
                event,
                GameEventKind::ZoneChange {
                    from: ZoneKind::Hand,
                    to: ZoneKind::Graveyard,
                    ..
                }
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
            ConditionPrimitive::BecomesTargetOfOpponent => matches!(
                event,
                GameEventKind::BecameTarget { object, by_controller, .. }
                    if *object == source && *by_controller != controller
            ),
            ConditionPrimitive::BeginningOfYourCombat => {
                controller == active_player
                    && matches!(
                        event,
                        GameEventKind::StepStarted {
                            step: Step::BeginningOfCombat
                        }
                    )
            }
            ConditionPrimitive::BeginningOfEachCombat => {
                matches!(
                    event,
                    GameEventKind::StepStarted {
                        step: Step::BeginningOfCombat
                    }
                )
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
            ConditionPrimitive::EachSurveils => {
                matches!(event, GameEventKind::CardsSurveiled { .. })
            }
            // CR 700.4, 603.10: matched in the live-source loop against a separate watching ability, whose `controller` gives `FilterController::You` and `Any` their meaning. `Dies` instead matches by identity in the departed/LKI loop.
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

/// A condition or operation source string resolves at load time
/// (`ScriptRuntime::load_condition`) to a recognized `Game.Conditions.*` tag
/// (`Primitive`, no further Lua calls), a custom Lua function (`Script`,
/// invoked each time through `ScriptRuntime::invoke_condition`), or
/// `PrimitiveOncePerTurn` (CR 603.2h) from `Game.Conditions.OncePerTurn(tag)`
/// wrapping any other tag. Matching is the same as `Primitive`. The cap is
/// enforced in `triggers::collect_triggers` and the `CollectTriggers` stage
/// against `GameState::once_per_turn_triggered`, keyed by the ability's
/// `AbilityId` and reset each turn. A `Script` condition can't use it, because
/// a custom Lua function has no `AbilityId`-keyed slot without threading one
/// through `ScriptContext`.
///
/// `Clone` only, because `ConditionPrimitive` owns a `FilterSpec`.
#[derive(Clone, Debug, PartialEq)]
pub enum ConditionSpec {
    Primitive(ConditionPrimitive),
    Script(ScriptHandle),
    PrimitiveOncePerTurn(ConditionPrimitive),
}

/// CR 603.4's intervening-if clause ("When/Whenever/At X, if Y, Z") is a pure
/// state predicate, not event-shaped like `ConditionPrimitive`. It is checked
/// once when the trigger fires (in `triggers::collect_triggers`, beside the
/// ability's `condition`) and again at resolution (`ResolveStackTop`,
/// CR 608.2a). It is reused for CR 611.2c conditional continuous effects
/// (`ContinuousEffect::condition`).
///
/// `ControllerControlsAtLeast` resolves "you" as the raw
/// `GameObject.controller`, not `layers::effective_controller`.
/// `ContinuousEffect::applies_to` has the same layer 2 gap, and the layered
/// value would recurse, since `applicable_effects_in_layer` calls
/// `effective_controller`.
/// TODO: more shapes, such as hand size and "still on the battlefield"
///
/// `Clone` only, because `ControllerControlsAtLeast` carries a `FilterSpec`,
/// which ripples to `InterveningIfSpec`.
#[derive(Clone, Debug, PartialEq)]
pub enum InterveningIfPrimitive {
    ControllerAtOrBelowLife(i32),
    /// CR 611.2c's overwhelmingly common real shape: "as long as you
    /// control `count` or more permanents matching `filter`."
    ControllerControlsAtLeast {
        filter: FilterSpec,
        count: u32,
    },
    /// CR 603.4, 702.138b: "if it escaped" (Kroxa, Uro), an ordinary ETB
    /// trigger's intervening-if. CR 702.138c and 702.138d's "escapes with"
    /// templates are `ReplacementPayload::EntersWithCountersEscape`. Checked
    /// against the source's `GameObject::entered_via_escape`, `false` if the
    /// source no longer resolves, like `ControllerControlsAtLeast`.
    Escaped,
    /// CR 603.4, 702.79a, 702.93a: "if it had no [kind] counters on it",
    /// checked against the object as it was when it died and not live state,
    /// since a `Dies` source is already purged by `retire_object` (the CR
    /// 603.10 problem `ConditionPrimitive::Dies` solves). Reads `lki` when
    /// given, which the one real producer, a departed `Dies` trigger, always
    /// has, both at collection (the LKI loop of `triggers::collect_triggers`)
    /// and at the CR 608.2a recheck (`StackObject::source_lki`). Otherwise it
    /// looks up `state.object(source)`, with the conservative default of
    /// `Escaped`.
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
            InterveningIfPrimitive::ControllerAtOrBelowLife(threshold) => state
                .player(controller)
                .is_some_and(|p| p.life <= *threshold),
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
                None => state
                    .object(source)
                    .is_some_and(|o| o.counters.count(kind) == 0),
            },
        }
    }
}

/// See `InterveningIfPrimitive`. The same `Primitive`/`Script` shape as
/// `ConditionSpec`. `Script` is checked through
/// `ScriptRuntime::invoke_intervening_if`, which passes a minimal state
/// snapshot (`state_to_lua`, currently per-player life totals) as the third
/// argument: `function(source, controller, snapshot)`.
/// TODO: live object-characteristic queries in that snapshot, such as a creature's power
/// `Clone` only, because `InterveningIfPrimitive` owns a `FilterSpec`.
#[derive(Clone, Debug, PartialEq)]
pub enum InterveningIfSpec {
    Primitive(InterveningIfPrimitive),
    Script(ScriptHandle),
}

/// A card's registered ability, from its script's
/// `Game.CreateTriggeredAbility`, `Game.CreateActivatedAbility` or
/// `Game.CreateManaAbility` builder (`script::builders`).
/// `ScriptRuntime::run_card_script` drains the builder's result into
/// `GameState` with an allocated id. It lives in the flat
/// `GameState.abilities`, like `continuous_effects`, so the engine can scan
/// every ability against an event or timing window without walking objects.
#[derive(Clone, PartialEq, Debug)]
pub struct RegisteredAbility {
    /// Stable for the ability's lifetime; see `AbilityId`. Minted by
    /// `GameState::allocate_ability_id` at registration. A hand-built fixture
    /// picks its own value.
    pub id: AbilityId,
    pub source: ObjectId,
    pub kind: AbilityKind,
    /// `None` means no filter, the same as
    /// `Some(ConditionSpec::Primitive(ConditionPrimitive::Always))`. Both
    /// spellings exist on purpose: `None` is for `Activated` and `Static`
    /// abilities, which have no condition concept, and `Always` is an explicit
    /// choice by a trigger author. Loading rejects a bare Lua `nil` so a
    /// typo'd `Game.Conditions.Typo` isn't silently unconditional (see
    /// `ScriptRuntime::load_condition`).
    pub condition: Option<ConditionSpec>,
    /// `None` means no cost: `ActivateAbility` treats it as an empty
    /// `CostSpec` and still routes through `AwaitingPayment` for an explicit
    /// acknowledgment (CR 118.5a). `Some(spec)` is the CR 602.2b cost, with
    /// `spec.additional` from `:AddCost` (tap, sacrifice, discard, life) and
    /// `spec.mana` from `Game.Costs.Mana("{1}{R}")`, parsed by
    /// `ManaCost::parse`.
    pub cost: Option<CostSpec>,
    /// `None` means no targets (CR 601.2c), as for
    /// `CardDefinition::target_requirement`. `ActivateAbility` validates
    /// non-mana-ability targets against `Some(TargetSpec::Primitive(req))`,
    /// whose `req.filter` can be a scripted `TargetFilter::Script` set through
    /// `:SetTarget(function)`.
    pub target: Option<TargetSpec>,
    pub operation: ScriptHandle,
    /// See `AbilityEffect`. `Some` for an effect resolved without a script call
    /// (a CR 605 mana ability's `AddMana`, or a `Game.Ops.*` primitive loaded
    /// by `ScriptRuntime::load_operation`). `None` falls back to invoking
    /// `operation`.
    pub effect: Option<AbilityEffect>,
    /// CR 603.4's intervening-if clause, separate from `condition` (the
    /// trigger-event check). See `InterveningIfSpec`. `None` for abilities
    /// without "if" text. Checked in `triggers::collect_triggers` and baked
    /// onto the `StackObject` (`PutTriggersApnap`) for `ResolveStackTop` to
    /// recheck (CR 608.2a).
    pub intervening_if: Option<InterveningIfSpec>,
    /// CR 603.3c, 700.2b: a modal triggered ability's modes, empty for
    /// non-modal ones. It reuses `SpellMode` and `ModeChoice`, since the shape
    /// matches a spell's. Only the timing differs: the choice is made as the
    /// ability goes on the stack (CR 603.3c, `AwaitingTriggerModeChoice` and
    /// `Engine::submit_trigger_mode_choice`). `SpellMode::additional_cost` is
    /// never set or read here, since CR 700.2h's cost text belongs to casting
    /// (CR 601.2b, 601.2f-h).
    pub modes: Vec<SpellMode>,
    /// Meaningless when `modes` is empty, but must be set since
    /// `RegisteredAbility` has no `Default`.
    // TODO: CR 700.2e opponent-chosen modes for a triggered ability; `push_ordered_triggers` doesn't special-case `ExactlyChosenByOpponent` or `UpToChosenByOpponent`, so the controller would be asked instead of the other player
    pub mode_choice: ModeChoice,
}

#[cfg(test)]
mod tests {
    use super::*;

    // These conditions never consult controller or active_player, so any two PlayerIds work.
    const CONTROLLER: PlayerId = PlayerId(0);
    const ACTIVE_PLAYER: PlayerId = PlayerId(0);

    #[test]
    fn deals_damage_matches_when_source_is_the_dealer() {
        let source = ObjectId(0);
        let other = ObjectId(1);
        let event = GameEventKind::DamageDealt {
            source,
            target: Target::Object(other),
            amount: 3,
            is_combat_damage: false,
        };
        assert!(ConditionPrimitive::DealsDamage.matches(source, CONTROLLER, ACTIVE_PLAYER, &event));
    }

    #[test]
    fn deals_damage_does_not_match_a_different_dealer() {
        let source = ObjectId(0);
        let dealer = ObjectId(1);
        let event = GameEventKind::DamageDealt {
            source: dealer,
            target: Target::Object(source),
            amount: 3,
            is_combat_damage: false,
        };
        assert!(!ConditionPrimitive::DealsDamage.matches(
            source,
            CONTROLLER,
            ACTIVE_PLAYER,
            &event
        ));
    }

    #[test]
    fn is_dealt_damage_matches_when_source_is_the_target() {
        let source = ObjectId(0);
        let dealer = ObjectId(1);
        let event = GameEventKind::DamageDealt {
            source: dealer,
            target: Target::Object(source),
            amount: 3,
            is_combat_damage: false,
        };
        assert!(ConditionPrimitive::IsDealtDamage.matches(
            source,
            CONTROLLER,
            ACTIVE_PLAYER,
            &event
        ));
    }

    #[test]
    fn is_dealt_damage_does_not_match_a_different_target() {
        let source = ObjectId(0);
        let other = ObjectId(1);
        let event = GameEventKind::DamageDealt {
            source,
            target: Target::Object(other),
            amount: 3,
            is_combat_damage: false,
        };
        assert!(!ConditionPrimitive::IsDealtDamage.matches(
            source,
            CONTROLLER,
            ACTIVE_PLAYER,
            &event
        ));
    }

    #[test]
    fn is_dealt_damage_does_not_match_a_player_target() {
        // A player being dealt damage is never "this" for a permanent's ability.
        let source = ObjectId(0);
        let dealer = ObjectId(1);
        let event = GameEventKind::DamageDealt {
            source: dealer,
            target: Target::Player(PlayerId(0)),
            amount: 3,
            is_combat_damage: false,
        };
        assert!(!ConditionPrimitive::IsDealtDamage.matches(
            source,
            CONTROLLER,
            ACTIVE_PLAYER,
            &event
        ));
    }

    #[test]
    fn deals_damage_and_is_dealt_damage_dont_match_unrelated_events() {
        let source = ObjectId(0);
        let event = GameEventKind::LifeChanged {
            player: PlayerId(0),
            delta: -1,
        };
        assert!(!ConditionPrimitive::DealsDamage.matches(
            source,
            CONTROLLER,
            ACTIVE_PLAYER,
            &event
        ));
        assert!(!ConditionPrimitive::IsDealtDamage.matches(
            source,
            CONTROLLER,
            ACTIVE_PLAYER,
            &event
        ));
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
        assert!(!ConditionPrimitive::BeginningOfYourUpkeep.matches(
            source,
            controller,
            active_player,
            &event
        ));
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
        assert!(ConditionPrimitive::BeginningOfEachUpkeep.matches(
            source,
            controller,
            active_player,
            &event
        ));
    }

    #[test]
    fn beginning_of_each_upkeep_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let event = GameEventKind::StepStarted { step: Step::End };
        assert!(!ConditionPrimitive::BeginningOfEachUpkeep.matches(
            source,
            CONTROLLER,
            ACTIVE_PLAYER,
            &event
        ));
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
        assert!(!ConditionPrimitive::BeginningOfYourUntap.matches(
            source,
            controller,
            active_player,
            &event
        ));
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
        assert!(ConditionPrimitive::BeginningOfEachUntap.matches(
            source,
            controller,
            active_player,
            &event
        ));
    }

    #[test]
    fn beginning_of_each_untap_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let event = GameEventKind::StepStarted { step: Step::Upkeep };
        assert!(!ConditionPrimitive::BeginningOfEachUntap.matches(
            source,
            CONTROLLER,
            ACTIVE_PLAYER,
            &event
        ));
    }

    // Dies and LeavesBattlefield match only last_known_info, since the object is already gone (CR 603.10).
    fn dummy_last_known_info(object_id: ObjectId) -> crate::types::object::GameObject {
        use crate::types::card::CardDefinition;
        use crate::types::card::{
            CardTypeSet, KeywordSet, ModeChoice, PTValue, ScriptReference, SupertypeSet,
        };
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
                mana_cost: ManaCost {
                    symbols: Vec::new(),
                },
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
            attached_to: None,
            attached_to_player: None,
            timestamp: crate::types::ids::SequenceNo(0),
            controlled_since_turn: 0,
            is_commander: false,
            is_token: false,
            visibility: VisibilityPolicy {
                public: true,
                visible_to: Vec::new(),
            },
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
        // Battlefield to Exile (a CR 614.13 "exile instead") is a LeavesBattlefield, not a Dies.
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
        assert!(ConditionPrimitive::LeavesBattlefield.matches(
            source,
            CONTROLLER,
            ACTIVE_PLAYER,
            &event
        ));
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
        assert!(!ConditionPrimitive::LeavesBattlefield.matches(
            source,
            CONTROLLER,
            ACTIVE_PLAYER,
            &event
        ));
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
        assert!(ConditionPrimitive::EachDrawsCard.matches(
            source,
            controller,
            ACTIVE_PLAYER,
            &event
        ));
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
        assert!(!ConditionPrimitive::EachDrawsCard.matches(
            source,
            CONTROLLER,
            ACTIVE_PLAYER,
            &event
        ));
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
        assert!(!ConditionPrimitive::DiscardsCard.matches(
            source,
            controller,
            ACTIVE_PLAYER,
            &event
        ));
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
        assert!(!ConditionPrimitive::DiscardsCard.matches(
            source,
            controller,
            ACTIVE_PLAYER,
            &event
        ));
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
        assert!(ConditionPrimitive::EachDiscardsCard.matches(
            source,
            controller,
            ACTIVE_PLAYER,
            &event
        ));
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
        assert!(!ConditionPrimitive::EachDiscardsCard.matches(
            source,
            CONTROLLER,
            ACTIVE_PLAYER,
            &event
        ));
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
        // Dies, not Discarded: the same card departs from a different zone.
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
        let event = GameEventKind::StepStarted {
            step: Step::BeginningOfCombat,
        };
        assert!(ConditionPrimitive::BeginningOfYourCombat.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_your_combat_does_not_match_someone_elses_combat() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted {
            step: Step::BeginningOfCombat,
        };
        assert!(!ConditionPrimitive::BeginningOfYourCombat.matches(
            source,
            controller,
            active_player,
            &event
        ));
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
        let event = GameEventKind::StepStarted {
            step: Step::BeginningOfCombat,
        };
        assert!(ConditionPrimitive::BeginningOfEachCombat.matches(
            source,
            controller,
            active_player,
            &event
        ));
    }

    #[test]
    fn beginning_of_each_combat_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let event = GameEventKind::StepStarted { step: Step::End };
        assert!(!ConditionPrimitive::BeginningOfEachCombat.matches(
            source,
            CONTROLLER,
            ACTIVE_PLAYER,
            &event
        ));
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
        assert!(!ConditionPrimitive::BeginningOfYourEndStep.matches(
            source,
            controller,
            active_player,
            &event
        ));
    }

    #[test]
    fn beginning_of_your_end_step_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let p0 = PlayerId(0);
        let event = GameEventKind::StepStarted {
            step: Step::BeginningOfCombat,
        };
        assert!(!ConditionPrimitive::BeginningOfYourEndStep.matches(source, p0, p0, &event));
    }

    #[test]
    fn beginning_of_each_end_step_matches_regardless_of_whose_turn_it_is() {
        let source = ObjectId(0);
        let controller = PlayerId(0);
        let active_player = PlayerId(1);
        let event = GameEventKind::StepStarted { step: Step::End };
        assert!(ConditionPrimitive::BeginningOfEachEndStep.matches(
            source,
            controller,
            active_player,
            &event
        ));
    }

    #[test]
    fn beginning_of_each_end_step_does_not_match_a_different_step() {
        let source = ObjectId(0);
        let event = GameEventKind::StepStarted { step: Step::Upkeep };
        assert!(!ConditionPrimitive::BeginningOfEachEndStep.matches(
            source,
            CONTROLLER,
            ACTIVE_PLAYER,
            &event
        ));
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
            land_play_modifiers: Vec::new(),
            abilities: Vec::new(),
            ledger: Vec::new(),
            rng: RngState {
                seed: 1,
                draws_consumed: 0,
            },
            next_event_id: 0,
            next_sequence_no: 0,
            next_object_id: 0,
            pending_step_skips: Vec::new(),
            once_per_turn_triggered: std::collections::HashSet::new(),
            commander_return_offered: std::collections::HashSet::new(),
            linked_ability_generation: std::collections::HashMap::new(),
            script_ability_ids: std::collections::HashSet::new(),
            ability_function_zones: std::collections::HashMap::new(),
            off_battlefield_scripts_run: std::collections::HashSet::new(),
            linked_exiles: Vec::new(),
            pending_timing_bypass: None,
            pending_free_cast: None,
        }
    }

    #[test]
    fn controller_at_or_below_life_holds_when_life_is_at_the_threshold() {
        let p0 = PlayerId(0);
        let state = state_with_life(p0, 5);
        assert!(InterveningIfPrimitive::ControllerAtOrBelowLife(5).holds(
            &state,
            p0,
            ObjectId(0),
            None
        ));
    }

    #[test]
    fn controller_at_or_below_life_holds_when_life_is_below_the_threshold() {
        let p0 = PlayerId(0);
        let state = state_with_life(p0, 3);
        assert!(InterveningIfPrimitive::ControllerAtOrBelowLife(5).holds(
            &state,
            p0,
            ObjectId(0),
            None
        ));
    }

    #[test]
    fn controller_at_or_below_life_does_not_hold_when_life_is_above_the_threshold() {
        let p0 = PlayerId(0);
        let state = state_with_life(p0, 6);
        assert!(!InterveningIfPrimitive::ControllerAtOrBelowLife(5).holds(
            &state,
            p0,
            ObjectId(0),
            None
        ));
    }

    #[test]
    fn controller_at_or_below_life_does_not_hold_for_an_unknown_player() {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let state = state_with_life(p0, 1);
        assert!(!InterveningIfPrimitive::ControllerAtOrBelowLife(5).holds(
            &state,
            p1,
            ObjectId(0),
            None
        ));
    }

    /// A minimal battlefield permanent for `ControllerControlsAtLeast`
    /// fixtures, parameterized on controller and card types.
    fn battlefield_object(
        id: ObjectId,
        controller: PlayerId,
        card_types: CardTypeSet,
    ) -> crate::types::object::GameObject {
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
                mana_cost: ManaCost {
                    symbols: Vec::new(),
                },
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
            attached_to: None,
            attached_to_player: None,
            timestamp: crate::types::ids::SequenceNo(0),
            controlled_since_turn: 0,
            is_commander: false,
            is_token: false,
            visibility: VisibilityPolicy {
                public: true,
                visible_to: Vec::new(),
            },
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
        state.objects.insert(
            ObjectId(0),
            battlefield_object(ObjectId(0), p0, CardTypeSet::ARTIFACT),
        );
        state.objects.insert(
            ObjectId(1),
            battlefield_object(ObjectId(1), p0, CardTypeSet::ARTIFACT),
        );
        state.objects.insert(
            ObjectId(2),
            battlefield_object(ObjectId(2), p0, CardTypeSet::ARTIFACT),
        );
        let primitive = InterveningIfPrimitive::ControllerControlsAtLeast {
            filter: artifacts_filter(),
            count: 3,
        };
        assert!(primitive.holds(&state, p0, ObjectId(0), None));
    }

    #[test]
    fn controls_at_least_does_not_hold_when_below_the_count() {
        let p0 = PlayerId(0);
        let mut state = state_with_life(p0, 20);
        state.objects.insert(
            ObjectId(0),
            battlefield_object(ObjectId(0), p0, CardTypeSet::ARTIFACT),
        );
        state.objects.insert(
            ObjectId(1),
            battlefield_object(ObjectId(1), p0, CardTypeSet::ARTIFACT),
        );
        let primitive = InterveningIfPrimitive::ControllerControlsAtLeast {
            filter: artifacts_filter(),
            count: 3,
        };
        assert!(!primitive.holds(&state, p0, ObjectId(0), None));
    }

    #[test]
    fn controls_at_least_drops_the_instant_a_matching_permanent_leaves() {
        let p0 = PlayerId(0);
        let mut state = state_with_life(p0, 20);
        state.objects.insert(
            ObjectId(0),
            battlefield_object(ObjectId(0), p0, CardTypeSet::ARTIFACT),
        );
        state.objects.insert(
            ObjectId(1),
            battlefield_object(ObjectId(1), p0, CardTypeSet::ARTIFACT),
        );
        state.objects.insert(
            ObjectId(2),
            battlefield_object(ObjectId(2), p0, CardTypeSet::ARTIFACT),
        );
        let primitive = InterveningIfPrimitive::ControllerControlsAtLeast {
            filter: artifacts_filter(),
            count: 3,
        };
        assert!(
            primitive.holds(&state, p0, ObjectId(0), None),
            "3 artifacts meets the threshold"
        );

        // Removing one below the threshold flips the same query, with no cache to invalidate.
        state.objects.remove(&ObjectId(2));
        assert!(
            !primitive.holds(&state, p0, ObjectId(0), None),
            "down to 2 artifacts, no longer holds"
        );
    }

    #[test]
    fn controls_at_least_with_a_you_filter_ignores_an_opponents_matching_permanents() {
        let p0 = PlayerId(0);
        let p1 = PlayerId(1);
        let mut state = state_with_life(p0, 20);
        state.objects.insert(
            ObjectId(0),
            battlefield_object(ObjectId(0), p0, CardTypeSet::ARTIFACT),
        );
        state.objects.insert(
            ObjectId(1),
            battlefield_object(ObjectId(1), p1, CardTypeSet::ARTIFACT),
        );
        state.objects.insert(
            ObjectId(2),
            battlefield_object(ObjectId(2), p1, CardTypeSet::ARTIFACT),
        );
        let primitive = InterveningIfPrimitive::ControllerControlsAtLeast {
            filter: artifacts_filter(),
            count: 3,
        };
        assert!(
            !primitive.holds(&state, p0, ObjectId(0), None),
            "p0 controls only 1 artifact - the opponent's 2 don't count toward p0's threshold"
        );
    }
}
