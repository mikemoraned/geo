use wasm_bindgen::prelude::*;
use web_sys::console;

use crate::{geometry, load, region_group::RegionGroup};

#[wasm_bindgen]
pub struct RegionSourceJS {
    name: String,
    url: String
}

impl RegionSourceJS {
    pub fn new(name: String, url: String) -> RegionSourceJS {
        RegionSourceJS { name, url }
    }
    
    pub async fn fetch(&self) -> Result<RegionGroup, JsValue> {
        console::log_1(&format!("Fetching geojson from '{}' ...", self.url).into());
    
        if let Ok(text) = load::fetch_text(&self.url).await {
            if let Ok(collection) = load::parse_geojson_to_geometry_collection(text) {
                let size = collection.len();
                console::log_1(&format!("parsed {size} geometries").into());
    
                geometry::log_area_statistics(&collection);
                let minimum_size = 0.000001;
                let filtered = geometry::filter_out_by_area(&collection, minimum_size);
                let filtered_size = filtered.len();
                let filtered_out = size - filtered_size;
                console::log_1(&format!("filtered out {filtered_out} geometries with area <= {minimum_size}").into());
    
                Ok(RegionGroup::new(self.name.clone(), filtered))
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
}