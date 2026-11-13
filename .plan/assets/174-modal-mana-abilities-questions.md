# 174 Modal mana abilities: questions report

Ticket question: what does a "choose one" mana ability need (CR 605.3b, 700.2) so that the load error 115 put on mana modes in an activated `:SetModes` can be lifted?

## Evidence gathered

- **The load error.** `decode_modes` in `crates/engine/src/script/decode.rs` refuses an `AddMana` or `AddManaChoice` mode when the owner is `ModeOwner::Activated`. `ManaAbilityBuilder` in `script/builders.rs` has no `:SetModes` and carries `// TODO: modal mana abilities CR 605.3b`. `docs/lua/not-available-yet.md`, `docs/lua/mana-abilities.md`, `docs/lua/activated-abilities.md` and `docs/implementation/39-lua-modes.md` list the gap.
- **No printed card needs it.** A search of the card database beside the main checkout (`/home/aten/Documents/Github/stackfish-db/stackfish.db` on this machine) found 14 cards with a modal activated ability. The query was `SELECT name FROM cards WHERE oracle_text LIKE '%}: Choose one%' OR oracle_text LIKE '%}: Choose two%' OR oracle_text LIKE '%: Choose one or%';`. A second query, `SELECT name, oracle_text FROM cards WHERE oracle_text LIKE '%•%Add %' AND oracle_text NOT LIKE '%Additional%';`, listed every card with a bulleted mode that mentions adding mana. None of their modes adds mana. Every card where a bulleted mode adds mana is something else: a trigger (Zuko, Conflicted; Galadriel, Light of Valinor; Frontier Siege), a spell (Jeska's Will, Open the Omenpaths) or a loyalty ability (Chandra, Flameshaper +2, which CR 605.1a excludes). Modes that create a Treasure or Powerstone token make a token and add no mana.
- **Rules.** CR 605.1a makes an activated ability a mana ability when it has no target, could add mana and is not a loyalty ability. CR 605.5a says an ability with a target is not a mana ability. CR 605.3b says a mana ability resolves at once without the stack. CR 601.2b and 700.2a say the controller announces the mode choice, and CR 602.2b applies that casting step to activation. CR 700.2c treats a mode's targets as absent unless that mode is chosen.
- **A related hole, proved by running code.** A throwaway test (since deleted) cast a sorcery whose `s.modes()` had `{ operation = Game.Ops.AddMana("R", 3) }`, chose that mode and resolved it. P0's pool held 0 red afterwards. `resolve_misc_or_script_effect` in `engine/resolve/misc_effects.rs` treats `AddMana` and `AddManaChoice` on the stack as no-ops, on the assumption that only mana abilities use them. Nothing refuses `Game.Ops.AddMana` as a spell operation, a trigger operation or a spell or trigger mode, so the script loads and the mana never arrives. About 70 instants and sorceries (Dark Ritual, Pyretic Ritual, Jeska's Will) and about 125 triggers add mana on resolution. CR 605.5a and 605.5b say these use the stack and resolve normally. `is_modal_resolvable` also makes an `AddMana` mode solo-only, which Jeska's Will's "choose both" needs lifted.

## Questions

### Q1. Should the engine build modal mana abilities now?

- A. Build them: `:SetModes` on the mana builder, modes on the mana branch of `ActivateAbility`, and a mode range on the Payment Decision's mana ability listing.
- B. Keep the load error. Record the shape as out of scope because no printed card has it, and leave the existing TODOs and docs in place.
- C. Keep the load error and leave the ticket open until a card appears.

Recommendation: B. The search found no card, so a build would add engine paths, wire fields and tests that no card exercises. The load error already stops a script from writing the shape by accident. The TODOs stay, as the CLAUDE.md comment rules require.

### Q2. If a card ever needs it, where is the mode announced for an ability that does not use the stack?

- A. On the existing `ActivateAbility.modes` field that 115 added, chosen in the same action that activates the ability. The mana branch picks the chosen mode's effect and then runs the `FreeMana` or `PaidMana` plan.
- B. A separate mode Decision after the activation.

Recommendation: A. CR 601.2b and 700.2a put the mode announcement inside the activation process that CR 602.2b sets out, and CR 605.3b gives a mana ability no point after activation where a second Decision could open. A matches how 115 announces modes on stack abilities.

### Q3. If built, which modal abilities count as mana abilities?

- A. Judge the printed ability as a whole: it is a mana ability only when no mode has a target and at least one mode adds mana. A modal ability with any targeted mode always uses the stack.
- B. Judge each activation by its chosen modes. CR 700.2c treats an unchosen mode's targets as absent, so an activation whose chosen modes have no target and could add mana meets CR 605.1a and resolves at once (CR 605.3b). An activation that chooses a targeted mode is a normal stack ability (CR 605.5a).

Recommendation: B. CR 700.2c means a targeted mode the player did not choose gives the activation no target, so A would send a mana-only activation through the stack for a reason the rules ignore. For the Payment Decision listing (CR 605.3a), the engine offers only the modes that have no target and add mana, so every activation made during payment is a mana ability.

### Q4. If built, how does a Payment Decision list a modal mana ability, and can the proof bot pay with one?

- A. The payer's mana ability listing (031) gains the same `modes: Option<ModeRange>` shape as `ActivateEntry`, limited to the modes Q3 allows during payment. How the bot picks among those modes stays open until a card exists to test it.
- B. Leave the listing alone and refuse modal mana abilities during payment.

Recommendation: A, recorded as the design only. Nothing is built now, and the proof deck gains no card because no card exists to add.

### Q5. What happens to the hole found while answering, where `AddMana` resolving from the stack does nothing?

- A. Surface one build ticket: resolve `AddMana` and `AddManaChoice` from the stack into the controller's pool (CR 605.5a, 605.5b), make an `AddMana` mode joinable in `is_modal_resolvable`, and decide how a color choice is announced for `AddManaChoice` on a spell or trigger.
- B. Surface a build ticket that turns the hole into a load error, and leave the build for later.
- C. Fold the fix into this grilling ticket.

Recommendation: A. A silent no-op breaks rituals and mana-adding triggers with no error, and about 195 printed cards use the shape. A load error alone would turn those cards from wrong into missing. The fix is engine work, which a grilling ticket does not ship.
