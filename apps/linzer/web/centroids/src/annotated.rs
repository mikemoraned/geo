use std::{iter::zip, vec};

use geo_types::{Geometry, GeometryCollection};
use geo::{Bearing, BoundingRect, Centroid, Coord, CoordsIter, Distance, Haversine, Length, Line, LineString, MultiLineString, Point};
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
                for coord in polygon.exterior_coords_iter() {
                    let polygon_ray = LineString::new(vec![centroid_coord.clone(), coord]);
                    polygon_rays.push(polygon_ray);
                }
                rays.push(MultiLineString::new(polygon_rays));
            }
        }

        rays
    }

    pub fn id_of_closest_centroid(&mut self, coord: &Coord) -> Option<usize> {
        let mut closest = None;
        for (id, centroid) in self.lazy_centroids().iter().enumerate() {
            let distance = Haversine::distance(coord.clone().into(), centroid.clone().into());
            if let Some((_, closest_distance)) = closest {
                if distance < closest_distance {
                    closest = Some((id, distance));
                }
            }
            else {
                closest = Some((id, distance));
            }
        }
        if let Some((id, _)) = closest {
            Some(id)
        }
        else {
            None
        }
    }

    pub fn summaries(&mut self) -> Vec<RegionSummary> {
        console::log_1(&"calculating summaries".into());
        let mut summaries: Vec<RegionSummary> = vec![];
        let centroids = self.lazy_centroids();

        for (id, (geometry, centroid)) in zip(self.collection.iter(),centroids.iter()).enumerate() {
            if let Geometry::Polygon(polygon) = geometry {
                let mut bearing_length_pairs = vec![];
                for point in polygon.exterior().points() {
                    let line = Line::new(centroid.clone(), point.clone());
                    let length = line.length::<Haversine>();
                    let bearing = Haversine::bearing(centroid.clone(), point.clone());
                    bearing_length_pairs.push((bearing, length));
                }
                let max_length = bearing_length_pairs.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).unwrap().1;

                let rays = bearing_length_pairs.into_iter().map(|(bearing, length)| {
                    Ray { bearing, length: length / max_length }
                }).collect();

                let summary = RegionSummary { id, centroid: centroid.clone(), rays };
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
    bearing: f64,
    length: f64,
}

#[wasm_bindgen]
#[derive(Serialize)]
pub struct RegionSummary {
    id: usize,
    centroid: Point<f64>,
    rays: Vec<Ray>,
}