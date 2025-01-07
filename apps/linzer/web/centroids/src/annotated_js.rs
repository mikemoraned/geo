use geo::GeometryCollection;
use gloo_utils::format::JsValueSerdeExt;
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

use crate::{annotated::Annotated, region_summary_js::RegionSummaryJS};

#[wasm_bindgen]
pub struct AnnotatedJS {
    annotated: Annotated,
    summaries: Vec<RegionSummaryJS>
}

impl AnnotatedJS {
    pub fn new(collection: GeometryCollection<f64>) -> AnnotatedJS {
        let annotated = Annotated::new(collection);
        let summaries = annotated.summaries.iter().map(|summary| RegionSummaryJS::new(summary.clone())).collect();
        AnnotatedJS { annotated, summaries }
    }
}

#[wasm_bindgen]
impl AnnotatedJS {
    pub fn centroids(&mut self) -> JsValue {
        return JsValue::from_serde(&self.annotated.centroids).unwrap();
    }

    pub fn centroid(&self) -> JsValue {
        let centroid = self.annotated.centroid();
        return JsValue::from_serde(&[ centroid.x, centroid.y ]).unwrap();
    }

    pub fn bounds(&self) -> JsValue {
        let bounds = self.annotated.bounds();
        return JsValue::from_serde(&bounds).unwrap();
    }

    // rays
    pub fn rays(&self) -> JsValue {
        return JsValue::from_serde(&self.annotated.rays()).unwrap();
    }

    pub fn summaries(&mut self) -> Vec<RegionSummaryJS> {
        self.summaries.clone()
    }

    pub fn most_similar_ids(&mut self, id: usize, min_score: f64) -> JsValue {
        let ids = self.annotated.most_similar_ids(id, min_score);
        return JsValue::from_serde(&ids).unwrap();
    }

    pub fn most_similar_regions(&mut self, id: usize, min_score: f64) -> Vec<SimilarRegionJS> {
        self.annotated.most_similar_regions(id, min_score).iter().map(|(summary, score)| {
            SimilarRegionJS::new(RegionSummaryJS::new(summary.clone()), *score)
        }).collect()
    }

    pub fn id_of_closest_centroid(&mut self, x: f64, y: f64) -> JsValue {
        let id = self.annotated.id_of_closest_centroid(&(x, y).into());
        return JsValue::from_serde(&id).unwrap();
    }
}

#[wasm_bindgen]
pub struct SimilarRegionJS {
    summary: RegionSummaryJS,
    score: f64
}

impl SimilarRegionJS {
    pub fn new(summary: RegionSummaryJS, score: f64) -> SimilarRegionJS {
        SimilarRegionJS { summary, score }
    }
}

#[wasm_bindgen]
impl SimilarRegionJS {
    #[wasm_bindgen(getter)]
    pub fn summary(&self) -> RegionSummaryJS {
        self.summary.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn score(&self) -> f64 {
        self.score
    }
}