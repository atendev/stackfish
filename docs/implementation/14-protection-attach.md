# 14-protection-attach

## Protection (CR 702.16)

Enforced at damage, targeting, and blocking as an OR across six quality fields on `EffectiveCharacteristics`:

- `protection_colors: ColorSet` — `AddProtectionColor`
- `protection_card_types: CardTypeSet` — `AddProtectionCardType`
- `protection_subtypes: Vec<Subtype>` — `AddProtectionSubtype`
- `protection_names: Vec<String>` — `AddProtectionName`
- `protection_everything: bool` — `AddProtectionEverything`
- `protection_players: Vec<FilterController>` — `AddProtectionPlayer` (`You`/`Opponent`, rejects `Any`)

All applied in layer 6; `RemoveAllAbilities` clears all six fields. `apply_protection` runs inside `apply_replacements_from` so mid-resolution redirects are rechecked. `target_is_legal`/`DeclareBlockers` consult `target_has_protection_from`.

`target_is_legal` gained a `source: ObjectId` parameter to know the source's color/type/name.

Protection from player uses the protected object's own controller as the reference point; no true arbitrary `PlayerId` binding yet.

Only CR 702.16b (Enchant/Equip) remains open — needs attach machinery, not a protection-specific gap.

## Aura attach (CR 303.4)

- `GameObject.attached_to`/`attachments` bidirectional; `commands::attach_permanent` is the sole writer.
- `ResolveStackTop` attaches an Aura on battlefield entry to its first (only) legal target.
- `sba::collect_illegally_attached_auras` feeds `resolve_deaths` (non-destroy) when target is gone/illegal/protected.
- `retire_object` purges the departed object's id from its parent's `attachments`.
- `GameEventKind::Attached` logs in-place attach changes.
- CR 303.4d (`phase5/aura-self-enchant`): two further SBA checks alongside the above -
  `sba::collect_self_enchanted_auras` (an Aura attached to itself dies; only reachable via a
  directly-constructed fixture today, no attach call site can target an Aura at itself) and
  `sba::collect_creature_auras` (an Aura whose *effective* card types include Creature - layer 4
  `AddCardType` - unattaches via `commit_unattach`, shared with Equipment's own 704.5n handling,
  then dies in the same pass). CR 303.4e (only the Aura's own controller activates its own
  ability) was already correct by construction - `ActivateAbility`'s actor check reads the
  ability's registered source's controller, which is the Aura itself for the Aura's own ability -
  confirmed by a regression test, no code change needed. Still open: CR 303.4d's "can't enchant
  more than one object" clause (no attach call site can attempt that), CR 303.4f/g/h/i (a
  permanent entering the battlefield pre-attached by a non-casting, non-equip-activation effect -
  no such effect exists in this engine yet), CR 303.4k (morph - no face-down mechanic exists), and
  CR 303.4m ("the enchanted X" as a script reference - no such reference plumbing exists yet,
  Aura-specific or generic).

## Equipment / Equip (CR 702.6)

- `AbilityEffect::Attach` primitive; `ActivateAbility` enforces "only as sorcery" for `Attach`.
- `commands::attach_permanent` handles reattach safely for Equipment.
- `sba::collect_illegally_attached_equipment`/`commit_unattach` keeps Equipment on battlefield, unattached in place.
- `Game.Tokens.Treasure` deferred — needs coroutine color choice.

## Attach-related keywords

- `Flying`, `Reach`, `Menace` were first combat keyword checks.
- `Fear`, `Intimidate`, `Shadow`, `Horsemanship` added as evasion keywords in `DeclareBlockers`.
- `KeywordSet` widened to `u32` to fit new bits.
- `Landwalk` and `Banding` still open; Flanking/Rampage are triggered abilities, not blocking restrictions.
