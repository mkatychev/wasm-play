use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn add_the_letter_a(mut string: String) -> String {
    string.push_str("aaaaa");
    string
}
