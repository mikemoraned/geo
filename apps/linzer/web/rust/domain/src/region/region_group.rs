use geo::{Centroid, Geometry, GeometryCollection, Point, Polygon};

pub struct RegionGroup {
    pub name: String,
    geometries: Vec<(Polygon<f64>, String, Point<f64>)>
}

impl RegionGroup {
    pub fn new(name: String, collection: GeometryCollection<f64>) -> RegionGroup {
        let mut geometries = vec![];
        for (i, geometry ) in collection.iter().enumerate() {
            if let Geometry::Polygon(polygon) = geometry {
                let centroid = polygon.centroid().unwrap();
                let id = format!("{}-{}", name, i);
                geometries.push((polygon.clone(), id, centroid));
            }
        }
        RegionGroup { name, geometries }
    }

    pub fn geometries(&self) -> &Vec<(Polygon<f64>, String, Point<f64>)> {
        &self.geometries
    }
}

#[cfg(test)]
mod test {
    use geo::{line_string, polygon, Geometry, GeometryCollection, Point, Polygon};

    use crate::region::region_group::RegionGroup;

    #[test]
    fn test_takes_generic_geometries_and_supplies_polygons_centroids_and_identifiers() {
        let polygon: Polygon<f64> = polygon![
            (x: 0.0, y: 0.0),
            (x: 1.0, y: 0.0),
            (x: 1.0, y: 1.0),
            (x: 0.0, y: 1.0),
            (x: 0.0, y: 0.0)
        ];
        let geometry = Geometry::Polygon(polygon);
        let collection = GeometryCollection::from(vec![geometry]);
        let group = RegionGroup::new("test".to_string(), collection);
        let geometries = group.geometries();
        assert_eq!(geometries.len(), 1);
        let (polygon, id, centroid) = &geometries[0];
        assert_eq!(id, "test-0");
        assert_eq!(polygon.exterior(), &line_string![
            (x: 0.0, y: 0.0),
            (x: 1.0, y: 0.0),
            (x: 1.0, y: 1.0),
            (x: 0.0, y: 1.0),
            (x: 0.0, y: 0.0)
        ]);
        assert_eq!(centroid, &Point::new(0.5, 0.5));
    }

    #[test]
    fn test_ignores_non_polygon_geometry() {
        let point = Geometry::Point(Point::new(0.0, 0.0));
        let collection = GeometryCollection::from(vec![point]);
        let group = RegionGroup::new("test".to_string(), collection);
        let geometries = group.geometries();
        assert_eq!(geometries.len(), 0);
    }
}
