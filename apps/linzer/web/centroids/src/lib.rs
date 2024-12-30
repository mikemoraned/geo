use std::{iter::zip, vec};

use wasm_bindgen::prelude::*;
use geo_types::{Geometry, GeometryCollection};
use geo::{Area, BoundingRect, Centroid, Coord, LineString, MultiLineString, Point};
use gloo_utils::format::JsValueSerdeExt;
use web_sys::console;

mod load;

fn filter_out_by_area(collection: &GeometryCollection<f64>, minimum_size: f64) -> GeometryCollection<f64> {
    let mut filtered = vec![];
    for entry in collection {
        if entry.unsigned_area() > minimum_size {
            filtered.push(entry.clone());
        }
    }
    GeometryCollection::from(filtered)
}

fn log_area_statistics(collection: &GeometryCollection<f64>) {
    let mut areas = vec![];
    for entry in collection {
        areas.push(entry.unsigned_area());
    }
    let min = areas.iter().min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();
    let max = areas.iter().max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();
    let sum: f64 = areas.iter().sum();
    let avg = sum / areas.len() as f64;
    console::log_1(&format!("min area: {min}, max area: {max}, avg area: {avg}").into());
}

#[wasm_bindgen]
pub struct Annotated {
    collection: GeometryCollection<f64>,
    centroids: Option<Vec<geo::Point<f64>>>,
}

#[wasm_bindgen]
impl Annotated {
    pub fn centroids(&mut self) -> JsValue {
        return JsValue::from_serde(&self.lazy_centroids()).unwrap();
    }

    #[wasm_bindgen(js_name = bounds)]
    pub fn bounds_js(&self) -> JsValue {
        let bounds = self.collection.bounding_rect().unwrap();
        return JsValue::from_serde(&bounds).unwrap();
    }

    pub fn rays(&mut self) -> JsValue {
        let mut rays: Vec<MultiLineString> = vec![];
        let centroids = self.lazy_centroids();

        for (geometry, centroid) in zip(self.collection.iter(),centroids.iter()) {
            let centroid_coord: Coord = centroid.clone().into();
            if let Geometry::Polygon(polygon) = geometry {
                let mut polygon_rays = vec![];
                for point in polygon.exterior().points() {
                    let polygon_ray = LineString::new(vec![centroid_coord.clone(), point.into()]);
                    polygon_rays.push(polygon_ray);
                }
                rays.push(MultiLineString::new(polygon_rays));
            }
        }

        return JsValue::from_serde(&rays).unwrap();
    }
}

impl Annotated {
    fn new(collection: GeometryCollection<f64>) -> Annotated {
        Annotated { collection, centroids: None }
    }

    pub fn bounds(&self) -> geo_types::Rect<f64> {
        self.collection.bounding_rect().unwrap()
    }

    pub fn collection(&self) -> &GeometryCollection<f64> {
        &self.collection
    }

    pub fn lazy_centroids(&mut self) -> Vec<Point<f64>> {
        if let Some(ref centroids) = self.centroids {
            centroids.clone()
        }
        else {
            let centroids = self.calculate_centroids();
            self.centroids = Some(centroids.clone());
            centroids
        }
    }

    fn calculate_centroids(&self) -> Vec<Point<f64>> {
        let size = self.collection.len();
        console::log_1(&format!("calculating centroids for {size} geometries").into());
        let mut centroids = vec![];
        for entry in self.collection.iter() {
            if let Some(centroid) = entry.centroid() {
                centroids.push(centroid);
            }
        }
        console::log_1(&"calculated centroids".into());
        centroids
    }
}

#[wasm_bindgen]
pub async fn annotate(source_url: String) -> Result<Annotated, JsValue> {
    console::log_1(&format!("Fetching geojson from '{source_url}' ...").into());

    if let Ok(text) = load::fetch_text(source_url).await {
        if let Ok(collection) = load::parse_geojson_to_geometry_collection(text) {
            let size = collection.len();
            console::log_1(&format!("parsed {size} geometries").into());

            log_area_statistics(&collection);
            let minimum_size = 0.000001;
            let filtered = filter_out_by_area(&collection, minimum_size);
            let filtered_size = filtered.len();
            let filtered_out = size - filtered_size;
            console::log_1(&format!("filtered out {filtered_out} geometries with area <= {minimum_size}").into());

            Ok(Annotated::new(filtered))
        }
        else {
            console::log_1(&"Failed to parse geojson".into());
            Err("failed to parse geojson".into())
        }
    }
    else {
        console::log_1(&"Failed to fetch geojson".into());
        Err("failed to fetch geojson".into())
    }
}
