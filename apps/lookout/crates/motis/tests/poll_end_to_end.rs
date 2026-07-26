//! End-to-end test for [`motis::poll::poll_once`] against the **real** local Motis server:
//! seed a throwaway redis (testcontainers) with GPS near Frankfurt Hbf, then run one poll
//! tick that queries live Motis and lands segments in a real [`Store`]. Unlike
//! `poll_docker` (which mocks Motis), this exercises the rail filter and the
//! train-number/agency `/trip` enrichment against live DELFI data.
//!
//! Named `end_to_end`, so only the `end-to-end` nextest profile runs it (via
//! `just end_to_end_test`). Needs Docker (the redis container) **and** a Motis server up
//! on `127.0.0.1:8080`.

mod common;

use std::time::Duration;

use chrono::Utc;
use common::{gps, lpush, start_redis, wait_ready, RAIL_MODES};
use motis::client::MotisClient;
use motis::poll::{poll_once, PollConfig, PollOutcome};
use motis::store::Store;
use motis::window::PositionWindow;

#[tokio::test]
async fn poll_once_captures_rail_from_local_motis_end_to_end() {
    let (_redis, url) = start_redis().await;
    let mut conn = wait_ready(&url).await;

    let now = Utc::now();
    let now_ms = now.timestamp_millis();
    // Recent GPS around Frankfurt Hbf — a train-rich box in any daytime window.
    lpush(&mut conn, &gps(1, now_ms - 60_000, 50.107, 8.663)).await;
    lpush(&mut conn, &gps(2, now_ms - 30_000, 50.110, 8.660)).await;
    lpush(&mut conn, &gps(3, now_ms, 50.113, 8.669)).await;

    let db = tempfile::NamedTempFile::new().expect("temp db");
    let store = Store::open(db.path()).expect("open store");
    let client = MotisClient::default(); // 127.0.0.1:8080
    let mut window = PositionWindow::new(Duration::from_secs(30 * 60));
    let config = PollConfig {
        recent_lookback: Duration::from_secs(5 * 60),
        query_window_half: Duration::from_secs(5 * 60),
        zoom: 8.0,
        sample_limit: 1000,
    };

    let outcome = poll_once(now, &mut conn, &client, &store, &mut window, &config)
        .await
        .expect("poll once against local motis");

    let PollOutcome::Queried { segments, .. } = outcome else {
        panic!("expected a Motis query, got {outcome:?}");
    };
    assert!(
        segments > 0,
        "expected some rail segments near Frankfurt Hbf"
    );

    // Inspect what landed: every stored segment is rail, and the `/trip` enrichment
    // populated an agency and at least one train number.
    let reopened = rusqlite::Connection::open(db.path()).expect("reopen db");
    let mut stmt = reopened
        .prepare("SELECT mode, agency_name, train_number FROM segment")
        .expect("prepare");
    let rows: Vec<(String, Option<String>, Option<i64>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");

    assert!(
        rows.iter()
            .all(|(mode, _, _)| RAIL_MODES.contains(&mode.as_str())),
        "every stored segment should be a rail mode; got {:?}",
        rows.iter().map(|r| r.0.as_str()).collect::<Vec<_>>()
    );
    assert!(
        rows.iter().any(|(_, agency, _)| agency.is_some()),
        "expected at least one segment's trip to resolve an agency"
    );
    assert!(
        rows.iter().any(|(_, _, number)| number.is_some()),
        "expected at least one train number from the /trip enrichment"
    );
}
