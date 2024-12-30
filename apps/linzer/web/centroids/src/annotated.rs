use std::{iter::zip, vec};

use geo_types::{Geometry, GeometryCollection};
use geo::{BoundingRect, Centroid, Coord, LineString, MultiLineString, Point};
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
                for point in polygon.exterior().points() {
                    let polygon_ray = LineString::new(vec![centroid_coord.clone(), point.into()]);
                    polygon_rays.push(polygon_ray);
                }
                rays.push(MultiLineString::new(polygon_rays));
            }
        }

        return rays;
    }
}
