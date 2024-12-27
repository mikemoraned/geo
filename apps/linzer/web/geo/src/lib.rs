use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub async fn annotate(source_url: String) -> Result<(), JsValue> {
    use web_sys::console;
    console::log_1(&format!("Fetching geojson from '{source_url}' ...").into());

    let geojson = reqwest::get(source_url)
    .await?
    .text()
    .await?;

    let size = geojson.len();

    console::log_1(&format!("Geojson size: {size}").into());

    console::log_1(&"Fetched geojson".into());
    Ok(())
}