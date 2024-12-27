use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    // Import `alert` from JS runtime
    fn alert(s: &str);
}

// Export `greet`
#[wasm_bindgen]
pub fn greet() {
    alert("Hello, world!");
}