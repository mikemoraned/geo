use std::collections::BTreeMap;

use arrow::datatypes::{Field, SchemaRef};
use domain::{
    CrossingCompactId, CrossingId, DeviceId, DeviceType, SessionId, StartedBy, TrainNumber,
};
use medallion::{Country, DatasetInfo, Row, file_schema};
use serde::{Deserialize, Serialize};
use serde_reflection::{ContainerFormat, Format, Registry, Samples, Tracer, TracerConfig};

use crate::{
    ALL, AccelReadingRowV2, AgencyId, DeviceSessionRowV2, ExtractManifestRowV2, Feed,
    GeometrySource, GpsReadingRowV2, Label, MotisCaptureRow, MotisSegmentRow, MotisSourceId,
    MotisSourceRowV2, MotisVersion, OVERTURE_EXTRACT, OverlapKind, OvertureRelease, RailMode,
    RawSampleRow, SessionCrossingRowV2, SessionRowV2, SessionSampleRowV2, StopId,
    TrainSegmentRowV2, TrainSessionRow, TrainSessionSampleRow, TripId, WaterCrossingRowV2,
};

const EXTENSION_NAME: &str = "ARROW:extension:name";
const EXTENSION_METADATA: &str = "ARROW:extension:metadata";

type Checks = BTreeMap<&'static str, &'static str>;

#[derive(Serialize)]
struct Declaration {
    layer: &'static str,
    partition_key: Option<&'static str>,
    semantics: &'static str,
    unique: &'static [&'static str],
    columns: Vec<Column>,
}

#[derive(Serialize)]
struct Column {
    name: String,
    data_type: String,
    nullable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    extension: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    crs: BTreeMap<&'static str, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    metadata: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    variants: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    checks: Checks,
}

struct Known {
    enums: BTreeMap<String, Vec<String>>,
    newtypes: BTreeMap<&'static str, Checks>,
}

enum Named {
    Enum(String),
    Newtype(String),
}

fn declaration<T: Row>(known: &Known) -> (&'static str, Declaration) {
    let info = T::DATASET.info();
    let schemas: Vec<(Country, SchemaRef)> = Country::ALL
        .into_iter()
        .map(|country| {
            let schema = file_schema::<T>(country).expect("lay out a file of the rows");
            (country, schema)
        })
        .collect();
    let named = named_columns::<T>();
    let columns = schemas[0]
        .1
        .fields()
        .iter()
        .map(|field| {
            let (variants, checks) = match named.get(field.name()) {
                Some(Named::Enum(name)) => {
                    let variants = known.enums.get(name).unwrap_or_else(|| {
                        panic!(
                            "{}: `{}` holds the enum {name}, which `enums` does not list",
                            info.name,
                            field.name()
                        )
                    });
                    (variants.clone(), Checks::new())
                }
                Some(Named::Newtype(name)) => {
                    let checks = known.newtypes.get(name.as_str()).unwrap_or_else(|| {
                        panic!(
                            "{}: `{}` holds {name}, which `newtypes` does not list",
                            info.name,
                            field.name()
                        )
                    });
                    (Vec::new(), checks.clone())
                }
                None => (Vec::new(), Checks::new()),
            };
            Column {
                name: field.name().clone(),
                data_type: field.data_type().to_string(),
                nullable: field.is_nullable(),
                extension: field.metadata().get(EXTENSION_NAME).cloned(),
                crs: schemas
                    .iter()
                    .filter_map(|(country, schema)| {
                        let field = schema.field_with_name(field.name()).ok()?;
                        Some((country.code(), crs_of(field)?))
                    })
                    .collect(),
                metadata: field
                    .metadata()
                    .iter()
                    .filter(|(key, _)| {
                        ![EXTENSION_NAME, EXTENSION_METADATA].contains(&key.as_str())
                    })
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
                variants,
                checks,
            }
        })
        .collect();
    (
        info.name,
        Declaration {
            layer: info.layer.as_str(),
            partition_key: info.partition_key,
            semantics: T::SEMANTICS,
            unique: T::UNIQUE,
            columns,
        },
    )
}

fn rowless(info: DatasetInfo) -> (&'static str, Declaration) {
    (
        info.name,
        Declaration {
            layer: info.layer.as_str(),
            partition_key: info.partition_key,
            semantics: "",
            unique: &[],
            columns: Vec::new(),
        },
    )
}

fn crs_of(field: &Field) -> Option<String> {
    let metadata: serde_json::Value =
        serde_json::from_str(field.metadata().get(EXTENSION_METADATA)?).ok()?;
    let id = &metadata["crs"]["id"];
    let code = match &id["code"] {
        serde_json::Value::String(code) => code.clone(),
        code => code.to_string(),
    };
    Some(format!("{}:{code}", id["authority"].as_str()?))
}

fn named_columns<T: Row>() -> BTreeMap<String, Named> {
    let mut tracer = Tracer::new(TracerConfig::default());
    let (row, _) = tracer
        .trace_value(&mut Samples::new(), &T::example())
        .expect("trace the example");
    let registry = tracer
        .registry()
        .expect("a registry of the example's formats");
    let Format::TypeName(row) = row else {
        panic!("{}: the example is not a named struct", T::DATASET.name);
    };
    let Some(ContainerFormat::Struct(fields)) = registry.get(&row) else {
        panic!("{}: `{row}` is not a struct", T::DATASET.name);
    };
    fields
        .iter()
        .filter_map(|field| Some((field.name.clone(), named(&field.value, &registry)?)))
        .collect()
}

fn named(format: &Format, registry: &Registry) -> Option<Named> {
    match format {
        Format::Option(inner) => named(inner, registry),
        Format::TypeName(name) => match registry.get(name)? {
            ContainerFormat::Enum(_) => Some(Named::Enum(name.clone())),
            ContainerFormat::NewTypeStruct(_) => Some(Named::Newtype(name.clone())),
            _ => None,
        },
        _ => None,
    }
}

fn variants<E: for<'de> Deserialize<'de>>() -> (String, Vec<String>) {
    let mut tracer = Tracer::new(TracerConfig::default());
    let (format, _) = tracer.trace_simple_type::<E>().expect("trace the enum");
    let Format::TypeName(name) = format else {
        panic!("the enum has no name to trace it by");
    };
    let registry = tracer.registry().expect("a registry of the enum's formats");
    let Some(ContainerFormat::Enum(variants)) = registry.get(&name) else {
        panic!("`{name}` is not an enum");
    };
    let variants = variants
        .values()
        .map(|variant| variant.name.clone())
        .collect();
    (name, variants)
}

fn enums() -> BTreeMap<String, Vec<String>> {
    BTreeMap::from([
        variants::<DeviceType>(),
        variants::<Feed>(),
        variants::<GeometrySource>(),
        variants::<OverlapKind>(),
        variants::<RailMode>(),
        variants::<StartedBy>(),
    ])
}

fn newtypes() -> BTreeMap<&'static str, Checks> {
    let name = ("domain::name::checked", domain::name::CHECKED_HASH);
    BTreeMap::from([
        (
            "AgencyId",
            Checks::from([("AgencyId::new", AgencyId::NEW_HASH)]),
        ),
        (
            "CrossingCompactId",
            Checks::from([("CrossingCompactId::new", CrossingCompactId::NEW_HASH)]),
        ),
        (
            "CrossingId",
            Checks::from([("CrossingId::new", CrossingId::NEW_HASH), name]),
        ),
        (
            "DeviceId",
            Checks::from([("DeviceId::new", DeviceId::NEW_HASH)]),
        ),
        ("Label", Checks::from([("Label::new", Label::NEW_HASH)])),
        (
            "MotisSourceId",
            Checks::from([("MotisSourceId::new", MotisSourceId::NEW_HASH)]),
        ),
        (
            "MotisVersion",
            Checks::from([("MotisVersion::new", MotisVersion::NEW_HASH)]),
        ),
        (
            "OvertureRelease",
            Checks::from([("OvertureRelease::new", OvertureRelease::NEW_HASH)]),
        ),
        (
            "SessionId",
            Checks::from([("SessionId::new", SessionId::NEW_HASH), name]),
        ),
        ("StopId", Checks::from([("StopId::new", StopId::NEW_HASH)])),
        (
            "TrainNumber",
            Checks::from([("TrainNumber::new", TrainNumber::NEW_HASH)]),
        ),
        ("TripId", Checks::from([("TripId::new", TripId::NEW_HASH)])),
    ])
}

#[test]
fn every_dataset_keeps_its_recorded_declaration() {
    let known = Known {
        enums: enums(),
        newtypes: newtypes(),
    };
    let declarations = [
        declaration::<RawSampleRow>(&known),
        declaration::<GpsReadingRowV2>(&known),
        declaration::<AccelReadingRowV2>(&known),
        declaration::<DeviceSessionRowV2>(&known),
        declaration::<MotisSegmentRow>(&known),
        declaration::<MotisCaptureRow>(&known),
        declaration::<MotisSourceRowV2>(&known),
        declaration::<TrainSegmentRowV2>(&known),
        declaration::<SessionRowV2>(&known),
        declaration::<SessionSampleRowV2>(&known),
        declaration::<TrainSessionRow>(&known),
        declaration::<TrainSessionSampleRow>(&known),
        declaration::<WaterCrossingRowV2>(&known),
        declaration::<SessionCrossingRowV2>(&known),
        declaration::<ExtractManifestRowV2>(&known),
        rowless(OVERTURE_EXTRACT.info()),
    ];

    let mut declared: Vec<&str> = declarations.iter().map(|(name, _)| *name).collect();
    let mut defined: Vec<&str> = ALL.iter().map(|dataset| dataset.name).collect();
    declared.sort_unstable();
    defined.sort_unstable();
    assert_eq!(
        declared, defined,
        "a dataset defined here has no declaration to snapshot"
    );

    for (name, declaration) in declarations {
        insta::assert_yaml_snapshot!(name, declaration);
    }
}
