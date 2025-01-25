use std::io::{BufWriter, Cursor};

use geo::Geometry;
use geozero::geojson::GeoJsonWriter;
use geozero::{geo_types::GeoWriter, geojson::{GeoJsonReader}, GeozeroDatasource, GeozeroGeometry};
use gloo_utils::format::JsValueSerdeExt;
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};
use web_sys::console;

use crate::{annotated::Annotated, region::region_group::RegionGroup};

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
    pub fn centroids_geojson(&mut self) -> JsValue {
        let centroids = self.annotated.centroids_geometry().clone();
        let geo_geometry = geo_types::GeometryCollection::from(centroids);

        let geojson = geojson::FeatureCollection::from(&geo_geometry);
        return JsValue::from_serde(&geojson).unwrap();
    }

    pub fn centroids_geojson_string(&mut self) -> JsValue {
        let centroids = self.annotated.centroids_geometry().clone();
        let geo_geometry = geo_types::GeometryCollection::from(centroids);

        let mut buf = Cursor::new(Vec::new());
        let mut geo_writer = GeoJsonWriter::new(&mut buf);
        Geometry::GeometryCollection(geo_geometry).process_geom(&mut geo_writer).unwrap();

        let bytes = buf.into_inner();
        let string = String::from_utf8(bytes).unwrap();
        return JsValue::from_str(&string);
    }

    pub fn regions_geojson(&mut self) -> JsValue {
        let centroids = self.annotated.regions_geometry().clone();
        let geo_geometry = geo_types::GeometryCollection::from(centroids);

        let geojson = geojson::FeatureCollection::from(&geo_geometry);
        return JsValue::from_serde(&geojson).unwrap();
    }

    pub fn regions_geojson_string(&mut self) -> JsValue {
        let centroids = self.annotated.regions_geometry().clone();
        let geo_geometry = geo_types::GeometryCollection::from(centroids);

        let mut buf = Cursor::new(Vec::new());
        let mut geo_writer = GeoJsonWriter::new(&mut buf);
        Geometry::GeometryCollection(geo_geometry).process_geom(&mut geo_writer).unwrap();

        let bytes = buf.into_inner();
        let string = String::from_utf8(bytes).unwrap();
        return JsValue::from_str(&string);
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
        console::log_1(&format!("finding regions similar to {target_id:?}, with score >= {min_score}").into());
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