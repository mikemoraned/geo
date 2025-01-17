use std::vec;

use geo_types::GeometryCollection;
use geo::Area;
use web_sys::console;

pub fn filter_out_by_area(collection: &GeometryCollection<f64>, minimum_size: f64) -> GeometryCollection<f64> {
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
    let min = areas.iter().min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();
    let max = areas.iter().max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();
    let sum: f64 = areas.iter().sum();
    let avg = sum / areas.len() as f64;
    console::log_1(&format!("min area: {min}, max area: {max}, avg area: {avg}").into());
}
