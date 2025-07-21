use std::path::Path;

use datafusion::{arrow::array::{AsArray, RecordBatch}, prelude::*};
use geo::{Area, BooleanOps, BoundingRect, Geometry, GeometryCollection, MultiPolygon, Rect};
use geozero::ToGeo;
use thiserror::Error;
use tracing::{debug, error, info};

use crate::model::GersId;

pub struct OvertureContext {
    ctx: SessionContext,
}

#[derive(Error, Debug)]
pub enum OvertureError {
    #[error("Unable to determine bounds")]
    CannotDetermineBounds,
}

#[derive(Copy, Clone, Debug)]
pub enum ClippingMode {
    ClipToRegion,
    KeepAsIs,
}

impl OvertureContext {
    pub async fn load_from_release<P: AsRef<Path>>(base: P) -> Result<Self, Box<dyn std::error::Error>> {
        info!("Loading Overture Maps context from release path: {:?}", base.as_ref());
        
        let path = base.as_ref();

        let session_config = SessionConfig::new().with_collect_statistics(true);
        let ctx = SessionContext::new_with_config(session_config);

        let overture_mapping = vec![
            ("division_area", "theme=divisions/type=division_area/"),
            ("land_cover", "theme=base/type=land_cover/"),
        ];

        let read_options = ParquetReadOptions::default().parquet_pruning(true);
        for (table_name, relative_path) in overture_mapping {
            let full_path = path.join(relative_path);
            debug!("Loading table {} from path: {:?}", table_name, full_path);

            ctx.register_parquet(table_name, full_path.to_str().unwrap(), read_options.clone()).await?;
        }

        info!("Overture Maps context loaded successfully");

        Ok(OvertureContext {
            ctx
        })
    }

    pub async fn find_geometry_by_id(
        &self,
        id: &GersId,
    ) -> Result<Option<Geometry>, Box<dyn std::error::Error>> {
        let search_tables = vec!["division_area"];
        for table_name in search_tables {
            debug!("Searching in table: {}", table_name);
            let df = self.ctx.table(table_name).await?;
            let matching = df
                .clone()
                .filter(col("id").eq(lit(id)))?
                .select(vec![col("geometry")])?
                .collect()
                .await?;
            if !matching.is_empty() {
                return convert_first_record_batch_to_geometry(&matching);
            }
        }

        Ok(None)
    }

     pub async fn find_land_cover_in_region(
        &self,
        region: &Geometry<f64>,
        clipping_mode: ClippingMode,
    ) -> Result<Option<Geometry>, Box<dyn std::error::Error>> {
        let bounds = region
            .bounding_rect()
            .ok_or(OvertureError::CannotDetermineBounds)?;
        debug!("finding water in bounds: {:?}", bounds);

        let matching = find_table_rows_intersecting_bounds(&self.ctx, "land_cover", &bounds).await?;
        debug!("found {} batches", matching.len());

        match clipping_mode {
            ClippingMode::ClipToRegion => {
                let mut intersections = vec![];
                let mut kept_geometries_count = 0;
                let mut ignored_geometries_count = 0;
                for batch in matching {
                    let geometry_col = batch.column(0).as_binary_view();
                    for geometry in geometry_col.iter() {
                        if let Some(geometry) = geometry {
                            let wkb = geozero::wkb::Wkb(geometry.to_vec());
                            match wkb.to_geo() {
                                Ok(geometry) => {
                                    let intersection = intersect(&region, &geometry);
                                    if intersection.signed_area() > 0.0 {
                                        kept_geometries_count += 1;
                                        intersections.push(Geometry::MultiPolygon(intersection));
                                    } else {
                                        ignored_geometries_count += 1;
                                    }
                                },
                                Err(e) => {
                                    error!("error converting WKB to Geometry: {}", e);
                                    return Err(Box::new(e));
                                }
                            }
                        }
                    }
                }
                debug!(
                    "found {} geometries, kept {}, ignored {}",
                    kept_geometries_count + ignored_geometries_count,
                    kept_geometries_count,
                    ignored_geometries_count
                );

                let collection = GeometryCollection::new_from(intersections);
                Ok(Some(Geometry::GeometryCollection(collection)))
            }
            ClippingMode::KeepAsIs => 
                convert_first_record_batch_to_geometry(&matching)
        }
    }
}

fn convert_first_record_batch_to_geometry(
    matching: &Vec<RecordBatch>,
) -> Result<Option<Geometry>, Box<dyn std::error::Error>> {
    for batch in matching {
        let geometry_col = batch.column(0).as_binary_view();
        if let Some(geometry) = geometry_col.iter().flatten().next() {
            let wkb = geozero::wkb::Wkb(geometry.to_vec());
            match wkb.to_geo() {
                Ok(geometry) => {
                    return Ok(Some(geometry.clone()));
                }
                Err(e) => {
                    error!("error converting WKB to Geometry: {}", e);
                    return Err(Box::new(e));
                }
            }
        }
    }

    Ok(None)
}

async fn find_table_rows_intersecting_bounds(
    ctx: &SessionContext,
    table_name: &str,
    bounds: &Rect<f64>,
) -> Result<Vec<RecordBatch>, Box<dyn std::error::Error>> {
    let xmin = bounds.min().x;
    let ymin = bounds.min().y;
    let xmax = bounds.max().x;
    let ymax = bounds.max().y;
    let sql = format!(
        "
        SELECT geometry FROM {table_name}
        WHERE 
                -- bounding boxes are overlapping if they *don't* overlap along all axes
                -- if we see our region's bounding box as A, and the geometry's bounding box as B:
                NOT (
                {xmax} <= bbox.xmin    -- A is entirely left of B
                OR bbox.xmax <= {xmin} -- A is entirely right of B
                OR bbox.ymin           -- A is entirely below B
                    >= 
                    {ymax} 
                OR {ymin}              -- A is entirely above B
                    >=
                    bbox.ymax  
            )  
        "
    );
    Ok(ctx.sql(&sql).await?.collect().await?)
}

fn intersect(geo1: &Geometry<f64>, geo2: &Geometry<f64>) -> MultiPolygon<f64> {
    match geo1 {
        Geometry::Polygon(poly1) => match geo2 {
            Geometry::Polygon(poly2) => poly1.intersection(poly2),
            Geometry::MultiPolygon(multi2) => {
                MultiPolygon::new(vec![poly1.clone()]).intersection(multi2)
            }
            _ => MultiPolygon::new(vec![]),
        },
        Geometry::MultiPolygon(multi1) => match geo2 {
            Geometry::Polygon(poly2) => {
                multi1.intersection(&MultiPolygon::new(vec![poly2.clone()]))
            }
            Geometry::MultiPolygon(multi2) => multi1.intersection(multi2),
            _ => MultiPolygon::new(vec![]),
        },

        _ => MultiPolygon::new(vec![]),
    }
}
