use domain::{geometry::{filter_out_by_area, geojson_string_to_collection, log_area_statistics}, region::region_group::RegionGroup};
use tracing::{info, warn};
use wasm_bindgen::prelude::*;

use crate::load;

#[wasm_bindgen]
#[derive(Debug)]
pub struct RegionSourceJS {
    name: String,
    url: String
}

impl RegionSourceJS {
    pub fn new(name: String, url: String) -> RegionSourceJS {
        RegionSourceJS { name, url }
    }
    
    pub async fn fetch(&self) -> Result<RegionGroup, JsValue> {
        info!("Fetching geojson from '{}' ...", self.url);
    
        if let Ok(text) = load::fetch_text(&self.url).await {
            if let Ok(collection) = geojson_string_to_collection(text) {
                let size = collection.len();
                info!("parsed {size} geometries");
    
                log_area_statistics(&collection);
                let minimum_size = 0.000001;
                let filtered = filter_out_by_area(&collection, minimum_size);
                let filtered_size = filtered.len();
                let filtered_out = size - filtered_size;
                info!("filtered out {filtered_out} geometries with area <= {minimum_size}");
    
                Ok(RegionGroup::new(self.name.clone(), filtered))
            }
            else {
                warn!("Failed to parse geojson");
                Err("failed to parse geojson".into())
            }
        }
        else {
            warn!("Failed to fetch geojson");
            Err("failed to fetch geojson".into())
        }
    }
}