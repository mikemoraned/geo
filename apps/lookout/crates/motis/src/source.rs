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
}
