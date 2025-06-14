use arrow::array::{AsArray, RecordBatch};
use datafusion::datasource::file_format::parquet::ParquetFormat;
use datafusion::datasource::listing::{
    ListingOptions, ListingTable, ListingTableConfig, ListingTableUrl,
};
use datafusion::prelude::*;
use geo::Geometry;
use geozero::ToGeo;
use serde::Deserialize;
use std::sync::Arc;

pub struct OvertureMaps {
    ctx: SessionContext,
}

#[derive(Deserialize, Debug)]
pub struct GersId(String);

impl OvertureMaps {
    pub async fn load_from_base(base: String) -> Result<Self, Box<dyn std::error::Error>> {
        let ctx = SessionContext::new();
        let session_state = ctx.state();

        let overture_mapping = vec![
            ("division_area", "theme=divisions/type=division_area/"),
            ("base_land_cover", "theme=base/type=land_cover/"),
        ];

        for (table_name, overture_path) in overture_mapping {
            println!("loading table: {} from path: {}", table_name, overture_path);
            // Create default parquet options
            let file_format = ParquetFormat::new();
            let listing_options =
                ListingOptions::new(Arc::new(file_format)).with_file_extension(".parquet");
            // println!("listing options: {:?}", &listing_options);

            let table_path = format!("{}/{}", &base, overture_path);

            // Parse the path
            let table_path = ListingTableUrl::parse(table_path)?;
            println!("path: {}", &table_path);

            // Resolve the schema
            let resolved_schema = listing_options
                .infer_schema(&session_state, &table_path)
                .await?;
            println!("schema: {:?}", &resolved_schema);

            let config = ListingTableConfig::new(table_path)
                .with_listing_options(listing_options)
                .with_schema(resolved_schema);

            // Create a new TableProvider
            let provider = Arc::new(ListingTable::try_new(config)?);

            ctx.register_table(table_name, provider)?;
        }

        Ok(OvertureMaps { ctx })
    }

    pub async fn find_geometry_by_id(
        &self,
        id: &GersId,
    ) -> Result<Option<Geometry>, Box<dyn std::error::Error>> {
        println!("finding geometry for id: {:?}", id);

        println!("looking in division_area table");
        let division_area_df = self.ctx.table("division_area").await?;
        // println!("division_area: {:?}", &division_area_df.schema());

        let division_area_match = find_geometry_by_id(&division_area_df, id).await?;
        if !division_area_match.is_empty() {
            return convert_record_batch_to_geometry(&division_area_match);
        }

        println!("looking in base_land_cover table");
        let base_land_cover_df = self.ctx.table("base_land_cover").await?;
        let base_land_cover_match = find_geometry_by_id(&base_land_cover_df, id).await?;
        if !base_land_cover_match.is_empty() {
            println!("found geometry in base_land_cover table");
            return convert_record_batch_to_geometry(&base_land_cover_match);
        }

        println!("no geometry found for id: {:?}", id);

        Ok(None)
    }
}

async fn find_geometry_by_id(
    df: &DataFrame,
    id: &GersId,
) -> Result<Vec<RecordBatch>, Box<dyn std::error::Error>> {
    let GersId(id) = id;
    let matching = df
        .clone()
        .filter(col("id").eq(lit(id)))?
        .select(vec![col("geometry")])?
        .collect()
        .await?;

    Ok(matching)
}

fn convert_record_batch_to_geometry(
    matching: &Vec<RecordBatch>,
) -> Result<Option<Geometry>, Box<dyn std::error::Error>> {
    for batch in matching {
        let geometry_col = batch.column(0).as_binary_view();
        for geometry in geometry_col.iter() {
            if let Some(geometry) = geometry {
                let wkb = geozero::wkb::Wkb(geometry.to_vec());
                match wkb.to_geo() {
                    Ok(geometry) => {
                        return Ok(Some(geometry.clone()));
                    }
                    Err(e) => {
                        println!("error converting WKB to Geometry: {}", e);
                        return Err(Box::new(e));
                    }
                }
            }
        }
    }

    Ok(None)
}
