use chrono::{DateTime, Utc};
use medallion::{Query, Root};
use medallion_model::{MOTIS_SOURCE, MotisSource, MotisSourceRow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    Written,
    AlreadyRecorded,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("reading the recorded sources: {0}")]
    Query(#[from] medallion::QueryError),
    #[error("recording the source: {0}")]
    Write(#[from] medallion::AppendError),
}

pub async fn register(
    root: &Root,
    source: &MotisSource,
    at: DateTime<Utc>,
) -> Result<Registration, SourceError> {
    if recorded(root)
        .await?
        .iter()
        .any(|row| row.source_id == source.id())
    {
        return Ok(Registration::AlreadyRecorded);
    }
    root.rows_of::<MotisSourceRow>()
        .append_rows(at, &[source.row(at)])
        .await?;
    Ok(Registration::Written)
}

pub async fn recorded(root: &Root) -> Result<Vec<MotisSourceRow>, SourceError> {
    let query = Query::new(root.clone());
    if !query
        .register_if_present(MOTIS_SOURCE, MOTIS_SOURCE.name)
        .await?
    {
        return Ok(Vec::new());
    }
    Ok(query
        .rows(&format!("SELECT * FROM {}", MOTIS_SOURCE.name))
        .await?)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use medallion_model::{Feed, MotisVersion};
    use url::Url;

    use super::*;

    fn source(feed: Feed) -> MotisSource {
        MotisSource {
            base_url: Url::parse("http://127.0.0.1:8080").expect("a URL"),
            motis_version: "v2.11.3".parse::<MotisVersion>().expect("a version"),
            feed,
            area: None,
            label: None,
        }
    }

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 7, 9, minute, 0).unwrap()
    }

    #[tokio::test]
    async fn a_new_source_is_written() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());

        let registration = register(&root, &source(Feed::Local), at(0))
            .await
            .expect("register");

        assert_eq!(registration, Registration::Written);
        let recorded = recorded(&root).await.expect("read back");
        assert_eq!(recorded, vec![source(Feed::Local).row(at(0))]);
    }

    #[tokio::test]
    async fn a_source_already_recorded_is_not_written_again() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        register(&root, &source(Feed::Local), at(0))
            .await
            .expect("first");

        let registration = register(&root, &source(Feed::Local), at(1))
            .await
            .expect("second");

        assert_eq!(registration, Registration::AlreadyRecorded);
        assert_eq!(recorded(&root).await.expect("read back").len(), 1);
    }

    #[tokio::test]
    async fn a_different_source_is_written_beside_the_first() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        register(&root, &source(Feed::Local), at(0))
            .await
            .expect("first");

        let registration = register(&root, &source(Feed::Transitous), at(1))
            .await
            .expect("second");

        assert_eq!(registration, Registration::Written);
        assert_eq!(recorded(&root).await.expect("read back").len(), 2);
    }

    #[tokio::test]
    async fn a_recorded_row_reads_back_as_the_source_written() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        register(&root, &source(Feed::Transitous), at(0))
            .await
            .expect("register");

        let row = recorded(&root).await.expect("read back").remove(0);

        assert_eq!(
            MotisSource::try_from(row).expect("a valid source"),
            source(Feed::Transitous)
        );
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    struct UnlabelledRow {
        source_id: medallion_model::MotisSourceId,
        base_url: String,
        motis_version: String,
        feed: Feed,
        country: Option<String>,
        region_id: Option<String>,
        region_name: Option<String>,
        overture_release: Option<String>,
        #[serde(with = "chrono::serde::ts_milliseconds")]
        registered_at: DateTime<Utc>,
    }

    impl medallion::Row for UnlabelledRow {
        type Layer = medallion::layers::Bronze;
        const DATASET: medallion::DatasetSpec<Self::Layer> = MOTIS_SOURCE;
        const INSTANTS: &'static [&'static str] = &["registered_at"];
    }

    #[tokio::test]
    async fn a_source_recorded_before_labels_reads_back_unlabelled_beside_a_labelled_one() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = Root::new(tmp.path());
        let unlabelled = source(Feed::Local);
        root.rows_of::<UnlabelledRow>()
            .append_rows(
                at(0),
                &[UnlabelledRow {
                    source_id: unlabelled.id(),
                    base_url: unlabelled.base_url.to_string(),
                    motis_version: unlabelled.motis_version.to_string(),
                    feed: unlabelled.feed,
                    country: None,
                    region_id: None,
                    region_name: None,
                    overture_release: None,
                    registered_at: at(0),
                }],
            )
            .await
            .expect("record a source before labels");
        let labelled = MotisSource {
            label: Some("delfi".parse().expect("a label")),
            ..source(Feed::Local)
        };
        register(&root, &labelled, at(1)).await.expect("register");

        let sources: Vec<MotisSource> = recorded(&root)
            .await
            .expect("read back")
            .into_iter()
            .map(|row| MotisSource::try_from(row).expect("a valid source"))
            .collect();

        assert_eq!(sources.len(), 2);
        assert!(sources.contains(&unlabelled), "{sources:?}");
        assert!(sources.contains(&labelled), "{sources:?}");
    }
}
