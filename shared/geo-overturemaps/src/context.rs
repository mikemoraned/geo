use std::path::Path;

use datafusion::{arrow::array::{AsArray, RecordBatch}, prelude::*};
use geo::Geometry;
use geozero::ToGeo;
use tracing::{debug, error, info};

use crate::model::GersId;

pub struct OvertureContext {
    ctx: SessionContext,
}

impl OvertureContext {
    pub async fn load_from_release<P: AsRef<Path>>(base: P) -> Result<Self, Box<dyn std::error::Error>> {
        info!("Loading Overture Maps context from release path: {:?}", base.as_ref());
        
        let path = base.as_ref();

        let session_config = SessionConfig::new().with_collect_statistics(true);
        let ctx = SessionContext::new_with_config(session_config);

        let overture_mapping = vec![
            ("division_area", "theme=divisions/type=division_area/"),
        ];

        let read_options = ParquetReadOptions::default().parquet_pruning(true);
        for (table_name, relative_path) in overture_mapping {
            let full_path = path.join(relative_path);
            debug!("Loading table {} from path: {:?}", table_name, full_path);

            ctx.register_parquet(table_name, full_path.to_str().unwrap(), read_options.clone()).await?;
        }

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
                return convert_record_batch_to_geometry(&matching);
            }
        }

        Ok(None)
    }
}

fn convert_record_batch_to_geometry(
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
