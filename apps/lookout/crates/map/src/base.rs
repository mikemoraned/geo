use std::io::Write;

use arrow::array::RecordBatch;
use geojson::{Geometry, GeometryValue};
use medallion::{Country, GEOMETRY, GeoError, Query, QueryError, Root, geometries};
use medallion_model::{OVERTURE_EXTRACT, WATER_CROSSING_V2};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::feature::{Feature, FeatureError, Tippecanoe};

pub const BASE_MIN_ZOOM: u8 = 7;

#[derive(Debug, thiserror::Error)]
pub enum BaseError {
    #[error("reading the store: {0}")]
    Query(#[from] QueryError),
    #[error("{dataset} has not been derived yet, so there is no base map to draw")]
    Missing { dataset: &'static str },
    #[error(
        "{country}'s crossings name {named} distinct {layer} features, and their extracts hold \
         {found} of them"
    )]
    Unfound {
        layer: &'static str,
        country: Country,
        named: i64,
        found: usize,
    },
    #[error(transparent)]
    Geo(#[from] GeoError),
    #[error("reading a feature's properties: {0}")]
    Properties(#[from] serde_arrow::Error),
    #[error(transparent)]
    Feature(#[from] FeatureError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BaseWritten {
    pub rail: usize,
    pub water: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct RailProperties {
    #[serde(rename(deserialize = "overture_rail_class"))]
    class: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WaterProperties {
    #[serde(rename(deserialize = "overture_water_subtype"))]
    subtype: Option<String>,
    #[serde(rename(deserialize = "overture_water_class"))]
    class: Option<String>,
}

struct Layer {
    tippecanoe: Tippecanoe,
    theme: &'static str,
    of_type: &'static str,
    id: &'static str,
    described_by: &'static [&'static str],
}

const RAIL: Layer = Layer {
    tippecanoe: Tippecanoe {
        layer: "rail",
        minzoom: BASE_MIN_ZOOM,
        maxzoom: None,
    },
    theme: "transportation",
    of_type: "segment",
    id: "overture_rail_id",
    described_by: &["overture_rail_class"],
};

const WATER: Layer = Layer {
    tippecanoe: Tippecanoe {
        layer: "water",
        minzoom: BASE_MIN_ZOOM,
        maxzoom: None,
    },
    theme: "base",
    of_type: "water",
    id: "overture_water_id",
    described_by: &["overture_water_subtype", "overture_water_class"],
};

const CROSSINGS: &str = "water_crossing_v2";
const FEATURE: &str = "feature";

pub async fn write_base_map(root: &Root, mut to: impl Write) -> Result<BaseWritten, BaseError> {
    let countries = medallion::countries_of(root, WATER_CROSSING_V2).await?;
    if countries.is_empty() {
        return Err(BaseError::Missing {
            dataset: WATER_CROSSING_V2.name,
        });
    }

    let mut written = BaseWritten::default();
    for country in countries {
        written.rail += write_layer::<RailProperties>(root, country, &RAIL, &mut to).await?;
        written.water += write_layer::<WaterProperties>(root, country, &WATER, &mut to).await?;
    }
    Ok(written)
}

async fn write_layer<P>(
    root: &Root,
    country: Country,
    layer: &Layer,
    mut to: impl Write,
) -> Result<usize, BaseError>
where
    P: DeserializeOwned + Serialize,
{
    let query = Query::new(root.clone());
    query
        .register_of_country(WATER_CROSSING_V2, CROSSINGS, country)
        .await?;
    query
        .register_within_every_partition(
            OVERTURE_EXTRACT,
            FEATURE,
            &[("theme", layer.theme), ("type", layer.of_type)],
        )
        .await?;

    let named = query
        .count(&format!(
            "SELECT COUNT(DISTINCT {id}) AS count FROM {CROSSINGS}",
            id = layer.id
        ))
        .await?;
    let found = query.sql(&named_features(layer)).await?;
    let found_rows: usize = found.iter().map(RecordBatch::num_rows).sum();
    if found_rows as i64 != named {
        return Err(BaseError::Unfound {
            layer: layer.tippecanoe.layer,
            country,
            named,
            found: found_rows,
        });
    }

    for batch in &found {
        let properties: Vec<P> = serde_arrow::from_record_batch(&without_geometry(batch)?)?;
        for (geometry, properties) in geometries(batch, GEOMETRY)?.iter().zip(properties) {
            Feature {
                tippecanoe: layer.tippecanoe,
                geometry: Geometry::new(GeometryValue::from(geometry)),
                properties,
            }
            .write_line(&mut to)?;
        }
    }
    Ok(found_rows)
}

fn named_features(layer: &Layer) -> String {
    let named: Vec<&str> = [layer.id]
        .into_iter()
        .chain(layer.described_by.iter().copied())
        .chain(["extract_id"])
        .collect();
    format!(
        "SELECT {columns}, {FEATURE}.{GEOMETRY}
         FROM (SELECT DISTINCT {listed} FROM {CROSSINGS}) AS named
         JOIN {FEATURE}
           ON {FEATURE}.id = named.{id} AND {FEATURE}.extract_id = named.extract_id",
        columns = named
            .iter()
            .map(|column| format!("named.{column}"))
            .collect::<Vec<_>>()
            .join(", "),
        listed = named.join(", "),
        id = layer.id,
    )
}

fn without_geometry(batch: &RecordBatch) -> Result<RecordBatch, GeoError> {
    let schema = batch.schema();
    let kept: Vec<usize> = (0..schema.fields().len())
        .filter(|index| schema.field(*index).name() != GEOMETRY)
        .collect();
    Ok(batch.project(&kept)?)
}
