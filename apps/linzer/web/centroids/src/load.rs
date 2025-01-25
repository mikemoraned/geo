
use geo_types::{Geometry, GeometryCollection};
use web_sys::console;

pub async fn fetch_text(source_url: &String) -> Result<String, Box<dyn std::error::Error>> {
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

pub fn parse_geojson_to_geometry_collection(text: String) -> Result<GeometryCollection<f64>, Box<dyn std::error::Error>> {
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
