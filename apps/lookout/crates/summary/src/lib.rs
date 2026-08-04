//! Reporting what a store holds, as text meant to be read at a glance.
//!
//! The layout is a column per question a reader is asking — is this dataset there, how much
//! of it is there, and what span does it cover — with one line per dataset and, on request,
//! one per partition. What each layer is for is not restated here: a summary describes the
//! store in front of it, not the design.

use medallion::Layer;
use medallion::summary::{ArtefactSummary, Contents, DatasetSummary, PartitionSummary};

/// The layers reported, in the order data flows through them.
const LAYERS: [Layer; 4] = [Layer::Landing, Layer::Bronze, Layer::Silver, Layer::Gold];

/// What is shown of a dataset holding nothing, in place of its measurements.
const ABSENT: &str = "absent";

/// How much of each dataset to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// One line per dataset.
    Datasets,
    /// One line per dataset, and one per partition below it.
    Partitions,
}

/// The report for one store: every dataset by layer, then the gold artefacts.
pub fn report(
    datasets: &[DatasetSummary],
    artefacts: &[ArtefactSummary],
    detail: Detail,
) -> String {
    let mut rows = Vec::new();
    for layer in LAYERS {
        let of_layer: Vec<&DatasetSummary> = datasets
            .iter()
            .filter(|dataset| dataset.layer == layer)
            .collect();
        let artefacts = match layer {
            Layer::Gold => artefacts,
            _ => &[],
        };
        if of_layer.is_empty() && artefacts.is_empty() {
            continue;
        }

        rows.push(Row::heading(layer.as_str()));
        rows.extend(
            of_layer
                .iter()
                .flat_map(|dataset| dataset_rows(dataset, detail)),
        );
        rows.extend(
            artefacts
                .iter()
                .flat_map(|artefact| artefact_rows(artefact, detail)),
        );
    }
    lay_out(&rows)
}

/// One dataset's line, and its partitions' lines when they were asked for.
fn dataset_rows(dataset: &DatasetSummary, detail: Detail) -> Vec<Row> {
    let mut rows = vec![Row::of(
        dataset.name,
        dataset.contents,
        &spread_over(&dataset.partitions),
    )];
    if detail == Detail::Partitions {
        rows.extend(
            dataset
                .partitions
                .iter()
                .map(|partition| Row::of(&indent(&partition.value), partition.contents, "")),
        );
    }
    rows
}

/// One artefact's line, and its versions' lines when they were asked for. An artefact is
/// not parquet, so it has no rows to report — only what it weighs, per run that wrote it.
fn artefact_rows(artefact: &ArtefactSummary, detail: Detail) -> Vec<Row> {
    let contents = artefact
        .versions
        .iter()
        .fold(Contents::default(), |mut total, version| {
            total.add(version.contents);
            total
        });
    let versions = match artefact.versions.last() {
        Some(latest) => format!(
            "{} versions, latest {}",
            artefact.versions.len(),
            latest.version
        ),
        None => ABSENT.to_string(),
    };

    let mut rows = vec![Row::of(&artefact.artifact, contents, &versions)];
    if detail == Detail::Partitions {
        rows.extend(
            artefact
                .versions
                .iter()
                .map(|version| Row::of(&indent(&version.version), version.contents, "")),
        );
    }
    rows
}

/// The span a dataset's partitions cover: the one value it holds, or the first and last of
/// several. Partition values sort in the order they were written — a date, or an id built
/// from an instant — so the ends of that order are the ends of the span.
fn spread_over(partitions: &[PartitionSummary]) -> String {
    match partitions {
        [] => String::new(),
        [only] => only.value.clone(),
        [first, .., last] => format!("{} … {} ({})", first.value, last.value, partitions.len()),
    }
}

fn indent(value: &str) -> String {
    format!("  {value}")
}

/// The measured columns of an entry: what it holds, before the span it covers.
const MEASURES: usize = 3;

/// One line of the report: either a heading, or a name, what it holds and what it covers.
enum Row {
    Heading(String),
    Entry {
        name: String,
        measures: [String; MEASURES],
        spread: String,
    },
}

impl Row {
    fn heading(layer: &str) -> Self {
        Row::Heading(layer.to_string())
    }

    /// An entry for `name`. Holding nothing is said once, rather than as three zeroes that
    /// read as measurements.
    fn of(name: &str, contents: Contents, spread: &str) -> Self {
        let (measures, spread) = match contents.is_empty() {
            true => (
                [ABSENT.to_string(), String::new(), String::new()],
                String::new(),
            ),
            false => (
                [
                    count(contents.rows, "row"),
                    count(contents.files as u64, "file"),
                    size(contents.bytes),
                ],
                spread.to_string(),
            ),
        };
        Row::Entry {
            name: name.to_string(),
            measures,
            spread,
        }
    }
}

/// The rows as text, each column as wide as its widest entry: the name to the left, the
/// measurements right-aligned so their magnitudes line up.
fn lay_out(rows: &[Row]) -> String {
    let mut name = 0;
    let mut measures = [0; MEASURES];
    for row in rows {
        if let Row::Entry {
            name: entry,
            measures: held,
            ..
        } = row
        {
            name = name.max(entry.chars().count());
            for (width, measure) in measures.iter_mut().zip(held) {
                *width = (*width).max(measure.chars().count());
            }
        }
    }

    let mut out = String::new();
    for row in rows {
        match row {
            Row::Heading(layer) => out.push_str(&format!("\n{layer}\n")),
            Row::Entry {
                name: entry,
                measures: held,
                spread,
            } => {
                let mut line = format!("  {entry:name$}");
                for (measure, width) in held.iter().zip(&measures) {
                    line.push_str(&format!("  {measure:>width$}"));
                }
                line.push_str(&format!("  {spread}"));
                out.push_str(line.trim_end());
                out.push('\n');
            }
        }
    }
    out
}

/// A count of `thing`s, in thousands separated for reading and pluralised.
fn count(things: u64, thing: &str) -> String {
    let digits: Vec<char> = things.to_string().chars().rev().collect();
    let grouped: Vec<String> = digits
        .chunks(3)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect();
    let count: String = grouped.join(",").chars().rev().collect();
    match things {
        1 => format!("{count} {thing}"),
        _ => format!("{count} {thing}s"),
    }
}

/// A size in the largest unit that leaves a number worth reading.
fn size(bytes: u64) -> String {
    const UNITS: [(&str, u64); 4] = [
        ("GiB", 1 << 30),
        ("MiB", 1 << 20),
        ("KiB", 1 << 10),
        ("B", 1),
    ];
    let (unit, scale) = UNITS
        .into_iter()
        .find(|(_, scale)| bytes >= *scale)
        .unwrap_or(("B", 1));
    match scale {
        1 => format!("{bytes} {unit}"),
        _ => format!("{:.1} {unit}", bytes as f64 / scale as f64),
    }
}

#[cfg(test)]
mod tests {
    use medallion::summary::VersionSummary;

    use super::*;

    fn contents(files: usize, rows: u64, bytes: u64) -> Contents {
        Contents { files, rows, bytes }
    }

    fn dataset(layer: Layer, name: &'static str, partitions: &[(&str, u64)]) -> DatasetSummary {
        let partitions: Vec<PartitionSummary> = partitions
            .iter()
            .map(|(value, rows)| PartitionSummary {
                value: value.to_string(),
                contents: contents(1, *rows, 1024),
            })
            .collect();
        let mut total = Contents::default();
        for partition in &partitions {
            total.add(partition.contents);
        }
        DatasetSummary {
            layer,
            name,
            partitions,
            contents: total,
        }
    }

    #[test]
    fn each_layer_heads_the_datasets_it_holds() {
        let datasets = [
            dataset(Layer::Bronze, "gps_reading", &[("2026-07-27", 10)]),
            dataset(Layer::Silver, "session", &[("2026-07-27", 2)]),
        ];

        let report = report(&datasets, &[], Detail::Datasets);

        let lines: Vec<&str> = report.lines().filter(|line| !line.is_empty()).collect();
        assert_eq!(lines[0], "bronze");
        assert!(lines[1].starts_with("  gps_reading"));
        assert_eq!(lines[2], "silver");
        assert!(lines[3].starts_with("  session"));
    }

    /// A layer nothing defines a dataset in is left out rather than shown as an empty
    /// heading.
    #[test]
    fn a_layer_with_no_datasets_is_not_reported() {
        let datasets = [dataset(Layer::Bronze, "gps_reading", &[("2026-07-27", 10)])];

        let report = report(&datasets, &[], Detail::Datasets);

        assert!(!report.contains("landing"));
        assert!(!report.contains("gold"));
    }

    /// The span is what a reader wants of a dataset with many partitions; the values
    /// themselves are there on request.
    #[test]
    fn many_partitions_are_reported_as_the_span_they_cover() {
        let datasets = [dataset(
            Layer::Bronze,
            "motis_segment",
            &[("2026-07-19", 1), ("2026-07-21", 2), ("2026-07-25", 3)],
        )];

        let report = report(&datasets, &[], Detail::Datasets);

        assert!(report.contains("2026-07-19 … 2026-07-25 (3)"), "{report}");
        assert!(!report.contains("2026-07-21"), "{report}");
    }

    #[test]
    fn one_partition_is_reported_as_itself() {
        let datasets = [dataset(Layer::Bronze, "gps_reading", &[("2026-07-27", 10)])];

        assert!(report(&datasets, &[], Detail::Datasets).contains("2026-07-27"));
    }

    #[test]
    fn asking_for_partitions_lists_every_one_of_them() {
        let datasets = [dataset(
            Layer::Bronze,
            "motis_segment",
            &[("2026-07-19", 1), ("2026-07-21", 2), ("2026-07-25", 3)],
        )];

        let report = report(&datasets, &[], Detail::Partitions);

        for value in ["2026-07-19", "2026-07-21", "2026-07-25"] {
            assert!(report.contains(value), "{value} missing from {report}");
        }
    }

    /// A dataset holding nothing says so, rather than reporting zeroes that read as
    /// measurements.
    #[test]
    fn a_dataset_holding_nothing_is_reported_as_absent() {
        let datasets = [dataset(Layer::Silver, "session", &[])];

        let report = report(&datasets, &[], Detail::Datasets);

        assert!(report.contains("session"));
        assert!(report.contains(ABSENT), "{report}");
    }

    #[test]
    fn an_artefact_is_reported_by_how_many_versions_it_has_and_which_is_latest() {
        let artefacts = [ArtefactSummary {
            artifact: "crossings".to_string(),
            versions: vec![
                VersionSummary {
                    version: "20260726T090000Z".to_string(),
                    contents: contents(1, 0, 2048),
                },
                VersionSummary {
                    version: "20260727T090000Z".to_string(),
                    contents: contents(1, 0, 4096),
                },
            ],
        }];

        let report = report(&[], &artefacts, Detail::Datasets);

        assert!(report.contains("gold"), "{report}");
        assert!(
            report.contains("2 versions, latest 20260727T090000Z"),
            "{report}"
        );
        assert!(report.contains("6.0 KiB"), "{report}");
    }

    #[test]
    fn counts_are_grouped_for_reading_and_pluralised() {
        assert_eq!(count(0, "row"), "0 rows");
        assert_eq!(count(1, "file"), "1 file");
        assert_eq!(count(999, "row"), "999 rows");
        assert_eq!(count(1_234, "row"), "1,234 rows");
        assert_eq!(count(9_876_543, "row"), "9,876,543 rows");
    }

    #[test]
    fn a_size_is_shown_in_the_largest_unit_that_leaves_a_number_to_read() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(512), "512 B");
        assert_eq!(size(1536), "1.5 KiB");
        assert_eq!(size(3 << 20), "3.0 MiB");
        assert_eq!(size(3 << 30), "3.0 GiB");
    }
}
