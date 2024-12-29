use wasm_bindgen::prelude::*;
use geo_types::{Geometry, GeometryCollection};
use geo::{Area, Centroid};
use gloo_utils::format::JsValueSerdeExt;
use web_sys::console;

async fn fetch_text(source_url: String) -> Result<String, Box<dyn std::error::Error>> {
    console::log_1(&format!("Fetching text from '{source_url}' ...").into());

    let response = reqwest::get(source_url).await?;
    match response.status() {
        reqwest::StatusCode::OK => {
            console::log_1(&"Response status: OK".into());

            let text = response.text().await?;
            console::log_1(&"Fetched text".into());
            Ok(text)
        },
        status => {
            let message = format!("Response status: NOT OK: {status}");
            console::log_1(&message.into());
            Err("failed to fetch geojson".into())
        }
    }
}

fn parse_geojson_to_geometry_collection(text: String) -> Result<GeometryCollection<f64>, Box<dyn std::error::Error>> {
    use geozero::geojson::*;
    use geozero::ToGeo;

    let geojson = GeoJsonString(text);
    if let Ok(Geometry::GeometryCollection(collection)) = geojson.to_geo() {
        console::log_1(&"Extracted geometries".into());
        Ok(collection.clone())
    }
    else {
        console::log_1(&"Failed to extract geometries".into());
        Err("failed to extract geometries".into())
    }
}

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
    collection: GeometryCollection<f64>
}

impl Annotated {
    fn new(collection: GeometryCollection<f64>) -> Annotated {
        Annotated { collection }
    }
}

#[wasm_bindgen]
impl Annotated {
    pub fn centroids(&self) -> JsValue {
        let size = self.collection.len();
        console::log_1(&format!("calculating centroids for {size} geometries").into());
        let mut centroids = vec![];
        for entry in self.collection.iter() {
            centroids.push(entry.centroid());
        }
        console::log_1(&"calculated centroids".into());
        
        JsValue::from_serde(&centroids).unwrap()
    }
}

#[wasm_bindgen]
pub async fn annotate2(source_url: String) -> Result<Annotated, JsValue> {
    console::log_1(&format!("Fetching geojson from '{source_url}' ...").into());

    if let Ok(text) = fetch_text(source_url).await {
        if let Ok(collection) = parse_geojson_to_geometry_collection(text) {
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


#[wasm_bindgen]
pub async fn annotate(source_url: String) -> Result<JsValue, JsValue> {
    console::log_1(&format!("Fetching geojson from '{source_url}' ...").into());

    if let Ok(text) = fetch_text(source_url).await {
        if let Ok(collection) = parse_geojson_to_geometry_collection(text) {
            let size = collection.len();
            console::log_1(&format!("parsed {size} geometries").into());

            log_area_statistics(&collection);
            let minimum_size = 0.000001;
            let filtered = filter_out_by_area(&collection, minimum_size);
            let filtered_size = filtered.len();
            let filtered_out = size - filtered_size;
            console::log_1(&format!("filtered out {filtered_out} geometries with area <= {minimum_size}").into());

            console::log_1(&format!("calculating centroids for {filtered_size} geometries").into());
            let mut centroids = vec![];
            for entry in filtered {
                centroids.push(entry.centroid());
            }
            console::log_1(&"calculated centroids".into());
            
            Ok(JsValue::from_serde(&centroids).unwrap())
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