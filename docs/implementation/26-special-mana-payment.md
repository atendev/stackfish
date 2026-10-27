# 26-special-mana-payment

## Slice: special-mana-payment (branch `phase5/special-mana-payment`)

Ticket 030. The backlog said hybrid, Phyrexian, snow and `{X}` payment were missing. They were mostly there. The
engine already parsed every symbol (`ManaCost::parse`), resolved X before payment (`ManaCost::resolve_x` in
`commands/cast.rs` and `activate.rs`), and validated hybrid, Phyrexian and `{S}` plans (`PaymentPlan::validate`). The real
gaps were the bot, the `{C}` cost symbol, and snow mana that nothing produced.

- `{C}` (CR 107.4c). `atom_branches` returned `None` for `AtomicManaSymbol::Colorless`, so any cost with `{C}` failed
  validation. It now has its own `PayBranch::Colorless`, payable with colorless mana (plain or snow) and nothing else.
  The old unit test that pinned this as unsupported became two tests; the bare `{X}` assertion stays, because callers
  resolve X first.
- Snow production (CR 106.3). `validate_activate_ability` reads the source's effective supertypes. If they include Snow,
  `ManaUnit::from_snow_source` turns each produced `Colored` or `Colorless` unit into its snow form. Restricted units are
  left alone, because the pool has no snow-restricted unit. No Lua change: `Game.Ops.AddMana` needed no snow letter.
  Documented in `docs/lua/mana-abilities.md`.
- Wire. `mana_cost_string` printed hybrid Phyrexian as `{U/P/G}`, which `ManaCost::parse` rejects. It now prints
  `{U/G/P}` with the Phyrexian color first. A round-trip test covers six costs.
- Bot. Payment moved from `bot.rs` into `bot_payment.rs`. `plan_payment` parses each symbol into branches (color,
  colorless, snow, generic, life) and searches them with backtracking, so two `{W/U}` symbols take different units and a
  `{S}` keeps its snow unit away from `{1}`. Mana comes before life, and a hybrid half that is a color comes before a
  generic half.
- Bot pool. `Pool` now counts snow units, so a snow unit pays `{S}` and a plain unit never does.
- Bot life. The bot pays a Phyrexian pip with 2 life only while its tracked life stays above 1. Tracking is
  `starting_life` (default 20, `Bot::with_starting_life`) plus `life_changed` events minus `damage_dealt` to its own
  player. Combat damage emits no `LifeChanged`, so the first version of the test found the bot paying 4 life at a
  life total below 4 (seed 7). Gaining life emits nothing either, so the estimate can only be too low.
- Bot X. A cast is judged affordable at X=0, and the bot picks X between 0 and the largest value of 0 to 2 that its pool
  can pay.
- Verify: `phase5_special_mana_payment.rs` (9 engine tests: `{C}` paid and refused, hybrid, X, `{X}{X}`, snow land
  output, non-snow land output, `{S}` from a land); 12 `bot_payment` unit tests; `phase5_special_mana_proof_game.rs`
  (30 two-player games with a nine-creature deck, no rejected Answer, no `script_error`, every costed creature entered
  the battlefield, snow mana and 2-life payments seen; same seed gives the same hash). Full workspace suite green, and
  `real_card_bot -- --ignored` still plays 30 games.

## Open

- A triggered mana ability's mana is not marked snow. Triggered mana abilities are not built.
- The bot never spends a restricted unit, and the Payment Decision still does not list sacrifice, discard, tap or
  graveyard-exile choices.
- The engine does not emit `LifeChanged` for combat damage or gaining life (backlog item 6). The bot works around it.
- The real-card decks in `docs/real_card_set.md` use none of these symbols. Covering real cards needs scripts for cards
  such as Dismember, Deathrite Shaman and Glaring Fleshraker in `../stackfish-db`.
- `Branch::Snow` in the bot takes the first snow bucket and does not backtrack over which one.
