use geo::{Centroid, Geometry, GeometryCollection, Point, Polygon};

pub struct RegionGroup {
    pub name: String,
    collection: GeometryCollection<f64>
}

impl RegionGroup {
    pub fn new(name: String, collection: GeometryCollection<f64>) -> RegionGroup {
        RegionGroup { name, collection }
    }

    pub fn geometries(&self) -> Vec<(Polygon<f64>, String, Point<f64>)> {
        let mut geometries = vec![];
        for (i, geometry ) in self.collection.iter().enumerate() {
            if let Geometry::Polygon(polygon) = geometry {
                let centroid = polygon.centroid().unwrap();
                let id = format!("{}-{}", self.name, i);
                geometries.push((polygon.clone(), id, centroid));
            }
        }
        geometries
    }
}
