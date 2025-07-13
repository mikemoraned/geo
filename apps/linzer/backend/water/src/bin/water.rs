use std::{ffi::OsStr, fs::File, io::BufWriter, path::PathBuf, sync::Arc};

use arrow_schema::{Field, Schema};
use clap::{command, Parser};
use config::Config;
use geo::geometry;
use geoarrow::{
    array::GeometryCollectionBuilder, datatypes::Dimension, io::geozero::ToGeometry,
    scalar::OwnedGeometry, GeometryArrayTrait,
};
use geozero::{geojson::GeoJsonWriter, GeozeroGeometry};
use overturemaps::overturemaps::WaterHandling;
use thiserror::Error;

/// Find routes in an area
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// config file defining the area
    #[arg(long)]
    area: PathBuf,

    /// base location for OvertureMaps data
    #[arg(long)]
    overturemaps: Option<String>,

    /// output a GeoJson or geoarrow file representing the water found
    /// the format is determined by the file extension
    /// e.g. `water.geojson` or `water.arrow`
    #[arg(long)]
    water: Vec<PathBuf>,

    /// water handling
    #[arg(long, value_enum)]
    handling: WaterHandling,
}

#[derive(Error, Debug)]
pub enum WaterError {
    #[error("Unable to save geometry")]
    CannotSaveGeometry,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("{:?}", args);

    let config: Config = Config::read_from_file(&args.area)?;

    let gers_id = &config.overturemaps.gers_id;
    if let Some(om_base) = args.overturemaps.as_ref() {
        use overturemaps::overturemaps::OvertureMaps;
        let om = OvertureMaps::load_from_base(om_base.clone()).await?;

        if let Some(bounds) = om.find_geometry_by_id(gers_id).await? {
            let water = om.find_water_in_region(&bounds, args.handling).await?;
            for path in &args.water {
                save(&water, path)?;
            }
        }
    }

    Ok(())
}

fn save(
    geo: &geo::geometry::Geometry<f64>,
    path: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    match geo {
        geo::geometry::Geometry::GeometryCollection(collection) => {
            match path.extension().and_then(OsStr::to_str) {
                Some("geojson") => {
                    let fout = BufWriter::new(File::create(path)?);
                    let mut gout = GeoJsonWriter::new(fout);
                    geo::geometry::Geometry::GeometryCollection(collection.clone())
                        .process_geom(&mut gout)?;

                    Ok(())
                }
                Some("arrow") => {
                    let mut builder: GeometryCollectionBuilder<_, 2> =
                        GeometryCollectionBuilder::new();
                    for geom in collection.0 {
                        builder.push_geometry(Some(&geom.to_geometry()?), false);
                    }
                    let array = builder.finish();

                    let field = Field::new("geometry", array.data_type().clone(), true);
                    let schema = Arc::new(Schema::new(vec![field]));
                    let batch =
                        RecordBatch::try_new(schema.clone(), vec![Arc::new(array)]).unwrap();
                    let fout = BufWriter::new(File::create(path)?);
                    write_ipc_stream(&mut fout, &schema, vec![batch]).unwrap();
                    Ok(())
                }
                _ => Err(Box::new(WaterError::CannotSaveGeometry)),
            }
        }

        _ => Err(Box::new(WaterError::CannotSaveGeometry)),
    }
}
