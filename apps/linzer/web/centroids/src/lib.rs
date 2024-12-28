use wasm_bindgen::prelude::*;
use geo_types::{Geometry, GeometryCollection};
use geojson::{quick_collection, GeoJson};
use geo::Centroid;

#[wasm_bindgen]
pub async fn annotate(source_url: String) -> Result<(), JsValue> {
    use web_sys::console;
    console::log_1(&format!("Fetching geojson from '{source_url}' ...").into());

    let response = reqwest::get(source_url).await?;
    match response.status() {
        reqwest::StatusCode::OK => {
            console::log_1(&"Response status: OK".into());

            let text = response.text().await?;
            console::log_1(&"Fetched geojson".into());
            if let Ok(geojson) = text.parse::<GeoJson>() {
                console::log_1(&"Parsed geojson".into());
                let collection: GeometryCollection<f64> = quick_collection(&geojson).unwrap();
                if let Some(Geometry::GeometryCollection(entries)) = collection.0.get(0) {
                    let size = entries.len();
                    console::log_1(&format!("calculating centroids for {size} geometries").into());
                    let mut centroids = vec![];
                    for entry in entries {
                        centroids.push(entry.centroid());
                    }
                    console::log_1(&"calculated centroids".into());
                }
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