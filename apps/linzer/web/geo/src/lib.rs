use wasm_bindgen::prelude::*;
use geojson::GeoJson;

#[wasm_bindgen]
pub async fn annotate(source_url: String) -> Result<(), JsValue> {
    use web_sys::console;
    console::log_1(&format!("Fetching geojson from '{source_url}' ...").into());

    let response = reqwest::get(source_url).await?;
    match response.status() {
        reqwest::StatusCode::OK => {
            console::log_1(&"Response status: OK".into());

            let text = response.text().await?;
            if let Ok(geojson) = text.parse::<GeoJson>() {
                console::log_1(&"Parsed geojson".into());
            }
            else {
                console::log_1(&"Failed to parse geojson".into());
                return Err("failed to parse geojson".into());
            }
            Ok(())        
        },
        status => {
            let message = format!("Response status: NOT OK: {status}");
            console::log_1(&message.into());
            Err("failed to fetch geojson".into())
        }
    }
}