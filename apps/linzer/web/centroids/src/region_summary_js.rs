use gloo_utils::format::JsValueSerdeExt;
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

use crate::region_summary::RegionSummary;



#[wasm_bindgen]
#[derive(Clone)]
pub struct RegionSummaryJS {
    summary: RegionSummary
}

impl RegionSummaryJS {
    pub fn new(summary: RegionSummary) -> RegionSummaryJS {
        RegionSummaryJS { summary }
    }
}

#[wasm_bindgen]
impl RegionSummaryJS {
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> usize {
        self.summary.id
    }
    #[wasm_bindgen(getter)]
    pub fn centroid(&self) -> JsValue {
        JsValue::from_serde(&self.summary.centroid).unwrap()
    }
    #[wasm_bindgen(getter)]
    pub fn bucket_width(&self) -> f64 {
        self.summary.bucket_width
    }
    #[wasm_bindgen(getter)]
    pub fn normalised(&self) -> JsValue {
        JsValue::from_serde(&self.summary.normalised).unwrap()
    }
    #[wasm_bindgen(getter)]
    pub fn dominant_degree(&self) -> JsValue {
        JsValue::from_serde(&self.summary.dominant().0).unwrap()
    }
    #[wasm_bindgen(getter)]
    pub fn dominant_length(&self) -> JsValue {
        JsValue::from_serde(&self.summary.dominant().1).unwrap()
    }
}

