# 99-backlog

## Live TODOs / unresolved gaps

- **Godot adapter (`phase5/godot-adapter`, B6)**: see `20-godot-adapter.md`. Open items:
  - Decision 009's "empty array on a wrong-thread call" holds only with gdext's own check compiled out. By default gdext
    panics first and GDScript gets `null`. Settle whether hosts are told to expect `null`, or the release build turns
    the gdext check off.
  - `create` serves only the proof card set until the carddb-backed `CardSource` exists.
  - Export (copy step into `res://`, macOS `.framework`), web, hot reload and the late-join snapshot are untouched.
  - The smoke test needs a Godot binary, so CI does not run it.
- **Proof game (`phase5/proof-game-bot`, B5)**: see `19-proof-game.md`. Fixed on the way: creatures outside the
  battlefield could attack and block, simultaneous deaths took ids in `HashMap` order, and a script with no
  `register_abilities` raised a `script_error`. Open items:
  - The bot declines every `may_cast` Decision, and it cannot pay a sacrifice, discard, tap or graveyard-exile cost,
    hybrid, Phyrexian, snow or `{X}` costs.
  - Other unordered `HashMap` walks may still decide an order a host sees. Grep `objects.values()` before trusting a
    new replay test.
  - No card in the set uses a sacrifice or discard cost, menace or a must-attack rule, so the engine atomicity item
    (a bad Answer that passes the facade checks) is untested by the bot.

- **Host Session (`phase5/host-session`, B3)**: see `18-host-session.md`. Open items:
  - No replay export and no carddb-backed `CardSource`.
  - A Payment Decision lists additional costs without their filters.
  - A panic inside the engine is not caught, and a pipeline error is fatal with no rollback.

- **Seat visibility (`phase5/seat-visibility`, B4)**: see `18-host-session.md`. Open items:
  - No `(Seat, state)` snapshot request. A Seat that joins late cannot learn the current state.
  - The engine writes no `Revealed` event and sets no `face_down`, so those two rules only have constructed-event tests.
  - Outboxes grow until a Seat sends a request.
  - Event ids other than the ones `view.rs` checks (trigger and replacement sources) are assumed to lie in public zones.
  - The engine's event-writing notes still lack the rule that free-text fields never name a hidden card.

- **Legal actions (`phase5/legal-actions`, B2)**: see `17-legal-actions.md`. Open items:
  - Target candidates ignore the announced X, so a `mana_value` filter that mentions X sees 0.
  - `DeclareBands` is not offered at a declare attackers Decision. It stays a direct `submit_action`.
  - A modal spell is listed when enough single modes are usable. Two modes that cannot combine are rejected only when
    the engine runs the cast.
  - Timing and zone permissions granted by continuous effects ("as though it had flash") are missing from the
    commands too, so entries inherit that gap.
  - The declare attackers Decision is offered only while the active player holds priority, and the declare blockers
    and damage Decisions come before priority. A host that mixes `submit_action` with `answer` can skip them.
- **Decision/Answer facade (`phase5/decision-answer-facade`, B1)**: see `16-decision-facade.md`. Open items:
  - `submit_action` still accepts any action from the priority player. The facade now checks a priority, attack, block
    or damage Answer against the listed entries (B2, `17-legal-actions.md`). What the entries cannot see still fails
    during `step()`: a pair of modes `combine_chosen_modes` cannot combine, menace, a "must attack" requirement, and
    the split rules the engine checks on its own. The engine atomicity item has to close that before a remote host
    sends untrusted Answers.
  - A nested mana ability activated during a Payment Decision (CR 605.3a) is not accepted through `Answer`. It still
    works through `submit_action`.
  - `Target::Ability` candidates are not enumerated for a script `SelectTarget` filter.
  - Rejection messages are the `Debug` text of the engine's error enum. B3 passes them through unchanged, so
    friendlier wording is still open.
  - The Decision id counter stays on `Engine` (B3 decided against moving it). Mixing `open_decision`/`answer` with
    direct `submit_*` calls on the same `Engine` leaves the counter out of step. The Session never does.
  - Unit tests cover the shapes, not every pause variant. The exhaustive match in `decision/open.rs` is what guards new
    variants.

- **Modal multi-word modes (`phase5/modal-multi-word-modes`)**: the only slice of the modal series that changes the data
  model. `AbilityEffect::Modal(Vec<Option<AbilityEffect>>)` is now `Modal { effects, words }`. The target words of all
  chosen modes still form one flat `TargetSpec::Multiple`, and `words[i]` says how many consecutive words mode i owns
  (1 for no requirement, which is the CR 700.2c placeholder, or for a `Primitive`; n for a mode whose own requirement
  is a `Multiple` of n words). `combine_chosen_modes` flattens a mode's own `Multiple` instead of rejecting it (an
  empty `Multiple` is still an error), and the trigger path gets this for free because it shares the function. The
  opponent-chosen spell path builds `words = vec![1; n]` (its modes are all target-less). In `resolve/modal.rs` the
  loop slices the per-word legal chunks by `word_range`, builds the mode's view with a `Primitive` (one word), a
  `Multiple` (several) or no requirement (the placeholder), and passes the mode's own chunk slice to the resolver,
  so `resolve_fight_effect` and `resolve_exchange_text_boxes_effect` read the right pair with no change.
  `ModalContinuation` carries `words`. `ExchangeTextBoxes` joins `is_modal_resolvable` (it is synchronous). Cast-time
  validation needed no change: it already works on the flat list, so too few targets are rejected. Tests:
  `phase5_modal_multi_word_modes.rs` (8): an exchange mode with gain life, the same with the exchange second (a
  non-zero word offset); three modes with spans of two, one and one (fight, tap, lose life) each reading their own
  words; two multi-word modes (exchange and fight) each reading their own pair; a fight mode whose word left before
  resolution does nothing while the other mode still runs; too few targets and an empty `Multiple` rejected at cast
  time; and a two-word fight on a modal triggered ability. `phase5_fight_bite.rs`'s rejection test became a positive
  one, and the four "unsupported kind" rejection tests now use `Cascade` (a keyword ability's effect, not a
  card-text mode). Left open: a mode that owns `up_to` or `at_least` words next to other modes relies on the
  existing backtracking assignment of targets to words (it is not retested per mode); the scenario TOML still has
  no field for a mode's own effect or target requirement; modal activated abilities (no modes in
  `commands/activate.rs`); nothing is rejected at cast time any more except the non-mode kinds (`ForEachPlayer`,
  `Cascade`, `MadnessCastOrDiscard`, `ReturnSelfWithCounter`, `AddMana*`, `Script`, a nested `Modal`).

- **Modal unless-paid (`phase5/modal-unless-paid`)**: `UnlessPaid` joins `is_modal_resolvable`, for both consequences
  (`Counter`, `SacrificeSelf`). No engine change was needed beyond the gate: `submit_unless_paid_payment` and
  `decline_unless_paid` both end in the standard `ReplacementCheck` tail (the sacrifice consequence through
  `ScriptedZoneMoveThen::StackResolution`), so `modal_continuation` runs the remaining modes after the payer answers.
  Tests: `phase5_modal_unless_paid.rs` (7): a spell mode that counters unless P1 pays {3}, declined (countered, next
  mode runs) and paid (spared, mana deducted, next mode runs); a rejected plan (`InvalidPlan`) and a wrong payer
  (`NotCurrentPayer`) leave the pause and the remaining modes intact, then a decline still finishes; an unless-paid
  mode followed by a destroy that pauses on a CR 616.1 choice (two pauses in a row); a synchronous mode printed
  before the unless-paid mode; and an echo-style trigger mode (sacrifice this unless you pay {2}) declined (source
  sacrificed, next mode runs) and paid. Left open: the other `UnlessPaidConsequence` shapes do not exist yet (no
  "pay life", discard or sacrifice cost, and ward's cost stays mana only); a modal ward is not a thing (ward is a
  triggered ability, not a mode); the `payer_queue` is still a single payer; `ExchangeTextBoxes` is the last kind
  rejected at cast time (slice 5). Test-harness note: the mana pool empties as the game leaves pre-game for upkeep,
  so a trigger test has to add mana after the trigger is on the stack.

- **Modal abilities (`phase5/modal-abilities`)**: `BecomeCopy` and `Attach` join `is_modal_resolvable`; `SacrificeSelf`,
  `Fight` and `Bite` were already in it. They act on the ability's own source permanent, so they only mean something on
  a modal triggered ability (a spell's source is not a permanent). The vehicle is a modal upkeep trigger
  (`RegisteredAbility::modes`). **There is no modal activated ability**: `commands/activate.rs` has no modes (its CR
  700.2b TODO is still open), so that is its own feature. No engine change was needed beyond the gate. Tests:
  `phase5_modal_abilities.rs` (11): sacrifice self before and after a synchronous mode, before a draw; a fight mode
  (the source and the target both take damage) and a bite mode (one-sided), each with `GainLife`; a fight followed by
  a draw (the damage is committed before the draw, checked by ledger order); a become-copy mode (`BecameCopy` logged,
  `copiable_override` set); an attach mode on an Equipment source; and the trigger-path chosen-by-opponent case with
  sacrifice self plus gain life, a destroy-all that pauses on CR 616.1 choices before gain life, and an unsupported
  kind rejected as `TriggerModeChoiceError::UnsupportedModeCombination`. Left open: a mode with two target words
  (`Fight` between two chosen creatures, slice 5); `Attach` for an Aura's enchant-host choice and Equip's cost-side
  rules are not exercised (only the resolution step); a modal ability with `Search` or a token mode is not retested
  here (the spell path covers it); `UnlessPaid` and `ExchangeTextBoxes` are still rejected.

- **Modal choice-pause modes (`phase5/modal-choice-pause-modes`)**: `Search` and `CreateToken` join
  `is_modal_resolvable`. No engine change was needed beyond the gate: both end their pauses in the standard
  `ReplacementCheck` tail, so `modal_continuation` resumes the remaining modes after the answer. Tests:
  `phase5_modal_choice_pause_modes.rs` (9): a search then a synchronous mode, the reverse, a search with no
  matching card (no pause), a search that fetches a Devour creature (two pauses: the find, then the nested
  `AwaitingSearchDevourChoice` through `SearchResume`), a token mode with a synchronous mode on either side, an Aura
  copy token with two legal hosts (`AwaitingTokenAttachChoice`), a search followed by an Aura token (two pauses in a
  row, so the continuation is installed again for the second), and a Treasure token before a search (the first mode
  is committed before the search pauses). Left open: Amplify's choice mid-search as a mode (same path as Devour,
  untested); a mode that makes tokens for a count above one with an attach choice on each (the loop is the single
  effect's own); `UnlessPaid`, `BecomeCopy`, `Attach` and `ExchangeTextBoxes` are still rejected (slices 3 to 5).

- **Modal spell coverage (`phase5/modal-spell-coverage`)**: tests only for the modal gate, plus one fix. The ten
  mode tests in `phase5_modal_spell_coverage.rs` found no engine bug: `Discard` (a pause by the targeted player, and
  an empty hand that does not pause), `Surveil`, `Regenerate` (a shield from an earlier mode stops a later destroy,
  and printed after the destroy it is too late), `Counter` (alone and followed by a destroy that pauses on a CR 616.1
  choice), and the chosen-by-opponent path (`submit_mode_choice`) with a draw, a destroy-all that pauses, and an
  unsupported kind rejected as `UnsupportedModeCombination`. The one bug: the scripted zone-move resolvers in
  `resolve/destruction.rs` (and `resolve_discard_random_effect`) passed `Vec::new()` as trailing events, which
  dropped the accumulated `events`. For an instant that carried its own Stack -> Graveyard `ZoneChange`, so a
  `Destroy` instant never logged that move. Each now takes `events` and passes it on, like `Mill` already did.
  Tests: 11 (10 mode tests and the ledger probe). Left open: `SacrificeSelf`, `Fight` and `Bite` as modes are still
  untested because a spell's source is not a permanent (`phase5/modal-abilities`).

- **Modal resolvable ops (`phase5/modal-resolvable-ops`)**: closes the "not usable as a mode of a multi-mode spell"
  note that the tap/untap, mill/lose life/sacrifice self, reanimation, fight/bite and exile-until-leaves entries
  below all carry. `AbilityEffect::Modal` no longer has its own inline copy of six effects. The new
  `engine/resolve/modal.rs` hands each chosen mode to the normal `resolve_ability_effect` against `mode_view`, a
  clone of the stack object with that mode's effect, its own target requirement (the CR 700.2c placeholder becomes
  `None`, so `affected_player` still means "the controller") and its own legal targets. `StackObject`
  and `CostPayment` derive `Clone` for this. A mode that moves cards, draws or waits for a choice ends in the usual
  `ReplacementCheck` -> `CommitEvents` tail, which always carries `StackObjectResolved`. While later modes remain,
  `Engine::modal_continuation` (`ModalContinuation`) holds them, and `resolve_commit_events_state` keeps that marker
  out of the ledger and calls `resume_modal_resolution` instead of leaving for the SBA loop (CR 608.2c: nothing
  runs between modes). That is how CR 616.1 ordering pauses, draw replacements and scry choices resume with no change
  to any resolver. Pending events from earlier synchronous modes (damage, counters) are flushed through the same
  pipeline before a pausing mode starts, so they land first. `is_modal_resolvable` is now: the synchronous effects
  (`GainLife`, `Damage`, `Counters`, `Pump`, `GainControl`, `Shuffle`, `Regenerate`, `LoseLife`, the six tap/untap
  forms, `Counter`, `Fight`, `Bite`), every scripted zone move (`Sacrifice`, `SacrificeSelf`, `SacrificeRandom`,
  `Destroy`, `DestroyAll`, `Exile`, `ExileSelf`, `ExileAll`, `ExileUntilSourceLeaves`, the three return-to-hand
  forms, both return-to-battlefield forms, `Mill`), `DrawCards`, `Discard`, `DiscardRandom`, `Scry`, `Surveil`.
  Tests: `phase5_modal_resolvable_ops.rs` (14). `phase5_modal_multi_mode.rs`'s rejection test used `Destroy`, which is
  supported now, so it uses `ExchangeTextBoxes`. Left open: **not supported as a mode** and still rejected at cast
  time: `Search` (its devour choices re-enter the search's own entry loop), `UnlessPaid` (payment pause),
  `CreateToken`, `BecomeCopy`, `Attach`, `ExchangeTextBoxes` (pairs two targets); `ForEachPlayer`, `Cascade`,
  `MadnessCastOrDiscard`, `ReturnSelfWithCounter`, `AddMana*` and `Script` are not card-text modes. **Untested as a
  mode**: `Discard`, `Surveil`, `Counter`, `Regenerate`, `Fight`/`Bite` (a spell's source is not a creature, so only
  an ability could use the one-word form, and a modal ability is not tested), `SacrificeSelf`, and the
  chosen-by-opponent paths (`submit_mode_choice`, `submit_trigger_mode_choice`) with the new kinds; they share the
  gate and the resolver. The mode loop reads each mode's legal targets once, before the first mode runs, so a mode
  that removes a later mode's target is not noticed until that mode's own resolver skips the stale id. A pause kind that never reaches `CommitEvents` with the
  stack object's `StackObjectResolved` would drop the remaining modes; `resolve_stack_top` clears the continuation
  at the start of each resolution so a stale one never leaks into the next. The destruction resolvers' dropped
  `events` were fixed in `phase5/modal-spell-coverage`; the modal loop still flushes first so damage and counters
  land before a later zone move.

- **Ward (`phase5/ward`)**: CR 702.21. New `GameEventKind::BecameTarget { object, by, by_controller }`, logged by
  `Engine::log_became_target` once per distinct battlefield object a stack object targets (CR 603.2c). It fires
  from `submit_payment` right after `SpellCast` (every cast and activation passes through there, so it also
  covers a free cast) and from `push_trigger_stack_object` for triggered abilities, never at the proposal step,
  so an unwound cast logs nothing. `by_controller` is on the event because a `ConditionPrimitive` only sees the
  event. `ConditionPrimitive::BecomesTargetOfOpponent` (`Game.Conditions.BecomesTargetOfOpponent`) matches it
  for `source` when `by_controller` differs from the trigger's controller. `PendingTrigger` gained
  `triggering_stack`, filled from `BecameTarget::by`. `TargetFilter::TriggeringStackObject` (`{ type =
  "triggering" }` in Lua and scenario TOML) is how the ward trigger targets the spell or ability that caused it:
  `push_ordered_triggers` fills the target itself (`Target::Object` for a spell, `Target::Ability` for an
  ability, via the now `pub(crate)` `is_ability_stack_entry`) and raises no CR 603.3d pause. If that stack object
  is gone the trigger is dropped. Legal at resolution while the target is still on the stack, so the CR 608.2b
  recheck fizzles it otherwise. The counter is the existing `AbilityEffect::UnlessPaid` with `Counter`, and the
  payer is the targeted stack object's controller. `Game.RegisterWard(source, cost)` (a Lua helper like
  `Game.RegisterKeyword`) registers one instance; two calls are two triggers (CR 702.21b). A trigger that
  targets a ward permanent needed one more piece: events logged while triggers are being put on the stack were
  never scanned, so `push_ordered_triggers` now records `Engine::recollect_triggers_since` and re-enters
  `CollectTriggers` when its queue empties (only the `BecameTarget` events set it, and a ward trigger targets a
  stack object, so it cannot loop). Also fixed the known bug from the hexproof/shroud entry:
  `submit_script_choice` now passes the resolving object's real `source` to `target_is_legal` instead of
  `stack_id`, so protection from a quality is checked against an ability's source permanent. Tests:
  `phase5_ward.rs` (13, the `Game.SelectTargets` one fails on the old code). Existing scenario
  `phase5_targeting_basics.toml` went from `ledger_len` 18 to 19 for the new event. Deviations from the plan: the
  helper is `Game.RegisterWard`, not `Game.Keywords.Ward`; the event carries `by_controller`. Left open: only a
  mana cost (no "Ward—Pay N life", discard or sacrifice, and no X); a spell copy (CR 707.10) is created without
  going through `submit_payment`, so it logs no `BecameTarget` (**TODO**), and a copy that retargets onto a ward
  permanent does not trigger it; no retargeting effect (CR 115.7) exists to test; ward can only be registered
  when the card's script runs, so it cannot be granted by a continuous effect; players get no event (no player
  hexproof, CR 702.11c); "opponent" is any other player; the trigger source is not rechecked, so a ward
  permanent that leaves before its trigger resolves still counters (that matches CR 702.21a); not usable as a
  mode of a multi-mode spell (`is_modal_resolvable` unchanged). Test-harness note: a cast spell is a new object
  (CR 400.7), so read the spell's id off `state.stack`, not from the hand id; two ward triggers for one
  controller pause on `AwaitingTriggerOrderChoice`.

- **Exile until it leaves the battlefield (`phase5/exile-until-leaves`)**: `AbilityEffect::ExileUntilSourceLeaves`
  (`Game.Ops.ExileUntilSourceLeaves`, bare tag), targeted and battlefield-only. It runs as
  `ScriptedZoneMoveKind::ExileUntilLeaves { source }`, which shares the `Exile` commit arm and pushes a
  `LinkedExile { source, exiled }` onto the new `GameState::linked_exiles` when the card really lands in exile. CR
  610.3a/b: if `top.source` is no longer a battlefield object when the ability resolves, nothing is exiled. The
  return is `Engine::return_linked_exiles`, called from `finish_sba_loop` once a pass finds nothing to do: every
  link whose source is gone goes back through `continue_scripted_zone_move(Reanimate { controller: None })` as one
  batch (CR 610.3c owner control, 610.3d simultaneous), then `ScriptedZoneMoveThen::SbaLoop` re-enters the SBA loop.
  Replacement checks, CR 616.1 pauses, enters triggers and entry modifiers are the reanimation ones. It is not a
  trigger and never uses the stack. `retire_object` drops a link when the exiled card leaves exile some other way.
  A token ceases in exile, so nothing returns. Tests: `phase5_exile_until_leaves.rs` (19). Left open: **cleanup
  step**: `finish_cleanup_sba_check` does not call `return_linked_exiles`, so a source that leaves during an SBA pass
  in the cleanup step returns its cards at the next `SbaLoop` pass instead; no phasing form (CR 610.4); no untargeted
  form ("exile each ..."), no graveyard source and no durations other than "source leaves" (until end of turn,
  until your next turn); no Oblivion Ring style form, which is a separate leaves-the-battlefield trigger linked to
  the exiled card (CR 607 linked abilities) and would need a way for a script to name "the exiled card"; no
  "returns tapped / with a counter / attacking" options; an Aura with several legal hosts stays in exile (the
  reanimation gap); the batch return is one card after another, not one simultaneous enters event (CR 603.6a);
  not usable as a mode of a multi-mode spell (`is_modal_resolvable` unchanged). The `ExileSelf` TODO for "until it
  leaves" is closed only for the targeted form. Test-harness note: a replacement effect's `source` must be a real
  object in the state, and `activate` needs `ability_index: u32`.

- **Fight and bite (`phase5/fight-bite`)**: `AbilityEffect::{Fight, Bite}` (`Game.Ops.Fight`, `Game.Ops.Bite`, bare
  tags). One resolver, `resolve_fight_effect` (`resolve/direct_effects.rs`), with a `mutual` flag. The creatures come
  from the target spec: a two-word `TargetSpec::Multiple` (first acts on second, read from `modal_chunk_legals`),
  one count-2 word (Fight only), or one count-1 word (the ability's `top.source` is the first creature). Each
  creature is the source of its own `DamageDealt` event (`is_combat_damage: false`, CR 701.14d), so the existing
  `apply_damage_event` and replacement pipeline give deathtouch, lifelink, wither, infect, protection prevention and
  "deals damage" triggers with no second pipeline. Power comes from `commands::effective_power` (now `pub(crate)`),
  both amounts are fixed before either event is pushed, and power 0 or less pushes no event. Fighting itself is one
  event of twice the power (CR 701.14c); a bite on itself deals its power once. CR 701.14b: a creature that left the
  battlefield, stopped being a creature or became an illegal target means neither deals damage, and the spell is
  not counted as fizzled while any target is still legal. Also new: `s.target_requirement()` may return an array of
  requirement tables, which builds `TargetSpec::Multiple` (`decode_target_spec_table`); an empty table, a mixed
  table and a non-table entry are load errors. `Engine::build_target_requirement` and the scenario loader needed no
  change. Tests: `phase5_fight_bite.rs` (49). Left open: **TODO(CR 608.2h)** a self-source fight or a bite whose
  creature left the battlefield does nothing instead of using last known information for its power; no "fights up
  to one target creature" (`up_to`/`at_least` words), no "fights each creature", no damage based on something other
  than power; not usable as a mode of a multi-mode spell (`is_modal_resolvable` unchanged); a triggered or
  activated ability cannot declare two target words (`:SetTarget` builds one `Primitive`, count 1), so "target
  creature you control fights target creature you don't control" works as a spell only; the scenario TOML
  `[cards.target_requirement]` cannot declare two words (use the script array form); a two-word spec where either
  word is `up_to` or `at_least` does nothing; damage to planeswalkers and battles keeps the existing TODO in
  `apply_damage_event`. A Fight or Bite whose pair is a `Multiple` with more than two words does nothing.

- **Reanimation (`phase5/reanimation`)**: `AbilityEffect::{ReturnToBattlefield { control, tapped },
  ReturnAllToBattlefield { filter, zone, control, tapped }}` (`Game.Ops.ReturnToBattlefield(opts?)`,
  `Game.Ops.ReturnAllToBattlefield(filter, zone?, opts?)`, both call-form) and `EntryControl { You, Owner }`
  (CR 110.2a, default `You`). They feed the new `ScriptedZoneMoveKind::Reanimate { controller, tapped }`
  (Graveyard or Exile source, read at resolution). The proposed `ZoneChange` is replacement-checked, so redirects and
  CR 616.1 ordering pauses work. `commit_reanimate` then builds the new object, runs the card script and calls
  `apply_enters_battlefield_modifiers` (enters tapped, enters with counters, Bloodthirst, Escape counters). Enters
  triggers fire off the ledger event. Instants and sorceries stay put (CR 400.4a). An Aura with exactly one legal
  host enters attached (CR 303.4f); with none it stays (CR 303.4g). `commit_scripted_zone_move` now returns the
  entry events (`CounterAdded`, `Attached`) that its two callers add to `trailing`. Temporary mutation: before the
  replacement check the loop writes the entering player onto the graveyard object's `controller` so entry filters
  see them. `commit_reanimate` resets it to the owner first on every path (redirect, Aura skip, answered ordering
  pause); an instant or sorcery skip and a stale target never get the write. Tests: `phase5_reanimation.rs` (44).
  Also fixed: a bare `SetOperation(Game.Ops.Mill)` (any call-form op passed uncalled) now fails with "must be
  called" instead of registering as a custom function. Left open: **Aura with several legal hosts** (CR 303.4f)
  stays in the graveyard because there is no entry-attach pause outside a stack object or a search; Clone, Devour
  and Amplify choices (CR 707.6, 702.82a, 702.38a) are not raised, so such a card enters as itself; no "enters with
  an additional counter" option (CR 122.6), no "enters attacking" (CR 508.4), no "under target opponent's control",
  no "may", no Zombify-style haste or end-step exile rider; no hand or library source; the all-form enters cards
  one after another, not as one simultaneous event (CR 603.6a, 614.12), so an earlier arrival can affect a later
  one; not usable as a mode of a multi-mode spell (`is_modal_resolvable` unchanged). **Persist/Undying divergence**:
  `ReturnSelfWithCounter` is untouched and still logs its `ZoneChange` straight to the ledger, with no replacement
  check and no `apply_enters_battlefield_modifiers`; a reanimated card gets both. Folding Persist/Undying onto
  `Reanimate` (controller `None`, plus one counter) is possible later. Test-harness note: a replacement effect's
  `source` must be a real object in the state, and two simultaneous enters triggers for one controller pause on
  `AwaitingTriggerOrderChoice`.

- **Mill, lose life and sacrifice self (`phase5/mill-lose-life-sacrifice-self`)**: `AbilityEffect::{Mill(u32),
  LoseLife(u32), SacrificeSelf}` (`Game.Ops.Mill(n)`, `Game.Ops.LoseLife(n)`, `Game.Ops.SacrificeSelf` bare tag).
  `Mill` and `LoseLife` share one variant for "you" and "target player": no declared target spec means the
  controller, a declared spec means the first legal `Target::Player`, and a target that is gone or never chosen
  (an "up to one" spec) means nothing happens, never a controller fallback (`direct_effects::affected_player`).
  Mill adds `ScriptedZoneMoveKind::Mill` and commits through `commands::commit_zone_move`, so each card is a
  replacement-checked Library to Graveyard move (Leyline of the Void style redirects work, CR 616.1 ordering
  pauses mid-mill) and a redirect to the shared Exile zone lands correctly. Milling more than the library holds
  mills what exists (CR 701.17b) and is not a draw, so CR 704.5b never applies. `LoseLife` subtracts without
  clamping and pushes `LifeChanged { delta: -n }`, the first resolution-time producer of that event.
  `SacrificeSelf` uses the `Sacrifice` zone-move path (indestructible irrelevant, not destruction, Dies triggers
  fire, a stale source does nothing) and checks `effective_controller == top.controller` (CR 701.21a).
  `ForEachPlayer` accepts `Mill` and `LoseLife` as bare tags (a small kind-plus-ids merge, no restructure). The
  stale CR 701.17 citation for sacrifice was fixed to 701.21a in the `Sacrifice` doc comment, the
  `game-constants.md` row and the stub only. Tests: `phase5_mill_lose_life_sacrifice_self.rs` (32). Left open:
  no "whenever a card is milled" or "whenever you lose life" condition (nothing consumes `LifeChanged`); no
  "would lose life" replacement and no "can't lose life" (CR 119.8); `GainLife`, damage to a player and lifelink
  still push no `LifeChanged`; no "the milled card" follow-up (CR 701.17c/d), mill as a cost, dredge or rad
  counters; a replacement that changes how many cards a mill moves isn't modeled (the batch is fixed first); no
  computed "lose life equal to X"; no team life (CR 119.4a); not usable as a mode of a multi-mode spell
  (`is_modal_resolvable` unchanged). Echo's `UnlessPaidConsequence::SacrificeSelf` still goes through
  `resolve_sacrifice_effect` and does not make the CR 701.21a control check. Other stale CR 701.17/701.20
  sacrifice citations elsewhere in the repo were not swept. Test-harness note: a replacement effect's `source`
  must be a real object in the state, and `submit_replacement_choice` names the candidate by that source id.

- **Hexproof and Shroud (`phase5/hexproof-shroud`)**: `KeywordSet::HEXPROOF` (CR 702.11b) and
  `KeywordSet::SHROUD` (CR 702.18a), accepted by `AddKeyword`/`RemoveKeyword`/`IgnoreKeyword` (`"Hexproof"`,
  `"Shroud"`), the scenario `keywords` list and the carddb keyword table. `target_is_legal` rejects a
  battlefield object whose effective keywords (through `layers::effective_characteristics`, so a granted or
  removed keyword counts) include Shroud, or Hexproof when `caster` isn't the object's effective controller.
  That covers cast, activation (Equip too), the CR 603.3d trigger candidate scan, `Game.SelectTargets` and the
  CR 608.2b resolution recheck. The `Object`, `AnyTarget` and `Script` arms check it; players are untouched.
  New `attachment_target_is_legal` is the same logic without the hexproof/shroud check, used by the two `sba.rs`
  Aura rechecks, the Equipment recheck and `legal_aura_entry_candidates` (CR 303.4f), so a hexproof or shroud
  creature keeps its Auras and Equipment (an Aura permanent doesn't target, CR 115.1b) while protection still
  unattaches them (CR 702.16d). Untargeted effects (`DestroyAll`, `TapAll`, `ExileAll`, `*Self`) never call it
  (CR 601.2c). Tests: `phase5_hexproof_shroud.rs` (28), a carddb unit test and
  `crates/scenario/tests/hexproof_shroud_keywords.rs` (2). Left open: no hexproof/shroud on players (CR
  702.11c); no "hexproof from [quality]" (CR 702.11d-g, needs a parameter); nothing reads `ignored_keywords`, so
  "can be the target as though it didn't have hexproof" (CR 702.11e) isn't expressible; no team model, so
  "opponent" is any other player. Ward (CR 702.21) was the recommended next slice and is done in
  `phase5/ward`. Known bug, fixed in `phase5/ward`: `engine/choice_answers.rs:179` passed the
  resolving `stack_id` as the `source` argument of `target_is_legal` for a `Game.SelectTargets` answer. For an
  activated or triggered ability that id isn't a real object, so the protection-from-source lookup silently
  finds nothing. Hexproof and shroud use `caster`, so they aren't affected. Test-harness note: attach by
  setting `attached_to` and the host's `attachments` directly, then call `check_state_based_actions`.

- **Tap and untap effects (`phase5/tap-untap-effects`)**: `AbilityEffect::{Tap, Untap, TapSelf, UntapSelf,
  TapAll { filter }, UntapAll { filter }}` (`Game.Ops.Tap`/`Untap`/`TapSelf`/`UntapSelf` bare tags,
  `Game.Ops.TapAll(filter)`/`UntapAll(filter)`). Targeted forms act on the resolving ability's legal object
  targets, self forms on `top.source`, `All` forms on every `matches_effect_filter` battlefield object (no zone
  argument: only permanents have tapped status, CR 110.5d). All six are synchronous (`resolve/direct_effects.rs`,
  one shared `flip_tapped`) and never pause. CR 701.26a/b: only an untapped permanent taps and only a tapped one
  untaps, so a no-op flip changes nothing and pushes no event (CR 603.2e). Convention: the `tapped` flag is set
  in place and the `Tapped`/`Untapped` event pushed as already happened, like the tap-cost, attack and
  untap-step producers (not applied in `CommitEvents` like `Counters`). `All` filters read the raw
  `object.controller`; only `untap_step` resolves the layered controller. A stale source id does nothing (CR
  400.7). Tapping never touches `CombatState` (CR 506.4b). Tests: `phase5_tap_untap_effects.rs` (22). Left open:
  no Lua `becomes tapped`/`becomes untapped` condition (events exist, `ConditionPrimitive` has none, CR
  603.2e); no "would become tapped/untapped" replacement event and no stun counters (CR 122.1d, 614), so an
  untap always succeeds; no "can't be tapped"/"doesn't untap" query (CR 614.17); `phased_out` not modeled; not
  usable as a mode of a multi-mode spell (`is_modal_resolvable` left unchanged). Test-harness note: this slice's
  "all effects don't target" test uses protection from everything because hexproof didn't exist yet; hexproof
  and shroud are real now (`phase5/hexproof-shroud`), and `phase5_hexproof_shroud.rs` has the `TapAll` cases.

- **Untargeted exile (`phase5/untargeted-exile`)**: `AbilityEffect::ExileSelf` (`Game.Ops.ExileSelf`, bare
  tag) exiles the resolving ability's own source; `AbilityEffect::ExileAll { filter, zone }`
  (`Game.Ops.ExileAll(filter, zone?)`) exiles every `matches_effect_filter` object in the Battlefield or
  every graveyard. Both feed `continue_scripted_zone_move(Exile)`, so replacement checks, departed
  triggers, token cessation and `reset_for_new_object` are inherited. `resolve_return_all_to_hand_effect`
  and the new exile-all share one `objects_in_zone_matching` collector. A stale source id does nothing
  (CR 400.7). Zone arg rejects Hand/Library/Stack/Exile (`Exile` can't source from them). Tests:
  `phase5_untargeted_exile.rs` (12), including the redirect-by-replacement test the bounce slice lacked
  (the exile path is shared, so it covers both). Left open: "until leaves the battlefield" exile (CR 610.3),
  face-down exile, hand/library as a source. Test-harness note: a replacement effect's `source` must be a real
  object in the state, or `event_target_matches` never matches.

- **Untargeted return to hand (`phase5/untargeted-return-to-hand`)**: `AbilityEffect::ReturnSelfToHand`
  (`Game.Ops.ReturnSelfToHand`, a bare tag like `Destroy`) moves the resolving ability's own source;
  `AbilityEffect::ReturnAllToHand { filter, zone }` (`Game.Ops.ReturnAllToHand(filter, zone?)`) moves every
  `matches_effect_filter` object in the Battlefield, every graveyard or Exile. Both feed
  `continue_scripted_zone_move(Bounce)`, so replacement checks, departed triggers, token cessation and
  `reset_for_new_object` are inherited. A source that already left its zone has a new id and the self form
  does nothing (CR 400.7). Zone arg rejects anything `Bounce` can't source from. Tests:
  `phase5_untargeted_return.rs` (12). Left open: no test redirects the bounce with a replacement effect
  (generic path shared with Exile); hand/library as a source; other untargeted verbs (exile all, etc.).
  Test-harness note: fixture battlefield cards need `run_card_script` by hand (the off-battlefield sweep
  skips them), with one shared `ScriptRuntime` so ability ids don't collide.

- **Zone-move reset + graveyard/exile sources (`phase5/zone-move-reset-and-graveyard-return`)**: new
  `GameObject::reset_for_new_object` (CR 400.7/108.4: clears tapped/damage/counters/attachments/copy state,
  controller = owner) is called on the new object by `sba::commit_death`, `commit_zone_move` (battlefield
  departures), the Exile and Bounce arms, and reanimation (its inline copy was replaced). Last-known info
  is a separate clone, so look-back triggers are unaffected. `ScriptedZoneMoveKind::accepts_source_zone`
  lets `Exile` take battlefield/graveyard targets and `ReturnToHand` battlefield/graveyard/exile, reading
  the real zone at resolution; the commit arms use the event's `from`. Tests: `phase5_zone_move_reset.rs`
  (6). Left open: untargeted forms ("return this card from your graveyard", "return all"), hand/library
  as a source, and the spell-to-graveyard / library / hand moves still clone without a reset (they carry no
  battlefield state). The "older arms don't reset" note in the `phase5/return-to-hand` entry is closed.

- **Return to hand + `SetFunctionsFrom("Hand")` (`phase5/return-to-hand`)**: `AbilityEffect::ReturnToHand` /
  `Game.Ops.ReturnToHand()` (targeted, Exile-shaped) through a new `ScriptedZoneMoveKind::Bounce`
  (Battlefield -> owner's Hand, replacement-checked, departed triggers collected). The commit arm clears
  counters/damage/tapped/attachments/copy override and sets controller to owner on the fresh object; the
  hand/library sweep then registers its hand abilities/CDA (first real end-to-end use of
  `phase5/hand-library-script-registration`). `:SetFunctionsFrom` now accepts `"Hand"` on every builder;
  activate/trigger/register paths already honored a declared zone. Cost-shape Cycling detection kept as a
  fallback. Tests: `phase5_return_to_hand.rs` (10); `a_hand_function_zone_is_still_a_clear_error` became
  the Stack equivalent. Left open: battlefield targets only (no graveyard/exile -> hand), no untargeted
  "return all"/"return this", no replacement-redirect test of the bounce itself (generic path, shared
  with Exile), Stack/Library/Command still rejected by `SetFunctionsFrom`. The older Exile/Sacrifice/Destroy
  arms still don't reset counters/tapped/damage on the new object (pre-existing; only matters for a
  card later returned from the graveyard/exile).

- **Hand/library script registration (`phase5/hand-library-script-registration`)**: `Engine::
  sync_off_battlefield_scripts` sweeps Hand and Library too (was Graveyard/Exile), so a CDA or hand-only
  ability registers for a card that arrived by any producer (redirected death, mulligan return, fixture),
  once. `run_card_script_for` marks the id in `off_battlefield_scripts_run` so the draw/opening-hand/tutor
  producers and the sweep never double-register. `run_card_script` now discards undeclared abilities in
  the Library like Graveyard/Exile (Hand still keeps triggered/activated ones, as before). Behavior note:
  fixture hand cards now have scripts run too (`phase5_script_self_registration` counts battlefield-source
  abilities). Tests: `phase5_hand_library_script_registration.rs` (6). Left open: the bounce verb
  it named is done in `phase5/return-to-hand`. Library cards are all
  scripted once on the first trigger-collection pass (one eval per card); command zone still unswept.

- **Characteristic-defining statics (`phase5/characteristic-defining-statics`)**: closes the CR 604.3
  gap the last three entries left. `StaticAbilityBuilder:SetCharacteristicDefining()` marks a
  `Game.CreateStaticAbility` as a CDA; `run_card_script` registers it in every zone instead of filtering it
  to the battlefield (the static queue's tuple grew a `bool`; the other six queues are unchanged).
  `:Register()` enforces CR 604.3a via `validate_characteristic_defining`: only `AddSubtype` (layer 4) and
  `AddColor`/`SetColors` (layer 5), `self_only`, no condition, no limited duration, no `SetFunctionsFrom`.
  Power/toughness CDAs were already real and zone-independent via `PTValue::CharacteristicDefining` on the
  definition (`invoke_cda`), so they're out of scope. No new cleanup or sweep - zone change mints a new id,
  `retire_object` purges, the next script run re-registers. Also folds in the Anger "as long as you control
  a Mountain" test: a graveyard-declared static with `SetCondition(ControlsAtLeast(...))` registers once
  and tracks the board live. Tests: `phase5_characteristic_defining_statics.rs` (7). Left open: a CDA only
  registers where a script run happens - hand/library are covered by `phase5/hand-library-script-registration`
  now; still not the command zone or outside the game; layer-4 *card type* and color-identity CDAs
  (`color_identity.rs` TODO) aren't covered (604.3a lists only colors/subtypes/P/T); "functions in any
  zone" statics that aren't CDAs (a spell's own cost reduction) are still discarded off the battlefield.

- **Graveyard/exile statics (`phase5/graveyard-exile-static-functions`)**: closes the "statics/
  replacement effects/modifiers that function from graveyard/exile (Anger-style)" gap the previous
  entry left. `:SetFunctionsFrom("Graveyard"|"Exile")` is now on `StaticAbilityBuilder`,
  `ReplacementEffectBuilder` and the five modifier builders (cost/hand-size/hand-reveal/draw-limit/
  land-play) through the same `function_zone_name` helper. Unlike triggered/activated abilities these
  have no `AbilityId`, so no side map: each `pending_*` queue became `Vec<(T, Option<ZoneKind>)>` and
  `run_card_script` filters with one `take_functioning` helper (declared zone == source zone, undeclared
  == battlefield) instead of the old blanket `clear()` off the battlefield. No new cleanup: a zone
  change mints a new id (CR 400.7), so `retire_object`'s purge-by-source already removes the effect.
  Tests: `phase5_graveyard_exile_static_functions.rs` (10). Left open: CR 604.3 CDAs/"functions in any
  zone" statics still discarded off the battlefield (TODO in `run_card_script`); no test card exercises
  a `SetCondition` ("as long as you control a Mountain") on a graveyard static - the condition gate is
  the existing one, unchanged; Hand/Stack zones still rejected.

- **Graveyard/exile script registration (`phase5/graveyard-exile-script-registration`)**: closes the
  "no script-registration point for graveyard/exile abilities" gap the idempotence slice left. Scoping
  found 15+ separate graveyard/exile producers (death, discard, mill, resolution, exile costs, ...), so
  instead of hooking each, `Engine::sync_off_battlefield_scripts` sweeps once at the top of
  `resolve_collect_triggers_state` and runs the script of every graveyard/exile object not yet in
  `GameState::off_battlefield_scripts_run` (sorted by id for deterministic ability ids; pruned by
  `retire_object`). A script declares the zone with `:SetFunctionsFrom("Graveyard"|"Exile")` on the
  triggered/activated builders; the zone lives in `GameState::ability_function_zones` (side map, same
  reason as `script_ability_ids`: ~400 `RegisteredAbility` literals) via `ScriptRegistry::
  pending_function_zones` (index-parallel to `pending_triggered`). `run_card_script` keeps a declared
  ability only while the source is in its zone, and discards every *undeclared* ability in
  graveyard/exile (an ETB trigger must not go live there). `commands::activate` and `collect_triggers`
  honor the declared zone; `remap_abilities`/`retire_object`/`purge_granted_abilities` prune the map.
  Tests: `phase5_graveyard_exile_script_registration.rs` (9). Left open: `:SetFunctionsFrom` rejects Hand/Stack/etc. (hand abilities stay
  cost-shape detected); a card entering the graveyard mid-batch registers at the next trigger-collection
  pass, so its own ability can't react to events in that same batch; CR 604.3 CDAs unchanged.

- **Script registration idempotence (`phase5/script-registration-idempotence`)**: closes the gap
  Cascade's own entry below names ("making `register_abilities(c)` genuinely safe to call more than
  once per object"). Scoping found it was already live, not just a cast-time hazard: a card whose script
  ran on hand entry (draw/opening hand/tutor) and was then played registered every ability **twice**
  (`commands::remap_abilities` carried the hand-time abilities to the new id, then battlefield entry
  registered a fresh set), and every non-ability registration (static/replacement effect, cost/land-play/
  hand-size/... modifier) took effect *while the card was still in hand* and was left behind under the
  dead hand id afterwards — a drawn Exploration granted an extra land drop from hand and kept it
  after being played. Confirmed with a probe (`land_play_modifiers.len() == 2` after one play) before
  changing anything. Fix, deliberately without touching the ~400 `RegisteredAbility`/effect/modifier
  struct-literal sites: `GameState::script_ability_ids` (a side set of the `AbilityId`s
  `run_card_script` minted); `remap_abilities` drops those instead of carrying them (hand-built
  abilities — tokens' Clue/Food, copy-granted triggers, test fixtures — still carry, so nothing that
  relied on remap changed); `retire_object`/`purge_granted_abilities` prune the set too.
  `run_card_script` now gates by the source's zone: off the battlefield, static/replacement/modifier
  registrations are discarded (CR 113.6); on the stack, only `Cast`-condition triggers register
  (CR 601.2a); off the stack, `Cast` triggers are discarded (nothing left to match). The stack-time
  run is `Engine::set_pending_payment`, the single funnel for all four real `CastSpell` producers
  (normal action, Madness, Cascade, script-requested cast) - the alternative to the reverted
  `CommandOutcome::stack_entries` field, with no 13-site struct ripple. `Game.Conditions.Cast` was
  already Lua-exposed but undocumented; it is documented now, so a card script can declare its own
  Cascade-style cast trigger. Behavior change to know about: a script-registered ability no longer
  survives a zone change by carrying over — the script re-runs for the destination zone instead.
  Tests: `phase5_script_registration_idempotence.rs` (4). Left open: CR 604.3 characteristic-defining
  abilities and other "functions in any zone" statics are now discarded off the battlefield (marked
  TODO in `run_card_script`; no script-registered shape needs the exception yet), and abilities that
  function from graveyard/exile now do (`phase5/graveyard-exile-script-registration`, entry above).

- **Land-play limit (`phase5/land-play-limit`, merged)**: CR 305.2's "you may play an additional
  land each turn" effects (Exploration, Azusa, ...) — `commands::execute_play_land` hardcoded its
  once-per-turn check against a bare literal `1`. The TODO's own excuse ("no continuous-effect
  query API yet") was stale: `DrawLimitModifier`/`effective_draw_limit` (CR 121.2b) already
  provides the exact shape needed — a per-player numeric continuous-effect modifier list plus a
  query function, already wired into a real enforcement choke point (`commands::propose_draw`).
  Built `LandPlayModifier`/`effective_land_play_limit` as its sibling, **not** a copy: CR 305.2's
  land-play increases genuinely stack (two Explorations means 3 land plays, not 2 — the max of the
  two), so `effective_land_play_limit` sums every matching modifier's `additional` rather than
  taking the latest timestamp the way `effective_draw_limit`/`effective_max_hand_size` do — no
  `timestamp` field on `LandPlayModifier` at all, since summing needs no tiebreak. The "no cleanup
  when the source leaves the battlefield" gap this cluster shared with `DrawLimitModifier`/
  `HandSizeModifier`/`HandRevealModifier` is closed now too (`phase5/modifier-source-cleanup`) — see
  `docs/implementation/04-sba.md`'s own entry; `commands::retire_object` purges all four lists by
  `source` the same way it already purged `state.abilities`/`continuous_effects`/
  `replacement_effects`. Lua-facing `Game.CreateLandPlayModifier`/`Game.LandPlayAffected` mirrors
  `Game.CreateDrawLimitModifier`/`Game.DrawLimitAffected` exactly (`:SetAffected` + `:SetAdditional`
  instead of `:SetLimit`, same queue-and-drain, placeholder-`controller` convention). The doc gap this slice flagged (no `docs/lua` page or
  `stubs/game.lua` entry for the hand-size/hand-reveal/draw-limit builders) is closed now
  (`phase5/player-modifier-lua-docs`): `docs/lua/player-modifiers.md` covers all four builders.

- **"Target opponent" (`phase5/target-opponent`, merged)**: `TargetFilter::Player` used to accept
  any player unconditionally — "target player" and "target opponent" (CR 115.1) were
  indistinguishable, a real gap `phase5_aura_enters_attached_player.rs` had already worked around
  with a hand-written `PlayerScript` predicate rather than a real fix. Widened to `Player {
  controller: Option<FilterController> }`, same "You"/"Opponent"/"Any" vocabulary `Object`'s own
  `controller` field already uses — `legality.rs`'s `target_is_legal` already had `caster:
  PlayerId` in scope, so the check itself was a few lines. Three call sites threaded the new
  field through: the Lua decoder (`decode_target_filter_table`'s `"player"` arm now reads the same
  `controller` key the `"object"` arm already did), and the TOML scenario format's own `"player"`
  case (reused the shared `TargetRequirementSpec.controller` field that already existed for
  `"object"`). Added a sibling test proving the built-in shape reaches the same "1 legal
  candidate, no pause" outcome the old scripted-predicate workaround did, without a script.

- **Sacrifice-self cost + Treasure token (`phase5/sacrifice-self-cost`, `phase5/treasure-token`,
  both merged)**: two small slices found by re-checking stale "blocked on X" comments against
  current code (the same trap this file's own note below, at "A doc comment blaming a gap...",
  already names) rather than trusting them. `AdditionalCost::Sacrifice{filter}` could only check
  type/supertype/subtype/controller, never the paid object's own identity — a "sacrifice this
  artifact" ability (Clue, Food) could actually be paid by sacrificing any *other* matching
  artifact instead. Added `AdditionalCost::SacrificeSelf` (mirrors `TapSelf`/`DiscardSelf`,
  `Game.Costs.SacrificeSelf` on the Lua side) and switched Clue/Food onto it. Also fixed a real,
  separate bug found while touching the same token builder: Food's `CardDefinition` was missing
  its `{T}` cost component entirely — CR 111.10b is "{2}, {T}, Sacrifice this token," not just
  "{2}, Sacrifice." Second slice: `NamedToken::Treasure`/`Game.Tokens.Treasure` (CR 111.10a) —
  its own "blocked on mana-choice coroutines" comment was stale by the time it was checked:
  `AbilityEffect::AddManaChoice` (`phase5/mana-add-choice`, merged earlier) already resolves
  synchronously off the `ActivateAbility` action's own `chosen_colors` field, no coroutine/
  player-pause mechanism needed at all — confirmed by reading `commands/activate.rs`'s own
  `AddMana`/`AddManaChoice` arm directly before building anything. Built the same way Clue/Food
  already are (a hand-built `RegisteredAbility` in `engine/resolve/tokens.rs`, not a
  Lua-scripted ability) — `TapSelf` + `SacrificeSelf`, no mana cost, `AddManaChoice { count: 1,
  choices: ColorSet::all() }`.

- **Mass-choice effects (Balance, `phase5/mass-choice-effects`, merged)**: `Game.ChooseN`,
  `Game.Ops.SacrificeChosen`/`DiscardChosen`/`SacrificeRandom`/`DiscardRandom`/`ForEachPlayer` all
  landed — full account in `docs/implementation/12-coroutines.md`'s own entry. Follow-up slice closed
  every gap named at merge time: `Game.Ops.ForEachPlayer` now accepts a bare `SacrificeRandom`/
  `DiscardRandom` tag directly (no wrapper function needed for those two); `Game.Ops.DiscardRandom`
  gained a `filter` parameter matching `SacrificeRandom`'s own shape; `Game.ChooseN` gained
  `opts.at_least` (CR 601.2c/115.6 "N or more," mandatory minimum then unbounded) alongside
  `opts.up_to`; `Game.ChooseN`'s `filter` now accepts the same scripted-predicate shape
  `:SetTarget`/`target_requirement()` do, not just the flat option-bag table. Left open:
  - `Game.Ops.ForEachPlayer`'s bare-tag shape still only accepts `SacrificeRandom`/`DiscardRandom`
    — no other `Game.Ops.*` primitive can run per-player without a wrapping function; no real card
    needs a third one yet.

- **Cascade (CR 702.85a)**: real now (`phase5/cascade`) — three real new pieces, each
  scoped wider than Cascade alone on purpose:
  - `ManaCost::mana_value` (CR 202.3) — didn't exist anywhere in this engine before.
    Reuses `resolve_x` (already existed for X-cost payment) rather than duplicating
    X-handling: a raw unresolved `{X}` contributes 0 (CR 202.3e's off-the-stack
    default, correct for a library/hand/graveyard card that's never on the stack);
    an on-stack spell's own mana value must call `resolve_x` first. Hybrid uses the
    larger component (CR 202.3f); Phyrexian contributes 1 (CR 202.3g). CR
    202.3b/c/d (DFC back faces, melded permanents, split cards) are explicit
    DESIGN.md non-goals, untouched.
  - CR 601.2i's "when you cast this spell" trigger point — genuinely new: traced
    the full payment-completion path first and found every real `CastSpell` in this
    engine *always* pauses for an explicit payment acknowledgment (even a bare
    `{0}`, per CR 118.5a), so there's exactly one real "becomes cast" commit point
    to hook (`Engine::continue_cost_zone_moves`'s own tail, gated on `on_paid ==
    PaymentResolution::StackObject` + the object still being `StackEntryKind::
    Spell` — an activated ability's own cost completion shares the same
    `PaymentResolution` variant and must not fire this). `GameEventKind::SpellCast`
    is a plain self-referential marker (unlike `Cycled`/`MadnessExiled`, no LKI
    needed — the spell is still alive on the stack when this fires, so `Condition
    Primitive::Cast` is matched by `collect_triggers`'s ordinary live loop, no CR
    603.10 departed fallback). `since_ledger` is already correct for this for free —
    confirmed by reading `finish_sba_loop`'s own `CollectTriggers { since }`
    derivation before writing anything, not assumed — so no pipeline-exit
    restructuring was needed, just logging the marker at the right point.
  - `AbilityEffect::Cascade` itself — a synchronous library walk (no per-card
    player choice, unlike `Search`'s own candidate-queue shape) that always pauses
    at the end (`ProcessState::AwaitingCascadeChoice`) for the real "cast the
    candidate, or not" decision. Library moves aren't replacement-checked anywhere
    in this engine (confirmed against `Search`'s own found-card mover before
    building this one the same way) — CR 616.1 concern deliberately narrowed away,
    same posture that precedent already takes. The "cast" branch
    (`Engine::submit_cascade_cast`) reuses the same synthetic-`CastSpell`-via-
    `execute_command` shape `phase5/madness`'s `submit_madness_cast` established,
    plus a new sibling one-shot grant `GameState::pending_free_cast` (Madness's own
    `pending_timing_bypass` only waives *timing*, since its own cost is real and
    printed; Cascade's candidate was never printed with any cost permission at all,
    so the grant has to waive the *cost itself* too — synthesized as a transient
    `AlternativeCost::Replacement` with an empty `ManaCost` at the exact call site
    that already handles every other alternative-cost shape). `Engine::
    bottom_cards_in_random_order` (new, `engine/pregame.rs`, next to
    `shuffle_library`) reuses the same per-swap Fisher-Yates/`RngDrawn`-logging
    technique, scoped to just the exiled subset rather than the whole library.
    Narrowed, named: CR 702.85a's own second mana-value check (re-verifying the
    candidate's mana value hasn't changed between being exiled and being cast) uses
    the value already computed by the walk rather than re-deriving it — no real
    card in this corpus can change a card's mana value between those two points.
    CR 702.85b ("as you cascade" actions) stays unmodeled, no real card needs it;
    CR 702.85c (multiple Cascade instances trigger separately) needed no code at
    all, falls out for free from ordinary trigger collection.

  A fourth piece was **tried and reverted**: a generic `CommandOutcome::
  stack_entries` field (mirroring `battlefield_entries`) that re-ran a card's own
  `run_card_script` a second time once it hit the stack, so a script could
  self-register `Cast` directly (CR 601.2a: abilities exist from the moment a
  spell hits the stack, not just once it resolves). This broke a real, unrelated,
  already-passing test (`phase5_escape_provenance.rs`) — every existing card's own
  `register_abilities(c)` is written assuming it's called *exactly once* per
  object lifecycle; running it a second time at cast time, on top of the
  pre-existing battlefield-entry call, double-registered the Escape test card's
  own ordinary ETB trigger (`commands::remap_abilities` carries the cast-time
  registration across the stack→battlefield move, and the pre-existing
  battlefield-entry `run_card_script` call then registers an independent *second*
  copy on the same final object) — surfaced as a genuine CR 603.3b two-copy
  ordering-choice pause instead of a single clean trigger, not a silent no-op.
  Root-caused via direct instrumentation (not guessed) before reverting. This
  slice's own tests register the Cascade trigger via direct `RegisteredAbility`
  construction instead (predicting the cast spell's own deterministic stack-object
  id — `next_object_id` starts at 1000 in every `testutil::two_player_state`, and
  nothing else in these tests allocates an id first), same "no Lua wiring proven
  this slice" posture Escape/Madness/Flashback/Buyback already take for their own
  Lua-inaccessible pieces. Making `register_abilities(c)` genuinely safe to call
  more than once per object was a real, separate, bigger gap — closed since by
  `phase5/script-registration-idempotence` (see the entry at the top of this file).

  Also touched, incidentally: fixed a real pre-existing bug in `execute_cast_spell`
  surfaced by Cascade being the first-ever `from_zone: Exile` alternative-cost
  producer (Flashback/Escape/Buyback are all `Graveyard`, a per-player zone) — its
  own zone-removal call assumed every `from_zone` is per-player, but Exile (like
  Battlefield) is shared, so casting from exile errored ("actor has no such zone")
  until this slice's own test exercised it. Also updated four pre-existing
  scenario TOML fixtures whose `ledger_len` assertions hardcode an exact ledger
  count (`phase5_script_self_registration`/`phase5_targeting_basics`/
  `phase5_script_bridge_primitives`/`phase5_script_bridge_trigger`) — each now +1
  for the new `SpellCast` marker a real cast always logs; found by running the
  full scenario suite to exhaustion (it stops at the first hard failure per run)
  rather than assuming only one fixture was affected.

- **Madness (CR 702.35)**: real now (`phase5/madness`) — the static "discard
  -> exile instead" half needed no new engine mechanism at all: `ReplacementPayload::
  ReplaceZoneDestination(ZoneKind::Exile)` matched against `EventFilter::WouldMoveZone
  { from: Hand, to: Graveyard }` was already fully generic (the pre-existing "Leyline
  of the Void" pattern `Engine::continue_discard_moves`/`continue_cost_zone_moves`
  both already route every real discard producer through), and hand-only self-
  registration was already solved by the Cycling slice's `run_card_script_for`
  widening. Two genuinely new pieces: a provenance marker (`GameEventKind::
  MadnessExiled`/`ConditionPrimitive::MadnessExiled`, the same purpose-built-marker
  shape `Cycled` already established, so "when this card is exiled this way" doesn't
  fire off an unrelated exile-instead-of-discard effect) and a real "cast now, or put
  it in your graveyard" decision (`AbilityEffect::MadnessCastOrDiscard`, `ProcessState::
  AwaitingMadnessChoice`, `Engine::decline_madness_cast`/`submit_madness_cast`) — the
  first pause in this engine whose "yes" branch is a full nested cast (its own
  targets/modes/payment) rather than a canned `Ops.*` effect. `submit_madness_cast`
  reuses `commands::execute_command`'s `CastSpell` arm wholesale (same alternative-cost
  machinery Flashback/Escape/Buyback already share, this is just the first to combine
  `from_zone: Exile` with `exile_on_leaving_stack: false`) rather than inventing a
  second casting path. CR 702.35b's own timing permission (the resolving trigger's "you
  may cast it" text lets this happen outside the caster's normal sorcery-speed window,
  same footing Cascade/Suspend's own "cast without paying, ignoring timing" grants,
  though those remain unmodeled) is a new one-shot `GameState::pending_timing_bypass:
  Option<ObjectId>` consumed by `commands::cast::execute_cast_spell` — a side channel
  rather than a new field on `PlayerActionKind::CastSpell` (~87 construction sites, the
  same ripple class this project has repeatedly avoided for that enum).
  `madness_marker_event`'s own provenance check is deliberately narrow (`phase5/madness`'s
  own scoping conversation): it re-derives "was this card's own madness redirect what
  won" from the post-replacement, pre-commit state rather than threading which
  `ReplacementCandidate` actually won CR 616.1's ordering choice through the pipeline —
  wrong only if a *second*, independent exile-instead-of-discard effect also applies to
  the same card and wins instead, which no real card in this corpus creates. Found and
  fixed a real, previously-unreachable bug while building this: `execute_cast_spell`'s
  own zone-removal call (`state.zone_mut(from_zone, Some(actor))`) assumed every
  alternative-cost `from_zone` is per-player - true for Flashback/Escape's own
  `Graveyard`, but Exile (like Battlefield) is a shared zone, so casting from exile
  panicked-then-errored ("actor has no such zone") until this slice's own test exercised
  it, the first real `from_zone: Exile` producer. Lua builder for `AlternativeCost::
  Replacement` is closed since (`phase5/script-alternative-costs`, `s.alternative_costs()`); this
  slice's own tests still use direct `CardDefinition`/`RegisteredAbility`/`ReplacementEffect`
  construction. Effect-driven discards ("target player discards a card," e.g. Mind Rot) are a real
  producer now too (`phase5/discard-target-player`, `Game.Ops.Discard(count)`) - each targeted
  discard is a replacement-checked Hand->Graveyard move, so Madness applies to it. Madness is also
  exercisable via cleanup hand-size discard and a cost-payment `Discard`, both proven end to end in
  `phase5_madness.rs`.

- **Buyback (CR 702.27)**: real now (`phase5/buyback-additional-cost`) — `CardDefinition.
  buyback_cost: Option<ManaCost>`, `PlayerActionKind::CastSpell.pay_buyback: bool`, `StackObject.
  bought_back: bool`. Confirmed by reading the resolution pipeline directly (not assumed) that the
  destination-decision code and `Engine::move_resolving_spell` were already fully generic over the
  target zone (`Engine::unwind_payment` already proved Hand-return works, for CR 601.6) — so this
  slice only had to add the "announce and remember an optional additional cost" half, one line in
  `engine/resolve/permanent_entry.rs` for the hand-instead-of-graveyard redirect, and a mana-symbol
  concatenation in `commands::cast::execute_cast_spell`. Per CR 702.27a's own narrower wording (vs.
  Flashback's 702.34a), the redirect is checked only on normal resolution, never on the CR 608.2b
  fizzle path. Still unmodeled: Buyback combined with an alternative cost or a modal spell (no real
  card does this). The Lua builder for `buyback_cost` is closed since (`phase5/script-alternative-costs`,
  `s.buyback_cost()`).

- **Escape (CR 702.138)**: real now (`phase5/escape-alternative-cost`) — `AdditionalCost::
  ExileFromGraveyard`/`CostPayment::Exile`, the same N-distinct-payments shape `TapPermanents`
  already established, checked against the graveyard. Built on top of `AlternativeCost::
  Replacement` (CR 601.2b flashback/escape-style alternative costs), which turned out to already
  be fully real and generic from an earlier, undocumented slice — `phase5_flashback_alternative_
  costs.rs` was the pre-existing proof, just never logged in `docs/implementation/`; closed
  retroactively in `02-zone-moves.md`. `commands::zone_ops::propose_zone_move` gained a real `to`
  parameter (was hardcoded to `Graveyard`, since Sacrifice/Discard/cleanup-discard all happened to
  end there) so the exile-cost payment can propose a `Graveyard -> Exile` move through the same
  pipeline. CR 702.138b/c/d ("if this permanent escaped," "escapes with [counters/ability]") are real
  now too (`phase5/escape-provenance`) — `AlternativeCost::Replacement.escape: bool` (the only thing
  that structurally distinguishes Escape from Flashback, since both share `from_zone: Graveyard`),
  `StackObject::escaped` (baked in at push time, same contract `exile_on_leaving_stack`/`bought_back`
  already establish), `GameObject::entered_via_escape` (set by `Engine::move_resolving_spell` once a
  spell with `escaped: true` resolves to the battlefield — small ripple confirmed before building:
  `GameObject::new` is itself still `todo!()`, so there are only ~6 real hand-written constructors
  total, everywhere else clones an existing object forward). `InterveningIfPrimitive::Escaped` (CR
  603.4's "if it escaped" clause — Kroxa/Uro's own real card pattern is an *ordinary* ETB trigger
  with this as its intervening-if, a different and more common shape than 702.138c/d's own literal
  "escapes with" templates) required widening `InterveningIfPrimitive::holds`'s signature to take a
  `source: ObjectId` (previously only `controller` — no existing variant needed the object itself).
  702.138c ("escapes with N counters") is real too — `ReplacementPayload::EntersWithCountersEscape`,
  mirroring `EntersWithCountersBloodthirst`'s own no-player-choice gated shape exactly, checked
  against `entered_via_escape` instead of `opponent_damaged_this_turn` (needed a 4th, easy-to-miss
  update site beyond the two `is_battlefield_entry_modifier`-style match lists in `replacement.rs`:
  `script/builders.rs`'s own `validate_replacement_filter_payload_pairing` allowlist, caught
  immediately by a real Lua `:Register()` call erroring in testing, not silently). 702.138d ("has
  [ability] if escaped") needed **no new mechanism at all** — a static ability's own
  `StaticAbilityBuilder:SetCondition` (CR 611.2c, `phase5/conditional-continuous-effects`) already
  accepts any `InterveningIfPrimitive`, so `Game.InterveningIf.Escaped` plugs in directly, and
  `layers::applicable_effects_in_layer` already re-evaluates a condition fresh off the *live* object
  on every query (no CR 603.10 departed-ability concern, unlike the ETB-trigger shape above). No Lua
  builder for the alternative-cost declaration itself (only the `ExileFromGraveyard` cost component
  and now `escape`'s own consumers are Lua-exposed) — same scope Flashback's own slice already
  established. CR 702.138c's own second sentence (a linked "when it enters this way" trigger) stays
  unmodeled — no real card in this corpus combines Escape with one.

- **Cycling (CR 702.29)**: plain Cycling (702.29a) and Typecycling (702.29e) are real now
  (`phase5/cycling-ability`) — `AdditionalCost::DiscardSelf`, and `commands::activate::
  execute_activate_ability`'s zone check now allows `Hand` (instead of requiring `Battlefield`)
  when the found ability's own cost contains it — CR 602.1's own long-standing "unless the ability
  itself says otherwise — no such override exists yet" comment is closed by this. Also closed a
  bigger, separate gap found while scoping: every `run_card_script` call site in this engine was
  battlefield-entry-only, so a card drawn into hand could never have self-registered a hand-only
  ability at all — `Engine::run_card_script_for` is now called by all three real Library→Hand
  producers (turn-based draw, `Game.Ops.DrawCards`, tutor-to-hand) plus the opening-hand deal, so a
  real Cycling card actually works end to end from a fresh game, not just via direct
  `RegisteredAbility` construction. CR 702.29c/d ("when you cycle this card" / "cycles or discards"
  triggers) are real now too (`phase5/cycling-triggers`) — `GameEventKind::Cycled { object,
  last_known_info }`, a purpose-built marker event logged alongside (not instead of) the ordinary
  Hand→Graveyard `ZoneChange`, only when `Engine::submit_payment` finds `AdditionalCost::DiscardSelf`
  in the cost being paid (`cycling_source`, threaded through `continue_cost_zone_moves` and
  `ReplacementResumeTarget::CostZoneMove`) — CR 702.29f means Typecycling's own `DiscardSelf` cost
  counts identically, no separate mechanism needed. `ConditionPrimitive::Cycled` is a new
  self-referential sibling of `Discarded` (which fires on *any* discard of its source, over-firing
  for this narrower meaning) — reuses the exact same CR 603.10 departed/LKI fallback loop
  `Discarded` itself uses, since a cycled card's own "when you cycle this card" ability is purged
  from `state.abilities` the moment it leaves hand too. CR 702.29d itself needed no new work at all —
  `DiscardsCard`/`EachDiscardsCard` already match any Hand→Graveyard `ZoneChange`, and a cycling
  discard is exactly that shape; `phase5_cycling_triggers.rs`'s own
  `cycles_or_discards_condition_already_fires_for_a_cycling_discard` test proves that coverage
  directly. CR 702.29b's "ability persists in every other zone" is still only half-real — a card
  leaving hand for any zone still purges its `RegisteredAbility` like every other ability in this
  engine (unobserved today: nothing queries "does this object have an activated ability").

- **Persist (CR 702.79a) / Undying (CR 702.93a)**: both real now (`phase5/persist-undying`) — each
  keyword is a single, self-contained sub-rule with no further lettered clauses, so this slice closes
  both completely. Trigger collection needed no new work at all — `ConditionPrimitive::Dies` (CR
  603.10's own LKI-fallback loop in `triggers::collect_triggers`) already covers "when this permanent
  is put into a graveyard from the battlefield" exactly. The new pieces: `InterveningIfPrimitive::
  HadNoCountersOfKind(CounterKind)` for "if it had no [kind] counters on it" — checked against the
  object *as it was the moment it died* (`lki.counters`, threaded through `collect_triggers`'s
  departed loop at collection time, and through the new `StackObject::source_lki` — populated at
  `push_trigger_stack_object` via a new `triggers::find_departed_lki` reverse-ledger scan — for the
  CR 608.2a resolution-time recheck), not live state (the pre-death object no longer resolves by
  then). Deliberately did *not* widen `PendingTrigger` itself to carry the LKI forward — it derives
  `Copy`/`PartialEq` (`triggers_pipeline::is_permutation`/`group_apnap` both rely on it) and a
  `GameObject` can't cheaply support either; since the exact same event this trigger was collected
  from is already sitting in `state.ledger` by resolution time, `find_departed_lki` just re-finds it
  by scanning backwards for the matching `object_id`, reusing `collect_triggers`'s own departed-loop
  match shape. `AbilityEffect::ReturnSelfWithCounter(CounterKind)` (`engine/resolve/reanimation.rs`)
  is the actual "return it to the battlefield" mover — the first producer in this engine that moves a
  card graveyard→battlefield directly (Escape/Buyback both *cast* from the graveyard via the stack;
  neither teleports a permanent). Locates the graveyard object via `ObjectLineage::previous ==
  Some(top.source)` (the same link `sba::commit_death` already mints), mints a fresh CR 400.7 id,
  and explicitly resets `tapped`/`damage_marked`/`deathtouched`/`counters`/`attachments`/
  `attached_to`/`phased_out`/`copiable_override`/`copy_generation` — unlike `Engine::
  move_resolving_spell`/`sba::commit_death`'s own clones (whose source object was never meaningfully
  tapped/damaged/countered in the first place, on the stack or freshly dead), the graveyard object
  cloned here really can carry those over from the moment it died, so they needed an explicit reset
  this slice's two precedents didn't. Logs its own `ZoneChange` straight to the ledger rather than
  routing it through `replacement::apply_replacements`/`ReplacementCheck` — deliberately narrow, no
  CR 616 "would enter, redirect elsewhere" support for this specific producer and no
  `apply_enters_battlefield_modifiers`-family interaction (no real Persist/Undying card needs
  either) — but ordinary "when this creature enters" triggers still fire off it for free, since
  `CollectTriggers` scans the ledger tail regardless of how an event got there. The ability's own
  counter is a plain `CounterAdded` pushed through the normal pipeline (not a direct mutation),
  avoiding this file's own logged double-apply mistake. Lua-exposed (`Game.InterveningIf.
  HadNoCounters(kind)`, `Game.Ops.ReturnSelfWithCounter(kind)`) since both keywords are common enough
  real cards to warrant it, unlike some narrower first-proof slices in this file.

- **Layer 2 internal filtering**: `applicable_effects_in_layer`/`applies_to` still read raw `GameObject.controller`, not `effective_controller` — deliberate to avoid recursive layer-2 resolution.
- **Layer 1 (copy)**: CR 707.9's two most common real exceptions are real now (`phase5/copy-exceptions`) — `CopyExceptions { set_power_toughness, add_keywords }`, applied to the snapshot by both `AbilityEffect::BecomeCopy` and `TokenSpec::CopyOf` before it's frozen. CR 707.9c/d (characteristics not copied, original retained) is real now too (`phase5/copy-retain-exceptions`) — `CopyExceptions.retain_name`/`retain_color`, narrowed to those two most-common real shapes; `CopyExceptions::apply` reads the copying object's own true `GameObject.definition` (never overwritten by any copy machinery, distinct from the frozen `copiable_override` snapshot) as a new `original: Option<&CardDefinition>` parameter — `None` only for `TokenSpec::CopyOf`, where a freshly created token genuinely has no prior "own identity" to retain from, a deliberate tested no-op rather than a gap. CR 707.9e/g's shared schema prerequisite is real now too (`phase5/copy-generation`) — `GameObject.copy_generation: u32`, bumped by both real copy-application sites (`resolve_become_copy_effect`/`submit_copy_choice`), and `GameState.linked_ability_generation: HashMap<AbilityId, u32>` (a side-channel tag, not a `RegisteredAbility` field — 100+ construction sites, no `Default`), consulted by `triggers::collect_triggers` before a tagged ability is allowed to match at all — real, directly tested enforcement of CR 707.9g's "the ability doesn't trigger." CR 707.9a's "gain a whole new ability as part of copying" is real now too (`phase5/vesuvan-linked-copy-ability`) — `CopyExceptions.grant_recurring_copy_trigger`, `commands::grant_linked_copy_ability` (a hardcoded Rust `RegisteredAbility` shape for Vesuvan Doppelganger's own linked upkeep ability, the only real card that needs one), called from both real copy producers right after their existing purge+re-register+bump steps. Found while building it: `linked_ability_generation`'s own CR 707.9g supersession check (built in `copy-generation`) is structurally unreachable through *either* real producer — `purge_granted_abilities` deletes the previous granted ability outright (by source) before a re-copy re-registers anything, so there's never a stale-but-present ability left for the tag to invalidate; confirmed by direct test, not just reasoned about. Still unmodeled: CR 603.3d (mislabeled 603.3c at the time this was written — triggered abilities that require targets weren't choosable yet) used to mean the granted ability could never actually fire through real gameplay, only exist correctly in `state.abilities`; that's closed now too (`phase5/trigger-target-selection`) — see this file's own **Triggered abilities** entry below for the real mechanism, and `phase5_trigger_target_selection.rs`'s own `vesuvans_granted_upkeep_ability_now_really_fires_and_becomes_a_copy` test for Vesuvan's granted ability actually resolving end to end for the first time. CR 707.9e's "additional effect" exception (a different shape from 9a's "gain an ability") is real now too (`phase5/copy-additional-effect-exception`) — `CopyExceptions.enters_with_additional_counters: Option<(CounterKind, u32)>` (Spark Double's "enters with an additional +1/+1 counter"), applied via a new `commands::apply_copy_additional_counters` at both real copy producers, pushing a plain `GameEventKind::CounterAdded` rather than mutating `GameObject.counters` directly (reuses `CommitEvents`' existing generic apply path). 707.9e's own second sentence — the exception's effect doesn't happen if another copy effect supersedes the first *during the same entering event* (the rule's own Altered Ego/Clone chain example) — is real now too (`phase5/chained-copy-effects`), closing what every prior slice in this cluster named as a bigger, separate gap. Turned out to need no new `ProcessState`/`StepStatus` variant and no recursive call stack: `submit_copy_choice` already re-runs the copied definition's own script, which self-registers a fresh `EntersAsCopy` if the copied creature is itself a Clone/Altered-Ego-family creature; this slice just re-checks `pending_copy_choice` right after that (same synthetic-event shape the original raise site uses, so it works even though the object already sits on the battlefield) and re-enters `AwaitingCopyChoice` if a chain is found. `ProcessState::AwaitingCopyChoice` gained `carry_forward_if_declined: Option<CopyExceptions>` — a chain defers this round's own additional-effect exceptions (counters, and `grant_linked_copy_ability`'s granted ability alike, since CR 707.9e's own phrasing covers both, not just counters) rather than dropping them; if the next pause picks a real target the deferred exceptions are conclusively superseded and never read again, but if it's declined, no second copy effect was actually *applied*, so the deferred exceptions apply after all (`apply_copy_additional_effect`, a new shared helper) — caught by a failing test on the first implementation attempt, which discarded the deferred exceptions unconditionally the moment a chain was merely *offered*, silently eating Spark Double's own counter in the offered-then-declined case. A chain can go arbitrarily deep with no cap - each link needs its own external `submit_copy_choice` call, so there's no runaway-loop risk. Altered Ego's own X-costed variable amount (as opposed to Spark Double's fixed one) is real now too (`phase5/altered-ego-x-counters`) — `CopyExceptions.enters_with_additional_counters`'s amount widened from a bare `u32` to `CounterAmount { Fixed(u32), X }` (one field, not a parallel `Option`, so "both set" isn't representable); `X` resolves against the entering object's own `StackObject.x_value`, captured once when `ProcessState::AwaitingCopyChoice` is first raised (`continue_permanent_entry`) and carried unchanged through any chained re-pause — the X belongs to the entering permanent itself (paid once, at cast time), not to whichever candidate is copied at any one link, confirmed by a direct test chaining through a declined copy and still landing the original X. `resolve_become_copy_effect` (`BecomeCopy`, Vesuvan's own producer) gets the identical `top.x_value` plumbing for symmetry, though no real card exercises `CounterAmount::X` through that path. `decode_copy_exceptions` accepts `counter_amount = "X"` (a string sentinel) alongside the existing numeric case, reading a raw `mlua::Value` first since a bare `.get::<u32>()` would silently fail to match a string; errors on anything that's neither a non-negative number nor `"X"`. `resolve_become_copy_effect` (`BecomeCopy`, CR 707.4) isn't given the same chaining treatment either - it doesn't move zones and has no existing chain-detection hook to extend, a narrower sibling gap named rather than silently extended into. The ETB-replacement "enters as copy" path (Clone itself) is real now too (`phase5/clone-enters-as-copy`, CR 707.6/614.12a) — `ReplacementPayload::EntersAsCopy`, `replacement::pending_copy_choice` (candidates are every battlefield creature, any controller, no `controller` parameter at all unlike Devour/Amplify), `ProcessState`/`StepStatus::AwaitingCopyChoice`, `Engine::submit_copy_choice(Option<ObjectId>)` — reuses `BecomeCopy`'s own purge+re-register machinery, then always runs `apply_enters_battlefield_modifiers` afterward with the entering spell's own real `colors_spent` carried through the pause, making CR 707.2's own Skyshroud Behemoth worked example (a copied Fading + "enters tapped" creature) real. Checked before Devour/Amplify in `ResolveStackTop`; the resume doesn't chain back into either (no real Clone-family card has those keywords). Search doesn't offer this choice either, same posture Devour/Amplify already have there.
- **Layer 3 (text-changing)**: CR 612.8 name-setting is real. CR 612.5 Exchange of Words is real now too (`phase5/exchange-of-words`) — `AbilityEffect::ExchangeTextBoxes`, `ability::apply_text_box` (overwrites just `oracle_text`/`keywords`/`script_ref`, unlike `CopyExceptions`' full-copy shape), reusing `BecomeCopy`'s `purge_granted_abilities`+`run_card_script` machinery symmetrically for both targets. CR 612.6 (Volrath's Shapeshifter, "full text") is real now too (`phase5/volraths-shapeshifter`) — `EffectPrimitive::SetFullText(Box<FullTextOverride>)`, modeled as a live `ContinuousEffect` (reusing `EffectDuration`'s existing expiry machinery for the card's real "until end of turn" duration) rather than a one-time mutation like `BecomeCopy`/`ExchangeTextBoxes`, since `layers::effective_characteristics` is a pure read-only query and can't re-run a script or grant a real ability as a side effect. Deliberately narrowed to the characteristic half only — name/mana cost/color/type line/keywords/power/toughness swap to the donor's, but the object's own registered abilities never change, a structural consequence named not hidden. `effective_characteristics` itself gained its first-ever "one layer overrides what every later layer treats as its base" shape: `mana_cost`/`supertypes`/`subtypes` became mutable locals (previously read straight off `def` at the end) so `SetFullText`'s own arm, folded into the same ordered Layer 3 pass `SetName` already runs in, can override them alongside `power`/`toughness`/`colors`/`card_types`/`keywords`. No producer wires a real activated ability that captures "target creature card in a graveyard" into a fresh `SetFullText` effect yet (would need a new `AbilityEffect` variant) — proven via direct construction only, same posture other first-proof slices in this cluster take; no Lua exposure either (`CardDefinition` is too large a struct to justify a full decoder for one card). CR 612.7 (Spy Kit, "all names of nonlegendary creature cards") remains unmodeled, confirmed as a much bigger, separate gap while scoping this slice — this engine has zero supporting substrate for it (no dynamic name-predicate matching anywhere, the legend rule's own name comparison is a fixed-string check; no card-catalog capability exists to enumerate "every nonlegendary creature card" at all), not just "its own one-off shape" the way 612.6 was.
- **Layer 7d (counters)**: real now (`phase5/layer7d-counter-interleaving`) — `CounterSet` tracks one timestamped batch per `add` call (`CounterSet::batches`, private-field internal redesign, no `GameObject`/`PlayerState` ripple), and `effective_characteristics`'s layer 7c fold interleaves `+1/+1`/`-1/-1` counters against `ModifyPT`/`DoublePT` by timestamp per CR 613.4c's literal text, instead of pre-netting them into the layer 7a base. That old shape was a real bug (not just an approximation): a counter added before a `SetPT` override got silently discarded along with the base it was baked into — CR 613.5's own Gray Ogre worked example is now a passing test. Still open: a counter's timestamp falling between a `DoublePT` and its same-field dependency in a way that changes raw timestamp order — theoretical, no real card exercises it.
- **CR 613.6-8 dependency**: real now (`phase5/script-effect-dependency`) — `layers::depends_on` resolves both sides through the existing `layers::resolve_primitive` helper before comparing, so a `Script` effect that decodes to a `DoublePT`/`ModifyPT` shape participates in the one real dependency rule this engine models exactly like a `Primitive` effect already did (either direction: script-depends-on-primitive, primitive-depends-on-script, or script-depends-on-script). Needed a new `double_pt` decode key on `ScriptRuntime::invoke_continuous_effect` since no card had ever authored one from Lua before (Berserk, the real card `DoublePT` models, isn't implemented). Still unmodeled: a script *dynamically* deciding it depends on another specific effect (vs. always resolving to a fixed structural shape) — would need mid-computation partial layer state exposed to Lua, which `state_to_lua`'s snapshot doesn't do, and no real card needs it since CR 613.8a dependency is a structural property of what an effect does. Cross-layer dependency stays unmodeled too, same as before.
- **Targeting**: scripted target-legality predicate is real now (`phase5/scripted-target-filter`) — `TargetFilter::Script(ScriptHandle)`, evaluated via `ScriptRuntime::invoke_target_legal`, nested inside `TargetRequirement` so `count`/`up_to` apply; the old top-level `TargetSpec::Script` (never constructed, no count field) was deleted rather than implemented as originally stubbed. "Another target" (CR 115.4) is real now too (`phase5/another-target`) — `TargetRequirement::exclude_source: bool`, checked in `commands::target_is_legal`, wired everywhere except `Game.SelectTargets`'s mid-resolution script choice (bare `TargetFilter`, no requirement to read a flag from). Player-target script predicates are real now too (`phase5/player-target-script-filter`) — `TargetFilter::PlayerScript(ScriptHandle)`, `ScriptRuntime::invoke_player_target_legal`, Lua entry point `:SetTarget({type="player", filter=function(...)})`, kept as a separate variant from `Script` rather than widening it (would've broken `Script`'s existing bare-object-id call contract). "N or more" counts are real now too (`phase5/target-count-at-least`) — `TargetRequirement::at_least: bool`, mutually exclusive with `up_to`, checked in `commands::validate_targets` (`targets.len() >= count`, no upper bound); "any number of target X" is just `count: 0, at_least: true`. Negative filters are real now too (`phase5/negative-target-filters`) — `TargetFilter::Object::excluded_card_types`/`excluded_subtypes`, checked in `commands::target_is_legal` alongside the existing positive `card_types`/`subtypes` matches (independent, so a filter can combine both, e.g. "target creature you don't control that isn't a Wall"). A spell-side scripted target requirement is real now too (`phase5/spell-target-script`) — scenario TOML's `target_requirement.filter = "script"` plus a raw-Lua `target_requirement.script` field, loaded into a real `ScriptHandle` post-construction (`register_target_script`) and patched onto the object's own `CardDefinition.target_requirement`, same timing fix `trigger_condition`/`trigger_operation` already use. No engine-crate change was needed — `TargetFilter::Script` already read generically off any `CardDefinition.target_requirement`. A card's own base target requirement can be declared entirely by its own script now too (`phase5/script-target-requirement`) — an optional `s.target_requirement()` function (no arguments, a sibling to `s.register_abilities(c)`, not a replacement), called once per `CardDefinition` build via `ScriptRuntime::build_target_requirement` rather than per object per zone-entry the way `run_card_script` is (an Aura's own Enchant restriction is needed before it's ever cast, while still in hand — too early for `run_card_script`'s battlefield-entry timing). Reuses `decode_target_filter_table` for the filter portion, plus a new `decode_target_requirement_table` wrapper for `count`/`up_to`/`at_least`/`exclude_source`. Wired into the scenario crate too (`PendingScriptTargetRequirement`/`register_script_target_requirement`, same placeholder-then-patch timing fix `register_target_script` already established) — an explicit TOML `[cards.target_requirement]` table always wins over the script when both are present, so every pre-existing fixture is unaffected. The scripted-predicate function form `:SetTarget` accepts is real from `target_requirement()` too now (`phase5/script-target-requirement-predicate`) — an explicit `filter` key on the returned table, checked by `decode_target_requirement_table` before falling back to the flat option-bag shape: a function value is handed to `decode_target_filter_value` (same decoder `:SetTarget` uses, so a bare function or `{type="player", filter=fn}` both work), yielding `TargetFilter::Script`/`PlayerScript` — both were already read generically off any `CardDefinition.target_requirement` before this slice, so no engine-side change was needed, only this one decode entry point. `target_requirement()`'s own public signature (`ScriptRuntime`/`Engine::build_target_requirement`) is unchanged; `decode_target_requirement_table` gained `&Lua`/`&mut ScriptRegistry` params, sourced from `ScriptRuntime`'s own private fields via its existing `Rc<RefCell<ScriptRegistry>>` interior mutability (still `&self`, not `&mut self`). The `initial_effect` module-key convention was renamed to `register_abilities` (`phase5/register-abilities-and-script-operation`) — a stale ygo-pro carryover name (where a card keeps one continuous identity for its whole time in a duel, so "initial effect" meant "runs once"; this engine's CR 400.7 adaptation re-runs it fresh every battlefield entry instead) that read as misleading once `target_requirement()`/`operation()` existed as siblings with genuinely different (once-per-`CardDefinition`) timing. Mechanical, 67 files. The same slice added `operation()` as `target_requirement()`'s sibling — CR 608.2m, a plain spell's own resolution effect (Armageddon, Shock), needed since a spell with no ability wrapper never reaches `register_abilities(c)`'s battlefield-entry timing either. New `AbilityEffect::Script(ScriptHandle)` variant (not a new `CardDefinition` field — the same ~400-site ripple class already flagged as prohibitive) carries a custom-function-fallback handle inside the *existing* `effect` field; `CastSpell` unpacks it onto `StackObject`'s real `operation`/`effect` split at push time. See `docs/implementation/05-script-bridge.md`'s own slice entry for the full design, including a real caller trap found while testing (a custom-function `ScriptHandle` is only valid against the specific `ScriptRuntime` that decoded it). Negated supertypes are real now too (`phase5/negated-target-supertypes`) — `TargetFilter::Object::excluded_supertypes`, the last of the three negation fields, same independent-check shape `excluded_card_types`/`excluded_subtypes` already established. Modal-spell `target_requirement` has a real TOML schema now too (`phase5/modal-spell-toml`) — `CardSpec.modes: Vec<SpellModeSpec>`, mode-selection itself (CR 700.2a "choose exactly one") was already engine-complete beforehand, this only closed the scenario-crate wiring gap. Multi-target-word spells are real now too (`phase5/multi-target-word-spells`) — `TargetSpec::Multiple(Vec<TargetRequirement>)`, split into one exact-count chunk per word by `commands::split_targets_by_requirement`; a uniform-verb multi-word spell (CR 608.2b's own Plague Spores example) needed zero effect-resolution changes, since `AbilityEffect::Destroy`/etc. already iterate the concatenated per-word legal-target list. `up_to`/`at_least` combined with multiple words is real now too (`phase5/targeting-multi-word-variable-count`) — `commands::split_targets_by_requirement` derives the one variable word's own count (if exactly one word is variable) as "whatever's left after every exact-count word claims its own share," regardless of word position. Two or more variable words in one spell is real now too (`phase5/multi-word-variable-count-filter-assignment`), closing that exact gap — no real card motivates this shape (confirmed while scoping), built anyway as dormant-but-correct machinery, confirmed with the user first. Rather than a new `PlayerAction` shape announcing each variable word's own count (the originally-assumed fix, and genuinely bigger — new plumbing across `CastSpell`/`ActivateAbility`/`submit_trigger_targets`), `split_targets_by_requirement` falls back to a new `assign_targets_by_filter` for 2+ variable words: a plain backtracking assignment of each submitted target to a word whose own `TargetFilter` it legally satisfies, accepting *any* assignment meeting every word's own count bound rather than requiring a *unique* one — every multi-word `AbilityEffect` in this engine already applies uniformly across the flattened concatenated target list regardless of word origin (CR 608.2b's own Plague Spores example), so which word a target is filed under has no resolution-time consequence, meaning "some valid partition exists" is the correct legality bar. The function's return type widened from borrowed, positionally-contiguous `Vec<&[Target]>` to owned `Vec<Vec<Target>>` (a filter-based assignment can't promise contiguity in submission order) — the existing 0/1-variable-word fast path is otherwise bit-for-bit unchanged, only `.to_vec()`'d at the end, so no existing behavior changed. All 3 real call sites already had `state`/`script_runtime`/`caster`/`source` in scope from a neighboring `target_is_legal` call, so no new plumbing was needed to thread the widened signature through. CR 700.2d (choosing 2+ modes) is real now too (`phase5/modal-multi-mode`) — `CardDefinition.mode_choice: ModeChoice { Exactly(u32), UpTo(u32) }`, `commands::execute_command`'s `CastSpell` arm rejects a repeated mode index or a count outside `mode_choice`'s range, combines 2+ chosen modes' own target requirements into a `TargetSpec::Multiple` (reusing `multi-target-word-spells`'s own machinery) and effects into a new `AbilityEffect::Modal(Vec<Option<AbilityEffect>>)`, resolved in card-print order by a new `ResolveStackTop` arm — narrowed to the synchronous `AbilityEffect` subset `commands::is_modal_resolvable` accepts (`GainLife`/`Damage`/`Counters`/`Pump`/`GainControl`/`Shuffle`); combining a mode using a paused/bespoke kind (`Destroy`/`Sacrifice`/`Exile`/`DrawCards`/`Search`/`BecomeCopy`/`CreateToken`/`Attach`/`ExchangeTextBoxes`) or a mode whose own `target_requirement` is itself `TargetSpec::Multiple` is rejected at cast time. This slice also added `CardDefinition.mode_choice` despite the exact "~403 struct-literal sites, no `Default` impl" ripple class `multi-target-word-spells` itself flagged as prohibitive and avoided - confirmed with the user first, since no smaller-footprint design existed (the mode-count is genuinely new per-card data). CR 700.2h (a mode's own additional cost) is real now too (`phase5/modal-mode-additional-cost`) — `SpellMode.additional_cost: Option<AdditionalCost>`, reusing the exact `AdditionalCost` enum an activated ability's own additional costs already use; `commands::execute_command`'s `CastSpell` arm just appends every chosen mode's own cost to the spell's overall `CostSpec.additional`. Turned out to need zero payment-engine changes — `PaymentPlan::validate`/`Engine::submit_payment` were already fully generic over `CostSpec.additional`, never special-cased by caller; the `commands.rs` comment claiming "no additional-cost support yet either way" for `CastSpell` was stale, the real gap was just that `CastSpell` never populated `cost.additional` with anything. CR 700.2d's own "same mode twice" exception is real now too (`phase5/modal-same-mode-twice`) — `ModeChoice::ExactlyRepeatable(u32)`/`UpToRepeatable(u32)`, new variants rather than a field on `Exactly`/`UpTo` or on `CardDefinition` (either would've hit the same "~403 struct-literal sites, no `Default` impl" ripple `modal-multi-mode` already flagged and paid once — new enum variants are additive, no existing construction site needed touching). `commands::execute_command`'s `CastSpell` arm just skips its own dedup check when the resolved `mode_choice` is one of the two repeatable variants; turned out to need zero changes anywhere else — `ResolveStackTop`'s `AbilityEffect::Modal` arm already resolves each chosen-mode chunk positionally and never looks back at which `SpellMode` index it came from, so two chunks from one repeated mode were already structurally indistinguishable from two chunks from different modes, and a repeated mode's own CR 700.2h additional cost was already collected once per chosen instance (not deduped by index) for the same reason. CR 700.2e (another player choosing) is real now too, narrowly (`phase5/modal-chosen-by-opponent`) — `ModeChoice::ExactlyChosenByOpponent(u32)`/`UpToChosenByOpponent(u32)`, again additive variants rather than a new field. Unlike 700.2d/h, this one needed genuinely new pause/resume plumbing: `PlayerActionKind::CastSpell` is a single-actor atomic command (87 construction sites), so a different player supplying the mode can't fit inside that one action. Instead, `CastSpell`'s modal branch defers entirely - pushes the spell with `StackObject::pending_mode_choice: Option<PendingModeChoice>` set and `chosen_modes`/`target_requirement`/`effect` left as empty/`None` placeholders - and `ProcessState::ResolveStackTop` now peeks at the stack top (without popping) before doing anything else, pausing on the new `StepStatus::AwaitingModeChoice` if it finds one pending. `Engine::submit_mode_choice` (a new method, unauthenticated by player like every other `submit_*_choice`) patches the real `chosen_modes`/`target_requirement`/`effect` onto the still-on-stack object using the exact same bare-single-mode/`TargetSpec::Multiple`+`AbilityEffect::Modal` construction `CastSpell`'s own caster-chosen branch uses, then re-enters `ResolveStackTop` for real. Narrowed hard to keep this one slice: every mode on a `ChosenByOpponent` card must have `target_requirement: None` and `additional_cost: None` (checked at cast time) - supporting either would mean deferring target selection and/or cost computation to the same late pause, real additional plumbing not built here - and the game must have exactly two non-eliminated players (CR 700.2e's own "if there's more than one other player, the controller decides which" sub-clause needs a further choice mechanism no 3+-player test scaffolding exists for yet). Combining with `*Repeatable` isn't supported either. CR 700.2i (pawprint budgets) is real now too (`phase5/modal-pawprint-budget`), closing the last named-unmodeled shape of CR 700.2 — `ModeChoice::PawprintBudget(u32)` plus a new `SpellMode.pawprint_weight: u32` field (~24 existing construction sites, mechanical, same ripple class every prior `ModeChoice`/`SpellMode` field addition already paid). Deliberately **not** folded into `ModeChoice::bounds()` (that method has no access to the modes list and can't derive a count bound from a weight budget alone — a budget of 3 could mean "at most 1 mode" or "at most 3," depending on individual weights); `bounds()` does return `(0, budget, false)` for this variant, but only as a display-only hint for `StepStatus::AwaitingTriggerModeChoice` — real enforcement is each real site's own explicit weight-sum check, same "special-cased ahead of `bounds()`, not folded into it" posture `*ChosenByOpponent` already established. Two real enforcement sites needed their own weight-sum branch: `commands::cast::execute_command`'s `CastSpell` arm, and `Engine::submit_trigger_mode_choice` (CR 603.3c) — a pawprint-budgeted *triggered* ability's own modes work for free once the type-level support existed, since `RegisteredAbility` already shares `ModeChoice`/`SpellMode` wholesale with `CardDefinition`. CR 700.2i's own wording sets no floor (unlike every other `ModeChoice` variant's implicit "at least one") — choosing zero modes is legal and the spell/ability still resolves doing nothing, falling out for free from `combine_chosen_modes`'s existing empty-slice handling. No repeatable pawprint-budget modes and no CR 700.2e combination — no real card combines either with this shape. Scenario TOML wiring stays unmodeled (`build_mode_choice`/`SpellModeSpec` only ever produce count-based variants) — proven via direct Rust construction only, same posture the Lua/TOML-inaccessible `*ChosenByOpponent` variants already took. CR 700.2 itself has no further known gap after this slice.
- **Activated abilities**: `RegisteredAbility` has a real stable `AbilityId` now (`phase5/ability-stable-id`) — `PendingTrigger` (the one case that actually spanned a time gap: collected at one point, consulted later after an ordering-choice pause and potentially intervening game actions) is keyed by it, via `commands::find_ability_by_id`. `PlayerActionKind::ActivateAbility.ability_index` deliberately stays positional by registration order — chosen and consumed in the same synchronous step, confirmed never actually at risk the way `PendingTrigger` was. A card's own Lua script can self-register a real CR 605 mana ability now too (`phase5/mana-ability-real-cost`) — `Game.CreateManaAbility(c)`, a dedicated sibling of `ActivatedAbilityBuilder` (no `:SetTarget` at all; `:SetOperation` only accepts `AddMana`/`AddManaChoice`) — and a mana ability can have a real cost beyond a bare implicit tap for the first time (`commands::PaymentResolution::ManaAbility`, threaded through `PendingPaymentRequest`/`ProcessState::AwaitingPayment`/`Engine::continue_cost_zone_moves`): omitting `:AddCost` entirely keeps the original hardcoded-tap/no-payment-pause shape `mana_ability` TOML has always had, but any real `Game.Costs.*` component (even a bare `TapSelf`) now pauses for a genuine `Engine::submit_payment` step first — reusing the existing Sacrifice/Discard cost-zone-move pipeline verbatim — while still never pushing a `StackObject` (CR 605.1a/605.3b). "Spend this mana only to..." restricted mana is real now too (`phase5/restricted-mana`), narrowed to the single most common real shape - a card-type filter on what the mana can be spent to cast (Mishra's Workshop) - `ManaUnit::RestrictedColored(Color, CardTypeSet)`/`RestrictedColorless(CardTypeSet)`, `ManaPool.restricted: Vec<ManaUnit>` (a side-channel multiset, since the flat aggregate fields can't tell a restricted unit apart from an ordinary one of the same color), `PaymentPlan::validate` checking eligibility off `source`'s own `GameObject` sitting in the `Stack` zone (so an activated ability's cost is never eligible, even for the right card type) and its card types intersecting the restriction, `Game.Ops.AddRestrictedMana(letter, cardType, count?)`. Object-specific restrictions ("only to cast the exiled card"), ability-activation restrictions, and any restriction shape beyond a single card-type filter all stay unmodeled. Mass destruction is real now too (`phase5/mass-destroy-effect`) — `AbilityEffect::DestroyAll(EffectFilter)`, CR 701.8/601.2c's "destroy all X" (Armageddon: "Destroy all lands"), which doesn't target at all. Reuses `Game.CreateStaticAbility`'s own `EffectFilter` shape (`controller`/`card_types`/`self_only`) rather than a second filter vocabulary, and the exact same `continue_scripted_zone_move(ScriptedZoneMoveKind::Destroy, ...)` pipeline the single-target `Destroy` effect already delegates to — CR 702.12b indestructibility and CR 616.1 ordering-choice pauses both apply for free. `matches_effect_filter` (extracted from `ContinuousEffect::applies_to`) and `decode_effect_filter_table` (extracted from `script.rs`'s `decode_affected_value`) are both now shared between the continuous-effect path and this one, rather than duplicated. `EffectFilter` subtypes/exclusion keys are real now too (`phase5/effect-filter-subtypes`) — `subtypes`/`excluded_subtypes: Option<Vec<Subtype>>`, same positive/negated pair `TargetFilter::Object` already has, checked against the raw `GameObject.definition.subtypes` (not `effective_characteristics`, same deliberate stance `card_types`/`controller` already take here to dodge the layer-2 recursion this file's own first backlog line names). "Destroy all Zombies" and a subtype-filtered static ability (`:SetAffected({ subtypes = {"Wall"} })`) both work now. Caught a real, separate pre-existing gap while testing: `event_target_matches` (the layers-aware near-duplicate of `matches_effect_filter` used for `WouldMoveZone`/`DamageDealt` replacement matching) had its own `card_types`-only check with no subtype equivalent — a subtype-filtered replacement effect (e.g. "opponent's Swamps enter tapped") silently matched every object regardless of subtype until this slice added the same check there too, a second near-identical block since the two functions resolve `controller` differently (raw field vs. `layers::effective_controller`) and can't share one implementation. `EffectFilter` supertypes/exclusion keys are real now too (`phase5/effect-filter-supertypes`) — `supertypes`/`excluded_supertypes: Option<SupertypeSet>`, a bitset like `card_types` (not a `Vec` like `subtypes`, since `CardDefinition.supertypes` is already a bitset), checked in both `matches_effect_filter` and `event_target_matches` from the start this time. "Destroy all legendary permanents" and a supertype-filtered static ability both work now. Scenario TOML `continuous_effect` fixtures can express a subtype/supertype filter too now (`phase5/scenario-continuous-effect-filter-subtypes`) — `ContinuousEffectSpec.filter_subtypes`/`filter_excluded_subtypes`/`filter_supertypes`/`filter_excluded_supertypes: Vec<String>`, wired into `build_continuous_effect`'s `EffectFilter` the same way `filter_card_types` already was, reusing `parse_supertypes` (already existed for `TargetRequirementSpec`) and the `Subtype`-map pattern `TargetRequirementSpec.subtypes` already established. Closes the TODO the previous two slices both left in place. A mana ability can now be activated
mid-cast too (`phase5/mana-ability-during-payment`, merged — CR 601.2g/605.3a: the
payer's own mana ability is legal even while a different cost's own payment pause is open, nesting
correctly through a real-cost mana ability's own nested pause) — full account in
`docs/implementation/06-abilities.md`'s own entry.
- **Triggered abilities**: CR 603.3d (a triggered ability that names a target genuinely pauses for its controller to choose one before going on the stack) is real now (`phase5/trigger-target-selection`) — `commands::legal_target_candidates`, `ProcessState`/`StepStatus::AwaitingTriggerTargets`, `Engine::push_ordered_triggers`/`submit_trigger_targets`. CR 603.3d's own zero-legal-candidates clause ("the ability is simply removed from the stack") is real too, not just the happy path. Directly unblocks Vesuvan Doppelganger's granted upkeep ability (`phase5/vesuvan-linked-copy-ability`'s own named gap) — see `phase5_trigger_target_selection.rs`'s own end-to-end test. CR 601.2c's multi-word form ("destroy target creature and target land") is real on a trigger now too (`phase5/trigger-multi-target`) — the pause widened from a single `TargetRequirement` to a full `TargetSpec` (`candidates` becoming one legal-candidate list per word), and `submit_trigger_targets` now calls the same `commands::validate_targets`/`split_targets_by_requirement` `CastSpell`/`ActivateAbility` already use (widened to `pub(crate)`) instead of a narrower single-word wrapper, which this slice deleted as redundant. One word with zero legal candidates voids the whole trigger (CR 601.2c/e's own all-or-nothing stance), not just that word. CR 603.3c itself (modal triggered abilities — a different rule than the target-selection one the two slices above close, despite this backlog having mislabeled the two under the same citation for several slices) is real now too (`phase5/trigger-modal-choice`) — `RegisteredAbility.modes: Vec<SpellMode>`/`.mode_choice: ModeChoice`, reused wholesale from `CardDefinition`'s own modal shape (~124 `RegisteredAbility` struct-literal sites across 50 files needed the two new fields, no `Default` impl — the same ripple class `modal-multi-mode` already paid on `CardDefinition`). `commands::combine_chosen_modes` (the CR 700.2c/d combination logic) and `ModeChoice::bounds` were extracted out of `CastSpell`'s own caster-chosen branch so a modal trigger's own choice — `ProcessState`/`StepStatus::AwaitingTriggerModeChoice`, `Engine::submit_trigger_mode_choice`, checked in `push_ordered_triggers` *before* the existing target check (603.3c precedes 603.3d, mirrors 601.2b before 601.2c) — reuses it instead of a third copy. A chosen mode needing a target chains straight into the *existing* `AwaitingTriggerTargets` pause (`ProcessState::AwaitingTriggerTargets` gained a `modal_effect: Option<Option<AbilityEffect>>` field so the combined effect survives that hand-off instead of being silently re-derived from the modal ability's own meaningless `effect` field); a combined requirement with zero legal candidates removes the ability from the stack, same CR 603.3d stance the plain path already took. No Lua exposure and no scenario TOML wiring — proven via direct `RegisteredAbility` construction only, same as `trigger-target-selection` itself. CR 603.3d's own Lua exposure is real now too (`phase5/trigger-target-lua-exposure`) — `TriggeredAbilityBuilder.target: Option<TargetFilter>`/`::SetTarget`, an exact mirror of `ActivatedAbilityBuilder`'s own `:SetTarget` (same `decode_target_filter_value` decode, same single-exact-count `TargetSpec::Primitive` wrapping at `:Register()` time, no CR 601.2c multi-word form from this builder) — so a card script can now declare a targeted triggered ability for the first time, not just direct Rust construction. CR 603.3c's own mode declaration stays Lua-inaccessible — confirmed with the user first, since unlike `:SetTarget` there's no existing modal Lua vocabulary anywhere to mirror (modal *spells* don't have one either), a bigger, more novel design task than this slice's narrow mirror. CR 700.2e "chosen by opponent" for a triggered ability's own modes is real now too (`phase5/trigger-mode-chosen-by-opponent`) — `ProcessState`/`StepStatus::AwaitingTriggerModeChoice` gained a `chooser: PlayerId` field, resolved once in `push_ordered_triggers` (mirroring `CastSpell`'s own `PendingModeChoice::chooser`) rather than the dispatch side always assuming `trigger.controller`; the same narrowing `CastSpell`'s own branch enforces (every mode targetless/costless, exactly one other non-eliminated player) is checked there too, but an invalid combination just skips the trigger outright (no "reject the cast" step for a trigger to fall back on, so this reuses CR 603.3d's own "no legal choice → removed from the stack" stance instead of a `CommandError`). `submit_trigger_mode_choice` itself needed no functional change — already player-unauthenticated like every other `submit_*`. **CR 700.2h turned out not to apply to triggered (or activated) abilities at all** — its own rule text says "some modal *spells*"/"paid *as the spell is cast*," with no "or ability" clause 700.2a/700.2e both have (checked directly against `comp_rules.md` before starting, not assumed) — so that half of the originally-scoped "700.2e/h" pairing was never real work, just a correctly-inapplicable citation.
- **Replacement effects**: `Game.Ops.DrawCards(n)` is real now (`phase5/replaceable-scripted-draw`, `Engine::continue_draw_cards`). ETB-tapped/counter modifiers are real now (`phase5/etb-replacement-modifiers`) but only as a narrow carve-out. CR 702.54a Bloodthirst is real now too (`phase5/bloodthirst`) — `ReplacementPayload::EntersWithCountersBloodthirst`, gated on `replacement::opponent_damaged_this_turn`'s pure `state.ledger` scan (no player choice needed, unlike its Sunburst/Devour/Amplify siblings). CR 702.44 Sunburst is real now too (`phase5/sunburst`) — `ReplacementPayload::EntersWithCountersSunburst`, no fields at all (unlike every other `EntersWithCounters*`); `kind`/`amount` are both derived at apply time from a new `colors_spent: ColorSet` parameter on `apply_enters_battlefield_modifiers`, sourced from `StackObject::costs_paid` via the new `cost::colors_spent` helper at the one call site that can supply it (`ResolveStackTop`). CR 702.44c's cross-keyword reference ("Modular—Sunburst") stays unmodeled — no such plumbing exists. CR 702.82a Devour is real now too (`phase5/devour`) — the first of the four "dynamic ETB counts" items needing a genuine mid-ETB player choice, closed with a real new pause: `ProcessState`/`StepStatus::AwaitingDevourChoice`, `replacement::pending_devour_choice` (checked only by `ResolveStackTop`, before `apply_enters_battlefield_modifiers` runs — `PlayLand`/`Search` never offer this choice, a named narrow gap), and `Engine::submit_devour_choice`, which reuses `continue_scripted_zone_move` for the actual sacrifices (now carrying a `trailing: Vec<GameEventKind>` field so `events_so_far` and the resulting `CounterAdded` survive even a nested CR 616.1 ordering-choice pause raised mid-sacrifice). CR 702.82c's "Devour [quality] N" is real now too (`phase5/devour-quality`) — `EntersWithCountersDevour.quality: Option<Subtype>`, checked in `pending_devour_choice` against `layers::effective_characteristics(...).subtypes` (safe to use the effective view here since this function is a one-shot call from `ResolveStackTop`, not inside the layer-2 recursion path `matches_effect_filter`'s raw-field policy dodges), `None` meaning "any creature" same as every card before this slice. `Game.ReplacementPayloads.EntersWithCountersDevour(perCreature, quality)`'s `quality` is an optional trailing string arg, so no existing card script needed updating. CR 702.38a Amplify is real now too (`phase5/amplify`) — `ReplacementPayload::EntersWithCountersAmplify`, `replacement::pending_amplify_choice`, `ProcessState`/`StepStatus::AwaitingAmplifyChoice`, `Engine::submit_amplify_choice`, reusing Devour's pause/resume shape but with a real divergence: revealing a hand card doesn't move it anywhere, so the resume needs no `continue_scripted_zone_move` at all, just a direct fold into `ReplacementCheck`. "Shares a creature type" is approximated as raw subtype overlap (`EffectiveCharacteristics::subtypes`), same imprecision `landwalk_subtypes`/`bands_with_other_subtypes` already accept. CR 702.38a's "can't reveal cards entering the battlefield at the same time" and CR 702.38b's "multiple separate Amplify instances work independently" both stay unmodeled (no simultaneous-entry producer exists; `pending_amplify_choice` matches only the first registered instance). CR 614.12a/b choice ordering stays unmodeled. Token creation as a producer is real now too (`phase5/token-etb-modifiers`) — see this file's own note below. General CR 616 propose/replace/commit for battlefield entry is real now too, narrowly, for the resolving-spell path only (`phase5/etb-zone-redirect`) — `ResolveStackTop` proposes a would-be `Battlefield` destination through `replacement::apply_replacements` *before* `move_resolving_spell` places anything, mirroring `sba::resolve_deaths`'s own propose-then-commit shape, so a `ReplaceZoneDestination`-style "can't enter, goes elsewhere instead" effect (already representable, just never checked before) now really works. Only an externally-granted redirect can fire (the entering permanent's own self-registered effects haven't run yet at this checkpoint); `PlayLand`/`Search` don't get this check either, same narrowing every prior mid-ETB slice already took. CR 616.1's ordering choice for two *simultaneous* redirects is real now too (`phase5/etb-redirect-ordering-choice`) — a new `ReplacementResumeTarget::PermanentEntry { top: StackObject, legal_targets, events }` carries the entire not-yet-placed `StackObject` through the pause (unlike every other resume target, nothing has been committed to any zone yet at this checkpoint, so a scalar id has nothing to look up); `resolve_permanent_entry`'s own tail (`move_resolving_spell`, Aura-attach, Copy/Devour/Amplify pause checks) was pulled into a shared `continue_permanent_entry` continuation, called both by the fresh synchronous path and by `submit_replacement_choice`'s own new `PermanentEntry` resume arm. Protection-as-replacement is real now too (`phase5/protection-as-replacement`, `replacement::protection_prevents`/`apply_one_replacement_or_protection`) — protection's damage prevention genuinely participates in CR 616.1's ordering choice now. The sentinel-collision limitation that slice left open is fixed too (`phase5/protection-replacement-collision-fix`) — `replacement::ReplacementCandidate { Registered(ObjectId), Protection(ObjectId) }` replaces the bare `ObjectId` both kinds of candidate used to share, threaded through `ReplacementOutcome::NeedsOrderingChoice`/`ProcessState`/`StepStatus::AwaitingReplacementChoice`/`Engine::submit_replacement_choice`; a permanent with both innate protection and its own registered effect on itself now genuinely offers a 2-way CR 616.1 choice instead of the registered effect always winning by dispatch-order accident. `apply_token_zone_lock` (CR 111.8) still has the old hardcoded-ahead-of-the-loop shape and the same theoretical CR 616.1 exposure, deliberately untouched (already flagged as unreachable via real gameplay). CR 614.12a is real for `Search` now too (`phase5/devour-amplify-search-entry`) — `pending_devour_choice`/`pending_amplify_choice` are checked from `continue_search_battlefield_entry` (a second real producer alongside `ResolveStackTop`), closing the "no card in this engine's test corpus fetches a Devour creature [via Search]" gap named above; `PlayLand`/`CreateToken` stay deliberately unwired (own TODO comments in `engine/action.rs`/`engine/resolve/tokens.rs` — a land can never carry Devour/Amplify text, and only `TokenSpec::CopyOf` could produce one, which no real card in this corpus needs). The real engineering piece: `Engine::continue_scripted_zone_move` (the shared Destroy/Sacrifice/Exile pipeline) used to unconditionally finalize into `ReplacementCheck` once its queue drained — wrong for a Devour sacrifice raised mid-Search, which still owes the rest of its own candidate queue — so `ScriptedZoneMoveThen { StackResolution, SearchResume }` generalizes that "what happens once the queue drains" decision, same shape `SbaResumeThen` already established for `resume_sba_death`; a nested CR 616.1 pause raised *during* the sacrifice itself (nothing new mechanically, but the first time this exact interaction was exercised end to end) correctly threads back through to the search's own pending state, not just the outer Devour-choice pause. CR 614.12b itself (the "combined costs must remain payable" clause across *simultaneous* ETB choices) stays unmodeled, confirmed as a green-field gap rather than a wiring one while scoping this slice — no producer in this engine ever enters multiple permanents "simultaneously" in the CR 110.5g sense at all; `CreateToken`'s `count` loop and `Search`'s own candidate loop are both strictly sequential (one full commit-or-pause per permanent before the next starts), so there's nothing for a cross-permanent payability constraint to apply across.
- **Planeswalkers/Battles/DFCs/Split/Sagas**: explicit DESIGN.md non-goals.
- ~~**Dead Phase-3 stubs**~~: removed (`phase5/stack-owner-and-dead-stubs`) - `Subtype::new`/`as_str`, `CardDefinition::has_type`/`has_supertype`/`has_subtype` (and the now-orphaned `CardType`/`Supertype` enums that only existed to serve those methods), `ManaCost::from_symbols`/`from_pool`/`is_empty`/`generic_total`, `ManaPool::empty`/`add`/`total`/`clear` - all confirmed zero real callers, deleted rather than implemented (nothing depends on them).
- **Face-down permanents**: `card_summary_to_lua` does not hide real info; no morph-style mechanic exists yet, so currently unreachable.
- ~~**Stack owner**~~: fixed (`phase5/stack-owner-and-dead-stubs`) - `StackObject::owner` added (a spell's is its card's owner; an ability's is its source permanent's owner, falling back to `controller` only for a departed trigger source with no live object to read). `sba::remove_players_objects`'s stack cleanup now matches CR 800.4a's actual two-clause shape: a spell (represented by a card) leaves by *owner*, an ability (not represented by a card) leaves by *controller* - previously both used `controller`, which was wrong for the (currently unreachable in real gameplay, but real once "cast a card you don't own" exists) owner/controller-mismatch case.
- **Cost modifier order**: fixed (`phase5/cost-modifier-order`) — `apply_cost_modifiers` sums every matching modifier's delta and floors once, so registration order no longer changes the resolved cost; no real player-choice pause was needed (CR 601.2f's "order matters" carve-out is about colored-pip interactions this engine's generic-only `CostModifier` already doesn't model). A Lua construction path is real now too (`phase5/cost-modifier-builder`) — `Game.CreateCostModifier(c)`/`CostModifierBuilder`, mirroring `Game.CreateStaticAbility`'s queue-and-drain shape; `:Register()` queues a placeholder `controller`, resolved from the source's own live controller at `run_card_script`'s drain step. CR 118.7 colored-pip-specific reduction is real now too (`phase5/cost-modifier-colored-pip`) — `CostModifier.target: CostModifierTarget` (`Generic`/`Colored(Color)`/`Colorless`), `ManaCost::apply_colored_cost_delta`/`apply_colorless_cost_delta` implementing CR 118.7b (no matching pips → redirects to generic)/118.7c/d (reduction exceeds what's there → zeroed, excess spills to generic). Verified by hand that summing first *per component* (not just per modifier) stays order-independent the same way the plain generic sum already was, as long as `Colored`/`Colorless` modifiers stay reduction-only — no real card increases a specific colored pip, so that's the deliberate narrowing rather than a gap. CR 118.7e/f (Hybrid/Phyrexian pip interactions) stay unmodeled; CR 118.7g (snow reduction) needed no new code since it was already just a `Generic`-target modifier by another name. `Game.CreateCostModifier`'s `:SetAmount(amount, color)` gained an optional trailing color-letter argument, omitted meaning `Generic` unchanged.
- **Enchant/Equip (CR 702.16b)**: attach machinery exists; Enchant side closed, Equip side closed. CR 303.4d/e real now (`phase5/aura-self-enchant`). CR 303.4d's third clause ("can't enchant more than one object," Aura's controller picks) is real now too (`phase5/aura-attach-choice`) — `commands::propose_attach`/`ProcessState::AwaitingAttachChoice`/`Engine::submit_attach_choice` — but deliberately dormant: no effect in this engine ever produces multiple simultaneous attach candidates for one Aura, so it's exercised only by direct construction, not real gameplay (confirmed as the right call with the user before building, since the underlying rule is real in Magic even though nothing here triggers it yet). Player-candidate attach *choice* stays out of scope (no producer generates a mixed object/player candidate set) - enchanting a player at all is real now, see below. CR 303.4f/g/h are real now too, for Auras (`phase5/aura-enters-attached`) — `SearchDestination::Battlefield::enters_attached`, `commands::legal_aura_entry_candidates`, and a real (not dormant) `ProcessState::AwaitingAuraEntryAttachChoice`/`Engine::submit_aura_entry_attach_choice` pause with a genuine continuation back into the search that raised it. Still unmodeled: 303.4i (an effect specifying an *explicit* illegal enchant target — nothing in this engine's `Search` shape can specify a target at all, so this is genuinely unreached, not narrowed), and 303.4k (morph, unreachable). CR 303.4m ("enchanted X" reference) is real now too (`phase5/enchanted-reference`) — `AffectedFilter::EnchantedOnly`, a new fieldless variant (not a field on `EffectFilter`, 129 struct-literal sites with no `Default` impl — same ripple class avoided elsewhere) resolved directly by `ContinuousEffect::applies_to` off the already-passed-in `&GameObject`'s own `attached_to`, no new state lookup. `ScriptRuntime::invoke_continuous_affects`/`invoke_continuous_effect` both gained a trailing `enchanted: Option<ObjectId>` Lua argument (appended after the existing `state` snapshot arg, so no existing script signature breaks) for a scripted Aura needing more than identity matching; `script::decode_affected_value` recognizes `{ enchanted_only = true }` from `:SetAffected`. `card_summary_to_lua` now exposes `attached_to`/`attachments` on every card summary too. Still unmodeled: CR 303.4m for a permanent enchanting a *player* (this engine's continuous-effect machinery only ever affects objects, not player-level static abilities - a separate, larger gap, unaffected by `phase5/aura-enchants-a-player` making enchanting a player possible at all), and a *replacement* effect's own "enchanted permanent" reference (e.g. Umbra armor, CR 702.89a) — `ReplacementEffect.affected: EffectFilter` is a different, non-`AffectedFilter` type, out of scope for this slice. Equipment/Fortification pre-attached entry is real now too (`phase5/attach-entry-equipment-fortification`) — `commands::legal_entry_attach_candidates` dispatches by permanent kind (Aura still reads its own `target_requirement`; Equipment/Fortification legality is just "every creature/land on the battlefield," per CR 702.6c's confirmation that an Equip ability's own quality restrictions don't narrow this), reusing the same pause/resume plumbing (renamed `AwaitingEntryAttachChoice`/`submit_entry_attach_choice`, no longer Aura-exclusive). The one real behavioral divergence from Aura: a 0-candidate Equipment/Fortification still enters the battlefield unattached (CR 301.5e/301.6), where a 0-candidate Aura stays put entirely (CR 303.4g). Found while scoping `phase5/script-target-requirement`: `CardSpec` (scenario crate) had no TOML field for a card's own subtypes at all — closed by `59f3444` ("update scenarios with `subtypes` and `supertypes`") — `CardSpec.subtypes`/`supertypes: Vec<String>` now exist and thread into `CardDefinition.subtypes`/`supertypes`, so a scenario fixture can mark a card "Aura" (`commands::is_aura` fires off it) or give a creature a subtype like "Wall" for a subtype-gated Enchant restriction to check against. CR 303.4b's "or player" half is real now too (`phase5/aura-enchants-a-player`) — `GameObject.attached_to_player: Option<PlayerId>` (a new sibling field, mutually exclusive with `attached_to`, not a widened `Option<Target>` — every existing object-attach call site stays untouched), `commands::attach_permanent_to_player`, `GameEventKind::AttachedToPlayer`, `ResolveStackTop`'s attach branch now matching `Target::Player` (previously silently dropped, so casting an Aura at a player resolved and then instantly died via the pre-existing "unattached" SBA check), and `sba::collect_illegally_attached_auras` extended for the player half (illegal if the enchanted player left the game — `PlayerState.eliminated`, not just re-checked by `target_is_legal` which only confirms the player still exists — or no longer matches the filter). Deliberately deferred: CR 303.4f/g entry via Search/token for a player target, the multi-candidate choice mixing object/player candidates, and CR 303.4m's "enchanted player" reference for continuous effects (separate, larger gap, unchanged). CR 303.4f/g's "or player" half for the search-entry path is real now too (`phase5/aura-enters-attached-player`) — `legal_aura_entry_candidates` now chains a `state.players` scan (`Target::Player`) after its existing `state.objects` scan, reusing `target_is_legal` unchanged (already fully generic over `Target::Object`/`Target::Player`); the candidate type threaded through `legal_aura_entry_candidates`/`legal_entry_attach_candidates`/`AwaitingEntryAttachChoice`/`submit_entry_attach_choice` widened from `Vec<ObjectId>`/`ObjectId` to `Vec<Target>`/`Target` throughout, with `submit_entry_attach_choice` gaining the same two-armed `attach_permanent`/`attach_permanent_to_player` dispatch `ResolveStackTop`'s cast-time attach branch already used. Still deliberately out of scope: mixed object-and-player candidates in one choice (no `TargetFilter` variant means "object or player," so no producer can create a mixed set), and token creation's own CR 303.4f/g support (`AbilityEffect::CreateToken` never touches `SearchDestination`/`legal_entry_attach_candidates` at all, a pre-existing and separate gap). Token creation's own CR 303.4f/g/301.5e/301.6 support is real now too (`phase5/token-enters-attached`) — only `TokenSpec::CopyOf` can realistically produce an Aura/Equipment/Fortification token (real cards: Estrid, the Masked / Bruna, the Fading Light); unlike Search's `enters_attached` opt-in flag, the attach dispatch always runs here, gated purely by `commands::is_aura`/`is_equipment`/`is_fortification` on the token's own resolved definition (zero ripple to any pre-existing `CreateToken` call site). `resolve_create_token_effect`'s old `for _ in 0..count` loop became `continue_create_token` (`engine/resolve/tokens.rs`, `remaining: u32`-driven, resumable), pausing via a new sibling `ProcessState`/`StepStatus::AwaitingTokenAttachChoice` (not a shared variant with `AwaitingEntryAttachChoice` — that one's resume is hard-wired to Search's own `VecDeque`-based library loop) and `Engine::submit_token_attach_choice`. CR 303.4g's token-specific second sentence ("if the Aura is a token, it isn't created") is real too — the candidate scan runs *before* the token's `GameObject` is ever built/inserted, so a 0-candidate Aura is skipped cleanly rather than built-then-undone. `count > 1` is real too — each token in one `CreateToken` effect independently runs the full 0/1/2+ dispatch, pausing and resuming per-token in sequence. Still deliberately out of scope: mixed object-and-player candidates (same standing non-goal), and CR 303.4m enchanted-player continuous effects (unrelated, unchanged). A replacement effect's own "enchanted permanent" reference is real now too (`phase5/umbra-armor`, CR 702.89a Umbra armor) — `EventFilter::WouldDieEnchanted` (fieldless sibling to `WouldDie`, resolved directly against the replacement effect's own `source.attached_to`) and `ReplacementPayload::UmbraArmor` (rewrites into a new `GameEventKind::UmbraArmorConsumed { enchanted, aura }`). The real novel piece: `sba::commit_death_or_regeneration` widened to return `Vec<(GameEvent, Vec<RegisteredAbility>)>` instead of a single pair, since Umbra Armor's own resolution commits two real events on two different objects (damage-clear on the enchanted permanent, a genuine destroy of the Aura itself via a direct nested `commit_death` call — CR 616 recursion deliberately skipped, named not glossed). Wired through `Game.CreateReplacementEffect`/`Game.EventFilters.WouldDieEnchanted`/`Game.ReplacementPayloads.UmbraArmor` the same way every other filter/payload pairing already is. Still unmodeled: no general "replacement effect referencing the enchanted permanent" beyond this one concrete shape; CR 702.89b's multiple-instances independence isn't specifically tested (no shared-state bug, just unexercised). CR 303.4m's "enchanted player" reference for continuous effects has a real first proof now too (`phase5/enchanted-player-hand-size`), deliberately narrowed — no general `AffectedFilter`-for-players primitive was built (research confirmed there was no existing mechanism to extend at all, unlike every other slice in this cluster, and no design-doc discussion ever sized the general case); instead `HandSizeModifier`/CR 402.2 (the one player-directed modifier mechanism with a real, already-tested enforcement path) gained a new `HandSizeAffected::EnchantedPlayer` variant, resolved via `attached_to_player` the same way `AffectedFilter::EnchantedOnly` already resolves object attachment, plus its own first-ever Lua builder (`Game.CreateHandSizeModifier`, closing a separately-flagged TODO from `phase5/max-hand-size` as a side effect). Still unmodeled: every other player-level continuous-effect shape (hand-reveal, draw-limiting, etc.) — each needs its own bespoke mechanism and enforcement point, confirmed during scoping to not share a common query with hand size or with each other. CR 701.20a/701.18c "plays with hand revealed" has a mechanism now too (`phase5/hand-reveal-modifier`), deliberately narrower still than the hand-size cut: `HandRevealAffected`/`HandRevealModifier`/`is_hand_revealed` mirror `HandSizeAffected`/`HandSizeModifier`'s exact shape (a separate sibling type, not shared — only the second occurrence), plus `Game.CreateHandRevealModifier`, but with **no enforcement wiring at all** — confirmed during scoping that no consumer exists to hook into: `GameObject.visibility`/`VisibilityPolicy::is_visible_to` is itself still `todo!()` with zero callers anywhere, and `script::encode::state_to_lua`/`card_summary_to_lua` structurally never encode hand contents for any player, revealed or not (only a `hand_count`) — there's nothing today for "revealed" to change from that consumer's perspective. Same "real but currently unreachable, named not hidden" stance `VisibilityPolicy` itself already takes; proven only via `is_hand_revealed`'s own direct unit tests. A real enforcement point would mean standing up a per-viewer state-snapshot boundary this engine doesn't have for any purpose yet — a separate, bigger architectural question, not bundled here. CR 121.2b "can't draw more than one card each turn"-style effects are real now too (`phase5/draw-limit-modifier`), the last of the named player-level continuous-effect shapes — `DrawLimitModifier`/`DrawLimitAffected`/`effective_draw_limit` mirror `HandSizeModifier`'s own shape, narrowed to `Filter(FilterController)` only (no `EnchantedPlayer` variant — no known real card needs an Aura-driven form of this one). Unlike hand-reveal, this one has real enforcement from the start: `cards_drawn_this_turn` (a ledger scan back to the most recent `GameEventKind::TurnStarted`, mirroring `replacement::opponent_damaged_this_turn`'s own convention) is checked directly inside `commands::propose_draw` — the one choke point both `Engine::continue_draw_cards` (`Game.Ops.DrawCards`) and the turn-based `Step::Draw` handling already shared, so a blocked draw reuses the exact "no draw happens" path the empty-library case already takes, and CR 121.2b's "instructions to draw multiple cards may still be partially carried out" falls out for free from `continue_draw_cards`'s existing per-card loop. Still unmodeled, confirmed genuinely unreached rather than narrowed: CR 121.2b's "can't choose to draw multiple" and "can't pay a cost that includes drawing multiple cards" clauses — no choice-based draw mechanism or draw-cost primitive exists in this engine at all. CR 303.4m's "enchanted player" reference for a *replacement* effect (named unmodeled ever since `phase5/umbra-armor` closed the object-attached half) is real now too (`phase5/enchanted-player-replacement`) — the one real, CR-doc-cited "enchanted player" card in this repo's own rules text, **Wheel of Sun and Moon** (CR 616's own worked example: "if a card would be put into enchanted player's graveyard from anywhere, instead reveal it and put it on the bottom of its owner's library"), unlike the hand-size/hand-reveal/draw-limit cluster above (none of which ever had a real card motivating their own `EnchantedPlayer` variant). `EventFilter::WouldEnterEnchantedPlayerGraveyard` (fieldless, sibling to `WouldMoveZone`) resolves `self.source.attached_to_player` against the moving card's own *owner* (CR 400.7 — graveyards are owner-keyed), ungated on `is_destroy` per CR 616's own "from anywhere" wording. No new `ReplacementPayload` was needed — `ReplaceZoneDestination(Library)` reused as-is, since `sba::commit_death`'s existing `to`-driven zone lookup already resolves a `Library` destination against the moving card's own owner and bottoms it there (proven originally for the Exile case, same code path, zero `sba.rs` changes this time). "Revealed" stays unmodeled — no per-viewer hidden-information boundary exists in this engine for it to mean anything yet, same standing gap `HandRevealModifier` already named. `HandRevealAffected`/`DrawLimitAffected::EnchantedPlayer` remain unbuilt, unchanged — still no real card motivating either. CR 303.4m's "enchanted player" reference for general *continuous* effects (as opposed to this one replacement-effect shape) is real now too (`phase5/enchanted-player-controls`), closing the last of the five separately-tracked CR 303.4m shapes named across this cluster — the real card: **Curse of Vitality**, "Creatures enchanted player controls have lifelink." `AffectedFilter::EnchantedPlayerControls` (fieldless, sibling to `EnchantedOnly`, same ripple-avoidance precedent) resolves in `ContinuousEffect::applies_to` as `source.attached_to_player.is_some_and(|p| object.controller == p)` — no `GameState` lookup needed, cheaper even than `HandSizeModifier`/`HandRevealModifier`'s own `EnchantedPlayer` arms (which only have a bare `source: ObjectId` to work with, not an already-borrowed `&GameObject`). `layers::applicable_effects_in_layer`'s three-way `AffectedFilter` match widened accordingly; `decode_affected_value` gained `{ enchanted_player_controls = true }` as a second short-circuit key, checked after `enchanted_only` so a (invalid) table setting both deterministically prefers the older one rather than erroring. Fieldless and exclusive with `controller`/`card_types`, same narrowing `EnchantedOnly` already took — no real "creatures enchanted player controls that are also a [type]" card exists to justify combining the two. `HandRevealAffected`/`DrawLimitAffected::EnchantedPlayer` remain a separate, unrelated, still-unbuilt gap (those are player-level modifiers, not object-affecting continuous effects — no shared mechanism links them to this slice). CR 303.4m itself has no further known gap.
- **Landwalk**: real now (`phase5/landwalk`, `EffectPrimitive::AddLandwalkSubtype`/`EffectiveCharacteristics::landwalk_subtypes`, checked in `commands::DeclareBlockers`) — single-subtype matching (covers all 5 basic landwalks). CR 702.14a's card-type/supertype-combination forms ("artifact landwalk," "nonbasic landwalk," "snow swampwalk") are real now too (`phase5/landwalk-quality-forms`) — `EffectPrimitive::AddLandwalkQuality(LandwalkQuality)`/`EffectiveCharacteristics::landwalk_qualities`, a separate primitive from the plain subtype form (every field of `LandwalkQuality` must hold simultaneously; explicitly requires the candidate to be a land, on top of whatever combination of card types/supertypes/subtypes it names). No further known gap in CR 702.14.
- **Banding**: plain form real now (`phase5/banding` - `KeywordSet::BANDING`, `PlayerActionKind::DeclareBands`, `CombatState::bands`, block propagation in `DeclareBlockers`, generalized attacker-or-blocker split in `AssignCombatDamage`). CR 702.22j/k are real now too (`phase5/banding-split-actor`) - `AssignCombatDamage` computes a required chooser per split id (the object's own controller by default, flipped to the other side's player when a `BANDING` creature is on the other end of that block) instead of hardcoding the active player; a damage step needing splits chosen by two different players at once (a plain multi-block plus a separate banding-blocked attacker, simultaneously) is rejected outright rather than attempted via a two-phase per-player submission - narrowed on purpose, named as a TODO on `AssignCombatDamage` itself. CR 702.22b/c's "bands with other [quality]" variant is real now too (`phase5/bands-with-other`) - `EffectPrimitive::AddBandsWithOther(Subtype)`/`EffectiveCharacteristics::bands_with_other_subtypes`, mirroring Landwalk's shape exactly; `DeclareBands` accepts a band matching either the plain form or this quality form. CR 702.22j/k's *other* trigger condition is real now too (`phase5/bands-with-other-split-trigger`) - `AssignCombatDamage`'s `has_bands_with_other_pair` helper flips the damage-split chooser whenever the creatures grouped by one block contain a bands-with-other[quality] creature and another creature sharing that quality, independent of any declared band, checked alongside the pre-existing `BANDING`-keyword flip. CR 702.22c/m's supertype form ("bands with other Legends," i.e. Legendary) is real now too (`phase5/bands-with-other-supertype`) - `EffectPrimitive::AddBandsWithOtherSupertype(SupertypeSet)`/`EffectiveCharacteristics::bands_with_other_supertypes` (OR-accumulated bitset, unlike the subtype form's `Vec`), a separate primitive rather than widening `AddBandsWithOther`'s payload; both `DeclareBands`'s quality check and `has_bands_with_other_pair` gained a second supertype pass alongside their existing subtype one. CR 702.22i (effect-triggered blocking) is real now too (`phase5/bands-with-other-effect-blocked`) - this needed a genuinely new, non-banding-specific primitive first: `AbilityEffect::BecomeBlocked` (CR 509.1h, "target attacking creature becomes blocked" - blocked by no specific creature), since this engine had zero producer for "a creature becomes blocked" outside the `DeclareBlockers` player action before this slice. `CombatState.blocks` (a flat list of real `(blocker, attacker)` pairs) can't represent "blocked, but by nothing," so a new sibling field, `CombatState.effect_blocked: Vec<ObjectId>`, holds it - paid the same "no `Default` impl, every construction site touched" ripple `modal-multi-mode` paid before (~60 test call sites, mechanical, compiler-verified), confirmed with the user first since no smaller-footprint design existed. `commands::combat::mark_effect_blocked` is the one mutation point (`engine/resolve/direct_effects.rs`'s `resolve_become_blocked_effect` is its only caller) - CR 702.22i itself falls out of the same function: if the target is in a declared band, every other living, still-attacking band member becomes blocked too, independently (not by a shared blocker, unlike CR 702.22h's declared-blocker propagation). `damage.rs`'s `declared_blocked_attackers` folds `effect_blocked` in alongside `blocks` - CR 510.1c already means a blocked attacker with zero *living* blockers deals no combat damage (unless trample), the exact rule this reuses, just reached with zero blockers from the start rather than every real one having died. Narrowed: `target_requirement` can only express "target creature," not "target *attacking* creature" (no `TargetFilter` predicate for combat state exists, and the Lua state snapshot doesn't expose combat data either) - targeting a non-attacking creature is a silent no-op at resolution rather than a real CR 608.2b illegal-target fizzle. CR 702.22b (losing Banding also strips bands-with-other) is real now too (`phase5/bands-with-other-keyword-loss`) — the "no effect removes a keyword yet, untestable" note above was stale by the time this was scoped: `EffectPrimitive::RemoveKeyword` was already real (`phase5/keyword-layer-fix-and-remove-keyword`), just never logged back here — caught by grepping the actual code before starting, not trusting the prose (this backlog's own standing lesson). The real gap was narrower: `layers::effective_characteristics`'s layer 6 fold already accumulated `bands_with_other_subtypes`/`bands_with_other_supertypes` fully independently of `keywords`, so `RemoveKeyword(BANDING)` cleared Banding itself but left a creature's separate bands-with-other grant untouched. One check added at the end of that fold, gated on this object's own *printed-or-copied* `def.keywords` containing Banding (CR 702.22b's own precondition — banding must actually have been *lost*, not simply never present) while the *effective* set no longer does — deliberately narrower than "effective set merely lacks `BANDING`", since `phase5/bands-with-other`'s own `quality_band_legal_with_no_banding_member_at_all` test already proved a creature can legally grant bands-with-other while never printing Banding at all (CR 702.22c's second clause allows this), a shape this slice must leave untouched. No further known gap in CR 702.22.

## Mistakes caught (don't repeat)

- Hybrid Phyrexian does **not** need a 3-slot container; `Phyrexian(Color)` atom already carries two payment options.
- PowerShell 5.1 `-replace` with a ScriptBlock is unsupported; use line-by-line array splicing for position-sensitive edits.
- Replacement stabilization loop needs a per-event `excluded` set, or a `Repeatable`/`LimitedUses` effect can match its own unchanged output forever and hang.
- A `ReplacementPayload` that both mutates `GameState` directly *and* proposes an event describing the same mutation (e.g. `CounterAdded`) double-applies once that event reaches `CommitEvents`'s own generic handler for it — pick exactly one (`EntersTapped` mutates directly since no event exists for it; `EntersWithCounters` only proposes, since `CommitEvents` already knows how to apply `CounterAdded`). Caught via a failing test asserting 2 counters and getting 4.
- This backlog's "Live TODOs" line and `docs/lua/`'s own prose can go stale after a slice merges without a doc-update pass (`Game.Ops.DrawCards(n)` was already made real by `phase5/replaceable-scripted-draw`, but both docs kept claiming it wasn't for several slices afterward). Before scoping a new slice off a backlog line, grep the actual code (function exists? test file exists?), don't just trust the prose.
- When a "candidate id" list can be populated from two different sources that might name the same real object (e.g. a registered `ReplacementEffect::source` and a separately-computed sentinel id), always dedup before deciding "how many candidates matched" — pushing both unconditionally produces a duplicate-id list that looks like a real 2+-way CR 616.1 choice but is actually just one match counted twice. Caught by a test asserting a single-match synchronous resolution and getting a spurious ordering-choice pause instead.
- A doc comment blaming a gap on "blocked on X, not designed yet" can itself go stale once X gets built for a *different* feature — `invoke_target`'s old comment blamed Lua coroutine support not existing, but coroutine support was added for `Game.SelectTargets` (backlog item 9) several slices before `TargetSpec::Script` was ever touched again. The two were never actually the same problem (one asks the player mid-resolution, the other is a synchronous per-candidate legality check, same shape `AffectedFilter::Script` already solved) — always re-check whether a doc's stated blocker is still true, and whether it was ever really the same blocker, before assuming a gap needs new infrastructure.
- `ManaCost::apply_cost_delta(0)` is not a no-op — it unconditionally drops an *explicit* `Generic(0)` symbol (an `{X}` cost resolved with X=0 needs one, per `PaymentPlan::validate`'s "explicit empty Mana entry" requirement), since a bare `0` never satisfies the function's own "adjusted > 0, push it back" condition. Any caller that used to skip calling it entirely when there was "nothing to apply" (empty modifier list, `prior_casts == 0`, ...) must keep skipping it explicitly when the *computed* delta is `0`, not just when the input list is empty — summing multiple deltas into one and always calling once exposed this at two call sites (`cost::apply_cost_modifiers`, commander tax) that had accidentally been relying on the "don't call it at all" short-circuit to dodge the bug.
- A "one source, one matching effect" assumption baked into a `source: ObjectId` re-lookup (`.position(|r| r.source == source)`) silently breaks once a single source can legitimately carry *two* matching effects at once — the second lookup just keeps re-finding the first. This engine's `ReplacementEffect` has no unique id, only `source`, so any loop that collects `Vec<ObjectId>` "matches" and re-derives each one's actual data by re-searching for its source is only safe as long as no source has 2+ matches — an invariant that held for every hand-authored test fixture until a copy effect (`BecomeCopy`/`EntersAsCopy`) could re-register more than one battlefield-entry-modifier payload under one freshly-shared id (CR 707.2's own Skyshroud Behemoth example: Fading + "enters tapped" on one creature). The general fix: collect real `usize` indices instead of a lossy key, and if anything in the loop can remove entries, process indices in reverse so removal never invalidates a not-yet-processed lower one. Caught by a test combining two payloads on one copied creature and getting one silently dropped instead of both applied — a second, independent instance of the same "lossy candidate id" mistake logged above (that one was about deduping a spuriously-doubled id; this one is about a single id validly meaning two different things at once), so treat *any* `source`-keyed re-lookup as suspect once a copy-family effect is involved, not just the two call sites this slice found.
- The *generic* CR 616 replacement pipeline (`apply_replacements_from`) and a *narrow* dedicated driver (`apply_enters_battlefield_modifiers`) can both independently claim to "match" the same event/payload pairing if nothing tells the generic one to back off — `EntersTapped`/`EntersWithCounters`-family payloads were always meant to be handled exclusively by the narrow driver (its own doc comments already said so), but `ReplacementEffect::matches` has no concept of "this payload belongs to someone else," so the generic pipeline matched them too, and two such matches sharing a source spuriously demanded a CR 616.1 ordering choice between effects that were never supposed to be ordered. When adding a new narrow driver for a payload family, check whether the generic pipeline can *also* see the same raw event later (it usually can, once pushed into `events_so_far`) and explicitly exclude that family from the generic match, not just assume the narrow driver "owns" it by convention.
- A third, opposite instance of items 29/32's "lossy candidate id" family: `protection_prevents` and a registered `ReplacementEffect` could share the *same* object id (the protected permanent's own) while being genuinely different CR 616.1 candidates, not duplicates — but the original fix reused item 29's own "dedup a shared id" instinct here too, which was wrong in this direction: deduping collapsed two *real, distinct* choices into one instead of removing a spurious double-count of one real choice. The tell: item 29's dedup was correct because both provenances (a registered effect's source, a separately-computed sentinel) really did name the same one real match; here, the two provenances (a registered effect, protection's own live characteristics check) are different *kinds* of thing that only happen to share an identifying `ObjectId` — bare-id equality can't distinguish "same match, seen twice" from "different matches, same object." Fixed by giving the two kinds their own real type (`ReplacementCandidate::Registered`/`Protection`) instead of relying on `ObjectId` identity to imply sameness at all. When a dedup-by-id feels right, first check whether the id is actually a *unique key for one concept* (item 29's case) or *an attribute shared by two different concepts* (this case and item 32's) — only the former should ever be deduped.
- A `ScriptHandle` returned from building/decoding a scripted value (e.g. `ScriptRuntime::build_target_requirement`'s scripted-predicate `filter`) is only valid against the specific `ScriptRuntime`/`Engine` instance whose registry it was registered in — the same caller trap `docs/implementation/05-script-bridge.md` already names for a custom-function operation handle, rediscovered the hard way while writing `phase5/script-target-requirement-predicate`'s own end-to-end test: building the requirement with a throwaway `ScriptRuntime::new()` and then feeding it into a separately-constructed `Engine::new(state)` (each with its own independent Lua VM/registry) made every cast silently fail — `commands::target_is_legal`'s `TargetFilter::Script`/`PlayerScript` arms call `.unwrap_or(false)` on the invoke result, so the resulting "unknown script handle" `RuntimeError` was swallowed into a plain rejection with no visible error, not a panic or loud failure. The tell: this only bites a *scripted* shape (one that carries a real `ScriptHandle`) — the pre-existing flat-filter-table tests in the same file happened to be safe because that shape carries no handle at all, so nothing about them exposed the trap. Fixed by building the scripted value from the real `Engine`'s own runtime (`Engine::build_target_requirement`, not a standalone `ScriptRuntime`) and patching it onto the already-inserted object — the same placeholder-then-patch timing convention already established for `register_target_script`/`register_script_target_requirement`. Any test (or real caller) that pre-builds a `CardDefinition` field containing a `ScriptHandle` outside the `Engine` that will actually evaluate it needs this same fix. Recurred once already, same exact shape: `phase5/aura-enchants-a-player`'s own `TargetFilter::PlayerScript` test built the handle from one `ScriptRuntime` and called `check_state_based_actions` (which takes a bare `&ScriptRuntime`, not tied to an `Engine`) with a *different* one — fixed by threading one shared `ScriptRuntime` through both. The general rule isn't specific to `Engine` at all: **any two calls involving the same `ScriptHandle` must share the exact same `ScriptRuntime` instance**, whichever functions happen to take it as a parameter.
- A test fixture that places a *live, on-battlefield* Aura with zero currently-legal enchant targets is unbuildable via this engine's own rules, not just hard to set up: if it currently has zero legal targets, its own existing attachment (or lack of one) is already illegal under that identical filter, and CR 704.5m's state-based action (`sba::collect_illegally_attached_auras`, gated on `o.zone == ZoneKind::Battlefield`) will have already destroyed it on the very first SBA pass — which in this engine's own pipeline happens well before a cast spell even gets to resolve (SBA is checked at every priority-granting point, including the one right after a spell is cast and paid for, long before both players pass and it resolves). Caught while writing `phase5/token-enters-attached`'s own "CR 303.4g: an Aura token with no legal target isn't created" test: an unattached `aura_source_definition` object placed directly on the battlefield in initial `GameState` construction was already gone (`state.object(source).is_none()`) by the time `pay_free_and_drain`'s first `drain()` call returned, well before `CreateToken`'s own resolution ever ran — silently exercising the pre-existing "copy source no longer resolves, skip" branch instead of the intended new "zero candidates, is_aura, don't create" branch, and passing for the wrong reason. Any fixture needing a *copy source* (or other read-only reference) that's an Aura with no currently-legal target should place it in a non-battlefield zone (graveyard, exile, hand) instead — `sba::collect_illegally_attached_auras`/`commands::legal_entry_attach_candidates` both only ever consider `ZoneKind::Battlefield` objects, so a graveyard placement is exempt from CR 704.5m entirely and needs no attachment of its own.
- Adding a field to a shared filter type (`EffectFilter`) and fixing every `E0063` the compiler reports is necessary but not sufficient — the compiler only finds *construction* sites, not every place that re-implements the same matching logic by hand instead of calling the shared function. `phase5/effect-filter-subtypes` added `subtypes`/`excluded_subtypes` to `EffectFilter` and to `matches_effect_filter` (the one function `ContinuousEffect::applies_to`/`AbilityEffect::DestroyAll` both call), and the full workspace build went green — but `types/effect.rs`'s `event_target_matches` (used for `WouldMoveZone`/`DamageDealt` replacement matching) is a *second*, separate implementation of the same controller/card_types checks, written by hand because it needs `layers::effective_controller` instead of a raw field, and it silently kept matching every object regardless of subtype since its own `card_types`-only check had nothing to reject on. The build stayed green throughout, since `event_target_matches` never destructures `EffectFilter` exhaustively — it only reads the fields it happens to check. Only a real end-to-end test (a subtype-filtered `EntersTapped` effect, asserting a *non-matching* object is spared) caught it; the "matching object gets tapped" half of the same test would have passed even with the gap, since the old code already let everything through. When a struct gains a new field, grep for every place that matches on/reads its *existing* fields by hand (not just `StructName {` construction sites) — a hand-written duplicate won't error at compile time just because it's now missing a check.
- A same-line-only "does this line already have the field" guard on a mass find-and-patch script (`perl -pi -e 's/.../.../ unless /field_name/'`) is blind to a field that exists on a *different* line of the same struct literal, and blind to *which type* it's even patching. `phase5/effect-filter-supertypes` mass-inserted `supertypes: None, excluded_supertypes: None,` right after every `excluded_subtypes: None,` line to backfill `EffectFilter`'s two new fields — but `TargetFilter::Object` (an unrelated, pre-existing type) happens to share the exact same four field names, and already had real `supertypes`/`excluded_supertypes` fields sitting a few lines *below* `excluded_subtypes: None,` in its own literals. The same-line check saw no `supertypes` on the line being edited and inserted anyway, producing a genuine duplicate-field `E0062` in every `TargetFilter::Object` literal it touched (15+ files) — compiler-caught, but not surfaced by `cargo build --workspace --tests` in one pass, since that command stops attempting further targets once an earlier package in the graph fails; needed `cargo build --tests -p stackfish-engine` (which compiles every integration-test binary as an independent unit) to force all of them to report at once. The general fix used: re-scan for the exact duplicate pattern (the inserted fields immediately followed within a few lines by the original pre-existing ones) and strip only the inserted copy, leaving genuine single insertions untouched. The general lesson: before a blanket per-line regex patch across a multi-type codebase, check whether the field names being matched are unique to the one struct being changed — if two types share a field vocabulary (as `EffectFilter`/`TargetFilter::Object`/`FilterSpec` all do here, by deliberate design precedent), a text-level patch needs type-aware scoping (grep for the enclosing `TypeName {`, not just the field name) or a full compiler-round-trip check immediately after, not just "does `E0063` go away."
- `crates/scenario/src/lib.rs`'s `build_initial_state`/`build_new_game_engine` used to register a card's own `s.operation()`/`s.target_requirement()` (the `PendingScriptOperation`/`PendingScriptTargetRequirement` collection, `docs/lua/card-script-basics.md`'s "two functions that aren't battlefield-timed") only when `CardSpec.script` — the raw TOML-inline `script = "..."` field — was set. A `db_script_id`-loaded card's real Lua source lives on the resolved `CardDefinition.script_ref` instead (same `ScriptReference::Inline` shape `CardDb::load_by_script_id` and the TOML-inline path both produce), so any DB-sourced spell/ability whose behavior comes from `s.operation()` (rather than the self-registering `s.register_abilities(c)` convention) silently resolved as a no-op — found via a real db-backed Balance (`Game.Ops.ForEachPlayer`/`ChooseN`) `card_tests` row that never paused for a choice at all, despite the identical Lua text working correctly when pasted inline. Fixed (`phase5/db-script-operation-fix`) by deriving the registered source from each object's own already-built `CardDefinition.script_ref` (`build_initial_state`) / from the post-pregame `engine.state()` object the card actually became (`build_new_game_engine`) instead of `spec.script` directly — same fallback either origin resolves to. The general lesson: when a `CardSpec`/TOML field and a `db_script_id` DB lookup are two ways of producing the *same* downstream `CardDefinition` shape, a post-construction bridging step must read the resolved shape, not the TOML-only source field — `spec.<field>.is_some()` is never a valid proxy for "does this card have a script" once a second construction path exists.
