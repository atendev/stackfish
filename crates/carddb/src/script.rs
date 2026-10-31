//! Stub script for cards with no active `card_scripts` row. `run_card_script` needs a
//! table with a callable `register_abilities`, and this registers nothing.

pub(crate) fn vanilla_stub() -> String {
    "return { register_abilities = function(c) end }".to_string()
}
