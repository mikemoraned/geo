
use annotated_js::AnnotatedJS;
use testcard::TestCard;
use wasm_bindgen::prelude::*;
use web_sys::console;

mod load;
mod geometry;
mod annotated;
mod annotated_js;
mod region_summary;
mod region_summary_js;
mod testcard;

#[wasm_bindgen]
pub fn testcard_at(x: f64, y: f64) -> TestCard {
    TestCard::new((x, y).into())
}

#[wasm_bindgen]
pub async fn annotate(source_url: String) -> Result<AnnotatedJS, JsValue> {
    console::log_1(&format!("Fetching geojson from '{source_url}' ...").into());

    if let Ok(text) = load::fetch_text(source_url).await {
        if let Ok(collection) = load::parse_geojson_to_geometry_collection(text) {
            let size = collection.len();
            console::log_1(&format!("parsed {size} geometries").into());

            geometry::log_area_statistics(&collection);
            let minimum_size = 0.000001;
            let filtered = geometry::filter_out_by_area(&collection, minimum_size);
            let filtered_size = filtered.len();
            let filtered_out = size - filtered_size;
            console::log_1(&format!("filtered out {filtered_out} geometries with area <= {minimum_size}").into());

            Ok(AnnotatedJS::new(filtered))
        }
        else {
            console::log_1(&"Failed to parse geojson".into());
            Err("failed to parse geojson".into())
        }
    }
    else {
        console::log_1(&"Failed to fetch geojson".into());
        Err("failed to fetch geojson".into())
    }
}
