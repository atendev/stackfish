//! Cross-boundary types shared by every other `script/` submodule — the
//! Lua<->Rust error type, the borrowed per-invocation context, the
//! coroutine-choice/operation-outcome types, and the shared script registry
//! — split out of the original monolithic `script.rs`.

use std::collections::HashMap;

use mlua::RegistryKey;

use crate::types::ability::RegisteredAbility;
use crate::types::cost::CostModifier;
use crate::types::effect::{ContinuousEffect, ReplacementEffect};
use crate::types::event::{GameEvent, GameEventKind};
use crate::types::game::GameState;
use crate::types::ids::{ObjectId, PlayerId, ScriptHandle, Target};
use crate::types::player::{DrawLimitModifier, HandRevealModifier, HandSizeModifier, LandPlayModifier};
use crate::types::target::TargetFilter;

#[derive(Debug)]
pub enum ScriptError {
    LoadError(String),
    RuntimeError(String),
}

pub(crate) fn to_lua_err(e: ScriptError) -> mlua::Error {
    mlua::Error::RuntimeError(format!("{e:?}"))
}

/// Borrowed view handed to a Lua callback for the duration of one
/// condition/cost/target/operation invocation.
///
/// `state` is the full `&GameState` — but that's fine on the Rust side; this
/// engine is the omniscient rules authority, not a player, so full access
/// here isn't itself the leak DESIGN.md's open question warns about. The
/// leak surface is strictly the Rust->Lua boundary: nothing here ever hands
/// `state` (or a piece of it) to Lua directly. `invoke_condition` and every
/// other state-aware `invoke_*` method instead builds a `state_to_lua`
/// snapshot — a plain table, not a live reference — which is the actual
/// hidden-information-safe boundary (see that function's own doc comment for
/// what it does and doesn't expose). `ScriptContext` itself doesn't need
/// narrowing so long as that invariant holds.
pub struct ScriptContext<'a> {
    pub state: &'a GameState,
    pub source: ObjectId,
    pub controller: PlayerId,
    pub event: Option<&'a GameEvent>,
    pub chosen_targets: Vec<Target>,
}

/// A card script's request for player input mid-resolution (backlog item
/// 9), yielded from a coroutine-invoked operation via `Game.SelectTargets`/
/// `Game.MayChoose`. Decoded from the yielded Lua table by
/// `decode_script_choice_request`.
#[derive(Clone, Debug)]
pub enum ScriptChoiceRequest {
    /// `Game.SelectTargets(filter)` — a single target matching a
    /// `TargetFilter` (no count > 1, same narrowing precedent every other
    /// targeting-adjacent slice already set — see `TargetFilter`'s own doc
    /// comment). `exclude_ids` (`phase5/mass-choice-effects`, `Game.ChooseN`)
    /// is a set of already-picked object ids to reject regardless of `filter`
    /// — empty for a bare `Game.SelectTargets` call; `Game.ChooseN`'s Lua
    /// loop grows it by one entry per iteration so repeat calls to the same
    /// underlying ask can't hand back a target already chosen this batch.
    /// Checked directly in `Engine::submit_script_choice`, not folded into
    /// `TargetFilter` itself — `TargetFilter::Object` is constructed at ~50
    /// call sites across this crate (card scripts' own `target_requirement`,
    /// costs, static-effect affected-filters, ...), and none of the others
    /// need a per-ask exclusion set, so widening that shared enum would mean
    /// touching every one of them for a need only this one request shape has.
    SelectTarget { filter: TargetFilter, exclude_ids: Vec<ObjectId> },
    /// `Game.MayChoose()` (backlog item 9a, CR 608.2d) — a plain yes/no
    /// choice with no filter to satisfy; any `bool` answer is legal, unlike
    /// `SelectTarget`'s `target_is_legal` check. `Game.ChooseN`'s `up_to`
    /// mode reuses this same request to ask "one more?" before each optional
    /// pick, rather than inventing a dedicated request shape.
    MayChoose,
}

/// Which zone-move verb a script operation's returned table asked for —
/// `phase5/mass-choice-effects`'s `Game.Ops.SacrificeChosen`/`DiscardChosen`,
/// consumed by `ScriptRuntime::handle_operation_result` decoding the
/// `sacrifice_chosen`/`discard_chosen` result keys. Deliberately a small
/// script-local enum rather than reusing `engine::process_state::
/// ScriptedZoneMoveKind` directly — this `script` module doesn't depend on
/// `engine`'s internals (the dependency runs the other way: `engine` reads
/// `OperationOutcome`), so the one `engine::resolve::misc_effects`/
/// `engine::resolve::each_player` call site that consumes this maps it to
/// the real `ScriptedZoneMoveKind` with a one-line `match`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScriptZoneMoveKind {
    Sacrifice,
    Discard,
}

/// What one `ScriptRuntime::invoke_operation`/`resume_operation` call
/// produced. `Finished` is the pre-existing synchronous behavior (the
/// operation ran to completion, any recognized effect key already applied)
/// — a script that never calls `Game.SelectTargets` only ever produces this,
/// so nothing about existing scripts' observable behavior changes.
/// `NeedsChoice` is new: the script yielded a real request and must be
/// resumed (`resume_operation`) with the player's answer before it can
/// continue — `Engine::submit_script_choice` is the only intended caller.
/// `PendingZoneMoves` (`phase5/mass-choice-effects`) is the `sacrifice_chosen`/
/// `discard_chosen` result-table generalization the pre-existing `gain_life`
/// arm's own TODO flagged — one entry per distinct verb the returned table
/// asked for (at most two today: a Balance-style per-player body can ask for
/// both a sacrifice batch and a discard batch in the same return), each a
/// real zone-move still to perform. Unlike `gain_life` (a direct `GameState`
/// mutation `handle_operation_result` can just do inline), a zone move needs
/// `Engine::continue_scripted_zone_move`'s full replacement-checked pipeline
/// — `ScriptRuntime` has no access to that (it lives one layer up, in
/// `engine`), so this variant hands the *request* back up instead of trying
/// to perform it itself, the same "outcome describes, engine acts" split
/// `NeedsChoice` already established.
/// `PendingCast` (CR 601.2/702.85a-style "you may cast this without paying
/// its mana cost") is `Game.Ops.CastSpell(card, opts)`'s own result-table
/// shape (`{ cast_spell = {...} }`), same "outcome describes, engine acts"
/// split `PendingZoneMoves` already established — casting needs
/// `commands::execute_command`'s full pipeline (payment pause, replacement
/// checks, ...), which `ScriptRuntime` has no access to. Follows the same
/// synthetic-`PlayerActionKind::CastSpell`-via-`execute_command` shape
/// `Engine::submit_cascade_cast`/`submit_madness_cast` already established
/// for their own (non-scripted) equivalents — `engine::resolve::misc_effects
/// ::resolve_misc_or_script_effect` is this variant's only consumer, and
/// mirrors those two methods' own command-building/pause-folding logic.
/// `free` (CR 702.85a's own "without paying its mana cost") reuses
/// `GameState::pending_free_cast`, which hardcodes `from_zone: Exile` (see
/// that field's own doc comment) — TODO: a script wanting "cast this for
/// free from a zone other than exile" isn't representable yet, same
/// limitation Cascade's own free cast already has.
/// `PendingDraw` (`draw_cards = N` on the returned table) is `Game.Ops.
/// DrawCards(n)`'s own custom-function-return equivalent — same "outcome
/// describes, engine acts" split, since the real pipeline
/// (`Engine::continue_draw_cards`) needs `&mut Engine` (touches
/// `self.departed_triggered_abilities`/`self.run_card_script_for`/the
/// ledger) and can itself pause for a CR 616.1 ordering choice when 2+
/// replacement effects compete for the same draw (a Words-of-Wind-style
/// redirect) - `ScriptRuntime` has access to neither. `player` is always
/// whichever `controller` the call this outcome came from was invoked
/// with, same as `gain_life` - a custom function can't make a *different*
/// player draw.
/// `PendingEvents` (`deal_damage`/`add_counters` on the returned table) is
/// lighter than the other three - `resolve_damage_effect`/
/// `resolve_counters_effect` (the primitive-tag fast path's own resolvers)
/// are pure event-builders that push `DamageDealt`/`CounterAdded` onto the
/// resolution's own accumulated event list and let the *existing*
/// `ReplacementCheck`/`CommitEvents` pipeline apply CR 615/616
/// prevention/replacement and deathtouch/wither/infect/lifelink generically
/// - same "hand back a constructed event, not a request" shape
/// `commands::apply_copy_additional_counters` already established for CR
/// 707.9e's own additional-counter exception. No pause of its own, so the
/// caller just folds these into its own `events`/`events_so_far` and
/// continues - correct whether or not this call happened inside a
/// `Game.Ops.ForEachPlayer` loop, since neither CR 615/616 nor CR 613 ask
/// for cross-player simultaneity the way CR 101.4 does for sacrifice/
/// discard.
#[derive(Debug)]
pub enum OperationOutcome {
    Finished,
    NeedsChoice { thread: ScriptHandle, request: ScriptChoiceRequest },
    PendingZoneMoves(Vec<(ScriptZoneMoveKind, Vec<ObjectId>)>),
    PendingCast { card: ObjectId, modes: Vec<u32>, targets: Vec<Target>, x_value: Option<u32>, free: bool },
    PendingDraw { amount: u32 },
    PendingEvents(Vec<GameEventKind>),
}

/// Shared, interior-mutable store for loaded Lua callbacks plus whatever a
/// currently-running card script has self-registered so far — wrapped in
/// `Rc<RefCell<_>>` so a Lua-bound closure (`Game.CreateTriggeredAbility`,
/// and the `TriggeredAbilityBuilder` it returns) can reach it without ever
/// needing `&mut GameState`, or even `&mut ScriptRuntime`, mid-callback.
/// This is the actual fix for the registration-time mutability gap flagged
/// since Phase 2 — a Lua callback only ever touches this small side-channel,
/// never `GameState` itself.
#[derive(Default)]
pub(crate) struct ScriptRegistry {
    /// One entry per loaded callback (a condition/operation/etc function),
    /// keyed by the opaque `ScriptHandle` data types reference it by. A
    /// `RegistryKey` rather than holding the `mlua::Function` directly since
    /// `Function` borrows from `lua` — the registry is `mlua`'s own escape
    /// hatch for stashing a Lua value independent of that borrow.
    pub(crate) scripts: HashMap<ScriptHandle, RegistryKey>,
    pub(crate) next_handle: u64,
    /// Abilities a currently-running `register_abilities` has `:Register()`ed,
    /// waiting to be drained into `GameState.abilities` once the whole card
    /// script call returns (`ScriptRuntime::run_card_script`). Lua never
    /// gets a `GameState` reference at all — this is what `:Register()`
    /// pushes onto instead. Shared by both `TriggeredAbilityBuilder` and
    /// `ActivatedAbilityBuilder` — the drain doesn't care about
    /// `AbilityKind`, so there's no need for a second, kind-specific queue.
    pub(crate) pending_triggered: Vec<RegisteredAbility>,
    /// `(index into pending_triggered, zone)` for each queued ability that
    /// declared `:SetFunctionsFrom(zone)` - a parallel list rather than a
    /// `RegisteredAbility` field (see `GameState::ability_function_zones`).
    /// Drained together with `pending_triggered`.
    pub(crate) pending_function_zones: Vec<(usize, crate::types::zone::ZoneKind)>,
    /// Same idea as `pending_triggered`, for `Game.CreateStaticAbility`'s
    /// builder (CR 604/611) — drained into `GameState.continuous_effects` by
    /// `run_card_script`. `timestamp` on each queued `ContinuousEffect` is a
    /// placeholder (`SequenceNo(0)`) until drain time, since the builder has
    /// no `&mut GameState` to call `allocate_sequence_no()` from inside a Lua
    /// callback — the drain step stamps the real value.
    /// Third element: `:SetCharacteristicDefining()` (CR 604.3), which makes
    /// the effect function in every zone.
    pub(crate) pending_static: Vec<(ContinuousEffect, Option<crate::types::zone::ZoneKind>, bool)>,
    /// Same idea as `pending_static`, for `Game.CreateReplacementEffect`'s
    /// builder (CR 614/701.19b) — drained into `GameState.replacement_effects`
    /// by `run_card_script`. Unlike `pending_static`'s `ContinuousEffect`, a
    /// `ReplacementEffect` has no `timestamp` field to stamp at drain time —
    /// nothing about CR 613.7 ordering applies to it.
    pub(crate) pending_replacement: Vec<(ReplacementEffect, Option<crate::types::zone::ZoneKind>)>,
    /// Same idea as `pending_static`, for `Game.CreateCostModifier`'s builder
    /// (CR 601.2f) — drained into `GameState.cost_modifiers` by
    /// `run_card_script`. `controller` on each queued `CostModifier` is a
    /// placeholder (`PlayerId(0)`) until drain time — the builder has no
    /// `&GameState` to read this source's live controller from inside a Lua
    /// callback, same reason `pending_static`'s `timestamp` is a placeholder
    /// too. The drain step resolves the real value there instead.
    pub(crate) pending_cost_modifiers: Vec<(CostModifier, Option<crate::types::zone::ZoneKind>)>,
    /// Same idea as `pending_cost_modifiers`, for `Game.CreateHandSizeModifier`'s
    /// builder (CR 402.2) — drained into `GameState.hand_size_modifiers` by
    /// `run_card_script`. `controller` on each queued `HandSizeModifier` is a
    /// placeholder (`PlayerId(0)`) until drain time, same reason and same
    /// fix as `pending_cost_modifiers`'s own placeholder `controller`.
    pub(crate) pending_hand_size_modifiers: Vec<(HandSizeModifier, Option<crate::types::zone::ZoneKind>)>,
    /// Same idea as `pending_hand_size_modifiers`, for `Game.
    /// CreateHandRevealModifier`'s builder (CR 701.20a/303.4m) — drained
    /// into `GameState.hand_reveal_modifiers` by `run_card_script`. Same
    /// placeholder-`controller`/`timestamp` pattern.
    pub(crate) pending_hand_reveal_modifiers: Vec<(HandRevealModifier, Option<crate::types::zone::ZoneKind>)>,
    /// Same idea as `pending_hand_reveal_modifiers`, for `Game.
    /// CreateDrawLimitModifier`'s builder (CR 121.2b) — drained into
    /// `GameState.draw_limit_modifiers` by `run_card_script`. Same
    /// placeholder-`controller`/`timestamp` pattern.
    pub(crate) pending_draw_limit_modifiers: Vec<(DrawLimitModifier, Option<crate::types::zone::ZoneKind>)>,
    /// Same idea as `pending_draw_limit_modifiers`, for `Game.
    /// CreateLandPlayModifier`'s builder (CR 305.2) — drained into
    /// `GameState.land_play_modifiers` by `run_card_script`. Placeholder-
    /// `controller` only (no `timestamp` - see `LandPlayModifier`'s own doc
    /// comment for why it has none).
    pub(crate) pending_land_play_modifiers: Vec<(LandPlayModifier, Option<crate::types::zone::ZoneKind>)>,
    /// Backlog item 9: a coroutine `Thread` suspended mid-`invoke_operation`
    /// at a `Game.SelectTargets` call, keyed by the same opaque
    /// `ScriptHandle` space `scripts` uses (shares `next_handle`, kept in a
    /// separate map since a suspended thread is consumed exactly once by
    /// `resume_operation`, unlike a loaded `Function` handle which is
    /// reusable). Same "`RegistryKey`, not the borrowed value itself"
    /// rationale as `scripts` — confirmed via a standalone spike
    /// (`phase5_coroutine_spike.rs`) that `Thread` round-trips through the
    /// registry the same way `Function` already does.
    pub(crate) suspended_threads: HashMap<ScriptHandle, RegistryKey>,
}

impl ScriptRegistry {
    pub(crate) fn next_script_handle(&mut self) -> ScriptHandle {
        let handle = ScriptHandle(self.next_handle);
        self.next_handle += 1;
        handle
    }
}
