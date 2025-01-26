use std::{io::Cursor, vec};

use geo::{Area, Geometry};
use geo_types::GeometryCollection;
use geozero::{geojson::GeoJsonWriter, GeozeroGeometry};
use tracing::{info, warn};

pub fn filter_out_by_area(
    collection: &GeometryCollection<f64>,
    minimum_size: f64,
) -> GeometryCollection<f64> {
    let mut filtered = vec![];
    for entry in collection {
        if entry.unsigned_area() > minimum_size {
            filtered.push(entry.clone());
        }
    }
    GeometryCollection::from(filtered)
}

pub fn log_area_statistics(collection: &GeometryCollection<f64>) {
    let mut areas = vec![];
    for entry in collection {
        areas.push(entry.unsigned_area());
    }
    let min = areas
        .iter()
        .min_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap();
    let max = areas
        .iter()
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap();
    let sum: f64 = areas.iter().sum();
    let avg = sum / areas.len() as f64;
    info!("min area: {min}, max area: {max}, avg area: {avg}");
}

pub fn collection_to_geojson_string(collection: GeometryCollection) -> String {
    let mut buf = Cursor::new(Vec::new());
    let mut writer = GeoJsonWriter::new(&mut buf);
    Geometry::GeometryCollection(collection)
        .process_geom(&mut writer)
        .unwrap();

    let bytes = buf.into_inner();
    String::from_utf8(bytes).unwrap()
}

pub fn geojson_string_to_collection(
    text: String,
) -> Result<GeometryCollection<f64>, Box<dyn std::error::Error>> {
    use geozero::geojson::*;
    use geozero::ToGeo;

    let geojson = GeoJsonString(text);
    if let Ok(Geometry::GeometryCollection(collection)) = geojson.to_geo() {
        info!("Extracted geometries");
        Ok(collection.clone())
    } else {
        warn!("Failed to extract geometries");
        Err("failed to extract geometries".into())
    }
}
