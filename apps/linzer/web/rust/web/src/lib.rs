
use annotated_js::AnnotatedJS;
use region_source_js::RegionSourceJS;
use testcard::TestCard;
use wasm_bindgen::prelude::*;

mod annotated_js;
mod region_signature_js;
mod region_source_js;
mod testcard;


#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    wasm_tracing::set_as_global_default();

    tracing::info!("starting wasm module");

    Ok(())
}

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
        tracing::info!("adding source for group '{}' at {}", name, url);
        self.sources.push(RegionSourceJS::new(name, url));
    }

    pub async fn annotate(&self) -> Result<AnnotatedJS, JsValue> {
        tracing::info!("generating annotations from sources: {:?}", self.sources);
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

