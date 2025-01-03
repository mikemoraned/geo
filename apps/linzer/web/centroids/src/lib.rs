
use annotated::{Annotated, RegionSummary};
use wasm_bindgen::prelude::*;
use geo_types::GeometryCollection;
use gloo_utils::format::JsValueSerdeExt;
use web_sys::console;

mod load;
mod geometry;
mod annotated;

#[wasm_bindgen]
pub struct AnnotatedJS {
    annotated: Annotated
}

impl AnnotatedJS {
    pub fn new(collection: GeometryCollection<f64>) -> AnnotatedJS {
        AnnotatedJS { annotated: Annotated::new(collection) }
    }
}

#[wasm_bindgen]
impl AnnotatedJS {
    pub fn centroids(&mut self) -> JsValue {
        return JsValue::from_serde(&self.annotated.lazy_centroids()).unwrap();
    }

    pub fn bounds(&self) -> JsValue {
        let bounds = self.annotated.bounds();
        return JsValue::from_serde(&bounds).unwrap();
    }

    pub fn rays(&mut self) -> JsValue {
        let rays = self.annotated.rays();
        return JsValue::from_serde(&rays).unwrap();
    }

    pub fn summaries(&mut self) -> Vec<RegionSummary> {
        // let summaries = self.annotated.summaries();
        // return JsValue::from_serde(&summaries).unwrap();
        return self.annotated.summaries();
    }

    pub fn id_of_closest_centroid(&mut self, x: f64, y: f64) -> JsValue {
        let id = self.annotated.id_of_closest_centroid(&(x, y).into());
        return JsValue::from_serde(&id).unwrap();
    }
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
