use std::{f64::consts::PI, iter::zip, vec};

use geo_types::{Geometry, GeometryCollection};
use geo::{BoundingRect, Centroid, Coord, CoordsIter, LineString, MultiLineString, Point};
use serde::Serialize;
use wasm_bindgen::prelude::wasm_bindgen;
use web_sys::console;

pub struct Annotated {
    collection: GeometryCollection<f64>,
    centroids: Option<Vec<geo::Point<f64>>>,
}

impl Annotated {
    pub fn new(collection: GeometryCollection<f64>) -> Annotated {
        Annotated { collection, centroids: None }
    }

    pub fn bounds(&self) -> geo_types::Rect<f64> {
        self.collection.bounding_rect().unwrap()
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

    pub fn rays(&mut self) -> Vec<MultiLineString> {
        let mut rays: Vec<MultiLineString> = vec![];
        let centroids = self.lazy_centroids();

        for (geometry, centroid) in zip(self.collection.iter(),centroids.iter()) {
            let centroid_coord: Coord = centroid.clone().into();
            if let Geometry::Polygon(polygon) = geometry {
                let mut polygon_rays = vec![];
                for coord in polygon.exterior_coords_iter().take(10) {
                    let polygon_ray = LineString::new(vec![centroid_coord.clone(), coord]);
                    polygon_rays.push(polygon_ray);
                }
                rays.push(MultiLineString::new(polygon_rays));
            }
        }

        rays
    }

    pub fn summaries(&mut self) -> Vec<RegionSummary> {
        console::log_1(&"calculating summaries".into());
        let mut summaries: Vec<RegionSummary> = vec![];
        let centroids = self.lazy_centroids();

        for (geometry, centroid) in zip(self.collection.iter(),centroids.iter()) {
            let _centroid_coord: Coord = centroid.clone().into();
            if let Geometry::Polygon(_polygon) = geometry {
                let mut angle_length_pairs = vec![];
                // for testing, create 4 rays, one each at 0, 90, 180, and 270 degrees
                const QUARTER_OF_A_CIRCLE : f64 = 90.0;
                const MINIMUM_LENGTH : f64 = 0.1;
                const LENGTH_STRIDE : f64 = (1.0 - MINIMUM_LENGTH) / 4.;
                for i in 0..4 {
                    let multiplier = i as f64;
                    let radians = multiplier * QUARTER_OF_A_CIRCLE;
                    let length = MINIMUM_LENGTH + (multiplier * LENGTH_STRIDE);
                    angle_length_pairs.push((radians, length));
                }
                // for coord in polygon.exterior_coords_iter().take(10) {
                //     let line = Line::new(centroid_coord, coord);
                //     let slope = line.slope();
                //     let radians = slope.sin();
                //     // let radians = PI / 2.0;
                //     let length = line.length::<Euclidean>();
                //     angle_length_pairs.push((radians, length));
                // }
                let max_length = angle_length_pairs.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).unwrap().1;

                let rays = angle_length_pairs.into_iter().map(|(angle, length)| {
                    Ray { angle, length: length / max_length }
                }).collect();

                let summary = RegionSummary { centroid: centroid.clone(), rays };
                summaries.push(summary);
            }
        }

        console::log_1(&"calculated summaries".into());
        summaries
    }
}

#[wasm_bindgen]
#[derive(Serialize)]
pub struct Ray {
    angle: f64,
    length: f64,
}

#[wasm_bindgen]
#[derive(Serialize)]
pub struct RegionSummary {
    centroid: Point<f64>,
    rays: Vec<Ray>,
}