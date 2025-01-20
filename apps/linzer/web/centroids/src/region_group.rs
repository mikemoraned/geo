use geo::GeometryCollection;

pub struct RegionGroup {
    pub name: String,
    pub collection: GeometryCollection<f64>
}

impl RegionGroup {
    pub fn new(name: String, collection: GeometryCollection<f64>) -> RegionGroup {
        RegionGroup { name, collection }
    }
}
