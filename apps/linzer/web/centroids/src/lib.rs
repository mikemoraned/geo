
use annotated_js::AnnotatedJS;
use region_source_js::RegionSourceJS;
use testcard::TestCard;
use wasm_bindgen::prelude::*;

mod load;
mod geometry;
mod annotated;
mod annotated_js;
mod region_summary;
mod region_summary_js;
mod testcard;
mod region_source_js;
mod region_group;

#[wasm_bindgen]
pub fn testcard_at(x: f64, y: f64) -> TestCard {
    TestCard::new((x, y).into())
}

#[wasm_bindgen]
pub async fn create_source(name: String, url: String) -> Result<RegionSourceJS, JsValue> {
    Ok(RegionSourceJS::new(name, url))
}

#[wasm_bindgen]
pub async fn annotate(source: &RegionSourceJS) -> Result<AnnotatedJS, JsValue> {
    if let Ok(regions) = source.fetch().await {
        Ok(AnnotatedJS::new(regions.collection))
    }
    else {
        Err("failed to fetch regions".into())
    }
}
