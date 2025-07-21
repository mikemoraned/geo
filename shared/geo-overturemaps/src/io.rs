use std::{fs::File, io::BufWriter, path::Path};

use geo::GeometryCollection;
use geozero::{geojson::GeoJsonWriter, GeozeroGeometry};
use tracing::debug;

pub fn save_as_geojson<P: AsRef<Path>>(geo: &geo::geometry::Geometry, path: P) -> Result<(), Box<dyn std::error::Error>> {
    debug!("Saving geometry as GeoJSON to {:?}", path.as_ref());
    let collection = GeometryCollection::new_from(vec![geo.clone()]);

    let fout = BufWriter::new(File::create(path)?);
    let mut gout = GeoJsonWriter::new(fout);
    geo::geometry::Geometry::GeometryCollection(collection).process_geom(&mut gout)?;

    Ok(())
}
