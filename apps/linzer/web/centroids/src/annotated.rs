use std::{iter::zip, vec};

use geo_types::{Geometry, GeometryCollection};
use geo::{Bearing, BoundingRect, Centroid, Coord, Distance, Haversine, InterpolatePoint, Length, Line, Point};
use web_sys::console;

use crate::region_summary::RegionSummary;

pub struct Annotated {
    collection: GeometryCollection<f64>,
    pub centroids: Vec<Point<f64>>,
    pub summaries: Vec<RegionSummary>
}

impl Annotated {
    pub fn new(collection: GeometryCollection<f64>) -> Annotated {
        let centroids = centroids(&collection);
        let summaries = summaries(&collection, &centroids);
        Annotated { collection, centroids, summaries }
    }

    pub fn bounds(&self) -> geo_types::Rect<f64> {
        self.collection.bounding_rect().unwrap()
    }

    pub fn most_similar_ids(&self, id: usize) -> Vec<usize> {
        self.most_similar_regions(id).into_iter().map(|summary| summary.id).collect()
    }

    pub fn most_similar_regions(&self, id: usize) -> Vec<RegionSummary> {
        let summaries = &self.summaries;
        let target_summary = summaries.get(id).unwrap();

        let mut distances : Vec<(RegionSummary, f64)> = summaries.clone().into_iter()
            .filter(|summary| summary.id != id)
            .map(|summary| {
                (summary.clone(), target_summary.distance_from(&summary))
            }).collect();
        distances.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        distances.into_iter().map(|(summary,_)| summary).take(10).collect()
    }

    pub fn id_of_closest_centroid(&self, coord: &Coord) -> Option<usize> {
        let mut closest = None;
        for (id, centroid) in self.centroids.iter().enumerate() {
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

    }


fn summaries(collection: &GeometryCollection<f64>, centroids: &Vec<Point<f64>>) -> Vec<RegionSummary> {
    let mut summaries: Vec<RegionSummary> = vec![];
    let size = collection.len();
    console::log_1(&format!("calculating summaries for {size} geometries").into());

    let bucket_width = 1.0;
    for (id, (geometry, centroid)) in zip(collection.iter(),centroids.iter()).enumerate() {
        if let Geometry::Polygon(polygon) = geometry {
            let mut bearing_length_pairs = vec![];
            let mut bucketed_degree_length_pairs = vec![];
            
            let points : Vec<Point<f64>> = polygon.exterior().points().collect();
            for i in 0..points.len() {
                let current = points[i].clone();
                let current_bearing = Haversine::bearing(centroid.clone(), current.clone());
                let current_length = Line::new(centroid.clone(), current.clone()).length::<Haversine>();
                bearing_length_pairs.push((current_bearing, current_length));
                
                let prev = points[(i + points.len() - 1) % points.len()].clone();
                let prev_bearing = Haversine::bearing(centroid.clone(), prev.clone());

                let bearing_diff = (current_bearing - prev_bearing).abs();
                if bearing_diff >= 0.5 {
                    // interpolate between prev and current to fill in the gaps
                    let num_samples = (bearing_diff / 0.5).ceil() as usize;
                    let step = 1.0 / num_samples as f64;
                    for i in 1..=num_samples {
                        let ratio = step * i as f64;
                        let point = Haversine::point_at_ratio_between(prev.clone(), current.clone(), ratio);
                        let bearing = Haversine::bearing(centroid.clone(), point.clone());
                        let degree = bearing.floor() as usize;
                        let length = Line::new(centroid.clone(), point.clone()).length::<Haversine>();
                        bucketed_degree_length_pairs.push((degree, length));
                    }
                };
                let current_degree = current_bearing.floor() as usize;
                bucketed_degree_length_pairs.push((current_degree, current_length));
            }
            
            let max_length = bearing_length_pairs.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).unwrap().1;

            let mut bucketed_by_degree: Vec<Option<f64>> = vec![None; 360];
            for ( degree, length ) in bucketed_degree_length_pairs.into_iter() {
                let normalised_length = length / max_length;
                if let Some(bucket) = bucketed_by_degree[degree] {
                    bucketed_by_degree[degree] = Some(bucket.max(normalised_length));
                }
                else {
                    bucketed_by_degree[degree] = Some(normalised_length);
                }
            }

            let normalised = bucketed_by_degree.into_iter().map(|bucket| {
                if let Some(bucket) = bucket {
                    bucket
                }
                else {
                    0.0
                }
            }).collect();

            let summary = RegionSummary::new(id, centroid.clone(), bucket_width, normalised);
            summaries.push(summary);
        }
    }

    console::log_1(&"calculated summaries".into());
    summaries
}

pub fn centroids(collection: &GeometryCollection<f64>) -> Vec<Point<f64>> {
    let size = collection.len();
    console::log_1(&format!("calculating centroids for {size} geometries").into());
    let mut centroids = vec![];
    for entry in collection.iter() {
        if let Some(centroid) = entry.centroid() {
            centroids.push(centroid);
        }
    }
    console::log_1(&"calculated centroids".into());
    centroids
}
