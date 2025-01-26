
use domain::{geometry::collection_to_geojson_string, region::region_group::RegionGroup};
use gloo_utils::format::JsValueSerdeExt;
use tracing::info;
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

use domain::annotated::Annotated;

use super::region_signature_js::RegionSignatureJS;

#[wasm_bindgen]
pub struct AnnotatedJS {
    annotated: Annotated,
    signatures: Vec<RegionSignatureJS>
}

impl AnnotatedJS {
    pub fn new(groups: Vec<RegionGroup>) -> AnnotatedJS {
        let annotated = Annotated::new(groups);
        let signatures = 
            annotated.signatures.iter()
            .map(|(_id, signature)| RegionSignatureJS::new(signature.clone()))
            .collect();
        AnnotatedJS { annotated, signatures }
    }
}

#[wasm_bindgen]
impl AnnotatedJS {
    pub fn centroids_geojson_string(&mut self) -> JsValue {
        let centroids = self.annotated.centroids_geometry().clone();
        let collection = geo_types::GeometryCollection::from(centroids);

        return JsValue::from_str(&collection_to_geojson_string(collection));
    }

    pub fn regions_geojson_string(&mut self) -> JsValue {
        let regions = self.annotated.regions_geometry().clone();
        let collection = geo_types::GeometryCollection::from(regions);

        return JsValue::from_str(&collection_to_geojson_string(collection));
    }

    pub fn rays(&self) -> JsValue {
        return JsValue::from_serde(&self.annotated.rays()).unwrap();
    }

    pub fn signatures(&mut self) -> Vec<RegionSignatureJS> {
        self.signatures.clone()
    }

    pub fn most_similar_ids(&mut self, id: String, min_score: f64) -> JsValue {
        let ids = self.annotated.most_similar_ids(id, min_score);
        return JsValue::from_serde(&ids).unwrap();
    }

    pub fn most_similar_regions(&mut self, target_id: JsValue, min_score: f64) -> Vec<SimilarRegionJS> {
        info!("finding regions similar to {target_id:?}, with score >= {min_score}");
        self.annotated.most_similar_regions(target_id.as_string().unwrap(), min_score).iter().map(|(signature, score)| {
            SimilarRegionJS::new(RegionSignatureJS::new(signature.clone()), *score)
        }).collect()
    }

    pub fn id_of_closest_centroid(&mut self, x: f64, y: f64) -> JsValue {
        let id = self.annotated.id_of_closest_centroid(&(x, y).into()).unwrap();
        return JsValue::from_str(&id);
    }
}

#[wasm_bindgen]
pub struct SimilarRegionJS {
    signature: RegionSignatureJS,
    score: f64
}

impl SimilarRegionJS {
    pub fn new(signature: RegionSignatureJS, score: f64) -> SimilarRegionJS {
        SimilarRegionJS { signature, score }
    }
}

#[wasm_bindgen]
impl SimilarRegionJS {
    #[wasm_bindgen(getter)]
    pub fn signature(&self) -> RegionSignatureJS {
        self.signature.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn score(&self) -> f64 {
        self.score
    }
}