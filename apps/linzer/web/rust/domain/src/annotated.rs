use std::collections::HashMap;

use crate::region::region_group::RegionGroup;
use geo::{
    Bearing, Coord, CoordsIter, Distance, Geometry, Haversine, InterpolatePoint, Length, Line,
    LineString, MultiLineString, Point,
};
use tracing::info;

use crate::signature::region_signature::RegionSignature;

pub struct Annotated {
    groups: Vec<RegionGroup>,
    pub signatures: HashMap<String, RegionSignature>,
}

impl Annotated {
    pub fn new(groups: Vec<RegionGroup>) -> Annotated {
        let signatures = signatures(&groups);
        Annotated { groups, signatures }
    }

    pub fn centroids_geometry(&self) -> Vec<Geometry<f64>> {
        let mut centroids = vec![];
        for group in self.groups.iter() {
            for (_polygon, _id, centroid) in group.geometries() {
                centroids.push(Geometry::Point(*centroid));
            }
        }
        centroids
    }

    pub fn regions_geometry(&self) -> Vec<Geometry<f64>> {
        let mut polygons = vec![];
        for group in self.groups.iter() {
            for (polygon, _id, _centroid) in group.geometries() {
                polygons.push(Geometry::Polygon(polygon.clone()));
            }
        }
        polygons
    }

    pub fn rays(&self) -> Vec<MultiLineString> {
        let mut rays: Vec<MultiLineString> = vec![];

        for group in self.groups.iter() {
            for (polygon, _id, centroid) in group.geometries().iter() {
                let centroid_coord: Coord = (*centroid).into();
                let mut polygon_rays = vec![];
                for coord in polygon.exterior_coords_iter() {
                    let polygon_ray = LineString::new(vec![centroid_coord, coord]);
                    polygon_rays.push(polygon_ray);
                }
                rays.push(MultiLineString::new(polygon_rays));
            }
        }

        rays
    }

    pub fn most_similar_ids(&self, id: String, min_score: f64) -> Vec<String> {
        self.most_similar_regions(id, min_score)
            .into_iter()
            .map(|(signature, _)| signature.id)
            .collect()
    }

    pub fn most_similar_regions(
        &self,
        target_id: String,
        min_score: f64,
    ) -> Vec<(RegionSignature, f64)> {
        let target_summary = self.signatures.get(&target_id).unwrap();
        info!("finding regions similar to {target_id}, with score >= {min_score}");

        let mut distances: Vec<(RegionSignature, f64)> = self
            .signatures
            .iter()
            .filter(|(id, _summary)| target_id.as_str() != id.as_str())
            .map(|(_id, signature)| (signature.clone(), target_summary.distance_from(signature)))
            .collect();
        distances.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        let scores: Vec<(RegionSignature, f64)> = distances
            .into_iter()
            .map(|(signature, score)| (signature, 1.0 - score))
            .collect();

        scores
            .into_iter()
            .filter(|(_, score)| *score >= min_score)
            .collect()
    }

    pub fn id_of_closest_centroid(&self, coord: &Coord) -> Option<String> {
        let mut closest = None;
        for group in self.groups.iter() {
            for (_polygon, id, centroid) in group.geometries() {
                let distance = Haversine::distance((*coord).into(), *centroid);
                if let Some((_, closest_distance)) = closest {
                    if distance < closest_distance {
                        closest = Some((id, distance));
                    }
                } else {
                    closest = Some((id, distance));
                }
            }
        }

        if let Some((id, _)) = closest {
            Some(id.clone())
        } else {
            None
        }
    }
}

fn signatures(groups: &[RegionGroup]) -> HashMap<String, RegionSignature> {
    let mut signatures = HashMap::new();
    for group in groups.iter() {
        let size = group.geometries().len();
        info!(
            "group '{}': calculating signatures for {} geometries",
            group.name, size
        );

        let bucket_width = 1.0;
        for (polygon, id, centroid) in group.geometries().iter() {
            let mut bearing_length_pairs = vec![];
            let mut bucketed_degree_length_pairs = vec![];

            let points: Vec<Point<f64>> = polygon.exterior().points().collect();
            for i in 0..points.len() {
                let current = points[i];
                let current_bearing = Haversine::bearing(*centroid, current);
                let current_length = Line::new(*centroid, current).length::<Haversine>();
                bearing_length_pairs.push((current_bearing, current_length));

                let prev = points[(i + points.len() - 1) % points.len()];
                let prev_bearing = Haversine::bearing(*centroid, prev);

                let bearing_diff = (current_bearing - prev_bearing).abs();
                if bearing_diff >= 0.5 {
                    // interpolate between prev and current to fill in the gaps
                    let num_samples = (bearing_diff / 0.5).ceil() as usize;
                    let step = 1.0 / num_samples as f64;
                    for i in 1..=num_samples {
                        let ratio = step * i as f64;
                        let point = Haversine::point_at_ratio_between(prev, current, ratio);
                        let bearing = Haversine::bearing(*centroid, point);
                        let degree = bearing.floor() as usize;
                        let length = Line::new(*centroid, point).length::<Haversine>();
                        bucketed_degree_length_pairs.push((degree, length));
                    }
                };
                let current_degree = current_bearing.floor() as usize;
                bucketed_degree_length_pairs.push((current_degree, current_length));
            }

            let max_length = bearing_length_pairs
                .iter()
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .unwrap()
                .1;

            let mut bucketed_by_degree: Vec<Option<f64>> = vec![None; 360];
            for (degree, length) in bucketed_degree_length_pairs.into_iter() {
                let normalised_length = length / max_length;
                if let Some(bucket) = bucketed_by_degree[degree] {
                    bucketed_by_degree[degree] = Some(bucket.max(normalised_length));
                } else {
                    bucketed_by_degree[degree] = Some(normalised_length);
                }
            }

            let normalised = bucketed_by_degree
                .into_iter()
                .map(|bucket| bucket.unwrap_or(0.0))
                .collect();

            let signature = RegionSignature::new(
                id.clone(),
                group.name.clone(),
                *centroid,
                bucket_width,
                normalised,
            );
            signatures.insert(id.clone(), signature);
        }

        info!("calculated signatures");
    }
    signatures
}
