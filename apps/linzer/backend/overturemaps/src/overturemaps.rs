use arrow::array::AsArray;
use datafusion::datasource::file_format::parquet::ParquetFormat;
use datafusion::datasource::listing::{ListingOptions, ListingTable, ListingTableConfig, ListingTableUrl};
use datafusion::prelude::*;
use std::sync::Arc;
use geo::Geometry;
use geozero::ToGeo;
use serde::Deserialize;

pub struct OvertureMaps {
    ctx: SessionContext
}


#[derive(Deserialize, Debug)]
pub struct GersId(String);

impl OvertureMaps {
    pub async fn load_from_base(base: String) -> Result<Self, Box<dyn std::error::Error>> {
        let ctx = SessionContext::new();
        let session_state = ctx.state();
        let table_path = format!("{}/theme=divisions/type=division_area/", &base);

        // Parse the path
        let table_path = ListingTableUrl::parse(table_path)?;
        println!("path: {}", &table_path);

        // Create default parquet options
        let file_format = ParquetFormat::new();
        let listing_options = ListingOptions::new(Arc::new(file_format))
            .with_file_extension(".parquet");
        println!("listing options: {:?}", &listing_options);

        // Resolve the schema
        let resolved_schema = listing_options
            .infer_schema(&session_state, &table_path)
            .await?;
        println!("schema: {:?}", &resolved_schema);

        let config = ListingTableConfig::new(table_path)
            .with_listing_options(listing_options)
            .with_schema(resolved_schema);
        // println!("config: {:?}", &config);

        // Create a new TableProvider
        let provider = Arc::new(ListingTable::try_new(config)?);

        // This provider can now be read as a dataframe:
        // let df = ctx.read_table(provider.clone());

        ctx.register_table("division_area", provider)?;

        Ok(OvertureMaps {
            ctx
        })
    }

    pub async fn find_geometry_by_id(&self, id: &GersId) -> Result<Option<Geometry>, Box<dyn std::error::Error>> {
        let division_area_df = self.ctx.table("division_area").await?;
        println!("division_area: {:?}", &division_area_df.schema());

        let GersId(id) = id;

        let matching = division_area_df
            .filter(col("id").eq(lit(id)))?
            .select(vec![col("geometry")])?;
        for batch in matching.collect().await? {
            let geometry_col = batch.column(0).as_binary_view();
            for geometry in geometry_col.iter() {
                if let Some(geometry) = geometry {
                    let wkb = geozero::wkb::Wkb(geometry.to_vec());
                    let geometry = wkb.to_geo()?;
                    return Ok(Some(geometry));
                }
            }
        }

        Ok(None)
    }
}