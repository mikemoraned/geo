use gloo_utils::format::JsValueSerdeExt;
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};
use web_sys::console;

use crate::{annotated::Annotated, region_group::RegionGroup, region_summary_js::RegionSummaryJS};

#[wasm_bindgen]
pub struct AnnotatedJS {
    annotated: Annotated,
    summaries: Vec<RegionSummaryJS>
}

impl AnnotatedJS {
    pub fn new(groups: Vec<RegionGroup>) -> AnnotatedJS {
        let annotated = Annotated::new(groups);
        let summaries = 
            annotated.summaries.iter()
            .map(|(_id, summary)| RegionSummaryJS::new(summary.clone()))
            .collect();
        AnnotatedJS { annotated, summaries }
    }
}

#[wasm_bindgen]
impl AnnotatedJS {
    pub fn centroids_geojson(&mut self) -> JsValue {
        let centroids = self.annotated.centroids_geometry().clone();
        let geo_geometry = geo_types::GeometryCollection::from(centroids);

        let geojson = geojson::FeatureCollection::from(&geo_geometry);
        return JsValue::from_serde(&geojson).unwrap();
    }

    pub fn regions_geojson(&mut self) -> JsValue {
        let centroids = self.annotated.regions_geometry().clone();
        let geo_geometry = geo_types::GeometryCollection::from(centroids);

        let geojson = geojson::FeatureCollection::from(&geo_geometry);
        return JsValue::from_serde(&geojson).unwrap();
    }

    pub fn rays(&self) -> JsValue {
        return JsValue::from_serde(&self.annotated.rays()).unwrap();
    }

    pub fn summaries(&mut self) -> Vec<RegionSummaryJS> {
        self.summaries.clone()
    }

    pub fn most_similar_ids(&mut self, id: String, min_score: f64) -> JsValue {
        let ids = self.annotated.most_similar_ids(id, min_score);
        return JsValue::from_serde(&ids).unwrap();
    }

    pub fn most_similar_regions(&mut self, target_id: JsValue, min_score: f64) -> Vec<SimilarRegionJS> {
        console::log_1(&format!("finding regions similar to {target_id:?}, with score >= {min_score}").into());
        self.annotated.most_similar_regions(target_id.as_string().unwrap(), min_score).iter().map(|(summary, score)| {
            SimilarRegionJS::new(RegionSummaryJS::new(summary.clone()), *score)
        }).collect()
    }

    pub fn id_of_closest_centroid(&mut self, x: f64, y: f64) -> JsValue {
        let id = self.annotated.id_of_closest_centroid(&(x, y).into()).unwrap();
        return JsValue::from_str(&id);
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