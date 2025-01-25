
use wasm_bindgen::prelude::*;
use web::{annotated_js::AnnotatedJS, region_source_js::RegionSourceJS, testcard::TestCard};
use web_sys::console;

mod load;
mod geometry;
mod annotated;
mod web;
mod region_summary;
mod region_group;

#[wasm_bindgen]
pub fn testcard_at(x: f64, y: f64) -> TestCard {
    TestCard::new((x, y).into())
}

#[wasm_bindgen]
pub struct BuilderJS {
    sources: Vec<RegionSourceJS>
}

impl BuilderJS {
    pub fn new() -> BuilderJS {
        BuilderJS { sources: Vec::new() }
    }
}

#[wasm_bindgen]
impl BuilderJS {
    pub fn source(&mut self, name: String, url: String) {
        console::log_1(&format!("adding source for group '{name}' at {url}").into());
        self.sources.push(RegionSourceJS::new(name, url));
    }

    pub async fn annotate(&self) -> Result<AnnotatedJS, JsValue> {
        console::log_1(&format!("generating annotations from sources: {:?}", self.sources).into());
        let mut groups = vec![];
        for source in &self.sources {
            groups.push(source.fetch().await?);
        }
        Ok(AnnotatedJS::new(groups))
    }
}

#[wasm_bindgen]
pub fn create_builder() -> BuilderJS {
    BuilderJS::new()
}

