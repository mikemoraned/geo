use geo::Geometry;
use geo_overturemaps::GersId;
use overturemaps::overturemaps::OvertureMaps;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BoundsError {
    #[error("Unable to find anything with that GERS Id")]
    CannotFindGersId,
    #[error("Could not find largest Polygon in MultiPolygon")]
    CannotFindLargestPolygon,
}

pub async fn read_bounds(
    gers_id: &GersId,
    om: &OvertureMaps,
    choose_largest_polygon: bool,
) -> Result<Geometry, Box<dyn std::error::Error>> {
    println!("Using overture maps");
    if let Some(geometry) = om.find_geometry_by_id(gers_id).await? {
        if let Geometry::MultiPolygon(ref multi) = geometry {
            use geo::Area;
            if choose_largest_polygon {
                println!("Choosing largest polygon from MultiPolygon");
                let largest_polygon = multi
                    .into_iter()
                    .max_by(|a, b| a.unsigned_area().partial_cmp(&b.unsigned_area()).unwrap())
                    .ok_or(Box::new(BoundsError::CannotFindLargestPolygon))?;
                Ok(Geometry::Polygon(largest_polygon.clone()))
            } else {
                Ok(geometry)
            }
        } else {
            Ok(geometry)
        }
    } else {
        Err(Box::new(BoundsError::CannotFindGersId))
    }
}
