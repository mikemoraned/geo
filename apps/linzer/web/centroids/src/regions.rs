use geo::GeometryCollection;

pub struct Regions {
    pub name: String,
    pub collection: GeometryCollection<f64>
}

impl Regions {
    pub fn new(name: String, collection: GeometryCollection<f64>) -> Regions {
        Regions { name, collection }
    }
}
