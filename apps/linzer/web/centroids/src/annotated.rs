
use std::collections::HashMap;

use geo::{Bearing, Coord, CoordsIter, Distance, Haversine, InterpolatePoint, Length, Line, LineString, MultiLineString, Point};
use web_sys::console;

use crate::{region_group::RegionGroup, region_summary::RegionSummary};

pub struct Annotated {
    groups: Vec<RegionGroup>,
    pub centroids: Vec<Point<f64>>,
    pub summaries: HashMap<String,RegionSummary>
}

impl Annotated {
    pub fn new(groups: Vec<RegionGroup>) -> Annotated {
        let centroids = centroids(&groups);
        let summaries = summaries(&groups);
        Annotated { groups, centroids, summaries }
    }

    pub fn rays(&self) -> Vec<MultiLineString> {
        let mut rays: Vec<MultiLineString> = vec![];

        for group in self.groups.iter() {   
            for (polygon, _id, centroid) in group.geometries().iter() {
                let centroid_coord: Coord = centroid.clone().into();
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

    pub fn most_similar_ids(&self, id: String, min_score: f64) -> Vec<String> {
        self.most_similar_regions(id, min_score).into_iter().map(|(summary, _)| summary.id).collect()
    }

    pub fn most_similar_regions(&self, target_id: String, min_score: f64) -> Vec<(RegionSummary,f64)> {
        let target_summary = self.summaries.get(&target_id).unwrap();

        let mut distances : Vec<(RegionSummary, f64)> = self.summaries.iter()
            .filter(|(id, _summary)| target_id.as_str() != id.as_str())
            .map(|(_id, summary)| {
                (summary.clone(), target_summary.distance_from(&summary))
            }).collect();
        distances.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        let scores : Vec<(RegionSummary,f64)> = distances.into_iter().map(|(summary, score)| (summary, 1.0 - score)).collect();

        scores.into_iter().filter(|(_, score)| *score >= min_score).collect()
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


fn summaries(groups: &[RegionGroup]) -> HashMap<String, RegionSummary> {
    let mut summaries= HashMap::new();
    for group in groups.iter() {        

        let size = group.geometries().len();
        console::log_1(&format!("group '{}': calculating summaries for {} geometries", group.name, size).into());

        let bucket_width = 1.0;
        for (polygon, id, centroid) in group.geometries().iter() {
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

            let summary = RegionSummary::new(id.clone(), group.name.clone(), centroid.clone(), bucket_width, normalised);
            summaries.insert(id.clone(), summary);
            
        }

        console::log_1(&"calculated summaries".into());
    }
    summaries
}

pub fn centroids(groups: &[RegionGroup]) -> Vec<Point<f64>> {
    let mut centroids = vec![];
    for group in groups.iter() {        
        for (_polygon, _id, centroid)  in group.geometries().iter() {
            centroids.push(centroid.clone());
        }
    }
    centroids   
}
