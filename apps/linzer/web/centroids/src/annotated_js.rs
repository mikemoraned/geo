use geo::GeometryCollection;
use gloo_utils::format::JsValueSerdeExt;
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

use crate::{annotated::Annotated, region_summary_js::RegionSummaryJS};

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
        return JsValue::from_serde(&self.annotated.centroids).unwrap();
    }

    pub fn bounds(&self) -> JsValue {
        let bounds = self.annotated.bounds();
        return JsValue::from_serde(&bounds).unwrap();
    }

    pub fn summaries(&mut self) -> Vec<RegionSummaryJS> {
        return self.annotated.summaries().iter().map(|summary| RegionSummaryJS::new(summary.clone())).collect();
    }

    pub fn most_similar_ids(&mut self, id: usize) -> JsValue {
        let ids = self.annotated.most_similar_ids(id);
        return JsValue::from_serde(&ids).unwrap();
    }

    pub fn id_of_closest_centroid(&mut self, x: f64, y: f64) -> JsValue {
        let id = self.annotated.id_of_closest_centroid(&(x, y).into());
        return JsValue::from_serde(&id).unwrap();
    }
}