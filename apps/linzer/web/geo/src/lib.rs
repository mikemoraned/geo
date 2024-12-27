use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn annotate() {
    use web_sys::console;
    console::log_1(&"Fetching geojson ...".into());
}