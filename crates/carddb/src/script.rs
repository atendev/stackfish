//! A vanilla card (no `card_scripts` row with `is_active = 1`) still needs a
//! `ScriptReference::Inline` - `CardDefinition.script_ref` isn't `Optional`.
//! `ScriptRuntime::run_card_script` expects the source to evaluate to a
//! table with a callable `register_abilities` field (the `s.
//! register_abilities(c)` module convention); this is the smallest source
//! that satisfies that shape while registering nothing.

pub fn vanilla_stub() -> String {
    "return { register_abilities = function(c) end }".to_string()
}
